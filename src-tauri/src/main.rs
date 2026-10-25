// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(feature = "cef-engine")]
    {
        // ① 必须早于任何 gtk_init：CEF 用 X11 子窗口嵌入，Wayland 后端拿不到 XID
        std::env::set_var("GDK_BACKEND", "x11");

        // ② CEF 会用 /proc/self/exe 拉起渲染/GPU/zygote 子进程（命令行带 --type=xxx）。
        //    子进程必须在这里返回，绝不能继续初始化 Tauri。
        if shift_browser_lib::cef_try_run_subprocess() {
            return;
        }
    }

    shift_browser_lib::run();
}
