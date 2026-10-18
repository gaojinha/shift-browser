//! Linux 信号接线：load_failed / TLS 错误 / 权限请求 / 崩溃恢复 / 标题同步
//! （create_webview 的子步骤，整个模块仅 Linux 编译）

use tauri::{AppHandle, Emitter, Manager};

use super::super::super::model::{now_ts, TabManager};
use super::super::is_internal_newtab_url;
use super::super::WEB_CRASH_COOLDOWN;

use crate::engine::webview::WebViewBackend;
use webkit2gtk::WebViewExt;

/// 崩溃错误页（data: 内联，无外部资源）：渲染进程连崩且在冷却窗内时展示。
/// 按 Ctrl+R 手动刷新。标题"网页崩溃"会同步到标签栏（title-notify 照常）。
const CRASH_ERROR_HTML: &str = "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">\
<title>网页崩溃</title><style>body{background:#0e1626;color:#e9eef8;\
font:15px/1.7 -apple-system,\"PingFang SC\",\"Microsoft YaHei\",sans-serif;\
display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0}\
.box{max-width:520px;padding:32px;text-align:center}\
h1{font-size:22px;margin:0 0 12px}p{color:#bcc8dd;margin:8px 0}\
code{color:#8a97b0;font-size:13px}</style></head><body><div class=\"box\">\
<h1>网页崩溃了</h1><p>这个页面短时间内多次崩溃，已停止自动重载。</p>\
<p>按 Ctrl+R 手动刷新试试；一直崩就是这个站与当前引擎犯冲。</p>\
<code>ShiftBrowser · 崩溃保护</code></div></body></html>";

/// 崩溃冷却判定（纯函数可单测）：距上次自动重载 ≥30s 才允许再重载（防崩溃循环），
/// 判定通过即记录本次时间。时间单位与 `now_ts` 一致（秒）。
pub(super) const CRASH_COOLDOWN_SECS: i64 = 30;

fn crash_reload_allowed(
    cooldowns: &mut std::collections::HashMap<String, i64>,
    tab_id: &str,
    now: i64,
) -> bool {
    let last = cooldowns.get(tab_id).copied().unwrap_or(0);
    if now.saturating_sub(last) >= CRASH_COOLDOWN_SECS {
        cooldowns.insert(tab_id.to_string(), now);
        true
    } else {
        false
    }
}

/// 给新建 child webview 接 GTK 信号：加载失败 / TLS 错误 / 权限 / 崩溃恢复 / 标题同步。
/// 监听 WebKit title 变化 → 实时同步标签标题（on_page_load 的 host 只是初始占位）
pub(super) fn wire_linux_signals(child: &tauri::Webview, app: &AppHandle, id: &str) {
    let app_for_title = app.clone();
    let id_for_title = id.to_string();
    crate::log_if_err!("webview", child.with_webview(move |pwv| {
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
            // 放行 WebKit 默认错误页（返回 false = 不阻止默认行为）：打不开就该去
            // 错误页（含重试），而不是静默白屏/旧页。TLS 走下面的专用拦截（有危险
            // 站点 UI），不受影响。事件照发，前端提示条照常。
            let _ = app_fail.emit(
                crate::contracts::events::PAGE_LOAD_FAILED,
                (tab_id_fail.clone(), failing_uri.to_string()),
            );
            false
        });
        // TLS 证书错误拦截（P2 · 2026-09-22）：缓存证书 + 通知前端弹"危险站点"页；
        // 返回 true 阻止 WebKit 默认错误页，是否放行交给前端"继续前往"决定。
        let app_tls = app2.clone();
        let tab_id_tls = tab_id.clone();
        wv.connect_load_failed_with_tls_errors(move |_wv, failing_uri, certificate, flags| {
            crate::tls::on_tls_error(&app_tls, &tab_id_tls, failing_uri, certificate, flags);
            true
        });
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
            let map = WEB_CRASH_COOLDOWN
                .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
            let allow_reload = crash_reload_allowed(&mut map.lock().unwrap_or_else(|e| e.into_inner()), &tab_id_crash, now);
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
            } else {
                // 30s 内连崩：不再自动重载（防崩溃循环），改载中文错误页——
                // 无响应/崩溃就该去错误页，而不是白屏干等，更不能弹应用级强退框。
                // data: 伪页面不进存量 url/历史（uri-notify/page_load 有同口径 guard）。
                log::warn!("[tabs] 崩溃冷却中，载错误页: {}", tab_id_crash);
                webview.load_html(CRASH_ERROR_HTML, None);
            }
        });
        // 地址监听与标题监听各持一份 owned 拷贝（'static 闭包不借函数参数）
        let app_for_uri = app2.clone();
        let id_for_uri = tab_id.clone();
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
        // 监听 WebKit 地址变化 → 实时同步标签地址（与 title-notify 对称）。
        // 单页应用内路由（pushState/replaceState）只换地址不整页加载，
        // 没有它地址栏永远是旧的，标签栏名字（标题）却已是新的——两岔。
        wv.connect_uri_notify(move |webview| {
            let Some(uri) = WebViewBackend::uri(webview) else {
                return;
            };
            let uri = uri.trim().to_string();
            // 伪页面（崩溃错误页 data:/空白页）不进存量：否则地址栏变 data:...。
            // 口径见 contracts::urls::is_pseudo_page（page_load/历史共用）。
            if uri.is_empty() || crate::contracts::urls::is_pseudo_page(&uri) {
                return;
            }
            let changed = {
                if let Some(state) = app_for_uri.try_state::<TabManager>() {
                    let need = state
                        .read_state()
                        .tabs
                        .get(&id_for_uri)
                        .map(|t| t.url != uri)
                        .unwrap_or(false);
                    if need {
                        state.mutate(|s| {
                            if let Some(tab) = s.tabs.get_mut(&id_for_uri) {
                                tab.url = uri;
                            }
                        });
                    }
                    need
                } else {
                    false
                }
            };
            if changed {
                let _ = app_for_uri.emit(crate::contracts::events::TABS_UPDATED, ());
            }
        });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 首次崩溃允许重载并记录时间() {
        let mut m = std::collections::HashMap::new();
        assert!(crash_reload_allowed(&mut m, "t1", 1000));
        assert_eq!(m.get("t1"), Some(&1000), "判定通过须记录本次时间");
    }

    #[test]
    fn 冷却窗口内连崩不再重载() {
        let mut m = std::collections::HashMap::new();
        assert!(crash_reload_allowed(&mut m, "t1", 1000));
        assert!(!crash_reload_allowed(&mut m, "t1", 1029), "29s 内不允许");
        assert!(
            crash_reload_allowed(&mut m, "t1", 1030),
            "恰好 30s 边界应放行（>= 语义）"
        );
        assert_eq!(m.get("t1"), Some(&1030), "放行后时间戳刷新");
    }

    #[test]
    fn 冷却按标签隔离互不影响() {
        let mut m = std::collections::HashMap::new();
        assert!(crash_reload_allowed(&mut m, "t1", 1000));
        assert!(crash_reload_allowed(&mut m, "t2", 1001), "别的标签不受 t1 冷却影响");
    }

    #[test]
    fn 时间倒退按饱和减法放行() {
        let mut m = std::collections::HashMap::new();
        assert!(crash_reload_allowed(&mut m, "t1", 2000));
        // now < last（时钟回拨）：saturating_sub 得 0 < 30 不放行，防异常时钟触发循环重载
        assert!(!crash_reload_allowed(&mut m, "t1", 1999));
    }
}
