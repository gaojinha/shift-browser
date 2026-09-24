// 锁屏对话框（R2-1b 拆分：从 lib.rs run()/setup 抽出）
//
// 启动锁屏：设置启用后，在创建任何 webview 之前弹 GTK 原生密码对话框。
// 注意：必须在 GTK 层面实现——Vue 锁屏覆盖层只能盖住 Vue 自己的 DOM，
// 盖不住另一个独立的 GTK webview（标签页）。

#[cfg(target_os = "linux")]
use gtk::prelude::*;
use tauri::Manager;

/// 浏览器是否处于锁定状态（启动锁屏期间为 true；前端 is_locked 查到后静默等待 app-unlocked）
pub(crate) static LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 启动锁屏状态查询（invoke handler 注册给前端）
pub(crate) fn is_locked() -> bool {
    LOCKED.load(std::sync::atomic::Ordering::SeqCst)
}

/// 启动锁屏：设置启用后弹 GTK 原生密码对话框验证账号密码。
///
/// 返回是否经历过锁屏启用（调用方据此决定是否发 `app-unlocked` 让前端重试初始化
/// ——锁屏期间 Db/TabManager 未就绪，主窗口 Vue 的 onMounted 初始化全部失败）。
///
/// 语义保证：锁屏启用时若用户取消（点"退出"）直接 `exit(0)`，不会正常返回；
/// 正常返回即代表解锁成功（或未启用锁屏）。
#[cfg(target_os = "linux")]
pub(crate) fn show_startup_lock(app: &tauri::App, db_state: &crate::db::Db) -> bool {
    let lock_enabled: bool = {
        let row = tauri::async_runtime::block_on(
            sqlx::query_as::<_, (String,)>(
                "SELECT value FROM settings WHERE key = 'lock_enabled'",
            )
            .fetch_optional(&db_state.pool),
        )
        .ok()
        .flatten();
        row.map(|r| r.0 == "true").unwrap_or(false)
    };

    if !lock_enabled {
        return false;
    }

    log::info!("[ShiftBrowser] 启动锁屏已启用，弹出 GTK 密码对话框");
    // 标记锁屏中：前端 is_locked 查到 true 后静默等待 app-unlocked，
    // 不再反复重试建标签（否则弹"创建标签页失败"toast）
    LOCKED.store(true, std::sync::atomic::Ordering::SeqCst);
    let parent_win = app
        .get_window("main")
        .and_then(|w| w.gtk_window().ok());

    loop {
        let dialog = gtk::Dialog::builder()
            .title("Shift Browser 已锁定")
            .modal(true)
            .resizable(false)
            .default_width(340)
            .default_height(160)
            .build();

        if let Some(ref pw) = parent_win {
            dialog.set_transient_for(Some(pw));
        }

        let content = dialog.content_area();
        content.set_spacing(8);
        content.set_margin(12);

        let label = gtk::Label::new(Some("输入账号密码解锁浏览器"));
        content.add(&label);

        let user_entry = gtk::Entry::new();
        user_entry.set_placeholder_text(Some("用户名"));
        content.add(&user_entry);

        let pass_entry = gtk::Entry::new();
        pass_entry.set_visibility(false);
        pass_entry.set_placeholder_text(Some("密码"));
        pass_entry.set_activates_default(true);
        content.add(&pass_entry);

        dialog.add_button("退出", gtk::ResponseType::Cancel);
        dialog.add_button("解锁", gtk::ResponseType::Ok);

        dialog.show_all();
        pass_entry.grab_focus();

        let response = dialog.run();

        let username = user_entry.text().to_string();
        let password = pass_entry.text().to_string();

        dialog.close();

        if response != gtk::ResponseType::Ok {
            log::info!("[ShiftBrowser] 启动锁屏取消，退出应用");
            std::process::exit(0);
        }

        match tauri::async_runtime::block_on(crate::db::users::verify_password(
            db_state, &username, &password,
        )) {
            Ok(true) => {
                log::info!("[ShiftBrowser] 启动锁屏验证通过: {}", username);
                // 验证通过后清除会话：启动锁屏只验证身份，不自动登录个人中心
                if let Err(e) = tauri::async_runtime::block_on(crate::db::users::clear_session(db_state))
                {
                    log::warn!("[ShiftBrowser] 启动锁屏验证通过但会话清除失败: {}", e);
                }
                // 显式 destroy 锁屏 dialog，避免 close 后残留；forget 阻止 drop 再次释放
                unsafe {
                    dialog.destroy();
                }
                std::mem::forget(dialog);
                // 立即恢复主窗口前台/焦点——否则用户必须点一下窗体才能交互
                // （modal dialog 关闭后焦点不自动回到父窗口）
                if let Some(ref pw) = parent_win {
                    pw.present();
                }
                // 解锁完成：清除锁屏标志（前端收到 app-unlocked 后开始初始化）
                LOCKED.store(false, std::sync::atomic::Ordering::SeqCst);
                break;
            }
            Ok(false) => {
                log::warn!("[ShiftBrowser] 启动锁屏密码错误，重新输入");
            }
            Err(e) => {
                log::error!("[ShiftBrowser] 启动锁屏验证出错: {}", e);
            }
        }
    }

    true
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn show_startup_lock(_app: &tauri::App, _db_state: &crate::db::Db) -> bool {
    false
}