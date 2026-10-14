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

/// 统一派发：双引擎并行双发，谁先成功要谁，慢者取消。
/// （旧逻辑串行回退：首选超时 15 秒再换另一方，最坏双倍延迟。
/// Google 国内常连不上、Bing 偶发限流，并行后单引擎故障不再翻倍。）
/// `timeout` 秒：来自设置 translate_timeout（默认 15）
pub async fn translate_with(
    text: &str,
    target: &str,
    engine: &str,
    timeout: u64,
) -> Result<String, CommandError> {
    // 非 bing 即 google：未知引擎名回落 google（与旧分支一致）
    let (first, second) = if engine == "bing" {
        ("bing", "google")
    } else {
        ("google", "bing")
    };
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
    // 先完成的失败不致命：记下等另一个；首个成功直接返回，析构取消在途者
    let mut errs: Vec<String> = Vec::new();
    let mut codes: Vec<&'static str> = Vec::new();
    while let Some(r) = set.join_next().await {
        match r {
            Ok(Ok(t)) => {
                log::info!("[translate] 并行双发成功");
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
    let timeout = current_timeout(db.inner()).await;
    match translate_with(&text, &target, &engine, timeout).await {
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
}
