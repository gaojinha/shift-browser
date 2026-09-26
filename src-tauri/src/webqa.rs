//! P2：本机 opencode 代理（Rust → opencode HTTP server）
//!
//! 定位：浏览器只做"手/入口"，大脑是本机 opencode。侧边栏把「问题 + 选区上下文」
//! 交给本机 opencode 回答，回答再流式推回前端。
//!
//! 安全铁律（P2 不可协商）：
//! - 凭据只存在 Rust 内存里：绝不进前端、绝不写日志、绝不落盘。`Debug` 手工实现，
//!   不 derive，避免 `{:?}` 意外泄密。
//! - 只连回环 `127.0.0.1`；**不碰 `0.0.0.0:4098`**（那是高哥有意对外暴露的实例）。
//! - 网页内容不出本机：prompt 只发给回环地址。
//! - 默认 agent 用 `plan`（opencode 侧 Disallows all edit tools），钉死"只读问答"，
//!   防止模型拿着网页上下文去改文件/跑命令。

use base64::Engine;
use futures_util::StreamExt;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::ipc::Channel;

use crate::error::{AppError, CommandResult};

/// 有意只连回环。opencode serve 以 `--hostname 127.0.0.1` 起在 4096。
const DEFAULT_BASE_URL: &str = "http://127.0.0.1:4096";

/// SSE 等待上限：防"迟迟不 idle"把协程和连接悬挂住。
const SSE_MAX_SECS: u64 = 120;

/// 隐私档 L2 固定三件套：问题 + 选区/段落 + 页面标题/网址。
const PRIVACY_L2: &str = "L2（问题 + 选区/段落 + 页面标题/网址）";

/// 默认 agent：`plan` 在 opencode 侧禁用一切编辑工具，正合适"只读问答"。
pub const DEFAULT_AGENT: &str = "plan";

// ───────────────────────── 凭据 ─────────────────────────

/// opencode serve 的 Basic 认证凭据。仅内存持有。
pub struct Credentials {
    pub username: String,
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    /// 手工实现：任何日志/错误链里都只暴露用户名，密码永不进 `{:?}`。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

impl Credentials {
    fn basic_header(&self) -> String {
        let raw = format!("{}:{}", self.username, self.password);
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
        )
    }
}

/// 从 `/proc` 自动发现 serve 进程的凭据。
///
/// 零配置优先：不要求高哥把密码填进设置项，也不落盘。做法是找命令行里
/// 带 `opencode` + `serve` 的进程，读它的环境变量
/// `OPENCODE_SERVER_USERNAME` / `OPENCODE_SERVER_PASSWORD`（同用户可读）。
///
/// 返回 `None` 表示"机器上没找到带认证的 serve 实例"（比如 serve 根本没起认证）。
fn discover_credentials_from_proc() -> Option<Credentials> {
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let pid = entry.file_name().to_string_lossy().to_string();
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Some(creds) = credentials_from_pid(&Path::new("/proc").join(&pid)) else {
            continue;
        };
        log::info!(
            "[webqa] 从 serve 进程 {} 发现 opencode 凭据（用户 {}）",
            pid,
            creds.username
        );
        return Some(creds);
    }
    None
}

/// 读单个进程的环境变量里那一对 opencode 认证变量。
fn credentials_from_pid(proc_dir: &Path) -> Option<Credentials> {
    // cmdline 是 NUL 分隔的，所以按 "opencode" + "serve" 两个独立片段匹配
    // （拼成 "opencode serve" 反而匹配不到）。
    let raw = std::fs::read(proc_dir.join("cmdline")).ok()?;
    let cmd = String::from_utf8_lossy(&raw);
    if !cmd.contains("opencode") || !cmd.contains("serve") {
        return None;
    }
    let env_raw = std::fs::read(proc_dir.join("environ")).ok()?;
    let mut username = None;
    let mut password = None;
    for item in String::from_utf8_lossy(&env_raw).split('\0') {
        if let Some(v) = item.strip_prefix("OPENCODE_SERVER_USERNAME=") {
            username = Some(v.to_string());
        } else if let Some(v) = item.strip_prefix("OPENCODE_SERVER_PASSWORD=") {
            password = Some(v.to_string());
        }
    }
    Some(Credentials {
        username: username?,
        password: password?,
    })
}

