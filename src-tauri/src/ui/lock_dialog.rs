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

    // 跨轮状态：上一轮失败提示（红字回显）+ 用户名回填（免得每次重输）
    let mut err_msg = String::new();
    let mut last_username = String::new();
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

        let label = gtk::Label::new(Some("输入账号密码解锁浏览器（数据都在，解开即回）"));
        content.add(&label);

        let user_entry = gtk::Entry::new();
        user_entry.set_placeholder_text(Some("用户名"));
        if !last_username.is_empty() {
            user_entry.set_text(&last_username);
        }
        content.add(&user_entry);

        let pass_entry = gtk::Entry::new();
        pass_entry.set_visibility(false);
        pass_entry.set_placeholder_text(Some("密码"));
        pass_entry.set_activates_default(true);
        content.add(&pass_entry);

        // 上一轮失败原因（红字提示）：错了让用户知道错哪，还剩几次，锁多久
        if !err_msg.is_empty() {
            let err_label = gtk::Label::new(Some(err_msg.as_str()));
            // GTK 主题无语义色保证，用内联样式兜底
            let provider = gtk::CssProvider::new();
            let _ = provider.load_from_data(b"label { color: #ff6b6b; }");
            err_label
                .style_context()
                .add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            err_label.set_line_wrap(true);
            content.add(&err_label);
        }

        dialog.add_button("退出", gtk::ResponseType::Cancel);
        dialog.add_button("解锁", gtk::ResponseType::Ok);

        dialog.show_all();
        pass_entry.grab_focus();

        let response = dialog.run();

        let username = user_entry.text().to_string();
        let password = pass_entry.text().to_string();
        last_username = username.clone();

        dialog.close();

        if response != gtk::ResponseType::Ok {
            log::info!("[ShiftBrowser] 启动锁屏取消，退出应用");
            std::process::exit(0);
        }

        // 防爆破（与登录同口径：5 次错锁 30 秒）：锁住时不验证，直接提示剩余秒数
        {
            use crate::db::users::{guard_get, now_ms};
            let (_, locked_until) = guard_get(username.trim());
            let now = now_ms();
            if locked_until > now {
                let remain = (locked_until - now) / 1000 + 1;
                err_msg = format!("尝试次数过多，请 {} 秒后再试", remain);
                log::warn!("[ShiftBrowser] 启动锁屏命中防爆破锁定，剩余 {} 秒", remain);
                continue;
            }
        }

        match tauri::async_runtime::block_on(crate::db::users::verify_password(
            db_state, &username, &password,
        )) {
            Ok(true) => {
                log::info!("[ShiftBrowser] 启动锁屏验证通过: {}", username);
                crate::db::users::guard_clear(username.trim());
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
                // 记一次失败：5 次后锁 30 秒（与登录同口径），并把剩余次数告诉用户
                crate::db::users::guard_record_failure(username.trim());
                let (fails, locked_until) =
                    crate::db::users::guard_get(username.trim());
                if locked_until > crate::db::users::now_ms() {
                    err_msg = "密码错误次数过多，已锁定 30 秒".to_string();
                } else {
                    let left = crate::db::users::MAX_LOGIN_FAILURES.saturating_sub(fails);
                    err_msg = format!("用户名或密码错误（还剩 {} 次机会）", left);
                }
            }
            Err(e) => {
                log::error!("[ShiftBrowser] 启动锁屏验证出错: {}", e);
                err_msg = "验证出错，请重试".to_string();
            }
        }
    }

    true
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn show_startup_lock(_app: &tauri::App, _db_state: &crate::db::Db) -> bool {
    false
}
