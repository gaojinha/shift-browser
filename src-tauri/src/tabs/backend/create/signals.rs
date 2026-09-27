//! Linux 信号接线：load_failed / TLS 错误 / 权限请求 / 崩溃恢复 / 标题同步
//! （create_webview 的子步骤，整个模块仅 Linux 编译）

use tauri::{AppHandle, Emitter, Manager};

use super::super::super::model::{now_ts, TabManager};
use super::super::is_internal_newtab_url;
use super::super::WEB_CRASH_COOLDOWN;

use webkit2gtk::WebViewExt;
use crate::engine::webview::WebViewBackend;

/// 给新建 child webview 接 GTK 信号：加载失败 / TLS 错误 / 权限 / 崩溃恢复 / 标题同步。
/// 监听 WebKit title 变化 → 实时同步标签标题（on_page_load 的 host 只是初始占位）
pub(super) fn wire_linux_signals(child: &tauri::Webview, app: &AppHandle, id: &str) {
    let app_for_title = app.clone();
    let id_for_title = id.to_string();
    let _ = child.with_webview(move |pwv| {
        let wv = pwv.inner().clone();
        let app2 = app_for_title;
        let tab_id = id_for_title;
        // 先注册加载失败监听（clone 后再 move 原值给 title_notify）
        let app_fail = app2.clone();
        let tab_id_fail = tab_id.clone();
        wv.connect_load_failed(move |_wv, _load_event, failing_uri, err| {
            log::warn!(
                "[tabs] load_failed: tab={} uri={} err={}",
                tab_id_fail,
                failing_uri,
                err.message()
            );
            // 阻止 WebKit 默认错误页显示（返回 true = 阻止默认行为）
            let _ = app_fail.emit(
                crate::contracts::events::PAGE_LOAD_FAILED,
                (tab_id_fail.clone(), failing_uri.to_string()),
            );
            true
        });
        // TLS 证书错误拦截（P2 · 2026-09-22）：缓存证书 + 通知前端弹"危险站点"页；
        // 返回 true 阻止 WebKit 默认错误页，是否放行交给前端"继续前往"决定。
        let app_tls = app2.clone();
        let tab_id_tls = tab_id.clone();
        wv.connect_load_failed_with_tls_errors(
            move |_wv, failing_uri, certificate, flags| {
                crate::tls::on_tls_error(
                    &app_tls,
                    &tab_id_tls,
                    failing_uri,
                    certificate,
                    flags,
                );
                true
            },
        );
        // 权限请求处理（安全 #2 · 2026-09-22）：默认挂起 + 白名单放行 + 通知前端
        crate::perms::attach(app2.clone(), tab_id.clone(), &wv);
        // Web 进程终止恢复（视频崩溃修复 · 2026-09-22）：崩溃/内存超限时
        // 自动重载一次（带 30s 冷却），并通知前端提示，避免标签直接变白。
        let app_crash = app2.clone();
        let tab_id_crash = tab_id.clone();
        wv.connect_web_process_terminated(move |webview, reason| {
            log::error!(
                "[tabs] web 进程终止: tab={} reason={:?}",
                tab_id_crash,
                reason
            );
            let now = now_ts();
            let map = WEB_CRASH_COOLDOWN.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
            let allow_reload = {
                let mut m = map.lock().unwrap_or_else(|e| e.into_inner());
                let last = m.get(&tab_id_crash).copied().unwrap_or(0);
                if now - last >= 30 {
                    m.insert(tab_id_crash.clone(), now);
                    true
                } else {
                    false
                }
            };
            use tauri::Emitter;
            let _ = app_crash.emit(
                crate::contracts::events::WEB_PROCESS_CRASHED,
                serde_json::json!({
                    "tabId": tab_id_crash,
                    "reason": format!("{:?}", reason),
                    "autoReload": allow_reload,
                }),
            );
            if allow_reload {
                log::info!("[tabs] 自动重载崩溃标签: {}", tab_id_crash);
                crate::engine::webview::WebViewBackend::reload(webview);
            }
        });
        // 监听 WebKit title 变化 → 实时同步标签标题
        wv.connect_title_notify(move |webview| {
            if let Some(t) = webview.title() {
                let title = t.trim().to_string();
                if title.is_empty() {
                    return;
                }
                // 内部启动页（shift://newtab/worldnav）标题固定"启动页"：
                // 页面自身的 <title>（"新标签页"）不覆盖，否则启动页标签
                // 刚出来一下又变成"新标签页"
                if let Some(uri) = WebViewBackend::uri(webview) {
                    if is_internal_newtab_url(&uri) {
                        return;
                    }
                }
                // 标题变化 → 更新内存态；仅在真的变化时通知前端（防高频重复 emit）。
                // 先读后写：无变化时不进 mutate，避免版本号空转带崩前端差分
                let changed = {
                    if let Some(state) = app2.try_state::<TabManager>() {
                        let need = state
                            .read_state()
                            .tabs
                            .get(&tab_id)
                            .map(|t| t.title != title)
                            .unwrap_or(false);
                        if need {
                            state.mutate(|s| {
                                if let Some(tab) = s.tabs.get_mut(&tab_id) {
                                    tab.title = title;
                                }
                            });
                        }
                        need
                    } else {
                        false
                    }
                };
                if changed {
                    let _ = app2.emit(crate::contracts::events::TABS_UPDATED, ());
                }
            }
        });
    });
}
