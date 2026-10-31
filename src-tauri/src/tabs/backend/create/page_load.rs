//! on_page_load 处理：延迟注入 + 标题/证书/favicon/历史记录（create_webview 的子步骤）

use tauri::{AppHandle, Emitter, Manager};
use url::Url;

use crate::db::{history, Db};

use super::super::super::model::TabManager;
use super::super::is_internal_newtab_url;

/// 延迟注入重试时序（ms）：800ms 首捕 + 3s/8s 兜底（脚本幂等，重复注入无害）
pub(super) const INJECT_RETRY_DELAYS_MS: [u64; 3] = [800, 3000, 8000];

/// 证书快照采集前等 TLS 信息就绪的延迟（ms）
pub(super) const TLS_MONITOR_DELAY_MS: u64 = 700;

/// 标签页标题推导：内部启动页 → "启动页"；无 host（data:/file: 等）→ url 原样；否则 → host
pub(super) fn derive_display_title(url: &Url) -> String {
    if is_internal_newtab_url(url.as_str()) {
        return "启动页".to_string();
    }
    let host = url.host_str().unwrap_or("").to_string();
    if host.is_empty() {
        url.to_string()
    } else {
        host
    }
}

/// favicon 是否可用：非空且不是 about: 页
pub(super) fn should_apply_favicon(fav: &str) -> bool {
    !fav.is_empty() && !fav.starts_with("about:")
}

/// 标题是否还是占位（空或仍等于 url）：是则允许用 host 占位覆盖，避免覆盖 document.title
pub(super) fn title_needs_placeholder(title: &str, url: &str) -> bool {
    title.is_empty() || title == url
}

/// 是否跳过历史记录：伪页面（口径 contracts::urls::is_pseudo_page）、无痕标签、AI 标签一律不记
pub(super) fn skip_history_for(url: &str, private: bool, origin: &str) -> bool {
    crate::contracts::urls::is_pseudo_page(url) || private || origin == "ai"
}

/// record_history 开关解析：未设置默认关（隐私优先），仅显式 "true" 开启
pub(super) fn history_record_enabled(enabled: Option<String>) -> bool {
    enabled.map(|v| v == "true").unwrap_or(false)
}

