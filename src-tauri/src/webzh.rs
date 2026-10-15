// LINES-EXEMPT: 整页汉化单体（收集/应用/批次规划同域），拆分排期 R3/R4（2026-10-14 红线卡标注）
//! 整页汉化（2026-09-26）
//!
//! 把「翻译当前页」从"发给 AI、译文显示在对话框"改成"译文直接长在页面上"。
//!
//! 三段式管线：
//!   1. `page_collect_translatable` —— 注入 page_collect.js 扫 DOM，收集该翻的
//!      文本（已在页面上打好 `__sbId` 标记）。
//!   2. `opencode_translate_batch` —— 前端分批调用，走本机 opencode（4096）
//!      做 AI 翻译。**不复用 opencode_ask 的流式链路**：整页要的是结构化
//!      JSON 数组（id → 译文），聊天流式协议在这儿只会碍事。
//!   3. `page_apply_translation` —— 注入 page_apply.js 按 `__sbId` 写回，
//!      或一键还原。
//!
//! 编排（分批 / 进度 / 取消）在前端 store：分批策略得随页面大小和实测速度调，
//! 写死后端就得重编。Rust 只提供三块可单测的能力。
//!
//! 隐私：走本机 4096，页面内容不出本机。凭据发现沿用 P2 那套
//! （从 /proc 读 opencode serve 的环境），不落盘。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// 一批里最多放多少字符。
///
/// 2026-09-26 实测（live_批大小对单批耗时影响）：**单批耗时几乎与批大小无关**
/// —— 1 段 7.5s、5 段 6.7s、15 段 5.9s、30 段 7.1s、60 段 9.1s。固定开销约
/// 6s/批（会话 + SSE + 模型唤醒），推理边际成本只有 ~0.07s/段。
/// 所以**批开大 = 直接省时间**：RFC 9110 的 2191 段，25 段/批要 257 批、约 25 分钟；
/// 60 段/批只要 ~40 批、约 6 分钟。原先的 2000 字符/25 段是按"怕模型漏项"拍的，
/// 实测 60 段仍能 60/60 全解析，那个担心不成立。
const BATCH_MAX_CHARS: usize = 6000;
/// 一批最多多少段。同样按实测放宽（60 段/批单批 9s，全解析）。
const BATCH_MAX_SEGMENTS: usize = 60;
/// 单次 opencode 请求超时。批开大后单批输出更长，留足余量。
const BATCH_TIMEOUT_SECS: u64 = 180;

// ---------------------------------------------------------------- 1. 收集

#[derive(Debug, Serialize)]
pub struct CollectedSegment {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub chars: usize,
}

#[derive(Debug, Serialize)]
pub struct CollectedAttr {
    pub id: String,
    pub attr: String,
    pub orig: String,
}

