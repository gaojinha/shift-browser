// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

// 阅读模式门面：用 readability 算法提取正文，生成干净的大字排版页，新标签打开
// 注意：readability::scrape 内部是 blocking reqwest，必须 spawn_blocking，
// 绝不能跑在 async 线程/主线程上
//
// R1 拆分（2026-09-25）：
//   - fetch：curl 抓取管线 + 逐跳 SSRF pin 校验（gpc 出网复用）
//   - render：ammonia 正文净化 + 阅读页模板
use tauri::{AppHandle, Manager};

#[path = "reader/fetch.rs"]
mod fetch;
#[path = "reader/render.rs"]
mod render;

// gpc.rs 等出网侧复用同一套公网校验/curl 管线
pub(crate) use fetch::{curl_fetch_with_redirects, validate_public_url};

use fetch::scrape_article;

/// 抓正文返回 JSON（不写文件）——供资讯面板"📖 内联阅读"用
#[tauri::command]
pub async fn fetch_article(url: String) -> Result<serde_json::Value, String> {
  let product = scrape_article(&url).await?;
  if product.content.len() < 200 {
    return Err("内容太短，可能不是文章页".to_string());
  }
  let content = render::sanitize_html(&product.content);
  Ok(serde_json::json!({
    "title": product.title,
    "content": content,
    "text": product.text,
  }))
}

/// 公共包装：供页面理解层（content_extract）复用抓取+readability 管线。
/// 出网操作，调用方须确保用户已授权。
pub async fn scrape_article_public(url: &str) -> Result<readability::extractor::Product, String> {
  scrape_article(url).await
}

/// 进入阅读模式：提取 url 正文 → 写 file:// 页面 → 返回新页面 URL（前端 createTab 打开）
#[tauri::command]
pub async fn enter_reader_mode(app: AppHandle, url: String) -> Result<String, String> {
  if !url.starts_with("http://") && !url.starts_with("https://") {
    return Err("只支持普通网页".to_string());
  }
  let product = scrape_article(&url).await?;

  if product.content.len() < 500 {
    return Err("提取内容太短，可能不是文章页".to_string());
  }

  let dir = app
    .path()
    .app_data_dir()
    .map_err(|e| e.to_string())?
    .join("reader");
  std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

  // 只保留最近 20 篇，旧的删掉
  let mut old: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
    .map_err(|e| e.to_string())?
    .filter_map(|e| e.ok().map(|e| e.path()))
    .collect();
  old.sort();
  while old.len() > 19 {
    let _ = std::fs::remove_file(&old[0]);
    old.remove(0);
  }

  let ts = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis())
    .unwrap_or(0);
  let path = dir.join(format!("r{}.html", ts));
  let content = render::sanitize_html(&product.content);
  let html = render::render_reader_html(&product.title, &url, &content);
  std::fs::write(&path, html).map_err(|e| e.to_string())?;
  Ok(format!("file://{}", path.display()))
}