/// 页面加载事件处理（Started/Finished 都触发）：
/// 任何事件都安排延迟注入；Finished 时额外做标题同步、证书监测、favicon 提取、历史记录。
pub(super) fn handle_page_load(
    webview: &tauri::Webview,
    payload: &tauri::webview::PageLoadPayload,
    app: &AppHandle,
    tab_id: &str,
) {
    // 开启页面 console 输出（诊断 JS 错误用）——任何加载事件都设置，
    // 不依赖 Finished（部分页面 Finished 不触发）
    crate::log_if_err!(
        "webview",
        webview.with_webview(move |pwv| {
            crate::engine::webview::enable_console_output(&pwv.inner());
        })
    );
    // 任何加载事件（Started/Finished，含后退/前进缓存恢复）都安排延迟注入：
    // - UserScript 只对"之后导航"生效，已加载页面必须 evaluate 补注入；
    // - 后退/前进时 WebKit 从 back-forward 缓存恢复页面，UserScript 不会重新执行，
    //   返回的页面会丢脚本 → 必须重新 evaluate（800ms/3s/8s 重试，幂等无害）。
    let app_ext = app.clone();
    let label_ext = webview.label().to_string();
    tauri::async_runtime::spawn(async move {
        for delay in INJECT_RETRY_DELAYS_MS {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            crate::extensions::inject_to_webview(&app_ext, &label_ext).await;
        }
    });
    // 选区缓存监听（2026-09-26）：用户"选中文字 → 点侧栏按钮"时，点击会让页面
    // 失焦、浏览器立刻清空选区，按钮 handler 再来读就啥都没有（真机实测：
    // 「翻译选中」永远报"没读到选中内容"）。这里在页面加载后注入监听，把选中的
    // Range 克隆存到 window 上，读取端回退使用。
    // 实测 `WebviewBuilder::initialization_script` 在这条构建路径下没生效，
    // 所以走 on_page_load 注入；脚本自带幂等（__sbSelWatch），重复注入无害。
    crate::log_if_err!(
        "webview",
        webview.with_webview(move |pwv| {
            crate::engine::webview::eval_js_with(
                &pwv.inner(),
                include_str!("../../../page_selwatch.js"),
                |r| {
                    if let Err(e) = r {
                        log::warn!("[webzh] 选区缓存监听注入失败: {e}");
                    }
                },
            );
        })
    );

    if payload.event() == tauri::webview::PageLoadEvent::Finished {
        let url = payload.url();
        // 内部页面（shift://newtab / shift://worldnav）标题固定"启动页"，
        // 不显示 host（否则标签页显示 "newtab"）；无 host 的 url 原样显示
        let title = derive_display_title(url);

        log::info!(
            "[tabs] page_load_finished: tab={} host={}",
            tab_id,
            crate::redact_url(url.as_ref())
        );

        // 证书监测（P3 · 2026-09-22）：HTTPS 页面加载完成后采集并记录证书快照，
        // 指纹变更/校验失败自动落事件（延迟 700ms 等 TLS 信息就绪）
        if url.scheme() == "https" {
            let app_cert = app.clone();
            let tab_id_cert = tab_id.to_string();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(TLS_MONITOR_DELAY_MS)).await;
                crate::tls::monitor(&app_cert, &tab_id_cert).await;
            });
        }

        // 提取 favicon 并更新 tab（更新后 emit 通知前端同步）
        let app_fav = app.clone();
        let tab_id_fav = tab_id.to_string();
        crate::log_if_err!("webview", webview.with_webview(move |pwv| {
            let gtk_wv = pwv.inner();
            crate::engine::webview::eval_js_with(
                &gtk_wv,
                "(function(){var l=document.querySelector('link[rel=\"icon\"],link[rel=\"shortcut icon\"]');var u=l?l.href:'';if(!u&&location.protocol!=='file:')u=location.origin+'/favicon.ico';return u;})()",
                move |res| {
                    if let Ok(fav) = res {
                        if should_apply_favicon(&fav) {
                            log::debug!("[tabs] favicon: tab={} {}", tab_id_fav, fav);
                            // 先读后写：无变化不进 mutate，版本号不空转
                            let state = app_fav.state::<TabManager>();
                            let need = state
                                .read_state()
                                .tabs
                                .get(&tab_id_fav)
                                .map(|t| t.favicon != fav)
                                .unwrap_or(false);
                            if need {
                                state.mutate(|s| {
                                    if let Some(t) = s.tabs.get_mut(&tab_id_fav) {
                                        t.favicon = fav;
                                    }
                                });
                            }
                            // favicon 更新 → 通知前端同步
                            if need {
                                let _ = app_fav.emit(crate::contracts::events::TABS_UPDATED, ());
                            }
                        }
                    }
                },
            );
        }));

        // 按设置记录浏览历史（record_history 开关，默认关 · 隐私优先）——先 clone，避免 title 被 move
        let app3 = app.clone();
        let url_str = url.to_string();
        let title_opt = Some(title.clone());

        // 伪页面（崩溃错误页 data:/空白页）不进存量 url、不记历史：
        // 否则地址栏变 data:...、历史里多一堆垃圾。标题照常走（标签栏显示"网页崩溃"是对的）。
        // 口径见 contracts::urls::is_pseudo_page（uri-notify 共用）。
        let is_pseudo = crate::contracts::urls::is_pseudo_page(url.as_str());
        // 无痕标签 / AI 标签一律不记历史（N1：AI 读 100 页不污染用户浏览历史；无痕 2026-09-22）。
        // 用标志位而非提前 return，保证前端同步事件照常发出（AI 标签也要显示在标签栏）。
        let mut skip_history = is_pseudo;
        if let Some(state) = app.try_state::<TabManager>() {
            state.mutate(|s| {
                if let Some(tab) = s.tabs.get_mut(tab_id) {
                    if !is_pseudo {
                        tab.url = url.to_string();
                    }
                    // 仅当还没有真标题（title-notify 未触发过）时用 host 占位，避免覆盖 document.title
                    if title_needs_placeholder(&tab.title, &tab.url) {
                        tab.title = title;
                    }
                }
                if let Some(t) = s.tabs.get(tab_id) {
                    skip_history = skip_history_for(url.as_str(), t.private, &t.origin);
                }
            });
        }
        // 页面加载完成（URL/标题变化）→ 通知前端同步（替代 2s 轮询）
        let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());

        tauri::async_runtime::spawn(async move {
            if skip_history {
                return;
            }
            let db = app3.state::<Db>();
            // 性能 P0：走 settings 缓存（原每次页面加载都查库）
            let enabled = crate::db::settings_cache::get(&db.pool, "record_history").await;
            // 隐私优先（2026-09-22）：未显式开启则不记录历史
            let record = history_record_enabled(enabled);
            if record {
                if let Err(e) = history::record_history(db, url_str, title_opt).await {
                    log::warn!("[tabs] record_history 失败: {}", e);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).expect("测试 URL 应可解析")
    }

    #[test]
    fn display_title_internal_newtab() {
        // 内部启动页（shift://newtab / worldnav，含旧 file:// 版）→ 固定"启动页"
        assert_eq!(derive_display_title(&u("shift://newtab")), "启动页");
        assert_eq!(derive_display_title(&u("shift://worldnav")), "启动页");
        assert_eq!(
            derive_display_title(&u(
                "file:///home/gao/.local/share/com.shiftbrowser.app/newtab/index.html"
            )),
            "启动页"
        );
    }

    #[test]
    fn display_title_host_and_fallback() {
        // 普通页面 → host（host_str 不含端口，口径锁死）
        assert_eq!(
            derive_display_title(&u("https://example.com/path?q=1")),
            "example.com"
        );
        assert_eq!(
            derive_display_title(&u("https://sub.example.com:8080/x")),
            "sub.example.com"
        );
        // 无 host（data:/file:）→ url 原样
        assert_eq!(
            derive_display_title(&u("data:text/html,<b>x</b>")),
            "data:text/html,<b>x</b>"
        );
        assert_eq!(
            derive_display_title(&u("file:///home/gao/x.html")),
            "file:///home/gao/x.html"
        );
    }

    #[test]
    fn favicon_apply_gate() {
        assert!(!should_apply_favicon(""));
        assert!(!should_apply_favicon("about:blank"));
        assert!(should_apply_favicon("https://example.com/favicon.ico"));
        assert!(should_apply_favicon("/favicon.ico"));
    }

    #[test]
    fn title_placeholder_gate() {
        // 空标题或标题仍等于 url → 允许 host 占位；真标题不覆盖
        assert!(title_needs_placeholder("", "https://example.com/"));
        assert!(title_needs_placeholder(
            "https://example.com/",
            "https://example.com/"
        ));
        assert!(!title_needs_placeholder("真实标题", "https://example.com/"));
    }

    #[test]
    fn history_skip_decision() {
        // 伪页面（data:/about:/blank:，contracts::urls::is_pseudo_page 同口径）→ 一律跳过
        assert!(skip_history_for("data:text/html,x", false, "web"));
        assert!(skip_history_for("about:blank", false, "web"));
        assert!(skip_history_for("blank:whatever", false, "web"));
        // 无痕 / AI 标签 → 跳过
        assert!(skip_history_for("https://example.com/", true, "web"));
        assert!(skip_history_for("https://example.com/", false, "ai"));
        // 普通页面、非无痕、非 AI → 记录
        assert!(!skip_history_for("https://example.com/", false, "web"));
    }

    #[test]
    fn history_record_enabled_default_off() {
        // 隐私优先：未设置默认关，仅显式 "true" 开启
        assert!(!history_record_enabled(None));
        assert!(history_record_enabled(Some("true".into())));
        assert!(!history_record_enabled(Some("false".into())));
        assert!(!history_record_enabled(Some("1".into())));
    }

    #[test]
    fn delay_constants_no_drift() {
        // 防漂移：注入重试时序与证书监测延迟（数字来自代码原文）
        assert_eq!(INJECT_RETRY_DELAYS_MS, [800, 3000, 8000]);
        assert_eq!(TLS_MONITOR_DELAY_MS, 700);
    }
}
