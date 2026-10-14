// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

// 阅读模式门面：用 readability 算法提取正文，生成干净的大字排版页，新标签打开
// 注意：readability::scrape 内部是 blocking reqwest，必须 spawn_blocking，
// 绝不能跑在 async 线程/主线程上
//
// R1 拆分（2026-09-25）：
//   - fetch：curl 抓取管线 + 逐跳 SSRF pin 校验（gpc 出网复用）
//   - render：ammonia 正文净化 + 阅读页模板
use crate::error::{CommandError, E_INTERNAL, E_NET, E_PERM, E_TIMEOUT, E_VALIDATION};
use tauri::{AppHandle, Manager};

#[path = "reader/fetch.rs"]
mod fetch;
#[path = "reader/render.rs"]
mod render;

// gpc.rs 等出网侧复用同一套公网校验/curl 管线
pub(crate) use fetch::{curl_fetch_with_redirects, validate_public_url};

use fetch::scrape_article;

/// 门面错误归类（R4-1 长尾）：fetch.rs 深层（curl/DNS/解析）仍为 String 形态，
/// 本层按稳定语义做门面级分流；深层全量类型化属后续批次。
/// 维护合约：fetch.rs 错误文案变化时，须同步本函数与文件尾 `tests` 内样本。
fn classify_scrape_error(message: String) -> CommandError {
    let code = if message.contains("内网") || message.contains("保留地址") {
        // SSRF 拦截（error.rs 口径：E_PERM 覆盖 SSRF/策略拒绝）
        E_PERM
    } else if message.contains("超时") {
        E_TIMEOUT
    } else if message.contains("只支持普通网页") || message.contains("URL") {
        // scheme 拒绝 / URL 解析类：参数问题（修正后才可重试）
        E_VALIDATION
    } else {
        // curl 失败 / HTTP 状态 / DNS / 正文提取失败等出网类
        E_NET
    };
    CommandError { code, message }
}

/// 抓正文返回 JSON（不写文件）——供资讯面板"📖 内联阅读"用
#[tauri::command]
pub async fn fetch_article(url: String) -> Result<serde_json::Value, CommandError> {
    let product = scrape_article(&url).await.map_err(classify_scrape_error)?;
    if product.content.len() < 200 {
        return Err(CommandError {
            code: E_VALIDATION,
            message: "内容太短，可能不是文章页".to_string(),
        });
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
pub async fn scrape_article_public(
    url: &str,
) -> Result<readability::extractor::Product, CommandError> {
    scrape_article(url).await.map_err(classify_scrape_error)
}

/// 进入阅读模式：提取 url 正文 → 写 file:// 页面 → 返回新页面 URL（前端 createTab 打开）
#[tauri::command]
pub async fn enter_reader_mode(app: AppHandle, url: String) -> Result<String, CommandError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(CommandError {
            code: E_VALIDATION,
            message: "只支持普通网页".to_string(),
        });
    }
    let product = scrape_article(&url).await.map_err(classify_scrape_error)?;

    if product.content.len() < 500 {
        return Err(CommandError {
            code: E_VALIDATION,
            message: "提取内容太短，可能不是文章页".to_string(),
        });
    }

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError {
            code: E_INTERNAL,
            message: e.to_string(),
        })?
        .join("reader");
    std::fs::create_dir_all(&dir).map_err(|e| CommandError {
        code: E_INTERNAL,
        message: e.to_string(),
    })?;

    // 只保留最近 20 篇，旧的删掉
    let mut old: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| CommandError {
            code: E_INTERNAL,
            message: e.to_string(),
        })?
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
    std::fs::write(&path, html).map_err(|e| CommandError {
        code: E_INTERNAL,
        message: e.to_string(),
    })?;
    Ok(format!("file://{}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 防漂移（R4-1 长尾）：门面错误归类钉住——
    /// fetch.rs 深层错误文案变化时，本样本与 `classify_scrape_error` 须同步。
    #[test]
    fn classify_scrape_error_maps_stable_categories() {
        let cases = [
            ("目标地址是内网/保留地址，已拦截（SSRF 防护）", E_PERM),
            ("抓取正文超时（20 秒），可能是慢站或网络问题", E_TIMEOUT),
            ("只支持普通网页", E_VALIDATION),
            ("URL 格式不合法", E_VALIDATION),
            ("抓取失败: HTTP 404", E_NET),
            ("DNS 解析失败: example.com", E_NET),
        ];
        for (msg, want) in cases {
            assert_eq!(classify_scrape_error(msg.to_string()).code, want, "{msg}");
        }
    }

    /// 防漂移（R4-1 长尾）：非 http(s) scheme 即时拒绝 → E_VALIDATION（不联网）
    #[tokio::test]
    async fn fetch_article_rejects_non_http_scheme() {
        let err = fetch_article("file:///etc/passwd".to_string())
            .await
            .unwrap_err();
        assert_eq!(err.code, E_VALIDATION);
    }

    /// 防漂移（R4-1 长尾）：私网 IP 字面量被 SSRF 拦截 → E_PERM（不联网，无 DNS 面）
    #[tokio::test]
    async fn fetch_article_blocks_private_ip_with_perm() {
        let err = fetch_article("http://127.0.0.1:8080/".to_string())
            .await
            .unwrap_err();
        assert_eq!(err.code, E_PERM);
    }
}
