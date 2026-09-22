//! 标签模块工具函数（P1-1 拆分 · 2026-09-22）
//! 纯函数，无平台依赖，可独立单测。

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）
#![allow(clippy::unwrap_used, clippy::expect_used)]

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
        log::info!("[privacy] URL 追踪参数已清洗: {} -> {}", crate::redact_url(&url), crate::redact_url(&cleaned));
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
