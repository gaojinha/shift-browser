// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(feature = "cef-engine")]
    {
        // ① 必须早于任何 gtk_init：CEF 默认用 X11 子窗口嵌入，Wayland 后端拿不到 XID。
        //    2026-10-29：改为跟随 SHIFT_CEF_OZONE（默认 x11），否则外部传
        //    GDK_BACKEND=wayland 会被这里覆盖，纯 Wayland（无 DISPLAY）下 GTK
        //    X11 后端 init 失败 → WebContext 直接 panic。
        let ozone = std::env::var("SHIFT_CEF_OZONE").unwrap_or_else(|_| "x11".to_string());
        if ozone == "wayland" {
            std::env::set_var("GDK_BACKEND", "wayland");
        } else {
            std::env::set_var("GDK_BACKEND", "x11");
        }

        // ② CEF 会用 /proc/self/exe 拉起渲染/GPU/zygote 子进程（命令行带 --type=xxx）。
        //    子进程必须在这里返回，绝不能继续初始化 Tauri。
        if shift_browser_lib::cef_try_run_subprocess() {
            return;
        }
    }

    shift_browser_lib::run();
}
