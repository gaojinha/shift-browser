//! CEF 启动期引导：子进程判定与 profile/缓存目录解析（从 `cef_cmd.rs` 拆出）。
//!
//! 拆出理由：这两块与 Tauri 命令注册无关，是"进程启动最早期的环境决策"，
//! 混在 500 行的命令文件里既难读也触发 400 行红线（2026-10-31）。

use cef::ImplCommandLine;
use tauri::{AppHandle, Manager};

/// **子进程入口**：CEF 用 `/proc/self/exe` 拉起渲染/GPU/zygote 子进程，
/// 子进程的命令行带 `--type=xxx`。命中时必须在这里跑完 CEF 子进程逻辑并**直接退出**，
/// 绝不能再往下走 Tauri 初始化（否则就是无限套娃 + 单实例误判）。
///
/// 返回 `true` 表示当前进程是 CEF 子进程且已处理完毕，调用方应立即 return。
pub fn try_run_subprocess() -> bool {
    use cef::{args::Args, execute_process, CefString};

    // 必须在任何 CEF 调用之前
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        return false;
    };

    let type_switch = CefString::from("type");
    if cmd_line.has_switch(Some(&type_switch)) != 1 {
        // 主进程
        return false;
    }

    let ret = execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    // ret >= 0：子进程已跑完；< 0：类型未知，交给主流程
    ret >= 0
}

/// CEF profile / 缓存根目录（**单一来源**）。
///
/// 优先级：`SHIFT_CEF_CACHE_DIR`（非空）→ 应用缓存目录下的 `cef/`。
///
/// 为什么必须是单一来源：CEF 的 Cookies/LocalStorage 落在这个目录里；工具栏页
/// 与启动页也写在它的 `shell/` 子目录。2026-10-26 实测踩坑——`init` 读了 env、
/// `open_shell_on_main` 自己又算了一次 `app_cache_dir()`，结果 profile 用了临时目录、
/// 工具栏页仍读 `~/.cache/com.shiftbrowser.app/shell/toolbar.html`（旧页面/旧会话），
/// 真机验证直接不可信。两个调用点从此只许调本函数。
pub fn resolve_cef_cache_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = match std::env::var("SHIFT_CEF_CACHE_DIR") {
        Ok(d) if !d.trim().is_empty() => std::path::PathBuf::from(d),
        _ => app
            .path()
            .app_cache_dir()
            .map(|p| p.join("cef"))
            .map_err(|e| format!("拿不到缓存目录: {e}"))?,
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("建缓存目录失败: {e}"))?;
    Ok(dir)
}