/// 拿凭据：先看用户是否显式配置过，没有再自动发现。
async fn resolve_credentials(db: tauri::State<'_, crate::db::Db>) -> CommandResult<Credentials> {
    // 显式设置优先（高哥手动配过就以他的为准）。走 settings_cache 内存缓存，不打库。
    let username = crate::db::settings_cache::get(&db.pool, "opencode_username").await;
    let password = crate::db::settings_cache::get(&db.pool, "opencode_password").await;
    if let (Some(u), Some(p)) = (username, password) {
        if !u.trim().is_empty() && !p.is_empty() {
            log::info!("[webqa] 使用设置项里的 opencode 凭据（用户 {}）", u);
            return Ok(Credentials {
                username: u,
                password: p,
            });
        }
    }
    discover_credentials_from_proc().ok_or_else(|| {
        AppError::net(
            "没找到本机 opencode serve 的认证凭据。请确认 127.0.0.1:4096 上的 \
             `opencode serve` 在跑且设置了 OPENCODE_SERVER_PASSWORD，\
             或在设置里填 opencode_username / opencode_password。"
                .to_string(),
        )
        .into()
    })
}

// ───────────────────────── HTTP 基础 ─────────────────────────

/// 建一个只带 Basic 认证的 client。连接超时 5s；SSE 长连接由外层 deadline 管。
fn client(creds: &Credentials) -> Result<reqwest::Client, AppError> {
    let mut headers = reqwest::header::HeaderMap::new();
    match reqwest::header::HeaderValue::from_str(&creds.basic_header()) {
        Ok(v) => {
            headers.insert(reqwest::header::AUTHORIZATION, v);
        }
        Err(_) => {
            // 凭据含非 ASCII 等非法 header 字符：明确报错，别静默裸奔
            return Err(AppError::net(
                "opencode 凭据含非法字符，无法组装认证头".to_string(),
            ));
        }
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::net(format!("opencode 客户端初始化失败：{e}")))
}

fn base_url() -> String {
    DEFAULT_BASE_URL.to_string()
}

// ───────────────────────── 流式事件契约 ─────────────────────────

/// 前端通道事件。独立于 `ai::StreamEvent`：那边是 AGNES 云端契约，
/// 这边要多带 session_id（多轮追问要复用会话），不污染既有枚举。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum WebqaEvent {
    /// 会话就绪（新建或复用），前端存下来供追问复用。
    Session { session_id: String },
    /// 增量文本（低延迟推送，可能重复到达；最终以 Done.full 为准）。
    Delta { content: String },
    /// 完整回复（流结束）。full 是权威值。
    Done { full: String, session_id: String },
    /// 出错（中断）。
    Error { message: String },
}

/// 取消注册表：request_id -> 取消标志。复用 ai.rs 的范式。
static CANCEL_FLAGS: OnceLock<Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>> =
    OnceLock::new();

fn cancel_flags() -> &'static Mutex<std::collections::HashMap<String, Arc<AtomicBool>>> {
    CANCEL_FLAGS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// 取消一次网页问答（前端"停止生成"按钮调用）。
#[tauri::command]
pub fn opencode_cancel(request_id: String) {
    if let Some(flag) = cancel_flags()
        .lock()
        .ok()
        .and_then(|m| m.get(&request_id).cloned())
    {
        flag.store(true, Ordering::Relaxed);
        log::info!("[webqa] 已请求取消 {}", request_id);
    }
}

fn is_cancelled(flag: &AtomicBool) -> bool {
    flag.load(Ordering::Relaxed)
}

// ───────────────────────── prompt 组装（隐私档 L2）─────────────────────────

/// 组装发给 opencode 的正文。
///
/// L2 = 问题 + 选区/段落 + 页面标题/网址。不带 cookie、不带完整 DOM、不出本机。
/// 末尾显式钉死"只读问答、不要执行文件/命令"——因为 session 的 cwd 落在
/// opencode serve 启动目录（不可按 session 指定），得在 prompt 里把边界说清。
pub fn build_prompt(
    question: &str,
    context: Option<&str>,
    page_title: Option<&str>,
    page_url: Option<&str>,
) -> String {
    let mut s = String::new();
    s.push_str("以下是用户在浏览器里选中的网页内容，请只依据它回答问题。\n\n");
    if let Some(url) = page_url.filter(|u| !u.trim().is_empty()) {
        s.push_str(&format!("【网址】{url}\n"));
    }
    if let Some(t) = page_title.filter(|t| !t.trim().is_empty()) {
        s.push_str(&format!("【标题】{t}\n"));
    }
    if let Some(c) = context.map(str::trim).filter(|c| !c.is_empty()) {
        s.push_str(&format!("【选中的内容】\n{c}\n"));
    } else {
        s.push_str("（用户没有选中任何内容）\n");
    }
    s.push_str(&format!("\n【问题】\n{question}\n"));
    s.push_str(
        "\n要求：直接回答，简洁分点；这是网页阅读问答，\
         不要读写任何文件、不要执行任何命令、不要调用工具。\n",
    );
    s
}

// ───────────────────────── session ─────────────────────────

