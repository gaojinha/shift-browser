//! 翻译模块（Google 免费接口，免 API key）
//!
//! 右键菜单的"翻译选中文字 / 翻译整个页面"通过 `shift://__translate__/...`
//! 协议调用本模块（lib.rs 处理器）。设计要点：
//! - `client=dict-chrome-ex` 是 Chrome 内置翻译同款端点，免 key，
//!   但国内可能受网络限制（必要时切 Bing 或配代理）
//! - 同步 block_on 调用——shift handler 是同步闭包，但 reqwest 异步
//!   不能直接 block——用 tauri::async_runtime::block_on 把 async 拉成同步
//! - 整页翻译分两步：先 eval JS 收集文本节点 JSON，再 spawn 异步翻译后 eval 写回

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

fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(CHROME_UA)
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest client 初始化失败")
    })
}

/// 当前翻译超时（秒）：settings.translate_timeout，默认 15，合法范围 5-120
pub async fn current_timeout(db: &crate::db::Db) -> u64 {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'translate_timeout'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    row.and_then(|r| r.0.parse::<u64>().ok())
        .filter(|v| *v >= 5 && *v <= 120)
        .unwrap_or(15)
}

/// Google 翻译（Chrome dict-chrome-ex 公共端点，无需 API key）
///
/// `target` 默认 zh-CN；源语言自动检测（sl=auto）；`timeout` 秒（设置里可调）
pub async fn translate_google(text: &str, target: &str, timeout: u64) -> Result<String, String> {
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
        .map_err(|e| format!("翻译请求失败: {}", e))?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("翻译响应解析失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("翻译服务返回 HTTP {}", status));
    }

    // 返回格式：[[["译文","原文",null,null,1],...],null,null,null,...]
    let segs = body
        .get(0)
        .and_then(|v| v.as_array())
        .ok_or_else(|| "翻译响应格式异常".to_string())?;
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

/// 抓取 Bing 翻译会话参数（IG / IID / key / token），缓存 30 分钟
async fn bing_session(timeout: u64) -> Result<BingSession, String> {
    let cell = BING_SESSION.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cell.lock().await;
    if let Some((ig, iid, key, token, at)) = guard.as_ref() {
        if at.elapsed().as_secs() < 1800 {
            return Ok((ig.clone(), iid.clone(), key.clone(), token.clone(), *at));
        }
    }
    let html = client()
        .get("https://cn.bing.com/translator")
        .timeout(std::time::Duration::from_secs(timeout))
        .send()
        .await
        .map_err(|e| format!("Bing 页面加载失败: {}", e))?
        .text()
        .await
        .map_err(|e| format!("Bing 页面读取失败: {}", e))?;
    let ig = extract_between(&html, "IG:\"", "\"").ok_or_else(|| "Bing IG 抓取失败".to_string())?;
    let iid = extract_between(&html, "data-iid=\"", "\"")
        .ok_or_else(|| "Bing IID 抓取失败".to_string())?;
    // 页面内嵌：params_AbusePreventionHelper = [key,"token",interval]
    let helper = extract_between(&html, "params_AbusePreventionHelper = [", "]")
        .ok_or_else(|| "Bing 防滥用参数抓取失败".to_string())?;
    let (key, token) = {
        let (k, rest) = helper
            .split_once(',')
            .ok_or_else(|| "Bing 防滥用参数格式异常".to_string())?;
        let t = rest.trim().trim_start_matches('"');
        let t = t.split('"').next().unwrap_or("");
        (k.trim().to_string(), t.to_string())
    };
    if key.is_empty() || token.is_empty() {
        return Err("Bing 会话参数为空".to_string());
    }
    let now = std::time::Instant::now();
    *guard = Some((ig.clone(), iid.clone(), key.clone(), token.clone(), now));
    Ok((ig, iid, key, token, now))
}

/// Bing 翻译（cn.bing.com 网页端同款接口，无需 API key）
///
/// 语言码差异：Google 用 zh-CN，Bing 用 zh-Hans；`timeout` 秒（设置里可调）
pub async fn translate_bing(text: &str, target: &str, timeout: u64) -> Result<String, String> {
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
        .map_err(|e| format!("Bing 翻译请求失败: {}", e))?;
    let status = resp.status();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Bing 响应解析失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("Bing 翻译返回 HTTP {}", status));
    }
    // 失败形态：{"statusCode":205,"errorMessage":""}（key/token 失效或参数被拒）
    // —— 清掉缓存的会话，下次调用重新抓取（自愈）
    if let Some(code) = v.get("statusCode") {
        if let Some(cell) = BING_SESSION.get() {
            let mut g = cell.lock().await;
            *g = None;
        }
        return Err(format!("Bing 翻译被拒（statusCode={}）", code));
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

/// 统一派发：按引擎选择翻译，**失败自动回退另一方**
/// （Google 国内常连不上、Bing 偶发限流，互为兜底可显著提升可用性）
/// `timeout` 秒：来自设置 translate_timeout（默认 15）
pub async fn translate_with(
    text: &str,
    target: &str,
    engine: &str,
    timeout: u64,
) -> Result<String, String> {
    if engine == "bing" {
        match translate_bing(text, target, timeout).await {
            Ok(t) => Ok(t),
            Err(e) => {
                log::warn!("[translate] Bing 失败，回退 Google: {}", e);
                translate_google(text, target, timeout).await
            }
        }
    } else {
        match translate_google(text, target, timeout).await {
            Ok(t) => Ok(t),
            Err(e) => {
                log::warn!("[translate] Google 失败，回退 Bing: {}", e);
                translate_bing(text, target, timeout).await
            }
        }
    }
}

/// 读取当前翻译源（settings.translate_engine，默认 google）
pub async fn current_engine(db: &crate::db::Db) -> String {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'translate_engine'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    row.map(|r| r.0).unwrap_or_else(|| "google".to_string())
}

/// 当前翻译目标语言（设置 → 通用 → 翻译目标语言，默认 zh-CN）
pub async fn current_target(db: &crate::db::Db) -> String {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'translate_target'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
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
            msg: Some(e),
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
