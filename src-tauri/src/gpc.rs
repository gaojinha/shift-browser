//! GPC（Global Privacy Control）站点声明感知（阶段 2 · 隐私主权）
//!
//! W3C GPC 规范：站点可在 `/.well-known/gpc.json` 声明自己是否遵守 GPC。
//! 本模块抓取并解析该文件，把"我们发了 GPC 信号"升级为"可验证站点认不认"
//! ——对齐阶段 2 铁律「信任需要证明」。
//!
//! 安全：出网复用 reader 的 SSRF 防护（只允许公网 http/https），
//! 抓取在 spawn_blocking 里跑，不阻塞主线程。

use serde::Serialize;

/// 站点 GPC 声明状态
#[derive(Debug, Serialize, Clone)]
pub struct GpcSiteStatus {
    /// 查询的 origin（如 https://example.com）
    pub origin: String,
    /// gpc.json 的 URL
    pub url: String,
    /// 是否成功拿到并解析（HTTP 2xx + JSON 合法）
    pub reachable: bool,
    /// 站点声明的 gpc 值（true = 声明遵守 GPC）
    pub declared: Option<bool>,
    /// 站点声明的最后更新日期（gpc.json 里的 lastUpdate）
    pub last_update: Option<String>,
    /// 人类可读结论
    pub summary: String,
    /// 失败原因（reachable=false 时）
    pub error: Option<String>,
}

/// 解析 gpc.json 内容（纯函数，便于单测）：返回 (gpc, lastUpdate)
pub fn parse_gpc_json(raw: &str) -> Result<(Option<bool>, Option<String>), String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("JSON 解析失败: {}", e))?;
    let gpc = v.get("gpc").and_then(|x| x.as_bool());
    let last = v
        .get("lastUpdate")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    Ok((gpc, last))
}

/// 由 origin 拼 gpc.json URL（只接受 http/https）
pub fn gpc_json_url(origin: &str) -> Result<String, String> {
    let o = origin.trim().trim_end_matches('/');
    if !o.starts_with("http://") && !o.starts_with("https://") {
        return Err("需要 http/https 站点".to_string());
    }
    Ok(format!("{}/.well-known/gpc.json", o))
}

/// 由完整 URL 提取 origin（scheme://host[:port]）；非 http(s) 返回 None
pub fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return None;
    }
    Some(u.origin().ascii_serialization())
}

/// 结论文案（纯函数）
pub fn summarize(reachable: bool, declared: Option<bool>) -> String {
    if !reachable {
        return "站点未提供 GPC 声明文件".to_string();
    }
    match declared {
        Some(true) => "站点声明遵守 GPC（会尊重你的退出偏好）".to_string(),
        Some(false) => "站点声明不遵守 GPC".to_string(),
        None => "站点提供了 gpc.json，但未明确 gpc 字段".to_string(),
    }
}

fn fail(origin: &str, url: String, summary: String, error: Option<String>) -> GpcSiteStatus {
    GpcSiteStatus {
        origin: origin.to_string(),
        url,
        reachable: false,
        declared: None,
        last_update: None,
        summary,
        error,
    }
}

/// 抓取并解析站点的 `/.well-known/gpc.json`
pub async fn check_gpc_json(origin: &str) -> GpcSiteStatus {
    let url = match gpc_json_url(origin) {
        Ok(u) => u,
        Err(e) => return fail(origin, String::new(), e.clone(), Some(e)),
    };

    // SSRF 防护（复用 reader 的校验：只允许公网地址）
    if let Err(e) = crate::reader::validate_public_url(&url).await {
        return fail(origin, url, format!("拒绝访问：{}", e), Some(e));
    }

    let u = url.clone();
    let fetched =
        tokio::task::spawn_blocking(move || crate::reader::curl_fetch_with_redirects(&u, None))
            .await;

    match fetched {
        Ok(Ok(raw)) => match parse_gpc_json(&raw) {
            Ok((declared, last_update)) => GpcSiteStatus {
                origin: origin.to_string(),
                url,
                reachable: true,
                declared,
                last_update,
                summary: summarize(true, declared),
                error: None,
            },
            Err(e) => fail(origin, url, format!("gpc.json 无法解析：{}", e), Some(e)),
        },
        Ok(Err(e)) => fail(origin, url, "站点未提供 GPC 声明文件".to_string(), Some(e)),
        Err(e) => fail(origin, url, "抓取线程异常".to_string(), Some(e.to_string())),
    }
}

/// 检测活动标签站点的 GPC 声明（阶段 2：GPC 完整化）。
#[tauri::command]
pub async fn gpc_site_check(
    state: tauri::State<'_, crate::tabs::TabManager>,
) -> Result<GpcSiteStatus, String> {
    let url = state
        .active_url()
        .ok_or_else(|| "没有活动标签".to_string())?;
    let origin = origin_of(&url).ok_or_else(|| "当前标签不是普通网页".to_string())?;
    Ok(check_gpc_json(&origin).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gpc_json_true_and_date() {
        let raw = r#"{"gpc":true,"lastUpdate":"2026-01-01"}"#;
        let (gpc, last) = parse_gpc_json(raw).unwrap();
        assert_eq!(gpc, Some(true));
        assert_eq!(last.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn parse_gpc_json_false_and_missing() {
        assert_eq!(parse_gpc_json(r#"{"gpc":false}"#).unwrap().0, Some(false));
        assert_eq!(parse_gpc_json(r#"{}"#).unwrap().0, None);
        assert!(parse_gpc_json("not json").is_err());
    }

    #[test]
    fn gpc_json_url_rules() {
        assert_eq!(
            gpc_json_url("https://example.com").unwrap(),
            "https://example.com/.well-known/gpc.json"
        );
        // 末尾斜杠应被规范化
        assert_eq!(
            gpc_json_url("https://example.com/").unwrap(),
            "https://example.com/.well-known/gpc.json"
        );
        assert!(gpc_json_url("file:///etc/passwd").is_err());
        assert!(gpc_json_url("tauri://localhost").is_err());
    }

    #[test]
    fn origin_of_extracts_scheme_host_port() {
        assert_eq!(
            origin_of("https://example.com/a/b?q=1").as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            origin_of("http://example.com:8080/x").as_deref(),
            Some("http://example.com:8080")
        );
        assert_eq!(origin_of("tauri://localhost/index.html"), None);
        assert_eq!(origin_of("about:blank"), None);
    }

    #[test]
    fn summarize_variants() {
        assert!(summarize(true, Some(true)).contains("遵守"));
        assert!(summarize(true, Some(false)).contains("不遵守"));
        assert!(summarize(true, None).contains("未明确"));
        assert!(summarize(false, None).contains("未提供"));
    }
}