/// 确保有一个可用 session：有 session_id 就验证它还在，没有就新建。
/// 返回 (session_id, baseline_message_ids)——后者用于识别"本次提问"新产生的
/// assistant 消息，避免把 session 里的历史回答当成本次结果。
async fn ensure_session(
    http: &reqwest::Client,
    existing: Option<&str>,
) -> Result<(String, Vec<String>), AppError> {
    if let Some(sid) = existing.filter(|s| !s.trim().is_empty()) {
        let ok = http
            .get(format!("{}/session/{}", base_url(), sid))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            let baseline = baseline_ids(http, sid).await.unwrap_or_default();
            return Ok((sid.to_string(), baseline));
        }
        log::info!("[webqa] 会话 {} 已失效，改建新会话", sid);
    }

    let resp = http
        .post(format!("{}/session", base_url()))
        .json(&json!({ "title": "网页随手问" }))
        .send()
        .await
        .map_err(|e| AppError::net(format!("建 opencode 会话失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::net(format!(
            "建 opencode 会话失败：HTTP {}",
            resp.status()
        )));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| AppError::net(format!("解析会话响应失败：{e}")))?;
    let sid = v
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::net("opencode 会话响应里没有 id".to_string()))?
        .to_string();
    Ok((sid, Vec::new()))
}

/// 取 session 现有消息 id，作为"提问前基线"。
async fn baseline_ids(http: &reqwest::Client, sid: &str) -> Result<Vec<String>, AppError> {
    let resp = http
        .get(format!("{}/session/{}/message", base_url(), sid))
        .send()
        .await
        .map_err(|e| AppError::net(format!("读取会话历史失败：{e}")))?;
    let arr: Value = resp
        .json()
        .await
        .map_err(|e| AppError::net(format!("解析会话历史失败：{e}")))?;
    Ok(arr
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|m| {
                    m.get("info")
                        .and_then(|i| i.get("id"))
                        .and_then(Value::as_str)
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default())
}

// ───────────────────────── SSE ─────────────────────────

/// 从 SSE 原始字节里切出下一个完整事件块的 `data:` 负载。
///
/// 返回 `(data, 已消费的字节数)`；缓冲不足时返回 `None` 等更多字节。
pub fn take_sse_data(buf: &[u8]) -> Option<(String, usize)> {
    // SSE 以空行（\n\n）分隔一个事件
    let sep = buf.windows(2).position(|w| w == b"\n\n")?;
    let block = &buf[..sep];
    let mut data = String::new();
    for line in String::from_utf8_lossy(block).lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim());
        }
    }
    Some((data, sep + 2))
}

/// 处理单个 SSE 事件，返回 (是否本次回答的文本增量, 是否流结束)。
pub fn fold_event(
    ev: &Value,
    sid: &str,
    target_msg: &mut Option<String>,
    baseline: &[String],
) -> Option<(Option<String>, bool)> {
    let props = ev.get("properties")?;
    // 全局事件流：只认本 session 的
    if props.get("sessionID").and_then(Value::as_str) != Some(sid) {
        return None;
    }
    match ev.get("type").and_then(Value::as_str) {
        // 锁定"本次提问"的 assistant 消息
        Some("message.updated") => {
            let info = props.get("info")?;
            if info.get("role").and_then(Value::as_str) == Some("assistant") {
                if let Some(id) = info.get("id").and_then(Value::as_str) {
                    if !baseline.iter().any(|b| b == id) {
                        *target_msg = Some(id.to_string());
                    }
                }
            }
            None
        }
        // 文本增量：只认 target 消息的 text 字段
        Some("message.part.delta") => {
            if props.get("messageID").and_then(Value::as_str) != target_msg.as_deref() {
                return None;
            }
            if props.get("field").and_then(Value::as_str) != Some("text") {
                return None;
            }
            let d = props.get("delta").and_then(Value::as_str).unwrap_or("");
            if d.is_empty() {
                None
            } else {
                Some((Some(d.to_string()), false))
            }
        }
        Some("session.idle") => Some((None, true)),
        _ => None,
    }
}

// ───────────────────────── 前端桥接命令 ─────────────────────────

