//! 证书管理 / 监测 / 验证（P1-P3 · 2026-09-22）
//!
//! - P1 看证书：`get_tls_info`（web_view.tls_info → gio 取值 → CertInfo）
//! - P2 验证/放行：挂钩 `load-failed-with-tls-errors`，拦截时缓存证书，
//!   `accept_cert_exception` 调 `allow_tls_certificate_for_host` 放行并落库
//! - P3 监测：每次读取即比对指纹（变更→事件），`scan_cert_expiry` 扫临期
//!
//! 注意：gio/gtk 对象非 Send，仅能在主线程操作；证书缓存用 thread_local。

use tauri::Manager;

use crate::error::{AppError, CommandResult};

// 证书信息采集与解析（CertInfo / gio 对象转换），拆出以控制本文件行数
#[path = "tls/cert.rs"]
mod cert;
pub use cert::CertInfo;
#[cfg(target_os = "linux")]
use cert::{build_info, collect, flags_to_names};
use cert::parse_host_port;

#[cfg(target_os = "linux")]
use gtk::gio;
#[cfg(target_os = "linux")]
use webkit2gtk::{WebContextExt, WebViewExt};

#[cfg(target_os = "linux")]
thread_local! {
    /// 最近一次 TLS 错误的证书缓存（host → 证书），供"继续前往"放行使用。
    /// gio 对象非 Send，只能在主线程访问。
    static PENDING_TLS_CERTS: std::cell::RefCell<std::collections::HashMap<String, gio::TlsCertificate>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// TLS 错误回调（在 GTK 信号里调，主线程）：缓存证书 + 落事件
#[cfg(target_os = "linux")]
pub fn on_tls_error(
    app: &tauri::AppHandle,
    tab_id: &str,
    failing_uri: &str,
    cert: &gio::TlsCertificate,
    flags: gio::TlsCertificateFlags,
) {
    let (host, port) = parse_host_port(failing_uri);
    // 缓存证书以供放行
    PENDING_TLS_CERTS.with(|m| {
        m.borrow_mut().insert(host.clone(), cert.clone());
    });
    let info = build_info(cert, flags, &host, port);
    let names = flags_to_names(flags);
    log::warn!("[cert] TLS 错误 host={} flags={:?}", host, names);
    use tauri::Emitter;
    let _ = app.emit(
        crate::contracts::events::TLS_ERROR,
        serde_json::json!({
            "tabId": tab_id,
            "host": host,
            "port": port,
            "flags": flags.bits(),
            "reasons": names,
            "fingerprint": info.fingerprint_sha256,
            "subject": info.subject,
            "issuer": info.issuer,
            "notAfter": info.not_after,
        }),
    );
}

// ================= P1：读取证书 =================

/// 读取指定标签当前页证书信息；顺带记录/更新监测快照（P3）
#[tauri::command]
pub async fn get_tls_info(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    id: String,
) -> CommandResult<Option<CertInfo>> {
    let mut info = read_webview_tls(&app, &id).await?;
    if let Some(ref mut ci) = info {
        // P3 监测：记录 + 变更检测
        record_ci(&db.pool, ci).await;
        ci.is_exception = crate::db::certificates::has_exception(&db.pool, &ci.host, ci.port as i64, &ci.fingerprint_sha256).await;
    }
    Ok(info)
}

/// 记录证书快照并写事件（首次/变更/校验失败）
async fn record_ci(pool: &sqlx::SqlitePool, ci: &CertInfo) {
    if let Some(kind) = crate::db::certificates::record(pool, ci).await {
        let (sev, detail) = match kind {
            "first_seen" => ("info", format!("首次记录 {} 的证书（fp {}）", ci.host, ci.short_fp())),
            "cert_changed" => ("warn", format!("证书指纹变更 → {}（疑似中间人/轮换）", ci.short_fp())),
            "verify_error" => ("danger", format!("证书校验失败：{}", ci.error_flags.join("、"))),
            _ => ("info", String::new()),
        };
        crate::db::certificates::add_event(pool, &ci.host, ci.port as i64, kind, sev, &detail).await;
    }
}

/// 页面加载完成后的自动监测（P3，由 tabs.rs 调用）：采集证书 + 记录/变更检测 + 落事件
pub async fn monitor(app: &tauri::AppHandle, id: &str) {
    let db = app.state::<crate::db::Db>();
    if let Ok(Some(ci)) = read_webview_tls(app, id).await {
        record_ci(&db.pool, &ci).await;
    }
}

#[cfg(target_os = "linux")]
async fn read_webview_tls(app: &tauri::AppHandle, id: &str) -> Result<Option<CertInfo>, AppError> {
    let window = app.get_window("main").ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let label = format!("tab-{}", id);
    let wv = window.get_webview(&label).ok_or_else(|| AppError::internal("webview 不存在"))?;
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<CertInfo>>();
    wv.with_webview(move |pwv| {
        let g = pwv.inner();
        let _ = tx.send(collect(&g));
    })
    .map_err(|e| AppError::internal(e.to_string()))?;
    // 主线程事件循环可能繁忙，加超时兜底，避免永久等待
    match tokio::time::timeout(std::time::Duration::from_secs(3), rx).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) => Err(AppError::internal("读取 TLS 信息失败（通道关闭）")),
        Err(_) => Err(AppError::timeout("读取 TLS 信息超时")),
    }
}

#[cfg(not(target_os = "linux"))]
async fn read_webview_tls(_app: &tauri::AppHandle, _id: &str) -> Result<Option<CertInfo>, AppError> {
    Err(AppError::validation("证书读取仅支持 Linux"))
}

// ================= 管理 / 监测命令 =================

