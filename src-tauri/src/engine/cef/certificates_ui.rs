//! CEF 工具栏的证书面板后端（F-CEF-21）。
//!
//! 列表数据在生成工具栏 HTML 时内嵌（只含面板展示用的元信息，字段与 WebKit 版
//! `CertificatesDialog` 的 `CertRecord` 对齐；不含证书链、例外详情等敏感/超量字段）；
//! 删除走 `SHIFT_ACT:certdel|<host>:<port>`（certificates 表主键即 host:port，无自增 id；
//! 二次确认在页面侧）。
//!
//! 接线说明（协调器接线时补，本模块不改 shell.rs / toolbar.rs）：
//! - `shell.rs::handle_action` 在密码面板之后同模式路由：`super::certificates_ui::handle(cmd, arg)`；
//! - 面板开合/视图加高：shell.rs 增证书面板 setter（同 `set_pw_panel` 模式），certpanel 分支接通；
//! - `certrefresh` 需经 `shell::run_toolbar_js` 重新执行 `window.__shiftCertsRender` 重渲染列表
//!   （`run_toolbar_js` 目前是 shell.rs 私有函数，本模块不可见，接线时一并处理）；
//! - 工具栏 HTML 生成处（cef_cmd.rs 的 `passwords_ui::init/passwords_json` 旁）加
//!   `certificates_ui::init(app)` + `certificates_json(app)` 内嵌。
//!
//! 本模块先交付后接线：二狗接 `shell.rs::handle_action` 与 cef_cmd.rs HTML 内嵌之前
//! （本卡禁改这两文件），整模块暂无业务引用——scoped dead_code 豁免（同
//! `capabilities/agent_actions.rs` 先例），接线完成后删掉下一行 `#![allow]`。
#![allow(dead_code)]

use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// 证书元信息 JSON（host/port/主体/签发者/有效期/指纹/变更数/last_seen/days_left），
/// 供内嵌进工具栏页面。不含 chain_json（证书链）等敏感或超量字段。
pub fn certificates_json(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "[]".to_string();
    };
    match tauri::async_runtime::block_on(crate::db::certificates::list_certificates(
        &db.pool,
    )) {
        Ok(rows) => {
            log::info!("[cef] 证书列表同步读取 {} 条（仅元信息）", rows.len());
            let list: Vec<serde_json::Value> = rows
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "host": c.host,
                        "port": c.port,
                        "subject": c.subject,
                        "issuer": c.issuer,
                        "not_before": c.not_before,
                        "not_after": c.not_after,
                        "fingerprint_sha256": c.fingerprint_sha256,
                        "change_count": c.change_count,
                        "last_seen": c.last_seen,
                        "days_left": c.days_left,
                    })
                })
                .collect();
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
        }
        Err(e) => {
            log::warn!("[cef] 读证书列表失败: {e}");
            "[]".to_string()
        }
    }
}

/// 解析删除动作参数 `host:port`（IPv6 支持括号形 `[::1]:8443`，也接受裸形 `::1:8443`）。
fn parse_host_port(arg: &str) -> Option<(String, i64)> {
    let (host, port) = arg.rsplit_once(':')?;
    let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port.parse().ok()?))
}

/// 处理工具栏动作。返回 true = 已处理。必须在 GTK 主线程调用（handle_action 本就在主线程）。
pub fn handle(cmd: &str, arg: &str) -> bool {
    match cmd {
        "certpanel" => {
            // TODO(F-CEF-21 接线)：shell.rs 加证书面板 setter（同 set_pw_panel 模式）后接通：
            // super::shell::set_cert_panel(arg == "1");
            log::info!("[cef] certpanel {arg}（面板高度管理待 shell.rs 接线）");
            true
        }
        "certrefresh" => {
            // TODO(F-CEF-21 接线)：经 shell::run_toolbar_js 执行 window.__shiftCertsRender()
            // 重渲染工具栏证书列表（run_toolbar_js 为 shell.rs 私有函数，接线时处理）。
            log::info!("[cef] certrefresh（列表重渲染待 shell.rs 接线）");
            true
        }
        "certdel" => {
            match parse_host_port(arg) {
                Some((host, port)) => delete_record(&host, port),
                None => log::warn!("[cef] certdel 参数无效: {arg}（应为 host:port）"),
            }
            true
        }
        _ => false,
    }
}

fn delete_record(host: &str, port: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    match tauri::async_runtime::block_on(crate::db::certificates::delete(&db.pool, host, port)) {
        Ok(()) => log::info!("[cef] 已删除证书记录 {host}:{port}"),
        Err(e) => log::warn!("[cef] 删除证书记录失败 {host}:{port}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_host_port_forms() {
        assert_eq!(
            parse_host_port("example.com:443"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(
            parse_host_port("[::1]:8443"),
            Some(("::1".into(), 8443))
        );
        assert_eq!(parse_host_port("::1:8443"), Some(("::1".into(), 8443)));
        // 畸形参数：无端口 / 端口非数字 / 空 host / 空串
        assert_eq!(parse_host_port("example.com"), None);
        assert_eq!(parse_host_port("example.com:abc"), None);
        assert_eq!(parse_host_port(":443"), None);
        assert_eq!(parse_host_port(""), None);
    }

    #[test]
    fn unknown_cmd_not_handled() {
        assert!(!handle("pwcopy", "1"));
        assert!(!handle("nav", "https://example.com"));
    }
}