/// 前端读当前选区（Tauri command）。
///
/// 为什么不直接在前端 `invoke('read_selection')`：`read_selection` 是
/// **api.sock 的 handler**（`api/handlers/mod.rs` 的 HANDLERS），只对 MCP
/// 客户端和 api.sock 生效，不是 Tauri command，前端 invoke 调不到。
/// 这里做个桥：复用同一套 `webqa::read_selection` 实现，不重复逻辑、
/// 也不把它塞进 `invoke_handler`（那会让"开放 API 工具"和"前端内部命令"混在一起）。
///
/// 键名保持下划线（`tab_id`/`max_chars`）：与 api handler 一致，少一层转换。
/// 注意前端那边是 `invoke`，没有 Tauri 的 camelCase 自动转换。
#[tauri::command]
pub async fn read_page_selection(
    app: tauri::AppHandle,
    tab_id: Option<String>,
    expand: Option<bool>,
    max_chars: Option<u64>,
) -> CommandResult<serde_json::Value> {
    // 构造与 api handler 相同形状的请求体，复用唯一实现
    let mut req = serde_json::Map::new();
    if let Some(t) = tab_id {
        req.insert("tab_id".into(), Value::String(t));
    }
    if let Some(e) = expand {
        req.insert("expand".into(), Value::Bool(e));
    }
    if let Some(m) = max_chars {
        req.insert("max_chars".into(), Value::Number(m.into()));
    }
    let out = crate::api::handlers::content::selection(&app, &Value::Object(req)).await;
    // api handler 用 E_* 结构表达业务结果（rejected/无选区不算故障），
    // 这里原样透传，前端按字段判断，不转换。
    Ok(out)
}

/// 连通性探测：凭据能否用、opencode 是否活着、有哪些 agent。
/// 不回显任何凭据。
#[tauri::command]
pub async fn opencode_probe(
    db: tauri::State<'_, crate::db::Db>,
) -> CommandResult<serde_json::Value> {
    let creds = resolve_credentials(db).await?;
    let http = client(&creds)?;
    let resp = http
        .get(format!("{}/config", base_url()))
        .send()
        .await
        .map_err(|e| {
            AppError::net(format!(
                "连不上本机 opencode（{e}）：确认 4096 上 serve 在跑"
            ))
        })?;
    if !resp.status().is_success() {
        return Err(AppError::net(format!("opencode 认证失败：HTTP {}", resp.status())).into());
    }
    let cfg: Value = resp
        .json()
        .await
        .map_err(|e| AppError::net(format!("解析 opencode 配置失败：{e}")))?;

    let agents = list_agents(&http).await;
    let ask_agent_present = agents.iter().any(|a| a == DEFAULT_AGENT);

    Ok(json!({
        "ok": true,
        "baseUrl": base_url(),
        "model": cfg.get("model").and_then(Value::as_str).unwrap_or_default(),
        "defaultAgent": cfg.get("default_agent").and_then(Value::as_str).unwrap_or_default(),
        "askAgent": DEFAULT_AGENT,
        // opencode 对不存在的 agent 会"静默回退到默认 agent"。默认 agent 是
        // build（有编辑权限），那等于偷偷放宽了只读边界 —— 必须显式报出来。
        "askAgentPresent": ask_agent_present,
        "askAgentFallbackRisk": !ask_agent_present,
        "agents": agents,
        "privacy": PRIVACY_L2,
    }))
}

/// 列出 opencode 可用 agent 名。
async fn list_agents(http: &reqwest::Client) -> Vec<String> {
    let Ok(resp) = http.get(format!("{}/agent", base_url())).send().await else {
        return Vec::new();
    };
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    let items: Vec<&Value> = match &v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(m) => m.values().collect(),
        _ => return Vec::new(),
    };
    items
        .iter()
        .filter_map(|a| a.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect()
}

/// 确认要用的 agent 真的存在。
///
/// 存在的理由：opencode 收到未知 agent 名不会报错，而是**悄悄换成默认 agent**。
/// 侧边栏要的是只读问答（`plan`，禁用编辑工具）；一旦被回退成 `build`，
/// 模型就带着"能改文件"的权限在网页上下文里跑 —— 这条防线不能靠运气。
async fn ensure_agent_exists(http: &reqwest::Client, agent: &str) -> CommandResult<()> {
    let agents = list_agents(http).await;
    if agents.is_empty() {
        return Err(AppError::net(
            "读不到 opencode 的 agent 列表，无法确认只读边界，先不提问".to_string(),
        )
        .into());
    }
    if agents.iter().any(|a| a == agent) {
        return Ok(());
    }
    Err(AppError::net(format!(
        "opencode 里没有 `{agent}` 这个 agent（现有：{}）。\
         它会被静默回退成有编辑权限的默认 agent，所以这里先拦住。",
        agents.join("、")
    ))
    .into())
}

/// `opencode_ask` 的入参。
///
/// 打包成结构体而不是八个位置参数：前端按字段名传，传错顺序/漏字段在编译期
/// 和调用点就暴露，而不是运行时错位。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskRequest {
    pub request_id: String,
    pub question: String,
    /// 选区/段落正文（隐私档 L2 的"上下文"）
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub page_title: Option<String>,
    #[serde(default)]
    pub page_url: Option<String>,
    /// 复用的 opencode session；空则新建（多轮追问靠它保持上下文）
    #[serde(default)]
    pub session_id: Option<String>,
    /// 覆盖默认 agent；空则用 `plan`（只读）
    #[serde(default)]
    pub agent: Option<String>,
}