#[tauri::command]
pub async fn list_certificates(
    db: tauri::State<'_, crate::db::Db>,
) -> CommandResult<Vec<crate::db::certificates::CertRecord>> {
    crate::db::certificates::list(&db.pool)
        .await
        .map_err(|e| AppError::db(e.to_string()).into())
}

/// 取单个 host:port 的监测记录（详情用）
#[tauri::command]
pub async fn get_certificate(
    db: tauri::State<'_, crate::db::Db>,
    host: String,
    port: i64,
) -> CommandResult<Option<crate::db::certificates::CertRecord>> {
    crate::db::certificates::get(&db.pool, &host, port)
        .await
        .map_err(|e| AppError::db(e.to_string()).into())
}

#[tauri::command]
pub async fn delete_certificate(
    db: tauri::State<'_, crate::db::Db>,
    host: String,
    port: i64,
) -> CommandResult<()> {
    crate::db::certificates::delete(&db.pool, &host, port)
        .await
        .map_err(|e| AppError::db(e.to_string()).into())
}

#[tauri::command]
pub async fn list_cert_exceptions(
    db: tauri::State<'_, crate::db::Db>,
) -> CommandResult<Vec<crate::db::certificates::CertException>> {
    crate::db::certificates::list_exceptions(&db.pool)
        .await
        .map_err(|e| AppError::db(e.to_string()).into())
}

#[tauri::command]
pub async fn delete_cert_exception(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    host: String,
    port: i64,
    fingerprint: String,
) -> CommandResult<()> {
    crate::db::certificates::delete_exception(&db.pool, &host, port, &fingerprint)
        .await
        .map_err(|e| AppError::db(e.to_string()))?;
    crate::db::certificates::add_event(&db.pool, &host, port, "exception_removed", "info", "取消证书例外").await;
    // 撤销后清理 WebKit 侧例外并重载（可能需要重新校验）
    #[cfg(target_os = "linux")]
    {
        let host2 = host.clone();
        let _ = app.run_on_main_thread(move || {
            PENDING_TLS_CERTS.with(|m| {
                m.borrow_mut().remove(&host2);
            });
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn list_cert_events(
    db: tauri::State<'_, crate::db::Db>,
    limit: Option<i64>,
) -> CommandResult<Vec<crate::db::certificates::CertEvent>> {
    crate::db::certificates::list_events(&db.pool, limit.unwrap_or(200))
        .await
        .map_err(|e| AppError::db(e.to_string()).into())
}

#[tauri::command]
pub async fn clear_cert_events(db: tauri::State<'_, crate::db::Db>) -> CommandResult<()> {
    crate::db::certificates::clear_events(&db.pool)
        .await
        .map_err(|e| AppError::db(e.to_string()).into())
}

/// 扫描临期/已过期证书（days 天内），写事件并返回列表
#[tauri::command]
pub async fn scan_cert_expiry(
    db: tauri::State<'_, crate::db::Db>,
    days: Option<i64>,
) -> CommandResult<Vec<crate::db::certificates::CertRecord>> {
    let days = days.unwrap_or(30);
    let list = crate::db::certificates::expiring_within(&db.pool, days)
        .await
        .map_err(|e| AppError::db(e.to_string()))?;
    for c in &list {
        let (kind, sev) = match c.days_left {
            Some(d) if d < 0 => ("expired", "danger"),
            Some(d) if d <= 7 => ("expiry_warn", "danger"),
            _ => ("expiry_warn", "warn"),
        };
        let detail = format!(
            "证书{}（剩 {} 天，{}）",
            if c.days_left.unwrap_or(0) < 0 { "已过期" } else { "临期" },
            c.days_left.unwrap_or(0),
            c.issuer.clone().unwrap_or_default()
        );
        crate::db::certificates::add_event(&db.pool, &c.host, c.port, kind, sev, &detail).await;
    }
    Ok(list)
}

// ================= P2：证书例外（继续前往） =================

/// 放行证书例外：WebKit 加白 + 落库 + 重载该标签
#[tauri::command]
pub async fn accept_cert_exception(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    id: String,
    host: String,
    port: i64,
    fingerprint: String,
    reason: Option<String>,
) -> CommandResult<()> {
    let reason = reason.unwrap_or_else(|| "用户确认继续前往".into());
    // 落库
    crate::db::certificates::add_exception(&db.pool, &host, port, &fingerprint, &reason)
        .await
        .map_err(|e| AppError::db(e.to_string()))?;
    crate::db::certificates::add_event(&db.pool, &host, port, "accepted", "warn", &format!("用户放行证书例外：{}", reason)).await;

    #[cfg(target_os = "linux")]
    {
        let host2 = host.clone();
        let id2 = id.clone();
        let app2 = app.clone();
        // 必须在主线程操作 GTK 对象
        app.run_on_main_thread(move || {
            let Some(window) = app2.get_window("main") else {
                return;
            };
            let label = format!("tab-{}", id2);
            let Some(wv) = window.get_webview(&label) else {
                return;
            };
            let _ = wv.with_webview(move |pwv| {
                let g = pwv.inner();
                // 1) 把缓存的证书加入 WebKit 白名单
                PENDING_TLS_CERTS.with(|m| {
                    if let Some(cert) = m.borrow().get(&host2) {
                        if let Some(ctx) = WebViewExt::context(&g) {
                            ctx.allow_tls_certificate_for_host(cert, &host2);
                            log::info!("[cert] 已放行 {} 的证书例外", host2);
                        }
                    } else {
                        log::warn!("[cert] 放行失败：{} 无缓存的错误证书", host2);
                    }
                });
                // 2) 重新加载
                WebViewExt::reload(&g);
            });
        })
        .map_err(|e| AppError::internal(e.to_string()))?;
    }
    Ok(())
}
