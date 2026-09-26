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

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

// R2-1c 拆分（2026-09-24）：命令分发（原 handle() 的 match）独立成子模块
// P3 桥接（2026-09-26）：webqa::read_page_selection 要复用 handlers::content::selection，
// 故提到 pub(crate)。仍在 crate 内部，不改变 api.sock 的对外协议。
pub(crate) mod handlers;
// R2-1c 拆分（2026-09-24）：状态面板 + 模型网关独立成子模块
pub(crate) mod dashboard;
// R2-1c 拆分（2026-09-24）：token/限速/IP 白名单等鉴权辅助独立成子模块
pub(crate) mod auth;
// R2-1c 拆分（2026-09-24）：HTTP 服务器（状态面板/模型网关/命令端点）独立成子模块
pub(crate) mod http;

use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::RwLock;

/// 套接字路径（应用数据目录，与 vault.key/sync.key 同级）
pub fn socket_path(app: &AppHandle) -> Option<PathBuf> {
    let db = app.try_state::<crate::db::Db>()?;
    Some(db.vault_path.with_file_name("api.sock"))
}

/// 最小 percent-encode（查询串用；不引额外依赖）
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 最小 JS 字符串转义（嵌入单引号 JS 字面量用；与 extensions/adblock.rs 先例一致）
fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// 起 API 服务（lib.rs setup 调用）
pub fn spawn(app: AppHandle) {
    let Some(path) = socket_path(&app) else {
        log::warn!("[api] 无法确定套接字路径，开放 API 未启动");
        return;
    };
    // 清理上次残留
    let _ = std::fs::remove_file(&path);
    // 同时起本地 HTTP API（阶段4 自托管服务端套件最小版）——必须在下面 move app 之前克隆。
    // token/tickets 以 Tauri state 共享：rotate_api_token / create_dashboard_ticket
    // 两个命令与 HTTP 服务操作同一份运行期状态
    if let Some(token) = auth::ensure_token(&app) {
        let token_shared = auth::ApiTokenShared(Arc::new(RwLock::new(token)));
        app.manage(token_shared.clone());
        app.manage(auth::DashboardTickets::new());
        http::spawn_http(app.clone(), token_shared);
    } else {
        log::warn!("[api] api-token 初始化失败，HTTP API 未启动");
    }
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
        Err(e) => {
            return crate::error::AppError::validation(format!("请求 JSON 非法: {}", e))
                .error_json()
        }
    };
    let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    // 只读模式（api_readonly=true）：拒绝一切写命令。
    // 写命令身份唯一来源：capabilities 注册表的 Risk 分类（P0-2，2026-09-24），
    // 不再在本文件维护手写第二份名单。
    if crate::capabilities::is_write(cmd) {
        let db = app.state::<crate::db::Db>();
        let ro = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'api_readonly'",
        )
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
        if ro {
            return crate::error::AppError::permission(format!(
                "只读模式已开启，写命令 {} 被拒绝",
                cmd
            ))
            .error_json();
        }
    }
    // R2-1c（2026-09-24）：命令分发已拆到 api/handlers.rs
    handlers::dispatch(app, &req, cmd).await
}

/// 模糊测试支撑（HANDOFF P2-5）
///
/// `fuzz/` 是独立 crate，无法访问本模块私有解析器；此 shim 仅在 `fuzz` feature
/// （或 lib 单测）下编译，对外签名只用基本类型，把任意字节/字符串喂入手写解析面。
/// 约定：**任何输入都不得 panic / 死循环**，只能返回结果码。
#[cfg(any(test, feature = "fuzz"))]
pub mod fuzz_helpers {
    
    use std::net::IpAddr;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, ReadBuf};

    /// 按给定绝对切点把 data 拆成任意大小的 read 返回，切点用尽后 EOF。
    /// 模拟 TCP 任意拆包/粘包；构造时自动排序去重并补齐末尾，永不产生 0 字节"假 EOF"
    pub(crate) struct ChaosReader {
        data: Vec<u8>,
        cuts: Vec<usize>,
        cut_idx: usize,
        pos: usize,
    }

    impl ChaosReader {
        pub(crate) fn new(data: Vec<u8>, mut cuts: Vec<usize>) -> Self {
            cuts.retain(|&c| c > 0 && c < data.len());
            cuts.sort_unstable();
            cuts.dedup();
            cuts.push(data.len());
            Self {
                data,
                cuts,
                cut_idx: 0,
                pos: 0,
            }
        }
    }

    impl AsyncRead for ChaosReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            loop {
                if this.cut_idx >= this.cuts.len() {
                    return Poll::Ready(Ok(())); // 真 EOF
                }
                let end = this.cuts[this.cut_idx].min(this.data.len());
                let n = end.saturating_sub(this.pos).min(buf.remaining());
                this.cut_idx += 1;
                if n == 0 {
                    continue; // 跳过落在当前位置之前的切点，不发 0 字节读
                }
                buf.put_slice(&this.data[this.pos..this.pos + n]);
                this.pos += n;
                return Poll::Ready(Ok(()));
            }
        }
    }

    /// 驱动 HTTP/1.1 解析器。返回：0=Complete，1=TooLarge，2=Bad
    pub async fn parse_http(data: Vec<u8>, cuts: Vec<usize>) -> u8 {
        let mut r = ChaosReader::new(data, cuts);
        match super::http::read_http_request(&mut r).await {
            super::http::HttpReq::Complete(..) => 0,
            super::http::HttpReq::TooLarge => 1,
            super::http::HttpReq::Bad => 2,
        }
    }

    /// percent 解码（任意输入不得 panic）
    pub fn url_decode(s: &str) -> String {
        super::http::url_decode(s)
    }

    /// form-urlencoded 取值
    pub fn kv_value(encoded: &str, key: &str) -> Option<String> {
        super::http::kv_value(encoded, key)
    }

    /// Content-Length 解析
    pub fn content_length(head: &str) -> Option<usize> {
        super::http::content_length_of(head)
    }

    /// Transfer-Encoding: chunked 识别
    pub fn is_chunked(head: &str) -> bool {
        super::http::is_chunked(head)
    }

    /// CIDR 匹配。返回：1=true，0=false，-1=非法 CIDR，-2=非法 IP
    pub fn cidr_contains(ip: &str, cidr: &str) -> i8 {
        match ip.parse::<IpAddr>() {
            Ok(ip) => match super::auth::cidr_contains(ip, cidr) {
                Some(true) => 1,
                Some(false) => 0,
                None => -1,
            },
            Err(_) => -2,
        }
    }
}

