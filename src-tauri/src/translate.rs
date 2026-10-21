// LINES-EXEMPT: 翻译引擎主流程：引擎选择、批量切分、重试与记忆回写共用一个请求上下文
//! 翻译模块（Google 免费接口，免 API key）
//!
//! 右键菜单的"翻译选中文字 / 翻译整个页面"通过 `shift://__translate__/...`
//! 协议调用本模块（lib.rs 处理器）。设计要点：
//! - `client=dict-chrome-ex` 是 Chrome 内置翻译同款端点，免 key，
//!   但国内可能受网络限制（必要时切 Bing 或配代理）
//! - 同步 block_on 调用——shift handler 是同步闭包，但 reqwest 异步
//!   不能直接 block——用 tauri::async_runtime::block_on 把 async 拉成同步
//! - 整页翻译分两步：先 eval JS 收集文本节点 JSON，再 spawn 异步翻译后 eval 写回

use crate::error::CommandError;
use reqwest::Client;
use serde::Serialize;
use std::sync::OnceLock;

/// Chrome UA（Google translate 端点需要 Chrome 标识）
const CHROME_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
  (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

/// reqwest 客户端（共享连接池，全局单例）
/// 全局超时只做兜底（120s），实际超时由每个请求的 RequestBuilder::timeout() 控制，
/// 值来自设置 translate_timeout（默认 15 秒，可在设置界面调整）
static CLIENT: OnceLock<Client> = OnceLock::new();

/// reqwest 客户端（共享连接池，全局单例）
/// 初始化失败不崩进程：降级用默认客户端（请求级 timeout 照常生效）
fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(CHROME_UA)
            .timeout(std::time::Duration::from_secs(
                crate::contracts::timeout::TRANSLATE_SECS,
            ))
            // 建连上限（单边地址族卡住时快速让路给另一族，见 contracts::timeout::CONNECT_TIMEOUT_SECS）
            .connect_timeout(std::time::Duration::from_secs(
                crate::contracts::timeout::CONNECT_TIMEOUT_SECS,
            ))
            .build()
            .unwrap_or_else(|e| {
                log::error!(
                    "[translate] reqwest client 初始化失败，用默认客户端兜底: {}",
                    e
                );
                Client::new()
            })
    })
}

/// 发送失败分类（纯函数，可单测）：reqwest 超时 → E_TIMEOUT，其余 → E_NET
/// （调用方据此分流"可重试"；超时不得降级成通用网络错误）
fn classify_send(is_timeout: bool, message: String) -> CommandError {
    CommandError {
        code: if is_timeout {
            crate::error::E_TIMEOUT
        } else {
            crate::error::E_NET
        },
        message,
    }
}

/// 请求发送失败（带 reqwest 错误）：按是否超时分流入码
fn send_failure(op: &str, e: &reqwest::Error) -> CommandError {
    classify_send(e.is_timeout(), format!("{}: {}", op, e))
}

/// 上游返回异常（非 2xx / 结构解析失败 / 页面参数抓取失败）→ E_NET
fn upstream_failure(message: impl Into<String>) -> CommandError {
    CommandError {
        code: crate::error::E_NET,
        message: message.into(),
    }
}

/// 当前翻译超时（秒）：settings.translate_timeout，默认 15，合法范围 5-120
pub async fn current_timeout(db: &crate::db::Db) -> u64 {
    let row: Option<(String,)> = crate::db::settings_cache::get(&db.pool, "translate_timeout")
        .await
        .map(|v| (v,));
    row.and_then(|r| r.0.parse::<u64>().ok())
        .filter(|v| *v >= 5 && *v <= 120)
        .unwrap_or(15)
}

