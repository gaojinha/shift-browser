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
  (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// reqwest 客户端（共享连接池，全局单例）
static CLIENT: OnceLock<Client> = OnceLock::new();

fn client() -> &'static Client {
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(CHROME_UA)
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("reqwest client 初始化失败")
    })
}

/// Google 翻译（Chrome dict-chrome-ex 公共端点，无需 API key）
///
/// `target` 默认 zh-CN；源语言自动检测（sl=auto）
pub async fn translate_google(text: &str, target: &str) -> Result<String, String> {
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

/// Bing token 缓存（Edge 端点签发 JWT，有效期约 10 分钟；缓存 8 分钟保守一点）
static BING_TOKEN: OnceLock<tokio::sync::Mutex<Option<(String, std::time::Instant)>>> =
    OnceLock::new();

/// 取 Bing/Edge 翻译 token（免费，无需 API key）
async fn bing_token() -> Result<String, String> {
    let cell = BING_TOKEN.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cell.lock().await;
    if let Some((t, at)) = guard.as_ref() {
        if at.elapsed().as_secs() < 480 {
            return Ok(t.clone());
        }
    }
    let resp = client()
        .get("https://edge.microsoft.com/translate/auth")
        .send()
        .await
        .map_err(|e| format!("Bing 取 token 失败: {}", e))?;
    let t = resp
        .text()
        .await
        .map_err(|e| format!("Bing token 读取失败: {}", e))?;
    let t = t.trim().to_string();
    if t.is_empty() {
        return Err("Bing 返回空 token".to_string());
    }
    *guard = Some((t.clone(), std::time::Instant::now()));
    Ok(t)
}

/// Bing 翻译（Edge 公共端点，无需 API key）
///
/// 与 Google 的语言码差异：Google 用 zh-CN，Bing 用 zh-Hans
pub async fn translate_bing(text: &str, target: &str) -> Result<String, String> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }
    let to = match target {
        "zh-CN" | "zh-cn" | "zh" => "zh-Hans",
        other => other,
    };
    let token = bing_token().await?;
    let url = format!(
        "https://api-edge.cognitive.microsofttranslator.com/translate?api-version=3.0&to={}",
        to
    );
    let resp = client()
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!([{ "Text": text }]))
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
    // 返回格式：[{"detectedLanguage":{...},"translations":[{"text":"译文","to":"zh-Hans"}]}]
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
pub async fn translate_with(text: &str, target: &str, engine: &str) -> Result<String, String> {
    if engine == "bing" {
        match translate_bing(text, target).await {
            Ok(t) => Ok(t),
            Err(e) => {
                log::warn!("[translate] Bing 失败，回退 Google: {}", e);
                translate_google(text, target).await
            }
        }
    } else {
        match translate_google(text, target).await {
            Ok(t) => Ok(t),
            Err(e) => {
                log::warn!("[translate] Google 失败，回退 Bing: {}", e);
                translate_bing(text, target).await
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
    match translate_with(&text, &target, &engine).await {
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
