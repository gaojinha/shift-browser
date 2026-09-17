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

/// 前端调用的统一翻译入口（与 SettingsDialog/AddressBar 的 invoke 约定一致）
///
/// 返回 `{ ok: true, data: "译文" }` 或 `{ ok: false, msg: "错误信息" }`
#[tauri::command]
pub async fn translate_text(text: String, target: Option<String>) -> Result<TranslateResult, String> {
  let target = target.unwrap_or_else(|| "zh-CN".to_string());
  match translate_google(&text, &target).await {
    Ok(data) => Ok(TranslateResult { ok: true, msg: None, data: Some(data) }),
    Err(e) => Ok(TranslateResult { ok: false, msg: Some(e), data: None }),
  }
}

/// 与前端 invoke 的返回类型对齐
#[derive(Debug, Serialize)]
pub struct TranslateResult {
  pub ok: bool,
  pub msg: Option<String>,
  pub data: Option<String>,
}