/// Google 翻译（Chrome dict-chrome-ex 公共端点，无需 API key）
///
/// `target` 默认 zh-CN；源语言自动检测（sl=auto）；`timeout` 秒（设置里可调）
pub async fn translate_google(
    text: &str,
    target: &str,
    timeout: u64,
) -> Result<String, CommandError> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let url = "https://translate.googleapis.com/translate_a/single";
    let resp = client()
        .get(url)
        .query(&[
            ("client", "dict-chrome-ex"),
            ("sl", "auto"),
            ("tl", target),
            ("dt", "t"),
            ("q", text),
        ])
        .timeout(std::time::Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|e| send_failure("翻译请求失败", &e))?;

    let status = resp.status();
    // 先判状态码再解析：上游限流/故障返回 HTML 错误页时，直接报 HTTP 状态，
    // 而不是"解析失败"掩盖真实原因（与 Bing 侧一致）
    if !status.is_success() {
        return Err(upstream_failure(format!("翻译服务返回 HTTP {}", status)));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| upstream_failure(format!("翻译响应解析失败: {}", e)))?;

    // 返回格式：[[["译文","原文",null,null,1],...],null,null,null,...]
    let segs = body
        .get(0)
        .and_then(|v| v.as_array())
        .ok_or_else(|| upstream_failure("翻译响应格式异常"))?;
    let result: String = segs
        .iter()
        .filter_map(|seg| seg.get(0).and_then(|s| s.as_str()))
        .collect::<Vec<_>>()
        .join("");
    Ok(if result.is_empty() {
        "（无翻译结果）".to_string()
    } else {
        result
    })
}

/// Bing 翻译会话参数缓存：(IG, IID, key, token, 抓取时间)
///
/// 说明：Edge 的 `edge.microsoft.com/translate/auth` 端点**已下线**（实测 404），
/// 改用 cn.bing.com/translator 网页端同款接口：先抓 IG/IID + 防滥用 key/token，
/// 再 POST `cn.bing.com/ttranslatev3`（实测可用，且 cn.bing.com 国内可直连）。
type BingSession = (String, String, String, String, std::time::Instant);
static BING_SESSION: OnceLock<tokio::sync::Mutex<Option<BingSession>>> = OnceLock::new();

/// 取 start..end 之间的子串（抓页面内嵌参数用，避免为此引入正则依赖）
fn extract_between(hay: &str, start: &str, end: &str) -> Option<String> {
    let i = hay.find(start)? + start.len();
    let rest = &hay[i..];
    let j = rest.find(end)?;
    Some(rest[..j].to_string())
}

/// 会话缓存是否新鲜（纯函数，可单测）：抓取后 1800 秒（30 分钟）内即新鲜。
fn bing_cache_fresh(at: std::time::Instant) -> bool {
    at.elapsed().as_secs() < 1800
}

/// 抓取 Bing 翻译会话参数（IG / IID / key / token），缓存 30 分钟（1800 秒）
///
/// 锁拆分（第六件）：拿会话与发请求分两段——先短持锁看缓存（命中直接克隆返回），
/// 未命中则放锁后发网络请求，拿到再短持锁回填。旧实现整段持锁（含最长 timeout 秒
/// 的网络等待），并行双发后多路翻译排队堵在同一把锁上；拆开后锁内只剩内存操作。
async fn bing_session(timeout: u64) -> Result<BingSession, CommandError> {
    let cell = BING_SESSION.get_or_init(|| tokio::sync::Mutex::new(None));
    // 第一段：短持锁，只读缓存
    {
        let guard = cell.lock().await;
        if let Some((ig, iid, key, token, at)) = guard.as_ref() {
            if bing_cache_fresh(*at) {
                return Ok((ig.clone(), iid.clone(), key.clone(), token.clone(), *at));
            }
        }
    }
    // 不持锁发网络请求（并发多路在此并行，不再串行等待）
    let (ig, iid, key, token) = fetch_bing_session(timeout).await?;
    // 第二段：短持锁回填（双重检查：并发下别人已刷新就用新的，自己这份丢掉）
    {
        let mut guard = cell.lock().await;
        if let Some((ig2, iid2, key2, token2, at2)) = guard.as_ref() {
            if bing_cache_fresh(*at2) {
                return Ok((
                    ig2.clone(),
                    iid2.clone(),
                    key2.clone(),
                    token2.clone(),
                    *at2,
                ));
            }
        }
        let now = std::time::Instant::now();
        *guard = Some((ig.clone(), iid.clone(), key.clone(), token.clone(), now));
        Ok((ig, iid, key, token, now))
    }
}