/// 发起一次网页问答（流式）。
///
/// 流程：确认凭据 → 确认 agent 存在（防静默回退到有编辑权限的默认 agent）→
/// 确保 session → 记基线 → 开 SSE → `prompt_async` → 过滤本 session 的
/// assistant 文本增量推给前端 → `session.idle` 收尾。
#[tauri::command]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub async fn opencode_ask(
    db: tauri::State<'_, crate::db::Db>,
    req: AskRequest,
    on_event: Channel<WebqaEvent>,
) -> CommandResult<()> {
    let AskRequest {
        request_id,
        question,
        context,
        page_title,
        page_url,
        session_id,
        agent,
    } = req;
    let creds = match resolve_credentials(db).await {
        Ok(c) => c,
        Err(e) => {
            on_event
                .send(WebqaEvent::Error {
                    message: e.to_string(),
                })
                .ok();
            return Err(e);
        }
    };
    let http = match client(&creds) {
        Ok(c) => c,
        Err(e) => {
            on_event
                .send(WebqaEvent::Error {
                    message: e.to_string(),
                })
                .ok();
            return Err(e.into());
        }
    };

    let agent = agent.unwrap_or_else(|| DEFAULT_AGENT.to_string());
    // 未知 agent 会被静默回退成有编辑权限的默认 agent → 宁可拒绝发问
    if let Err(e) = ensure_agent_exists(&http, &agent).await {
        on_event
            .send(WebqaEvent::Error {
                message: e.to_string(),
            })
            .ok();
        return Err(e);
    }

    let (sid, baseline) = match ensure_session(&http, session_id.as_deref()).await {
        Ok(v) => v,
        Err(e) => {
            on_event
                .send(WebqaEvent::Error {
                    message: e.to_string(),
                })
                .ok();
            return Err(e.into());
        }
    };
    on_event
        .send(WebqaEvent::Session {
            session_id: sid.clone(),
        })
        .ok();

    // 先开 SSE 再发 prompt：反了就可能漏掉开头的事件
    let sse = http
        .get(format!("{}/event", base_url()))
        .send()
        .await
        .map_err(|e| AppError::net(format!("订阅 opencode 事件流失败：{e}")))?;

    let prompt = build_prompt(
        &question,
        context.as_deref(),
        page_title.as_deref(),
        page_url.as_deref(),
    );

    let cancel = Arc::new(AtomicBool::new(false));
    cancel_flags()
        .lock()
        .unwrap()
        .insert(request_id.clone(), cancel.clone());

    // Channel → 闭包：让 IO 核心与 Tauri 解耦
    let mut push = |e: WebqaEvent| {
        on_event.send(e).ok();
    };

    let result = drive(
        AskRun {
            http: &http,
            sid: &sid,
            baseline: &baseline,
            agent: &agent,
            prompt: &prompt,
            cancel: &cancel,
            emit: &mut push,
        },
        sse,
    )
    .await;

    cancel_flags().lock().unwrap().remove(&request_id);
    result
}

/// SSE 字节累积器：把分片字节拼成完整事件。
///
/// 抽成独立结构的原因：SSE 是"半截事件"常态，必须**跨 chunk 攒够才解析**。
/// 纯内存、可单测，不掺 IO。
#[derive(Default)]
pub struct SseAccumulator {
    buf: Vec<u8>,
}

impl SseAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂进一段字节，返回本次攒齐的 `data:` 负载（可能有多个）。
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some((data, used)) = take_sse_data(&self.buf) {
            self.buf.drain(..used);
            if !data.is_empty() {
                out.push(data);
            }
        }
        out
    }

    /// 还没凑完的事件残留字节数（仅测试/诊断用）。
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

/// 一次问答的运行上下文。收拢 `drive` 的散参数（clippy 限 7 个，且本来就该
/// 打包成一坨而不是八个位置参数）。
struct AskRun<'a> {
    http: &'a reqwest::Client,
    sid: &'a str,
    baseline: &'a [String],
    agent: &'a str,
    prompt: &'a str,
    cancel: &'a AtomicBool,
    /// 事件出口。用闭包而不是 tauri `Channel`：Channel 在测试里造不出来，
    /// 用闭包才能让整条 IO 路径被真实 Rust 测试覆盖（而不是拿 Python 复刻一遍）。
    emit: &'a mut (dyn FnMut(WebqaEvent) + Send),
}

