//! 证书信息采集与解析（CertInfo 数据结构 + gio 证书对象 → CertInfo 转换）
//!
//! 注意：gio/gtk 对象非 Send，collect/build_info 仅能在主线程操作。

use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use gtk::{gio, glib};
#[cfg(target_os = "linux")]
use gtk::gio::prelude::TlsCertificateExt;
#[cfg(target_os = "linux")]
use webkit2gtk::WebViewExt;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CertInfo {
    pub host: String,
    pub port: u16,
    pub fingerprint_sha256: String,
    pub subject: String,
    pub issuer: String,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    pub dns_names: Vec<String>,
    pub ip_addresses: Vec<String>,
    /// TlsCertificateFlags 位图（0 = 无错误）
    pub errors: u32,
    pub error_flags: Vec<String>,
    pub chain: Vec<String>,
    pub valid: bool,
    /// 该证书是否已有用户放行例外
    pub is_exception: bool,
}

impl CertInfo {
    pub fn short_fp(&self) -> String {
        self.fingerprint_sha256.chars().take(16).collect()
    }
}

#[cfg(target_os = "linux")]
pub(super) fn flags_to_names(flags: gio::TlsCertificateFlags) -> Vec<String> {
    let mut out = Vec::new();
    let pairs = [
        (gio::TlsCertificateFlags::UNKNOWN_CA, "未知的 CA（自签或未受信任的签发者）"),
        (gio::TlsCertificateFlags::BAD_IDENTITY, "证书与主机名不匹配"),
        (gio::TlsCertificateFlags::NOT_ACTIVATED, "证书尚未生效"),
        (gio::TlsCertificateFlags::EXPIRED, "证书已过期"),
        (gio::TlsCertificateFlags::REVOKED, "证书已被吊销"),
        (gio::TlsCertificateFlags::INSECURE, "证书使用了不安全的算法"),
        (gio::TlsCertificateFlags::GENERIC_ERROR, "证书校验通用错误"),
    ];
    for (f, name) in pairs {
        if flags.contains(f) {
            out.push(name.to_string());
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn fmt_dt(dt: Option<glib::DateTime>) -> Option<String> {
    dt.and_then(|d| d.format("%Y-%m-%d %H:%M:%S").ok().map(|s| s.to_string()))
}

/// 从 webkit WebView 收集证书信息
#[cfg(target_os = "linux")]
pub(crate) fn collect(wv: &webkit2gtk::WebView) -> Option<CertInfo> {
    let (cert, flags) = WebViewExt::tls_info(wv)?;
    let uri = WebViewExt::uri(wv).map(|s| s.to_string()).unwrap_or_default();
    let (host, port) = parse_host_port(&uri);
    Some(build_info(&cert, flags, &host, port))
}

#[cfg(target_os = "linux")]
pub(crate) fn build_info(
    cert: &gio::TlsCertificate,
    flags: gio::TlsCertificateFlags,
    host: &str,
    port: u16,
) -> CertInfo {
    // 指纹：DER → SHA-256
    let fp = cert
        .certificate()
        .map(|der| {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(&*der);
            hex::encode(h.finalize())
        })
        .unwrap_or_default();

    // 证书链：逐级取 issuer
    let mut chain = Vec::new();
    let mut cur = Some(cert.clone());
    let mut guard = 0;
    while let Some(c) = cur {
        if guard > 8 {
            break;
        }
        guard += 1;
        if let Some(name) = c.subject_name() {
            chain.push(name.to_string());
        }
        cur = c.issuer();
    }

    CertInfo {
        host: host.to_string(),
        port,
        fingerprint_sha256: fp,
        subject: cert.subject_name().map(|s| s.to_string()).unwrap_or_default(),
        issuer: cert.issuer_name().map(|s| s.to_string()).unwrap_or_default(),
        not_before: fmt_dt(cert.not_valid_before()),
        not_after: fmt_dt(cert.not_valid_after()),
        dns_names: cert
            .dns_names()
            .into_iter()
            .map(|b| String::from_utf8_lossy(&b).trim().to_string())
            .collect(),
        ip_addresses: cert.ip_addresses().into_iter().map(|a| a.to_string()).collect(),
        errors: flags.bits(),
        error_flags: flags_to_names(flags),
        chain,
        valid: flags.is_empty(),
        is_exception: false,
    }
}

pub(super) fn parse_host_port(uri: &str) -> (String, u16) {
    match url::Url::parse(uri) {
        Ok(u) => (
            u.host_str().unwrap_or("").to_string(),
            u.port_or_known_default().unwrap_or(443),
        ),
        Err(_) => (String::new(), 443),
    }
}