/// 纯网络抓取 Bing 会话参数（不碰锁，由 bing_session 分段调用）
async fn fetch_bing_session(
    timeout: u64,
) -> Result<(String, String, String, String), CommandError> {
    let html = client()
        .get("https://cn.bing.com/translator")
        .timeout(std::time::Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|e| send_failure("Bing 页面加载失败", &e))?
        .text()
        .await
        .map_err(|e| upstream_failure(format!("Bing 页面读取失败: {}", e)))?;
    let ig = extract_between(&html, "IG:\"", "\"")
        .ok_or_else(|| upstream_failure("Bing IG 抓取失败"))?;
    let iid = extract_between(&html, "data-iid=\"", "\"")
        .ok_or_else(|| upstream_failure("Bing IID 抓取失败"))?;
    // 页面内嵌：params_AbusePreventionHelper = [key,"token",interval]
    let helper = extract_between(&html, "params_AbusePreventionHelper = [", "]")
        .ok_or_else(|| upstream_failure("Bing 防滥用参数抓取失败"))?;
    let (key, token) = {
        let (k, rest) = helper
            .split_once(',')
            .ok_or_else(|| upstream_failure("Bing 防滥用参数格式异常"))?;
        let t = rest.trim().trim_start_matches('"');
        let t = t.split('"').next().unwrap_or("");
        (k.trim().to_string(), t.to_string())
    };
    if key.is_empty() || token.is_empty() {
        return Err(upstream_failure("Bing 会话参数为空"));
    }
    Ok((ig, iid, key, token))
}

