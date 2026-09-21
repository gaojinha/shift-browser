//! 本地开放 API（阶段 4 · 开放生态）
//!
//! 让外部程序（脚本/工具）驱动浏览器。选 **Unix 域套接字 + 行式 JSON**：
//! - 套接字文件权限 0600（仅本用户可连），不做网络监听，天然本地隔离；
//! - 协议：每行一个 JSON 请求 → 每行一个 JSON 响应。
//!
//! 命令：
//! - `{"cmd":"status"}`                     → `{"ok":true,"data":{"version":…,"tabs":N}}`
//! - `{"cmd":"list_tabs"}`                  → `{"ok":true,"data":[Tab…]}`
//! - `{"cmd":"open_url","url":"https://…"}` → `{"ok":true,"data":Tab}`
//!
//! 用法示例：
//! ```bash
//! printf '{"cmd":"list_tabs"}\n' | socat - UNIX-CONNECT:~/.local/share/com.shiftbrowser.app/api.sock
//! ```

use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

/// 套接字路径（应用数据目录，与 vault.key/sync.key 同级）
pub fn socket_path(app: &AppHandle) -> Option<PathBuf> {
    let db = app.try_state::<crate::db::Db>()?;
    Some(db.vault_path.with_file_name("api.sock"))
}

/// 起 API 服务（lib.rs setup 调用）
pub fn spawn(app: AppHandle) {
    let Some(path) = socket_path(&app) else {
        log::warn!("[api] 无法确定套接字路径，开放 API 未启动");
        return;
    };
    // 清理上次残留
    let _ = std::fs::remove_file(&path);
    tauri::async_runtime::spawn(async move {
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log::warn!("[api] 绑定 {} 失败: {}", path.display(), e);
                return;
            }
        };
        // 权限 0600：仅本用户可连
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        log::info!("[api] 开放 API 已启动: {}", path.display());
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let (r, mut w) = stream.into_split();
                        let mut lines = BufReader::new(r).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let resp = handle(&app, &line).await;
                            let mut s = serde_json::to_string(&resp)
                                .unwrap_or_else(|_| "{\"ok\":false}".to_string());
                            s.push('\n');
                            if w.write_all(s.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    });
                }
                Err(e) => log::warn!("[api] accept 失败: {}", e),
            }
        }
    });
}

/// 处理一条请求
async fn handle(app: &AppHandle, line: &str) -> Value {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"ok": false, "error": format!("请求 JSON 非法: {}", e)}),
    };
    let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    match cmd {
        "status" => {
            let tabs = app
                .try_state::<crate::tabs::TabManager>()
                .map(|s| s.all_tabs().len())
                .unwrap_or(0);
            json!({"ok": true, "data": {"version": env!("CARGO_PKG_VERSION"), "tabs": tabs}})
        }
        "list_tabs" => {
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            json!({"ok": true, "data": state.all_tabs()})
        }
        "open_url" => {
            let url = req
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if url.is_empty() {
                return json!({"ok": false, "error": "缺少 url"});
            }
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::create_tab(app.clone(), state, url).await {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "close_tab" | "switch_tab" | "reload_tab" => {
            let id = req
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() {
                return json!({"ok": false, "error": "缺少 id"});
            }
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            let r = match cmd {
                "close_tab" => crate::tabs::close_tab(app.clone(), state, id).await,
                "switch_tab" => crate::tabs::switch_tab(app.clone(), state, id).await,
                _ => crate::tabs::reload_tab(app.clone(), state, id).await,
            };
            match r {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "navigate" => {
            let id = req
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let url = req
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() || url.is_empty() {
                return json!({"ok": false, "error": "缺少 id 或 url"});
            }
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::navigate_tab(app.clone(), state, id, url).await {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "active_tab" => {
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            let active = state.all_tabs().into_iter().find(|t| t.is_active);
            json!({"ok": true, "data": active})
        }
        _ => json!({"ok": false, "error": format!("未知命令: {}", cmd)}),
    }
}
