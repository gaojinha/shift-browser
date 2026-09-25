//! 杂项命令：锁屏状态查询 / 前端日志桥 / 一键自检 / 关于页信息
//! （体量小、无共同域，集中安放以保持 lib.rs 只做胶水）

/// 查询是否处于启动锁屏状态（前端初始化前调用，不依赖 Db）
#[tauri::command]
pub(crate) fn is_locked() -> bool {
    crate::lock_dialog::is_locked()
}

/// 前端日志桥：把前端关键节点的执行情况写进 Rust 日志，
/// 便于排查"前端白屏/事件收不到/初始化不执行"这类看不到 console 的问题。
#[tauri::command]
pub(crate) fn front_log(msg: String) {
    log::info!("[front] {}", msg);
}

/// 一键自检：汇总版本/存储/运行时/能力状态（只读，不修改任何状态）
#[tauri::command]
pub(crate) async fn selfcheck_cmd(app: tauri::AppHandle) -> Result<crate::selfcheck::SelfCheckReport, String> {
    let report = crate::selfcheck::run(&app).await;
    log::info!(
        "[selfcheck] 健康={} 版本项={} 存储项={} 运行时项={} 能力项={}",
        report.healthy,
        report.versions.len(),
        report.storage.len(),
        report.runtime.len(),
        report.capabilities.len()
    );
    // 关键环境事实写入日志（排查问题时第一手资料：版本/引擎/路径）
    let env_line: Vec<String> = report
        .versions
        .iter()
        .map(|i| format!("{}={}", i.name, i.detail))
        .collect();
    log::info!("[selfcheck] 环境: {}", env_line.join(" | "));
    // 能力状态也记一行（过滤器是否就绪、拦截是否开启、隐私档位）
    let cap_line: Vec<String> = report
        .capabilities
        .iter()
        .map(|i| format!("{}={}", i.name, i.detail))
        .collect();
    log::info!("[selfcheck] 能力: {}", cap_line.join(" | "));
    for item in report
        .versions
        .iter()
        .chain(report.storage.iter())
        .chain(report.runtime.iter())
        .chain(report.capabilities.iter())
    {
        if !item.ok {
            log::warn!("[selfcheck] 异常项: {} - {}", item.name, item.detail);
        }
    }
    Ok(report)
}

/// 「关于」页信息（只读，打开即显示，不依赖自检）
#[tauri::command]
pub(crate) fn app_about_cmd(app: tauri::AppHandle) -> crate::about::AboutInfo {
    let info = crate::about::collect(&app);
    log::info!(
        "[about] {} v{} · WebKitGTK {} · {} · {}",
        info.app_name,
        info.version,
        info.webkit_version,
        info.os_arch,
        info.profile
    );
    info
}