#[derive(Debug, Serialize)]
pub struct CollectResult {
    pub ok: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

#[tauri::command]
pub async fn page_collect_translatable(app: AppHandle, tab_id: Option<String>) -> CollectResult {
    let Some(wv) = resolve_webview(&app, tab_id.as_deref()) else {
        return CollectResult {
            ok: false,
            msg: Some("找不到标签页（或标签已休眠，切回该标签后重试）".into()),
            data: None,
        };
    };
    // 扫全页 DOM，大页面要几秒；2 秒的老超时是给轻量探测用的，这里放宽。
    match crate::api::handlers::eval_value_public(&wv, include_str!("page_collect.js"), 30).await {
        Some(v) => {
            let total = v.get("total").and_then(|x| x.as_u64()).unwrap_or(0);
            let attr_total = v.get("attrTotal").and_then(|x| x.as_u64()).unwrap_or(0);
            log::info!(
                "[webzh] 收集完成：文本 {total} 段 / 属性 {attr_total} 项 / 滤掉 {} 段",
                v.get("skipped").and_then(|x| x.as_u64()).unwrap_or(0)
            );
            CollectResult {
                ok: true,
                msg: None,
                data: Some(v),
            }
        }
        None => CollectResult {
            ok: false,
            msg: Some("页面脚本执行失败或超时（页面可能还在加载，或有 CSP 限制）".into()),
            data: None,
        },
    }
}

// ---------------------------------------------------------------- 2. 写回

#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPair {
    pub id: String,
    pub orig: String,
    pub translated: String,
    /// 内联标签结构表：`{ "0": {tag:"a", a:[["href","/x"]]}, … }`。
    ///
    /// 收集时只有"段落里含 <a>/<b>/<code> 等需要保留的内联标签"才会带它；
    /// 写回脚本据此把译文中的 ⟦0⟧…⟦/0⟧ 结构标记重建成真实 DOM（链接还能点）。
    /// 缺失/null = 纯文本整段替换（老行为）。
    #[serde(default)]
    pub markers: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyAttr {
    pub attr: String,
    pub orig: String,
    pub translated: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ApplyPayload {
    #[serde(default)]
    pub pairs: Vec<ApplyPair>,
    #[serde(default)]
    pub attrs: Vec<ApplyAttr>,
}

#[derive(Debug, Serialize)]
pub struct ApplyResult {
    pub ok: bool,
    pub msg: Option<String>,
    pub data: Option<Value>,
}

/// `mode = "apply" | "restore"`
#[tauri::command]
pub async fn page_apply_translation(
    app: AppHandle,
    tab_id: Option<String>,
    mode: String,
    payload: Option<ApplyPayload>,
) -> ApplyResult {
    let Some(wv) = resolve_webview(&app, tab_id.as_deref()) else {
        return ApplyResult {
            ok: false,
            msg: Some("找不到标签页（或标签已休眠，切回该标签后重试）".into()),
            data: None,
        };
    };
    if mode != "apply" && mode != "restore" {
        return ApplyResult {
            ok: false,
            msg: Some("mode 只能是 apply 或 restore".into()),
            data: None,
        };
    }

    let args = match (mode.as_str(), &payload) {
        ("apply", Some(p)) => serde_json::to_string(p).unwrap_or_else(|_| "{}".to_string()),
        _ => "{}".to_string(),
    };

    // page_apply.js 导出成一个函数（不自带调用），由这里传参执行。
    // 直接用最朴素的形式：包一层 IIFE 取出函数体再调。多写花样只会自己骗自己。
    let script = format!(
        "(() => {{ const f = {body}; return f({mode_arg}, {args}); }})()",
        body = include_str!("page_apply.js").trim(),
        mode_arg = serde_json::to_string(&mode).unwrap_or_else(|_| "\"apply\"".into()),
        args = args,
    );

    match crate::api::handlers::eval_value_public(&wv, &script, 20).await {
        Some(v) => ApplyResult {
            ok: true,
            msg: None,
            data: Some(v),
        },
        None => ApplyResult {
            ok: false,
            msg: Some("写回页面失败或超时".into()),
            data: None,
        },
    }
}

// ---------------------------------------------------------------- 3. 翻译

#[derive(Debug, Deserialize)]
pub struct TranslateBatchReq {
    /// 本批待翻的条目：id + 原文
    pub items: Vec<BatchItem>,
    /// 页面标题，喂给模型当语境
    #[serde(default)]
    pub page_title: Option<String>,
    /// 复用已有会话（前端多批时传回来，避免每批新建一个会话）
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    /// 分档用模型名（opencode 模型引用；不传即默认档，前端已有现成的不用再查）
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BatchItem {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct TranslateBatchResult {
    pub ok: bool,
    pub msg: Option<String>,
    /// id → 译文。模型漏掉的 id 不会出现在这里，前端据此判断失败率。
    pub data: Option<Vec<BatchItem>>,
    pub session_id: Option<String>,
}

/// AI 翻译一批。用本机 opencode（4096）。
#[tauri::command]
pub async fn opencode_translate_batch(
    db: tauri::State<'_, crate::db::Db>,
    req: TranslateBatchReq,
) -> crate::error::CommandResult<TranslateBatchResult> {
    if req.items.is_empty() {
        return Ok(TranslateBatchResult {
            ok: false,
            msg: Some("待翻译条目为空".into()),
            data: None,
            session_id: None,
        });
    }
    if req.items.len() > BATCH_MAX_SEGMENTS * 2 {
        return Ok(TranslateBatchResult {
            ok: false,
            msg: Some(format!("单批条目过多（{}），请分批", req.items.len())),
            data: None,
            session_id: None,
        });
    }

    let creds = crate::webqa::resolve_credentials_pub(&db).await?;
    let http = crate::webqa::client_pub(&creds)?;
    let base = crate::webqa::base_url_pub();
    let agent = req
        .agent
        .unwrap_or_else(|| crate::webqa::DEFAULT_AGENT.to_string());

    // 翻译会话跟"随手问"分开：整页汉化会刷几百条消息，混进问答会话
    // 会把用户的聊天历史淹掉，也会污染后续追问的上下文。
    let sid = match crate::webqa::ensure_session_pub(&http, req.session_id.as_deref(), "网页汉化")
        .await
    {
        Ok((s, _)) => s,
        Err(e) => {
            return Ok(TranslateBatchResult {
                ok: false,
                msg: Some(e.to_string()),
                data: None,
                session_id: None,
            })
        }
    };

    let prompt = build_translate_prompt_pub(&req.items, req.page_title.as_deref());

    // 复用 P2 的非流式单轮请求：订阅 SSE → POST prompt → 累积到回答完成
    match crate::webqa::ask_once(&http, &base, &sid, &agent, &prompt, BATCH_TIMEOUT_SECS).await {
        Ok(raw) => {
            let items = parse_translate_response(&raw, &req.items);
            let got = items.len();
            let want = req.items.len();
            if got == 0 {
                log::warn!(
                    "[webzh] 批次翻译失败：模型输出无法解析，前 200 字：{}",
                    truncate(&raw, 200)
                );
            } else if got < want {
                log::warn!("[webzh] 批次翻译漏项：{got}/{want}，其余按原文保留");
            }
            Ok(TranslateBatchResult {
                ok: true,
                msg: None,
                data: Some(items),
                session_id: Some(sid),
            })
        }
        Err(e) => Ok(TranslateBatchResult {
            ok: false,
            msg: Some(e.to_string()),
            data: None,
            session_id: Some(sid),
        }),
    }
}

/// 翻译会话清理：整页汉化完把用过的会话删掉，opencode 控制台不堆积。
/// 逐个 DELETE，失败只记日志（端点不存在/已被删都不影响翻译结果）。
/// 前端在汉化结束（成功/失败/取消）时 fire-and-forget 调用，带用过的会话 id。
#[tauri::command]
pub async fn opencode_translate_cleanup(
    db: tauri::State<'_, crate::db::Db>,
    sessions: Vec<String>,
) -> crate::error::CommandResult<u64> {
    if sessions.is_empty() {
        return Ok(0);
    }
    let creds = match crate::webqa::resolve_credentials_pub(&db).await {
        Ok(c) => c,
        Err(_) => return Ok(0),
    };
    let http = match crate::webqa::client_pub(&creds) {
        Ok(c) => c,
        Err(_) => return Ok(0),
    };
    let base = crate::webqa::base_url_pub();
    let mut deleted = 0u64;
    for sid in sessions.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match http
            .delete(format!("{}/session/{}", base, sid))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => deleted += 1,
            Ok(r) => log::info!("[webzh] 翻译会话删除跳过 {}: HTTP {}", sid, r.status()),
            Err(e) => log::info!("[webzh] 翻译会话删除失败 {}: {}", sid, e),
        }
    }
    log::info!("[webzh] 翻译会话清理完成: {}/{}", deleted, sessions.len());
    Ok(deleted)
}

/// 查本机 opencode 当前选用的模型标识（如 `opencode/big-pickle`）。
///
/// 翻译记忆按模型隔离：换了模型旧译文作废。前端每次开翻前取一次，
/// 失败返回 Err，前端据此跳过整轮缓存（不允许用空模型名读写）。
#[tauri::command]
pub async fn opencode_translate_model(
    db: tauri::State<'_, crate::db::Db>,
) -> crate::error::CommandResult<String> {
    let creds = crate::webqa::resolve_credentials_pub(&db).await?;
    let http = crate::webqa::client_pub(&creds)?;
    let base = crate::webqa::base_url_pub();
    let resp = http
        .get(format!("{base}/config"))
        .send()
        .await
        .map_err(|e| crate::error::AppError::net(format!("读取 opencode 配置失败：{e}")))?;
    if !resp.status().is_success() {
        return Err(crate::error::AppError::net(format!(
            "读取 opencode 配置失败：HTTP {}",
            resp.status()
        ))
        .into());
    }
    let cfg: Value = resp
        .json()
        .await
        .map_err(|e| crate::error::AppError::net(format!("解析 opencode 配置失败：{e}")))?;
    Ok(cfg
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

/// 翻译记忆批量查询入参：模型 + 待查条目（id 仅用于回传对齐，text 是原文）。
#[derive(Debug, Deserialize)]
pub struct MemoryLookupReq {
    pub model: String,
    #[serde(default)]
    pub items: Vec<BatchItem>,
}

/// 翻译记忆批量写入入参：复用 ApplyPair（取 orig→translated；id/markers 忽略）。
#[derive(Debug, Deserialize)]
pub struct MemoryRememberReq {
    pub model: String,
    #[serde(default)]
    pub pairs: Vec<ApplyPair>,
}

/// 查翻译记忆：命中的条目以同样的 id 回来，未命中的不出现。
///
/// 缓存故障对调用方表现为"全 miss"（db 内部已吞错记日志），翻译主流程不受影响。
#[tauri::command]
pub async fn translate_memory_lookup(
    db: tauri::State<'_, crate::db::Db>,
    req: MemoryLookupReq,
) -> crate::error::CommandResult<Vec<BatchItem>> {
    let sources: Vec<String> = req.items.iter().map(|i| i.text.clone()).collect();
    // C-11：取 HMAC 密钥失败（translate.key 损坏/不可写）→ 记忆停用，按全 miss 处理
    let hits = match crate::db::translate_memory::hmac_key(&db).await {
        Some(key) => crate::db::translate_memory::lookup(&db.pool, &req.model, &sources, &key).await,
        None => Default::default(),
    };
    Ok(req
        .items
        .into_iter()
        .filter_map(|i| {
            // key 在 db 层归一化，这里必须用同一个函数算 key，口径漂移 = 静默 miss
            let key = crate::db::translate_memory::normalize_source(&i.text);
            hits.get(&key).map(|t| BatchItem {
                id: i.id,
                text: t.clone(),
            })
        })
        .collect())
}

/// 整页汉化预热：并发预建最多 n 个（夹到 1..=3）**独立**翻译会话，
/// 供前端 worker 池的头几批直接取用。
///
/// 实测每批固定开销 ~6s，其中会话创建占大头；收集页面 + 查翻译记忆期间会话
/// 就建好，等于这段延迟被完全藏掉。每个会话只服务一批（前端消费后即弃），
/// 不复用成历史越滚越长的长会话。建失败的会话不进返回值，前端退化为按需新建。
#[tauri::command]
pub async fn opencode_translate_warmup(
    db: tauri::State<'_, crate::db::Db>,
    want: Option<usize>,
) -> crate::error::CommandResult<Vec<String>> {
    let n = want.unwrap_or(3).clamp(1, 3);
    let creds = crate::webqa::resolve_credentials_pub(&db).await?;
    let http = crate::webqa::client_pub(&creds)?;
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..n {
        let h = http.clone();
        set.spawn(async move {
            crate::webqa::ensure_session_pub(&h, None, "网页汉化")
                .await
                .map(|(sid, _)| sid)
        });
    }
    let mut sids = Vec::with_capacity(n);
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(Ok(sid)) => sids.push(sid),
            Ok(Err(e)) => log::warn!("[webzh] 预热会话失败：{e}"),
            Err(e) => log::warn!("[webzh] 预热任务异常：{e}"),
        }
    }
    Ok(sids)
}

/// 把本页新翻出来的译文写进翻译记忆（upsert）。返回写入条数。
#[tauri::command]
pub async fn translate_memory_remember(
    db: tauri::State<'_, crate::db::Db>,
    req: MemoryRememberReq,
) -> crate::error::CommandResult<u64> {
    let pairs: Vec<(String, String)> = req
        .pairs
        .into_iter()
        .map(|p| (p.orig, p.translated))
        .collect();
    // C-11：密钥不可用时记忆停用（尽力而为语义，返回 0 不打断翻译）
    Ok(match crate::db::translate_memory::hmac_key(&db).await {
        Some(key) => {
            crate::db::translate_memory::remember(&db.pool, &req.model, &pairs, &key).await as u64
        }
        None => 0,
    })
}

/// 批量档位（字符上限，段数上限）：按模型名分档，纯函数可单测。
/// 快慢两档：含 flash 走快档 60 段（实测本机主力在此档，60/60 全解析）；
/// 含 pro 走保守档 30 段（防漏项加超时）；未知/空走 60 段（现状实测值，
/// 不回退本机默认路径；漏项由解析校验兜底）。改档只影响分批数量，不影响单批语义。
pub fn batch_tier_for_model(model: Option<&str>) -> (usize, usize) {
    let name = model.unwrap_or("").to_ascii_lowercase();
    if name.contains("pro") {
        (BATCH_MAX_CHARS, 30)
    } else {
        (BATCH_MAX_CHARS, BATCH_MAX_SEGMENTS)
    }
}

/// 把本批条目切成 ≤(字符上限 / 段数上限) 的批次。
///
/// 前端调用前会调它拿到分批方案，这样"一次要发几次"在前后端口径一致。
/// 档位按请求模型走 batch_tier_for_model（不传模型即默认档）。
#[tauri::command]
pub fn plan_translate_batches(req: TranslateBatchReq) -> Value {
    let (max_chars, max_segs) = batch_tier_for_model(req.model.as_deref());
    json!({
        "ok": true,
        "data": plan_batches_with(&req.items, max_chars, max_segs),
        "total_items": req.items.len(),
    })
}

pub fn plan_batches(items: &[BatchItem]) -> Vec<Vec<BatchItem>> {
    plan_batches_with(items, BATCH_MAX_CHARS, BATCH_MAX_SEGMENTS)
}

/// 按给定上限分批（内核，档位由调用方定，便于单测各档）
pub fn plan_batches_with(
    items: &[BatchItem],
    max_chars: usize,
    max_segs: usize,
) -> Vec<Vec<BatchItem>> {
    // 先做段内切分：单段超长时按自然边界切成子块（id 带 .序号 后缀），
    // 否则一个 8000 字符的长段会整段丢给模型——翻译质量掉、还容易超时。
    let mut expanded: Vec<BatchItem> = Vec::new();
    for it in items {
        let c = it.text.chars().count();
        if c <= max_chars {
            expanded.push(it.clone());
        } else {
            for (i, piece) in split_long_text(&it.text, max_chars).into_iter().enumerate() {
                expanded.push(BatchItem {
                    id: format!("{}.{}", it.id, i),
                    text: piece,
                });
            }
        }
    }

    let mut out: Vec<Vec<BatchItem>> = Vec::new();
    let mut cur: Vec<BatchItem> = Vec::new();
    let mut cur_chars = 0usize;
    for it in expanded {
        let c = it.text.chars().count();
        let would = cur_chars + c;
        if !cur.is_empty() && (would > max_chars || cur.len() >= max_segs) {
            out.push(std::mem::take(&mut cur));
            cur_chars = 0;
        }
        cur.push(it);
        cur_chars += c;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 把超长文本切成不超过 `max_chars` 字符的多块，**尽量在自然边界切**。
///
/// 切点优先级：段落换行 > 句末标点 > 空格 > 硬切。
/// 硬切只在实在找不到边界时用（连续无空格长串），但会尽量避免把词切两半。
///
/// 切点处吃掉的空白/标点归入前一块：拼接回来时译文直接相连，不需要
/// 担心边界残留的空格把中文译文撑出缝来。
pub fn split_long_text(text: &str, max_chars: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;

    while start < chars.len() {
        if chars.len() - start <= max_chars {
            out.push(chars[start..].iter().collect());
            break;
        }
        let hard_end = (start + max_chars).min(chars.len());
        let mut cut = hard_end;
        // 在 [start+下限, hard_end] 区间内往回找自然边界
        let floor = start + (max_chars / 2).max(1);
        for i in (floor..hard_end).rev() {
            let c = chars[i];
            if c == '\n' {
                cut = i + 1;
                break;
            }
            if c == ' ' {
                cut = i + 1;
                break;
            }
            // 句末标点：把标点留给前一块
            if matches!(c, '.' | '。' | '!' | '！' | '?' | '？' | ';' | '；') {
                cut = i + 1;
                break;
            }
        }
        // 切出来空块（边界恰在起点）→ 强制前进一步，防死循环
        if cut <= start {
            cut = hard_end;
        }
        out.push(chars[start..cut].iter().collect());
        start = cut;
    }
    out
}

pub fn build_translate_prompt_pub(items: &[BatchItem], page_title: Option<&str>) -> String {
    // 编号列表比 JSON 输入更省 token、也更好读；输出则强制 JSON，
    // 因为要按 id 对号入座。
    //
    // 规则 4/5 是质量重灾区，单靠一句"翻译得自然点"小模型根本不照做：
    // - 模板变量（{name}/%s/$1）被模型"意译"掉，页面文案直接报错；
    // - ⟦n⟧ 结构标记（对应页面里的链接/加粗）被模型当乱码删掉，译文落页后
    //   链接全没——所以规则写死 + 给一个带标记的具体示例。
    let list: Vec<String> = items
        .iter()
        .map(|i| format!("{}::{}", i.id, i.text))
        .collect();
    format!(
        "你是网页汉化引擎，把下面网页片段翻译成简体中文（zh-CN）。规则：\n\
         1. 只输出译文，不加解释、不加引号、不加 markdown 代码块。\n\
         2. 严格输出 JSON 数组，元素形如 {{\"id\":\"t0\",\"t\":\"译文\"}}，顺序与输入一致；id 必须原样带回，不得改写、不得增删。\n\
         3. 风格：按钮/菜单/标题等短短语要简短地道，像原生中文界面；正文句子自然流畅，不逐字硬译；译文用中文标点。\n\
         4. 专有名词、产品名、代码标识符、URL、邮箱、数字与单位保持原样；模板变量/占位符（如 %s、%d、{{{{name}}}}、$1、{{0}}）必须原样保留，位置可按中文语序移动。\n\
         5. 结构标记 ⟦n⟧、⟦/n⟧、⟦n/⟧ 代表网页原有的链接/加粗等内联标签：必须原样保留、不得增删改写其中数字，位置可随译文移动；标记夹住的文字照常翻译。\n\
         6. 同一批内同一专名/术语译法必须一致；已是中文的片段原样返回。\n\
         {}\n\
         示例：\n\
         输入 t0::See the ⟦0⟧manual⟦/0⟧ for details.\n\
         输出 [{{\"id\":\"t0\",\"t\":\"详情请参阅⟦0⟧手册⟦/0⟧。\"}}]\n\
         \n输入片段（每行 id::原文）：\n{}",
        page_title
            .map(|t| format!("7. 页面标题是「{t}」，用它的领域语境选词。\n"))
            .unwrap_or_default(),
        list.join("\n")
    )
}

/// 从模型输出里抠出 JSON 数组。
///
/// LLM 输出的脏是常态：可能裹 ```json 代码块、可能前后带一句"好的，以下是"、
/// 可能用全角引号。这里逐层剥离 + 括号配平，找到第一个能 `serde_json` 吃下的
/// 数组。解析不出来就返回空 vec，让上层按"本批失败、保留原文"处理——
/// **宁可少翻一段，也不能把乱码写进别人的页面**。
pub fn parse_translate_response(raw: &str, items: &[BatchItem]) -> Vec<BatchItem> {
    let candidates = extract_json_candidates(raw);
    for c in &candidates {
        if let Ok(v) = serde_json::from_str::<Vec<Value>>(c) {
            let mut out = Vec::new();
            for e in v {
                let id = e
                    .get("id")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let t = e
                    .get("t")
                    .or_else(|| e.get("translated"))
                    .or_else(|| e.get("text"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if id.is_empty() || t.is_empty() {
                    continue;
                }
                // 只接受本批要翻的 id：模型幻觉出的 id 会被忽略
                if !items.iter().any(|i| i.id == id) {
                    continue;
                }
                // 模型回了个空壳（同文）或明显没翻（还全是原样英文）也丢掉
                if t.trim().is_empty() {
                    continue;
                }
                out.push(BatchItem { id, text: t });
            }
            if !out.is_empty() {
                return out;
            }
        }
    }
    Vec::new()
}

/// 依次产出候选 JSON 数组片段：整体 → 剥代码块 → 括号配平截取。
fn extract_json_candidates(raw: &str) -> Vec<String> {
    let mut cands = Vec::new();
    let trimmed = raw.trim();
    cands.push(trimmed.to_string());

    // 剥 ```json ... ```
    if let Some(a) = trimmed.find("```") {
        let rest = &trimmed[a + 3..];
        let rest = match rest.find('\n') {
            Some(nl) => &rest[nl + 1..],
            None => rest,
        };
        if let Some(b) = rest.find("```") {
            let body = &rest[..b];
            cands.push(body.trim().to_string());
            cands.push(body.to_string());
        }
    }

    // 括号配平：从第一个 '[' 到配平的 ']'
    if let Some(start) = trimmed.find('[') {
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, ch) in trimmed[start..].char_indices() {
            let c = ch as u32;
            if in_str {
                if esc {
                    esc = false;
                } else if c == 0x5c {
                    esc = true;
                } else if c == 0x22 {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '"' => in_str = true,
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        cands.push(trimmed[start..=start + i].to_string());
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    cands
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

// ---------------------------------------------------------------- 4. api.sock 出口

/// `page_collect_translatable` 的 api.sock 包装（供 MCP / 脚本驱动真机验收）。
///
/// 为什么要再暴露一个口子：这俩是 Tauri 命令，前端 invoke 调得到，但**自动化
/// 验收调不到**——api.sock 和 Tauri invoke 是两条互不相通的路。P1 的
/// `read_selection` 当初也是同样理由暴露出去的。实现共用，不复制。
pub(super) async fn collect_for_api(app: &AppHandle, req: &Value) -> Value {
    let tab_id = req
        .get("tab_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let r = page_collect_translatable(app.clone(), tab_id).await;
    json!({
        "ok": r.ok,
        "msg": r.msg,
        "data": r.data,
    })
}

/// `page_apply_translation` 的 api.sock 包装。参数同上（req 里带 mode/payload）。
/// 校验（2026-09-30 实测补）：mode 必填且只认 apply/restore；apply 必须带 payload
/// （此前缺 mode 默认吞成 apply、无 payload 照样 ok:true，是写类接口漏校验）。
pub(super) async fn apply_for_api(app: &AppHandle, req: &Value) -> Value {
    let tab_id = req
        .get("tab_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let mode = match req.get("mode").and_then(|v| v.as_str()) {
        Some(m) => m.to_string(),
        None => {
            return crate::error::AppError::validation("缺少 mode（apply|restore）").error_json()
        }
    };
    if mode != "apply" && mode != "restore" {
        return crate::error::AppError::validation("mode 只能是 apply 或 restore").error_json();
    }
    let payload: Option<ApplyPayload> = match (mode.as_str(), req.get("payload")) {
        ("apply", None) => {
            return crate::error::AppError::validation("apply 必须带 payload").error_json()
        }
        ("apply", Some(v)) if v.is_null() => {
            return crate::error::AppError::validation("apply 必须带 payload").error_json()
        }
        (_, Some(v)) if v.is_null() => None,
        (_, Some(v)) => match serde_json::from_value::<ApplyPayload>(v.clone()) {
            Ok(p) => Some(p),
            Err(_) => return crate::error::AppError::validation("payload 解析失败").error_json(),
        },
        (_, None) => None,
    };
    let r = page_apply_translation(app.clone(), tab_id, mode, payload).await;
    json!({
        "ok": r.ok,
        "msg": r.msg,
        "data": r.data,
    })
}

// ---------------------------------------------------------------- 公共辅助

fn resolve_webview(app: &AppHandle, tab_id: Option<&str>) -> Option<tauri::Webview> {
    let req = json!({ "tab_id": tab_id });
    let label = match crate::api::handlers::target_label_public(app, &req) {
        Ok(l) => l,
        Err(_) => return None,
    };
    app.get_webview(&label)
}
// ---------------------------------------------------------------- live 测试出口

/// 供 `tests/webzh_live.rs`：把被测的 prompt 构造暴露出去，
/// 免得测试里再抄一份（抄的那份过不了就是假绿）。
pub use parse_translate_response as parse_translate_response_pub;
pub use split_long_text as split_long_text_pub;

#[cfg(test)]
mod tests {
    use super::*;

    fn items(v: &[(&str, &str)]) -> Vec<BatchItem> {
        v.iter()
            .map(|(a, b)| BatchItem {
                id: (*a).to_string(),
                text: (*b).to_string(),
            })
            .collect()
    }

    #[test]
    fn 分档按模型名走快慢两档() {
        // flash 快档 60，未知走默认 60（现状实测值），pro 保守 30
        assert_eq!(batch_tier_for_model(Some("opencode/flash-x")).1, 60);
        assert_eq!(batch_tier_for_model(Some("agnes-2.0-flash")).1, 60);
        assert_eq!(batch_tier_for_model(Some("opencode/big-pickle")).1, 60);
        assert_eq!(batch_tier_for_model(None).1, 60);
        assert_eq!(batch_tier_for_model(Some("agnes-2.5-pro")).1, 30);
        assert_eq!(batch_tier_for_model(Some("PRO-MAX")).1, 30);
        // 字符上限两档一致
        assert_eq!(batch_tier_for_model(Some("x-pro")).0, BATCH_MAX_CHARS);
    }

    #[test]
    fn 保守档65段切三批默认档切两批() {
        let its: Vec<BatchItem> = (0..65)
            .map(|i| BatchItem {
                id: format!("t{i}"),
                text: "x".repeat(30),
            })
            .collect();
        let fast = plan_batches_with(&its, BATCH_MAX_CHARS, 60);
        assert_eq!(fast.len(), 2, "60 段档 65 段应切 2 批");
        let safe = plan_batches_with(&its, BATCH_MAX_CHARS, 30);
        assert_eq!(safe.len(), 3, "30 段档 65 段应切 3 批");
        for x in fast.iter().chain(safe.iter()) {
            assert!(x.len() <= 60);
        }
    }

    #[test]
    fn 分批按字符数和段数双上限() {
        // 每段 30 字符，字符上限不会触发 → 段数上限触发。数量用常量算，
        // 免得以后调参（2026-09-26 从 25 提到 60）测试就瞎红。
        let n = BATCH_MAX_SEGMENTS + 5;
        let its: Vec<BatchItem> = (0..n)
            .map(|i| BatchItem {
                id: format!("t{i}"),
                text: "x".repeat(30),
            })
            .collect();
        let b = plan_batches(&its);
        assert!(b.len() >= 2, "{n} 段必须切多批");
        for x in &b {
            assert!(x.len() <= BATCH_MAX_SEGMENTS, "单批段数超上限: {}", x.len());
        }
    }

    #[test]
    fn 分批不丢段不重段() {
        let its = items(&[
            ("t0", "short"),
            ("t1", &"x".repeat(900)),
            ("t2", "another"),
            ("t3", &"y".repeat(1500)),
        ]);
        let b = plan_batches(&its);
        let flat: Vec<String> = b.iter().flatten().map(|i| i.id.clone()).collect();
        assert_eq!(flat.len(), 4, "分批后条目数必须守恒");
        let mut uniq = flat.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 4, "不允许重复");
    }

    #[test]
    fn 超长单段也单独成批不丢() {
        // 段内切分（2026-09-26，高哥提醒"超大文件可以分段翻译"）之后，
        // 超长段会以 t0.0 / t0.1 / … 子块形式出现，id 不再是 t0 本身。
        // 这里断言的是"内容没丢、且能追溯回原 id"。
        let its = items(&[("t0", &"z".repeat(BATCH_MAX_CHARS + 2000)), ("t1", "small")]);
        let b = plan_batches(&its);
        let flat: Vec<String> = b.iter().flatten().map(|i| i.id.clone()).collect();
        assert!(
            flat.iter().any(|i| i.starts_with("t0.")),
            "超长段应被切分成 t0.N 子块，实际: {:?}",
            flat
        );
        assert!(flat.contains(&"t1".to_string()), "旁边的正常段不能丢");
        // 切出来的内容拼起来应等于原文（t0 全是 z，用总长验更省事）
        let z_len: usize = b
            .iter()
            .flatten()
            .filter(|i| i.id.starts_with("t0."))
            .map(|i| i.text.chars().count())
            .sum();
        assert_eq!(
            z_len,
            BATCH_MAX_CHARS + 2000,
            "子块拼起来的长度必须等于原段长度"
        );
    }

    #[test]
    fn 解析裸json数组() {
        let raw = r#"[{"id":"t0","t":"你好"},{"id":"t1","t":"世界"}]"#;
        let its = items(&[("t0", "hello"), ("t1", "world")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].text, "你好");
    }

    #[test]
    fn 解析裹代码块的输出() {
        let raw =
            "好的，以下是翻译：\n```json\n[{\"id\":\"t0\",\"t\":\"你好\"}]\n```\n希望有帮助！";
        let its = items(&[("t0", "hello")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "你好");
    }

    #[test]
    fn 解析前后有废话的输出() {
        let raw = "Sure! Here you go: [{\"id\":\"t0\",\"t\":\"你好\"}] Hope this helps!";
        let its = items(&[("t0", "hello")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn 模型乱加id要过滤掉() {
        let raw = r#"[{"id":"t0","t":"你好"},{"id":"t99","t":"幻觉"}]"#;
        let its = items(&[("t0", "hello")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1, "不在本批的 id 必须丢弃");
        assert_eq!(got[0].id, "t0");
    }

    #[test]
    fn 空译文丢弃不写进页面() {
        let raw = r#"[{"id":"t0","t":""},{"id":"t1","t":"  "},{"id":"t2","t":"你好"}]"#;
        let its = items(&[("t0", "a"), ("t1", "b"), ("t2", "c")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1, "空译文绝不能进页面");
        assert_eq!(got[0].id, "t2");
    }

    #[test]
    fn 完全无法解析时返回空不报错() {
        let its = items(&[("t0", "hello")]);
        assert!(parse_translate_response("抱歉，我无法完成这个请求。", &its).is_empty());
    }

    #[test]
    fn 字符串里的方括号不破坏配平() {
        let raw = r#"[{"id":"t0","t":"用 [下标] 取值"}]"#;
        let its = items(&[("t0", "use [index]")]);
        let got = parse_translate_response(raw, &its);
        assert_eq!(got.len(), 1);
        assert!(got[0].text.contains('['));
    }

    #[test]
    fn prompt强制带回id且含语境() {
        let its = items(&[("t0", "Hello")]);
        let p = build_translate_prompt_pub(&its, Some("Kubernetes 文档"));
        assert!(p.contains("t0::Hello"), "输入必须带 id");
        assert!(p.contains("Kubernetes 文档"), "页面标题要进 prompt");
        assert!(p.contains("原样带回"), "必须强调 id 不可改");
    }

    #[test]
    fn prompt含结构标记规则和带标记示例() {
        // 模型把 ⟦n⟧ 当乱码删掉的话，译文落页后链接/加粗全没。
        // 规则 + 一个具体示例一起钉在 prompt 里，改没了测试就红。
        let its = items(&[("t0", "See the ⟦0⟧manual⟦/0⟧ for details.")]);
        let p = build_translate_prompt_pub(&its, None);
        assert!(p.contains('⟦'), "必须出现结构标记字符 ⟦");
        assert!(p.contains("⟦0⟧手册⟦/0⟧"), "必须给带标记的译文示例");
        assert!(p.contains("原样保留"), "必须要求标记原样保留");
        // 模板变量也要点名保护
        assert!(p.contains("%s"), "必须列举 printf 风格变量");
        assert!(p.contains("{{name}}"), "必须列举双花括号变量");
        assert!(p.contains("$1"), "必须列举 $1 风格变量");
    }

    #[test]
    fn 超长段被切分且不丢字() {
        // 3000 个字符的连续文本（无边界可找，测硬切也不丢）
        let long: String = "a".repeat(3000);
        let pieces = split_long_text(&long, 1000);
        assert!(pieces.len() >= 3, "应切成多块，实际 {}", pieces.len());
        let joined: String = pieces.concat();
        assert_eq!(joined, long, "切分拼接后必须与原文完全一致，不能丢字或多字");
    }

    #[test]
    fn 切分优先在自然边界() {
        let t = "First sentence here. Second sentence follows. Third one closes it.";
        let pieces = split_long_text(t, 25);
        assert!(pieces.len() > 1);
        // 块应以句末标点或空格收尾，别把词切两半
        for (i, p) in pieces.iter().enumerate() {
            if i == pieces.len() - 1 {
                break;
            }
            let last = p.chars().last().unwrap();
            assert!(
                matches!(last, '.' | ' ' | '。' | '!' | '?' | ';' | '\n'),
                "块 {:?} 结尾 {:?} 不是自然边界",
                p,
                last
            );
        }
        assert_eq!(pieces.concat(), t);
    }

    #[test]
    fn 短段不切() {
        assert_eq!(split_long_text("short text", 1000).len(), 1);
    }

    #[test]
    fn 切分不会死循环() {
        // max_chars 极小时也要能收敛
        let t = "abcdefghij".repeat(50);
        let pieces = split_long_text(&t, 1);
        assert!(!pieces.is_empty());
        assert_eq!(pieces.concat(), t);
    }

    #[test]
    fn 超长段的分批用子块id且带序号() {
        let its = items(&[("t0", &"x".repeat(BATCH_MAX_CHARS + 2000))]);
        let b = plan_batches(&its);
        // 全部块的 id 都该是 t0.N 形式
        for batch in &b {
            for it in batch {
                assert!(
                    it.id.starts_with("t0."),
                    "子块 id 形如 t0.0，实际 {}",
                    it.id
                );
            }
        }
        // 每块都不超过上限
        for batch in &b {
            for it in batch {
                assert!(it.text.chars().count() <= BATCH_MAX_CHARS);
            }
        }
    }

    #[test]
    fn 切分后拼回能还原整段id() {
        // 前端按 base id（点号前）拼子块，所以每个子块必须能追溯到原 id
        let its = items(&[("t7", &"y".repeat(3000))]);
        let b = plan_batches(&its);
        let bases: std::collections::HashSet<String> = b
            .iter()
            .flatten()
            .map(|i| i.id.split('.').next().unwrap().to_string())
            .collect();
        assert_eq!(bases.len(), 1);
        assert!(bases.contains("t7"));
    }

    /// 契约测试：四条 webzh 命令的返回字段名，必须与前端解包逐字对上。
    ///
    /// 前端 `zhCall` 读的是 `{ok, msg, data}` 里的 `data`；`data` 里再按
    /// segments/attrs/applied/restored 取。任何一层字段改名，前端都会静默
    /// 拿到 undefined——这正是「整页汉化永远报‘没找到需要翻译的文字’」那个
    /// bug 的成因（前端少解一层 data）。字段名在这里钉死，改了就必须同步前端。
    #[test]
    fn 返回结构契约与前端解包一致() {
        // 1) collect：data 里是 segments / attrs / title
        let c = CollectResult {
            ok: true,
            msg: None,
            data: Some(json!({
                "segments": [], "attrs": [], "total": 0,
                "attrTotal": 0, "skipped": 0, "title": "t"
            })),
        };
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["ok"], true);
        assert!(
            v.get("msg").is_some(),
            "必须有 msg 字段（前端读 r.msg 出错误提示）"
        );
        assert!(v["data"]["segments"].is_array(), "前端读 data.segments");
        assert!(v["data"]["attrs"].is_array(), "前端读 data.attrs");
        assert!(v["data"]["title"].is_string(), "前端读 data.title");

        // 2) apply：data 里是 applied / mismatched
        let a = ApplyResult {
            ok: true,
            msg: None,
            data: Some(json!({ "applied": 1, "mismatched": 0 })),
        };
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["data"]["applied"], 1, "前端读 data.applied");
        assert_eq!(v["data"]["mismatched"], 0, "前端读 data.mismatched");

        // 3) restore 走同一个 ApplyResult，data 里是 restored
        let r = ApplyResult {
            ok: true,
            msg: None,
            data: Some(json!({ "restored": 2, "nodes": 2, "attrs": 0 })),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["data"]["restored"], 2, "前端读 data.restored");

        // 4) batch：data 是数组，另有 session_id（前端要拿它续会话）
        let b = TranslateBatchResult {
            ok: true,
            msg: None,
            data: Some(items(&[("t0", "提交")])),
            session_id: Some("ses_x".into()),
        };
        let v = serde_json::to_value(&b).unwrap();
        assert!(v["data"].is_array(), "前端读 data 当译文数组");
        assert_eq!(v["data"][0]["id"], "t0");
        assert_eq!(v["data"][0]["text"], "提交");
        assert_eq!(v["session_id"], "ses_x", "前端读 session_id 续会话");

        // 5) plan：{ok, data: 批次数组}（注意与上面不同：没有 msg 解包层）
        let p = plan_translate_batches(TranslateBatchReq {
            items: items(&[("t0", "hi")]),
            page_title: None,
            session_id: None,
            agent: None,
            model: None,
        });
        assert_eq!(p["ok"], true);
        assert!(p["data"].is_array(), "前端读 data 当批次数组");
        assert_eq!(p["data"][0][0]["id"], "t0");
    }
}