/// Bing 翻译（cn.bing.com 网页端同款接口，无需 API key）
///
/// 语言码差异：Google 用 zh-CN，Bing 用 zh-Hans；`timeout` 秒（设置里可调）
pub async fn translate_bing(
    text: &str,
    target: &str,
    timeout: u64,
) -> Result<String, CommandError> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let to = match target {
        "zh-CN" | "zh-cn" | "zh" => "zh-Hans",
        other => other,
    };
    let (ig, iid, key, token, _) = bing_session(timeout).await?;
    let url = format!(
        "https://cn.bing.com/ttranslatev3?isVertical=1&IG={}&IID={}",
        ig, iid
    );
    let form: [(&str, &str); 5] = [
        ("fromLang", "auto-detect"),
        ("text", text),
        ("to", to),
        ("key", key.as_str()),
        ("token", token.as_str()),
    ];
    let resp = client()
        .post(&url)
        .header("Referer", "https://cn.bing.com/translator")
        .form(&form)
        .timeout(std::time::Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|e| send_failure("Bing 翻译请求失败", &e))?;
    let status = resp.status();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| upstream_failure(format!("Bing 响应解析失败: {}", e)))?;
    if !status.is_success() {
        return Err(upstream_failure(format!("Bing 翻译返回 HTTP {}", status)));
    }
    // 失败形态：{"statusCode":205,"errorMessage":""}（key/token 失效或参数被拒）
    // —— 清掉缓存的会话，下次调用重新抓取（自愈）
    if let Some(code) = v.get("statusCode") {
        if let Some(cell) = BING_SESSION.get() {
            let mut g = cell.lock().await;
            *g = None;
        }
        // 凭证失效（key/token 被拒）→ E_AUTH：调用方语义是"重新取凭证"，
        // 与网络故障（可重试）区分；会话缓存已在上面清掉，下次自愈
        return Err(CommandError {
            code: crate::error::E_AUTH,
            message: format!("Bing 翻译被拒（statusCode={}）", code),
        });
    }
    // 成功格式：[{"translations":[{"text":"译文","to":"zh-Hans"}],"detectedLanguage":{...}}]
    let result = v
        .get(0)
        .and_then(|x| x.get("translations"))
        .and_then(|x| x.as_array())
        .and_then(|arr| arr.first())
        .and_then(|x| x.get("text"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(if result.is_empty() {
        "（无翻译结果）".to_string()
    } else {
        result
    })
}

/// 单引擎跑一次（owned 参数，供并行双发 spawn，要求 'static）。
async fn translate_one(
    engine: String,
    text: String,
    target: String,
    timeout: u64,
) -> Result<String, CommandError> {
    if engine == "bing" {
        translate_bing(&text, &target, timeout).await
    } else {
        translate_google(&text, &target, timeout).await
    }
}

/// 统一翻译：按数据流模式派发。
///
/// - `dual_mode = "dual"`（旧行为）：双引擎并行双发，谁先成功要谁，慢者取消。
/// - `dual_mode = "single"`（默认，C-8 单发）：只发首选引擎（`engine`），
///   不向另一引擎发送——用户内容不外流到两家。超时/失败直接报错，
///   不换引擎（换引擎=换一家第三方，非"降级"；让用户在设置里切换）。
///
/// `timeout` 秒：来自设置 translate_timeout（默认 15）
pub async fn translate_with(
    text: &str,
    target: &str,
    engine: &str,
    dual_mode: &str,
    timeout: u64,
) -> Result<String, CommandError> {
    // 非 bing 即 google：未知引擎名回落 google（与旧分支一致）
    let (first, second) = if engine == "bing" {
        ("bing", "google")
    } else {
        ("google", "bing")
    };

    if dual_mode == "dual" {
        // ---- 双发：原并行双发逻辑 ----
        let mut set = tokio::task::JoinSet::new();
        set.spawn(translate_one(
            first.to_string(),
            text.to_string(),
            target.to_string(),
            timeout,
        ));
        set.spawn(translate_one(
            second.to_string(),
            text.to_string(),
            target.to_string(),
            timeout,
        ));
        let mut errs: Vec<String> = Vec::new();
        let mut codes: Vec<&'static str> = Vec::new();
        while let Some(r) = set.join_next().await {
            match r {
                Ok(Ok(t)) => {
                    log::info!("[translate] 双发成功");
                    return Ok(t);
                }
                Ok(Err(e)) => {
                    log::warn!("[translate] 一路失败，等另一路: {}", e);
                    codes.push(e.code);
                    errs.push(e.message);
                }
                Err(e) => {
                    log::warn!("[translate] 引擎任务异常: {}", e);
                    errs.push(e.to_string());
                }
            }
        }
        Err(CommandError {
            code: aggregate_dual_fail(&codes),
            message: format!("双引擎均失败（{}）", errs.join("；")),
        })
    } else {
        // ---- 单发（默认）：只走首选引擎，不向第二家发送 ----
        log::debug!("[translate] 单发模式 engine={first}");
        translate_one(
            first.to_string(),
            text.to_string(),
            target.to_string(),
            timeout,
        )
        .await
    }
}

/// 单段引擎翻译 + 翻译记忆（C-9）：命中记忆直接返回，未命中走网络并写回。
///
/// 目的（S-9 G-2）：重访页面、常见 UI 文案、重复划词不再每次都发第三方——
/// 既省时（秒回）又减少外发（隐私）。缓存是「尽力而为」：记忆读写失败绝不影响
/// 翻译本身（db 层已吞异常）。键把「引擎 + 目标语言」编成命名空间
/// （见 `db::translate_memory::engine_scope`）。
///
/// C-11：`mem_key` 是翻译记忆 HMAC 密钥（`db::translate_memory::hmac_key` 的结果）；
/// None = 密钥不可用（translate.key 损坏/不可写），记忆停用但翻译照常。
pub async fn translate_text_cached(
    pool: &sqlx::SqlitePool,
    text: &str,
    target: &str,
    engine: &str,
    dual_mode: &str,
    timeout: u64,
    mem_key: Option<&[u8; 32]>,
) -> Result<String, CommandError> {
    let scope = crate::db::translate_memory::engine_scope(engine, target);
    let key = crate::db::translate_memory::normalize_source(text);
    if !key.is_empty() {
        if let Some(mem_key) = mem_key {
            let hits = crate::db::translate_memory::lookup(
                pool,
                &scope,
                std::slice::from_ref(&key),
                mem_key,
            )
            .await;
            if let Some(t) = hits.get(&key) {
                log::debug!("[translate] 记忆命中，跳过第三方请求");
                return Ok(t.clone());
            }
        }
    }
    let out = translate_with(text, target, engine, dual_mode, timeout).await?;
    if !key.is_empty() {
        if let Some(mem_key) = mem_key {
            crate::db::translate_memory::remember(
                pool,
                &scope,
                &[(text.to_string(), out.clone())],
                mem_key,
            )
            .await;
        }
    }
    Ok(out)
}

/// 批量引擎翻译 + 翻译记忆（C-9）：只对未命中的段落发第三方，命中直接回填。
///
/// 并发度 5（复用 [`translate_batch_plain`]）；失败段落保持空串，与既有批量口径一致。
/// C-11：`mem_key` = None 时记忆停用，全部段落走第三方（语义同 [`translate_text_cached`]）。
pub async fn translate_batch_cached(
    pool: &sqlx::SqlitePool,
    texts: &[String],
    target: &str,
    engine: &str,
    dual_mode: &str,
    timeout: u64,
    mem_key: Option<&[u8; 32]>,
) -> Vec<String> {
    let scope = crate::db::translate_memory::engine_scope(engine, target);
    let hits = match mem_key {
        Some(mem_key) => crate::db::translate_memory::lookup(pool, &scope, texts, mem_key).await,
        None => Default::default(),
    };
    let mut out: Vec<String> = vec![String::new(); texts.len()];
    let mut miss: Vec<usize> = Vec::new();
    for (i, t) in texts.iter().enumerate() {
        let key = crate::db::translate_memory::normalize_source(t);
        match hits.get(&key) {
            Some(v) if !v.is_empty() => out[i] = v.clone(),
            _ => miss.push(i),
        }
    }
    if miss.is_empty() {
        return out;
    }
    let miss_texts: Vec<String> = miss.iter().map(|&i| texts[i].clone()).collect();
    let fresh = translate_batch_plain(&miss_texts, target, engine, dual_mode, timeout).await;
    let mut to_remember: Vec<(String, String)> = Vec::new();
    for (k, &i) in miss.iter().enumerate() {
        let t = &fresh[k];
        if !t.is_empty() {
            out[i] = t.clone();
            to_remember.push((texts[i].clone(), t.clone()));
        }
    }
    if !to_remember.is_empty() {
        if let Some(mem_key) = mem_key {
            crate::db::translate_memory::remember(pool, &scope, &to_remember, mem_key).await;
        }
    }
    out
}

/// 批量翻译（无缓存）：每批 5 段并发、保序；失败段落留空串。
/// 与既有 proto 批量路径的并发口径一致，供缓存版与 DB 缺失兜底复用。
pub(crate) async fn translate_batch_plain(
    texts: &[String],
    target: &str,
    engine: &str,
    dual_mode: &str,
    timeout: u64,
) -> Vec<String> {
    let mut out: Vec<String> = vec![String::new(); texts.len()];
    for chunk_start in (0..texts.len()).step_by(5) {
        let chunk_end = (chunk_start + 5).min(texts.len());
        let mut set = tokio::task::JoinSet::new();
        for (offset, t) in texts[chunk_start..chunk_end].iter().enumerate() {
            let i = chunk_start + offset;
            let t = t.clone();
            let tgt = target.to_string();
            let eng = engine.to_string();
            let dm = dual_mode.to_string();
            set.spawn(async move { (i, translate_with(&t, &tgt, &eng, &dm, timeout).await) });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((i, Ok(t))) = joined {
                out[i] = t;
            }
        }
    }
    out
}

/// 双引擎均失败时归码（纯函数，可单测）：两路全是超时 → E_TIMEOUT（可重试）；
/// 混入其他失败或任务异常 → E_NET
fn aggregate_dual_fail(codes: &[&'static str]) -> &'static str {
    if !codes.is_empty() && codes.iter().all(|c| *c == crate::error::E_TIMEOUT) {
        crate::error::E_TIMEOUT
    } else {
        crate::error::E_NET
    }
}

/// 读取当前翻译源（settings.translate_engine，默认 google）
pub async fn current_engine(db: &crate::db::Db) -> String {
    let row: Option<(String,)> = crate::db::settings_cache::get(&db.pool, "translate_engine")
        .await
        .map(|v| (v,));
    row.map(|r| r.0).unwrap_or_else(|| "google".to_string())
}

/// 当前翻译目标语言（设置 → 通用 → 翻译目标语言，默认 zh-CN）
pub async fn current_target(db: &crate::db::Db) -> String {
    let row: Option<(String,)> = crate::db::settings_cache::get(&db.pool, "translate_target")
        .await
        .map(|v| (v,));
    row.map(|r| r.0).unwrap_or_else(|| "zh-CN".to_string())
}

/// 当前翻译数据流模式（设置 → 通用 → 数据流，默认 single）
pub async fn current_dual_mode(db: &crate::db::Db) -> String {
    let row: Option<(String,)> = crate::db::settings_cache::get(&db.pool, "translate_dual_mode")
        .await
        .map(|v| (v,));
    row.map(|r| r.0).unwrap_or_else(|| "single".to_string())
}

/// 前端调用的统一翻译入口（与 SettingsDialog/AddressBar 的 invoke 约定一致）
///
/// 返回 `{ ok: true, data: "译文" }` 或 `{ ok: false, msg: "错误信息" }`
#[tauri::command]
pub async fn translate_text(
    db: tauri::State<'_, crate::db::Db>,
    text: String,
    target: Option<String>,
) -> Result<TranslateResult, String> {
    let target = target.unwrap_or_else(|| "zh-CN".to_string());
    let engine = current_engine(db.inner()).await;
    let dual_mode = current_dual_mode(db.inner()).await;
    let timeout = current_timeout(db.inner()).await;
    let mem_key = crate::db::translate_memory::hmac_key(db.inner()).await;
    match translate_text_cached(
        &db.pool,
        &text,
        &target,
        &engine,
        &dual_mode,
        timeout,
        mem_key.as_ref(),
    )
    .await
    {
        Ok(data) => Ok(TranslateResult {
            ok: true,
            msg: None,
            data: Some(data),
        }),
        Err(e) => Ok(TranslateResult {
            ok: false,
            // 信封出口不变：msg 仍为人读字符串（TranslatePanel 直接展示 r.msg）
            msg: Some(e.to_string()),
            data: None,
        }),
    }
}

/// 与前端 invoke 的返回类型对齐
#[derive(Debug, Serialize)]
pub struct TranslateResult {
    pub ok: bool,
    pub msg: Option<String>,
    pub data: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bing_cache_freshness_window_is_1800s() {
        let now = std::time::Instant::now();
        // 刚抓的、1700 秒前的都新鲜
        assert!(bing_cache_fresh(now));
        assert!(bing_cache_fresh(now - std::time::Duration::from_secs(1700)));
        // 1900 秒前、3600 秒前的都过期（留足余量，不掐 1800 整点防抖动）
        assert!(!bing_cache_fresh(
            now - std::time::Duration::from_secs(1900)
        ));
        assert!(!bing_cache_fresh(
            now - std::time::Duration::from_secs(3600)
        ));
    }

    /// 防漂移（R4-1）：发送失败分类——reqwest 超时必须归 E_TIMEOUT（可重试），
    /// 其余网络失败归 E_NET；超时不得降级成通用网络错误
    #[test]
    fn classify_send_maps_timeout_and_net() {
        assert_eq!(
            classify_send(true, "超时".into()).code,
            crate::error::E_TIMEOUT
        );
        assert_eq!(
            classify_send(false, "断连".into()).code,
            crate::error::E_NET
        );
    }

    /// 防漂移（R4-1）：双引擎聚合归码——两路全超时才归 E_TIMEOUT；
    /// 混入其他失败归 E_NET；两路都是任务异常（无码）也归 E_NET
    #[test]
    fn aggregate_dual_fail_only_all_timeout_is_timeout() {
        use crate::error::{E_NET, E_TIMEOUT};
        assert_eq!(aggregate_dual_fail(&[E_TIMEOUT, E_TIMEOUT]), E_TIMEOUT);
        assert_eq!(aggregate_dual_fail(&[E_TIMEOUT, E_NET]), E_NET);
        assert_eq!(aggregate_dual_fail(&[]), E_NET);
    }

    /// 防漂移（R4-1）：translate_text 信封出口不变——msg 仍是人读字符串
    /// （TranslatePanel 直接展示 r.msg；信封 {ok,msg,data} 形态不因类型化漂移）
    #[test]
    fn dual_fail_message_keeps_legacy_envelope_shape() {
        let e = CommandError {
            code: aggregate_dual_fail(&[crate::error::E_TIMEOUT]),
            message: format!("双引擎均失败（{}）", ["超时"].join("；")),
        };
        assert_eq!(e.message, "双引擎均失败（超时）");
    }

    /// extract_between：命中 / 多候选取首个 / start 或 end 缺失 / 空串边界
    #[test]
    fn extract_between_边界与首个候选() {
        // 命中
        assert_eq!(
            extract_between("a=\"XYZ\";b", "a=\"", "\";").as_deref(),
            Some("XYZ")
        );
        // 多个候选取首个
        assert_eq!(
            extract_between("[x]-[y]-[z]", "[", "]").as_deref(),
            Some("x")
        );
        // start 缺失 / end 缺失 → None
        assert_eq!(extract_between("abc", "Q", "c"), None);
        assert_eq!(extract_between("abc", "a", "Z"), None);
        // start 之后无 end（end 只在 start 之前出现）→ None
        assert_eq!(extract_between("end-start", "start", "end"), None);
        // 相邻无内容 → 空串（而非 None）
        assert_eq!(extract_between("<>", "<", ">").as_deref(), Some(""));
        // 空输入
        assert_eq!(extract_between("", "a", "b"), None);
    }

    /// upstream_failure：错误文本透传，码固定为 E_NET（上游异常不可当超时重试）
    #[test]
    fn upstream_failure_passes_message_and_uses_net_code() {
        let e = upstream_failure("HTTP 500");
        assert_eq!(e.code, crate::error::E_NET);
        assert_eq!(e.message, "HTTP 500");
        // 接受 String 与 &str（impl Into<String>）且原样保留，含中文与空串
        let e2 = upstream_failure(String::from("解析失败：缺 tkk"));
        assert_eq!(e2.message, "解析失败：缺 tkk");
        assert_eq!(upstream_failure("").message, "");
    }

    /// classify_send：超时 / 非超时两条路径；message 原样透传（含空串）
    #[test]
    fn classify_send_passes_message_through_on_both_branches() {
        let t = classify_send(true, "reqwest timeout".into());
        assert_eq!(t.code, crate::error::E_TIMEOUT);
        assert_eq!(t.message, "reqwest timeout");
        let n = classify_send(false, String::new());
        assert_eq!(n.code, crate::error::E_NET);
        assert_eq!(n.message, "");
    }

    /// bing_cache_fresh 边界：1799s 仍新鲜，恰 1800s 即过期（< 1800 语义）
    #[test]
    fn bing_cache_fresh_exact_boundary() {
        let now = std::time::Instant::now();
        let d = |s: u64| std::time::Duration::from_secs(s);
        assert!(bing_cache_fresh(now - d(1799)));
        assert!(!bing_cache_fresh(now - d(1800)));
        assert!(!bing_cache_fresh(now - d(1801)));
    }

    /// C-9 端到端：命中记忆时不发第三方请求。
    ///
    /// 用「预置记忆 + 1 秒超时」验证：只有命中缓存才能拿到预置译文；若未命中
    /// 会走真实网络，返回的不会恰是「登录」（断言失败），或 1 秒内超时 Err（unwrap 失败）。
    #[tokio::test]
    async fn 记忆命中不发第三方请求() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        let dir = std::env::temp_dir().join(format!(
            "sb_tr_cache_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(dir.join("tr.db"))
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../migrations/20261015000000_translate_memory_hmac.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let scope = crate::db::translate_memory::engine_scope("google", "zh-CN");
        let mem_key = [7u8; 32];
        crate::db::translate_memory::remember(
            &pool,
            &scope,
            &[("Sign in".to_string(), "登录".to_string())],
            &mem_key,
        )
        .await;
        // 原文带多余空白：归一化后仍应命中
        let got = translate_text_cached(
            &pool,
            "Sign   in",
            "zh-CN",
            "google",
            "single",
            1,
            Some(&mem_key),
        )
        .await
        .unwrap();
        assert_eq!(got, "登录");
    }
}
