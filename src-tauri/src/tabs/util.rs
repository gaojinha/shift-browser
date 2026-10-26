//! 标签模块工具函数（P1-1 拆分 · 2026-09-22）
//! 纯函数，无平台依赖，可独立单测。

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

/// 从下载 URL 推导下载文件名：
/// 用 url 解析取解码后的 path（%2e%2e/%2f 点段按 URL 规范被规范化掉），
/// 取最后一段，再经 sanitize_filename 剥路径分隔符/点段/控制字符，防路径穿越；
/// 解析不了的 URL 退回按 '?' 取首段；空段兜底 "download"。
///
/// **单一来源**：WebKit 下载信号（`tabs/backend/layout/gtk.rs`）与 CEF 下载
/// （`engine/cef/shell.rs`）都必须调这一份，不许各写各的。
pub fn download_filename_from_url(url: Option<&str>) -> String {
    url.map(|u| {
        let path = url::Url::parse(u)
            .map(|p| p.path().to_string())
            .unwrap_or_else(|_| u.split('?').next().unwrap_or(u).to_string());
        let name = path.rsplit('/').next().unwrap_or("download").to_string();
        sanitize_filename(&name)
    })
    .unwrap_or_else(|| "download".to_string())
}

/// 清洗 URL 中的跨站追踪参数（隐私防护 P0）
/// 覆盖 utm_* 系列 + 主流平台点击 ID（fbclid/gclid/msclkid/spm 等）
pub fn clean_tracking_params(url: &str) -> String {
    const TRACKING_KEYS: &[&str] = &[
        "fbclid",
        "gclid",
        "dclid",
        "msclkid",
        "yclid",
        "twclid",
        "igshid",
        "ttclid",
        "mc_eid",
        "mc_cid",
        "mkt_tok",
        "_hsenc",
        "_hsmi",
        "vero_id",
        "oly_enc_id",
        "ref_src",
        "ref_url",
        "spm",
        "scm",
        "share_source",
        "share_medium",
        "from_source",
        "wd_uid",
        "s_kwcid",
        "elqTrackId",
        "trk",
    ];
    const TRACKING_PREFIXES: &[&str] = &["utm_", "pk_", "hsa_", "at_", "ito_"];
    // 只处理 http/https/query 存在的 URL
    if !url.contains('?') || !(url.starts_with("http://") || url.starts_with("https://")) {
        return url.to_string();
    }
    let Some((base, rest)) = url.split_once('?') else {
        return url.to_string();
    };
    let (query, fragment) = match rest.split_once('#') {
        Some((q, f)) => (q, Some(f)),
        None => (rest, None),
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| {
            let key = kv.split('=').next().unwrap_or("").to_ascii_lowercase();
            if key.is_empty() {
                return false;
            }
            if TRACKING_KEYS.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
                return false;
            }
            !TRACKING_PREFIXES.iter().any(|p| key.starts_with(p))
        })
        .collect();
    let cleaned = match (kept.is_empty(), fragment) {
        (true, Some(f)) => format!("{}#{}", base, f),
        (true, None) => base.to_string(),
        (false, Some(f)) => format!("{}?{}#{}", base, kept.join("&"), f),
        (false, None) => format!("{}?{}", base, kept.join("&")),
    };
    if cleaned != url {
        crate::extensions::CLEANED_PARAMS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        log::info!(
            "[privacy] URL 追踪参数已清洗: {} -> {}",
            crate::redact_url(url),
            crate::redact_url(&cleaned)
        );
    }
    cleaned
}

/// 净化下载文件名（防路径穿越）：
/// 剥掉 / \ 路径分隔符、空字节/控制字符、首尾点段（防 ../ 与隐藏文件），限长 200 字符
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0' | '\n' | '\r' | '\t'))
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return "download".to_string();
    }
    let mut s: String = cleaned.chars().take(200).collect();
    if s.is_empty() {
        s = "download".to_string();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracking_params_removed_and_clean_kept() {
        // utm_ 前缀族
        assert_eq!(
            clean_tracking_params("https://a.com/x?utm_source=rss&id=7"),
            "https://a.com/x?id=7"
        );
        // 精确键族（fbclid/gclid/spm…）
        assert_eq!(
            clean_tracking_params("https://a.com/x?fbclid=abc&id=7&q=1"),
            "https://a.com/x?id=7&q=1"
        );
        // 大小写不敏感（UTM_/FBCLID 变体同样清除）
        assert_eq!(
            clean_tracking_params("https://a.com/x?UTM_CAM=1&FBCLID=z"),
            "https://a.com/x"
        );
        // 无值键也按 key 判定清除（?utm_source 无 = 号）
        assert_eq!(
            clean_tracking_params("https://a.com/x?utm_source"),
            "https://a.com/x"
        );
    }

    #[test]
    fn tracking_clean_preserves_structure() {
        // fragment 保留
        assert_eq!(
            clean_tracking_params("https://a.com/x?utm_source=1#sec"),
            "https://a.com/x#sec"
        );
        assert_eq!(
            clean_tracking_params("https://a.com/x?id=1&utm_source=1#sec"),
            "https://a.com/x?id=1#sec"
        );
        // 全部被清且有正常参数：问号不残留
        assert_eq!(
            clean_tracking_params("https://a.com/x?gclid=1"),
            "https://a.com/x"
        );
        // 干净 URL 原样返回
        assert_eq!(
            clean_tracking_params("https://a.com/x?id=1&q=%20"),
            "https://a.com/x?id=1&q=%20"
        );
    }

    #[test]
    fn tracking_clean_passthrough_non_http() {
        // 非 http(s) 或无 query 不动（内部页/本地路径绝不改写）
        assert_eq!(clean_tracking_params("shift://newtab?utm_source=1"), "shift://newtab?utm_source=1");
        assert_eq!(clean_tracking_params("https://a.com/noquery"), "https://a.com/noquery");
        assert_eq!(clean_tracking_params(""), "");
    }

    #[test]
    fn sanitize_filename_blocks_traversal_and_control_chars() {
        // 路径穿越剥干净
        assert_eq!(sanitize_filename("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_filename("..\\..\\secret"), "secret");
        // 控制字符（\0 \n \r \t）剥掉
        assert_eq!(sanitize_filename("re\nport\r.txt"), "report.txt");
        assert_eq!(sanitize_filename("a\0b"), "ab");
    }

    #[test]
    fn sanitize_filename_fallbacks_and_limits() {
        // 空/纯点 → download 兜底
        assert_eq!(sanitize_filename(""), "download");
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename(" . "), "download");
        // 首尾点裁剪（防隐藏文件），中间点保留
        assert_eq!(sanitize_filename(".hidden"), "hidden");
        assert_eq!(sanitize_filename("file.txt"), "file.txt");
        // 首尾空白裁剪
        assert_eq!(sanitize_filename("  报表.xlsx  "), "报表.xlsx");
        // 限长 200
        let long = "x".repeat(300);
        assert_eq!(sanitize_filename(&long).chars().count(), 200);
    }
}
