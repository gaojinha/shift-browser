// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

// 标签页管理器
// 管理标签元数据 + 嵌入式 webview 生命周期
// 与 Electron 版 electron/tabManager.ts 对应（v4.5.0）
//
// R2-1 拆分（2026-09-24）：
// - 纯数据状态机（Tab/TabManager/NavState）→ tabs/model.rs
// - GTK/webview 后端实现 → tabs/backend.rs
// - IPC 命令门面 → tabs/commands.rs
// - 工具函数 → tabs/util.rs
// 本文件仅保留模块声明与重导出。

use crate::db::Db;
use tauri::AppHandle;

#[cfg(target_os = "linux")]
// P1-1 拆分（2026-09-22）：工具函数独立成子模块
#[path = "tabs/util.rs"]
pub mod util;
pub(crate) use util::clean_tracking_params;

// R2-1 拆分（2026-09-24）：纯数据状态机独立成子模块
#[path = "tabs/model.rs"]
pub mod model;
pub use model::{NavState, Tab, TabManager};
pub(crate) use model::now_ts;

// R2-1 拆分（2026-09-24）：GTK/webview 后端实现独立成子模块
#[path = "tabs/backend.rs"]
pub mod backend;
pub(crate) use backend::*;

pub mod commands;