/// SSE 读取 + 事件收敛。
///
/// 只做 IO；事件语义全在 `fold_event`，SSE 切块全在 `SseAccumulator`，两者都有单测。
async fn drive(run: AskRun<'_>, sse: reqwest::Response) -> CommandResult<()> {
    let AskRun {
        http,
        sid,
        baseline,
        agent,
        prompt,
        cancel,
        emit,
    } = run;
    // 真正发问
    let resp = http
        .post(format!("{}/session/{}/prompt_async", base_url(), sid))
        .json(&json!({ "agent": agent, "parts": [{ "type": "text", "text": prompt }] }))
        .send()
        .await
        .map_err(|e| AppError::net(format!("发问失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::net(format!("opencode 拒收提问：HTTP {}", resp.status())).into());
    }

    // SSE 连接此刻已经握在手里（ask 里先开订阅再发问，防漏事件）
    let mut stream = sse.bytes_stream();
    let mut acc = SseAccumulator::new();
    let mut target_msg: Option<String> = None;
    let mut delta_buf = String::new();

    // timeout 包住整个读循环：外层 Err=超时，内层才是循环自己的结果
    let loop_res: Result<Result<(), AppError>, tokio::time::error::Elapsed> =
        tokio::time::timeout(Duration::from_secs(SSE_MAX_SECS), async {
            while let Some(chunk) = stream.next().await {
                if is_cancelled(cancel) {
                    return Err(AppError::net("已停止生成".to_string()));
                }
                let bytes = chunk.map_err(|e| AppError::net(format!("读事件流失败：{e}")))?;
                for data in acc.push(&bytes) {
                    let Ok(ev) = serde_json::from_str::<Value>(&data) else {
                        continue;
                    };
                    if let Some((delta, done)) = fold_event(&ev, sid, &mut target_msg, baseline) {
                        if let Some(d) = delta {
                            delta_buf.push_str(&d);
                            emit(WebqaEvent::Delta { content: d });
                        }
                        if done {
                            return Ok(());
                        }
                    }
                }
            }
            Err(AppError::net(format!(
                "事件流在回答完成前断开（已收到 {} 字）",
                delta_buf.chars().count()
            )))
        })
        .await;

    match loop_res {
        Ok(inner) => {
            emit(WebqaEvent::Done {
                full: delta_buf,
                session_id: sid.to_string(),
            });
            inner.map_err(Into::into)
        }
        Err(_) => {
            let msg = format!("等待回答超时（{} 秒）", SSE_MAX_SECS);
            emit(WebqaEvent::Error {
                message: msg.clone(),
            });
            Err(AppError::net(msg).into())
        }
    }
}

// ───────────────────────── 单测 ─────────────────────────
// ================================================================
// 整页汉化（webzh.rs）的公共出口
//
// 汉化要的是「非流式、拿结构化结果」，跟随手问的聊天流式协议不是一回事，
// 但底层三样东西完全共用：凭据发现、HTTP 客户端、会话管理。
// 这里开 pub 出口给 webzh 用，避免那边把同一套逻辑抄第二遍。
// 抄第二遍的代价不是"多几行"——是以后改凭据发现方式要改两处，漏一处就出玄学问题。
// ================================================================

/// 供 webzh：解析 opencode 凭据（从 /proc 扫 serve 进程，不落盘）。
pub async fn resolve_credentials_pub(
    db: tauri::State<'_, crate::db::Db>,
) -> CommandResult<Credentials> {
    resolve_credentials(db).await
}

/// 供 webzh：构造 HTTP 客户端（共享连接池）。
pub fn client_pub(creds: &Credentials) -> Result<reqwest::Client, AppError> {
    client(creds)
}

/// 供 webzh：opencode 基址。
pub fn base_url_pub() -> String {
    base_url()
}

/// 供 webzh：确保会话可用，返回 (session_id, baseline 消息 id 列表)。
///
/// 第二个参数是会话标题：汉化会话单独起名，便于事后在 opencode 界面里认出来。
pub async fn ensure_session_pub(
    http: &reqwest::Client,
    existing: Option<&str>,
    title: &str,
) -> Result<(String, Vec<String>), AppError> {
    if let Some(sid) = existing.filter(|s| !s.trim().is_empty()) {
        let ok = http
            .get(format!("{}/session/{}", base_url(), sid))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            let baseline = baseline_ids(http, sid).await.unwrap_or_default();
            return Ok((sid.to_string(), baseline));
        }
        log::info!("[webqa] 会话 {sid} 已失效，改建新会话");
    }
    let resp = http
        .post(format!("{}/session", base_url()))
        .json(&json!({ "title": title }))
        .send()
        .await
        .map_err(|e| AppError::net(format!("建 opencode 会话失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::net(format!(
            "建 opencode 会话失败：HTTP {}",
            resp.status()
        )));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| AppError::net(format!("解析建会话响应失败：{e}")))?;
    let sid = v
        .pointer("/id")
        .or_else(|| v.get("id"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| AppError::internal("opencode 建会话未返回 id"))?
        .to_string();
    let baseline = baseline_ids(http, &sid).await.unwrap_or_default();
    Ok((sid, baseline))
}

/// 供 webzh：非流式发一轮 prompt，拿回完整文本。
///
/// 复用随手问那套 SSE 订阅 + `fold_event` 累积，但**不往前端发任何事件**：
/// 汉化要的是"这一批全部译完的一次性结果"，中间增量对写回 DOM 没用。
///
/// 先订阅后发问：反了会漏掉开头的事件，答案变残缺。
pub async fn ask_once(
    http: &reqwest::Client,
    base: &str,
    sid: &str,
    agent: &str,
    prompt: &str,
    timeout_secs: u64,
) -> Result<String, AppError> {
    let sse = http
        .get(format!("{base}/event"))
        .send()
        .await
        .map_err(|e| AppError::net(format!("订阅 opencode 事件流失败：{e}")))?;

    let resp = http
        .post(format!("{base}/session/{sid}/prompt_async"))
        .json(&json!({ "agent": agent, "parts": [{ "type": "text", "text": prompt }] }))
        .send()
        .await
        .map_err(|e| AppError::net(format!("发翻译请求失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::net(format!(
            "opencode 拒收翻译请求：HTTP {}",
            resp.status()
        )));
    }

    let baseline = baseline_ids(http, sid).await.unwrap_or_default();
    let mut stream = sse.bytes_stream();
    let mut acc = SseAccumulator::new();
    let mut target_msg: Option<String> = None;
    let mut buf = String::new();

    let res: Result<Result<(), AppError>, tokio::time::error::Elapsed> =
        tokio::time::timeout(Duration::from_secs(timeout_secs), async {
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| AppError::net(format!("读事件流失败：{e}")))?;
                for data in acc.push(&bytes) {
                    let Ok(ev) = serde_json::from_str::<Value>(&data) else {
                        continue;
                    };
                    if let Some((delta, done)) = fold_event(&ev, sid, &mut target_msg, &baseline) {
                        if let Some(d) = delta {
                            buf.push_str(&d);
                        }
                        if done {
                            return Ok(());
                        }
                    }
                }
            }
            Err(AppError::net(format!(
                "翻译事件流提前断开（已收到 {} 字）",
                buf.chars().count()
            )))
        })
        .await;

    match res {
        Ok(inner) => inner.map(|_| buf),
        Err(_) => Err(AppError::net(format!(
            "翻译超时（{} 秒内未完成，已收到 {} 字）",
            timeout_secs,
            buf.chars().count()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_按空行切事件() {
        let raw = b"event: message.part.delta\ndata: {\"a\":1}\n\ndata: {\"b\":2}\n\n";
        let (first, used) = take_sse_data(raw).unwrap();
        assert_eq!(first, "{\"a\":1}");
        let rest = &raw[used..];
        let (second, _) = take_sse_data(rest).unwrap();
        assert_eq!(second, "{\"b\":2}");
    }

    #[test]
    fn sse_半截事件等更多字节() {
        assert!(take_sse_data(b"data: {\"a\":1}\n").is_none());
    }

    #[test]
    fn accumulator_分片到达也能拼回() {
        let mut acc = SseAccumulator::new();
        // 真实网络会把一个事件劈成任意几段：第一段不该产出任何事件
        assert!(acc.push(b"data: {\"ty").is_empty());
        assert_eq!(acc.pending(), 10, "半截应留在缓冲里");
        // 第二段把第一个事件补齐，并顺带带来第二个完整事件
        let out = acc.push(b"pe\":\"x\"}\n\ndata: {\"b\":2}\n\n");
        assert_eq!(
            out,
            vec!["{\"type\":\"x\"}".to_string(), "{\"b\":2}".to_string()]
        );
        assert_eq!(acc.pending(), 0, "消费完不该留残留");
    }

    #[test]
    fn accumulator_一次喂入多事件() {
        let mut acc = SseAccumulator::new();
        let out = acc.push(b"data: {\"n\":1}\n\ndata: {\"n\":2}\n\ndata: {\"n\":3}\n\n");
        assert_eq!(out.len(), 3);
        assert_eq!(out[2], "{\"n\":3}");
    }

    #[test]
    fn accumulator_多行data按sse规范用换行拼接() {
        let mut acc = SseAccumulator::new();
        let out = acc.push(b"data: {\"a\":\ndata: 1}\n\n");
        assert_eq!(out, vec!["{\"a\":\n1}".to_string()]);
    }

    #[test]
    fn accumulator_忽略非data行与心跳() {
        let mut acc = SseAccumulator::new();
        // 事件名在 event: 行、payload 在 data: 行；只认 data
        let out = acc.push(":heartbeat注释\n\ndata: {\"a\":1}\n\n".as_bytes());
        assert_eq!(out, vec!["{\"a\":1}".to_string()]);
    }

    #[test]
    fn fold_忽略别家session事件() {
        let ev = json!({"type":"message.part.delta","properties":{
            "sessionID":"ses_other","messageID":"m1","field":"text","delta":"偷"}});
        let mut t = Some("m1".to_string());
        assert!(fold_event(&ev, "ses_mine", &mut t, &[]).is_none());
    }

    #[test]
    fn fold_锁定基线之后的assistant消息() {
        let ev = json!({"type":"message.updated","properties":{
            "sessionID":"ses_mine","info":{"id":"msg_new","role":"assistant"}}});
        let mut t: Option<String> = None;
        fold_event(&ev, "ses_mine", &mut t, &["msg_old".to_string()]);
        assert_eq!(t.as_deref(), Some("msg_new"));
    }

    #[test]
    fn fold_不把历史消息当本次回答() {
        let ev = json!({"type":"message.updated","properties":{
            "sessionID":"ses_mine","info":{"id":"msg_old","role":"assistant"}}});
        let mut t: Option<String> = None;
        fold_event(&ev, "ses_mine", &mut t, &["msg_old".to_string()]);
        assert!(t.is_none(), "基线内的历史 assistant 消息不该被锁定");
    }

    #[test]
    fn fold_忽略用户消息的delta() {
        // 用户消息没有 target，因此它的 delta 不能进正文（否则会答自己的问题）
        let ev = json!({"type":"message.part.delta","properties":{
            "sessionID":"ses_mine","messageID":"msg_user","field":"text","delta":"提问原文"}});
        let mut t: Option<String> = None;
        assert!(fold_event(&ev, "ses_mine", &mut t, &[]).is_none());
    }

    #[test]
    fn fold_take_text字段的delta() {
        let ev = json!({"type":"message.part.delta","properties":{
            "sessionID":"ses_mine","messageID":"m1","field":"text","delta":"你好"}});
        let mut t = Some("m1".to_string());
        let (d, done) = fold_event(&ev, "ses_mine", &mut t, &[]).unwrap();
        assert_eq!(d.as_deref(), Some("你好"));
        assert!(!done);
    }

    #[test]
    fn fold_idle表示流结束() {
        let ev = json!({"type":"session.idle","properties":{"sessionID":"ses_mine"}});
        let mut t: Option<String> = None;
        let (d, done) = fold_event(&ev, "ses_mine", &mut t, &[]).unwrap();
        assert!(d.is_none());
        assert!(done);
    }

    #[test]
    fn fold_忽略非text字段的delta() {
        let ev = json!({"type":"message.part.delta","properties":{
            "sessionID":"ses_mine","messageID":"m1","field":"reasoning","delta":"思考"}});
        let mut t = Some("m1".to_string());
        assert!(fold_event(&ev, "ses_mine", &mut t, &[]).is_none());
    }

    #[test]
    fn prompt_含问题选区与网址() {
        let p = build_prompt(
            "这段讲了什么",
            Some("这是选中的正文"),
            Some("某篇文章"),
            Some("https://example.com/a"),
        );
        assert!(p.contains("这段讲了什么"));
        assert!(p.contains("这是选中的正文"));
        assert!(p.contains("https://example.com/a"));
        assert!(p.contains("某篇文章"));
        // 边界声明必须在，防模型去动 serve cwd 里的文件
        assert!(p.contains("不要读写任何文件"));
    }

    #[test]
    fn prompt_无选区时明说没选() {
        let p = build_prompt("随便问问", None, None, None);
        assert!(p.contains("用户没有选中任何内容"));
        // 空标题/网址不该留空标签
        assert!(!p.contains("【网址】"));
        assert!(!p.contains("【标题】"));
    }

    #[test]
    fn prompt_空白选区等同没选() {
        let p = build_prompt("问", Some("   \n  "), Some("t"), Some("u"));
        assert!(p.contains("用户没有选中任何内容"));
    }

    #[test]
    fn debug_不泄密码() {
        let c = Credentials {
            username: "abc".into(),
            password: "sup3rsecret".into(),
        };
        let dumped = format!("{c:?}");
        assert!(!dumped.contains("sup3rsecret"));
        assert!(dumped.contains("<redacted>"));
    }

    #[test]
    fn basic_header_是标准base64() {
        let c = Credentials {
            username: "u1".into(),
            password: "p1".into(),
        };
        let h = c.basic_header();
        assert_eq!(
            h,
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(b"u1:p1")
            )
        );
    }
}
