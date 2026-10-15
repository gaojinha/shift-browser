// LINES-EXEMPT: 标签生命周期命令（建/切/关/导同域），拆分排期 R3（2026-10-14 红线卡标注）
//! 标签生命周期命令（R1 二次拆分 · 2026-09-24）：
//! 创建 / 关闭 / 全部关闭 / 切换 / 空闲休眠。
//! 本文件 unwrap/expect 均为安全用法（Mutex 毒化恢复）。

use super::super::*;
use crate::error::{AppError, CommandResult};
use tauri::{Emitter, Manager};

#[tauri::command]
pub async fn create_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    url: String,
    private: Option<bool>,
    after_active: Option<bool>,
    // J6：后台打开（true = 不抢焦点，保持当前活动标签；默认 false = 现行为）
    background: Option<bool>,
    // 延迟加载（会话恢复用）：http(s) 页先建占位，切到该标签时再载真址；
    // AI 标签与内部页传 false 直接真载（读内容不能等）
    defer: Option<bool>,
) -> CommandResult<Tab> {
    let is_private = private.unwrap_or(false);
    // 新标签插入位置：true = 插到当前活动标签之后（"在列表页右边打开"）；默认追加末尾
    let insert_after = after_active.unwrap_or(false);
    let background = background.unwrap_or(false);
    let defer = defer.unwrap_or(false);
    // 隐私防护：新建标签前清洗跨站追踪参数（只影响 http/https，内部页面不受影响）
    let url = clean_tracking_params(&url);
    log::info!("[tabs] create_tab: url={}", crate::redact_url(&url));
    // P2：URL scheme 白名单（安全闸，口径见 validate_create_url）
    validate_create_url(&url)?;
    let id = TabManager::uuid();
    // 建页限流：WebKit 建页是重操作，同时最多 4 个，恢复会话连建时排队（阈值见 contracts::limits）
    static CREATE_SEM: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    let sem = CREATE_SEM.get_or_init(|| {
        tokio::sync::Semaphore::new(crate::contracts::limits::TAB_CREATE_CONCURRENCY)
    });
    let _permit = sem
        .acquire()
        .await
        .map_err(|e| crate::error::AppError::internal(format!("建页限流器异常: {}", e)))?;
    // 本地新标签页（file:// 路径含 /newtab/ 或 shift://newtab/worldnav）标题显示"启动页"，
    // 不显示冗长 URL/host。注意：协议改版后 URL 是 shift://newtab，旧的 contains("/newtab/")
    // 判断匹配不到，必须按 scheme+host 判断
    let title = if is_internal_newtab_url(&url) {
        "启动页".to_string()
    } else {
        url.clone()
    };

    // 延迟占位：defer 且 http(s) 时 webview 先建本地占位页，真址记账，
    // 切到该标签时再载（会话恢复连建不打爆内存与并发）
    let deferred_url = TabManager::deferred_placeholder(defer, &url);
    let webview_url = deferred_url.unwrap_or(&url);
    TabManager::create_webview(&app, &id, webview_url, is_private)?;
    log::info!(
        "[tabs] create_tab: webview 创建成功 id={} private={} deferred={}",
        id,
        is_private,
        deferred_url.is_some()
    );

    let tab = Tab {
        id: id.clone(),
        url,
        title,
        favicon: String::new(),
        is_active: !background,
        is_pinned: false,
        is_muted: false,
        hibernated: false,
        private: is_private,
        origin: "user".to_string(),
    };
    state.touch_active(&id);

    // 更新状态：新标签激活，旧标签取消激活（background 时保持当前活动标签不变）
    // 单写锁内一次改完，中途无观察者（状态收敛实施）
    let old_active = state.mutate(|s| {
        let prev = s.active_id.clone();
        if !background {
            for t in s.tabs.values_mut() {
                t.is_active = false;
            }
        }
        s.tabs.insert(id.clone(), tab.clone());
        // 延迟账：占位页的真址记账，切标签时载入并销账
        if deferred_url.is_some() {
            s.deferred_loads.insert(id.clone(), tab.url.clone());
        }
        // 插入位置：after_active=true 时插到旧活动标签之后（无活动标签则追加末尾）
        let pos = if insert_after {
            prev.as_ref()
                .and_then(|p| s.order.iter().position(|x| x == p))
                .map(|p| p + 1)
                .unwrap_or(s.order.len())
        } else {
            s.order.len()
        };
        if insert_after {
            log::info!(
                "[tabs] create_tab: 插到活动标签之后 pos={}/{}",
                pos,
                s.order.len()
            );
        }
        s.order.insert(pos, id.clone());
        if background {
            prev
        } else {
            s.active_id.replace(id.clone())
        }
    });

    // 新建标签页可见性：前台标签立即切换（show 新、hide 旧），避免新 webview 透明期透出旧页面；
    // 后台标签（J6）保持隐藏，用户当前页面继续显示，webview 仍在后台加载。
    if background {
        let _ = TabManager::set_webview_visible(&app, &id, false);
    } else if let Some(old) = old_active {
        let _ = TabManager::switch_visibility(&app, &id, Some(&old));
    } else {
        let _ = TabManager::set_webview_visible(&app, &id, true);
    }

    // 建完推同步：API 开的标签前端不知情（切过去都找不到），推一次即跟上。
    // 会话恢复连建时会有 N 次，前端是事件驱动单次同步，开销可接受。
    let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
    Ok(tab)
}

/// 批量建标签（会话恢复用，一次 IPC 带走一组地址）
/// 语义：逐个走 create_tab（限流加延迟占位照吃），单个失败跳过不整批炸；
/// 返回与入参等长、按位对齐的数组（失败位为 None），前端按下标认领分组。
/// 私有/内部页照常：defer 只对 http(s) 生效，其余直接真载（见 deferred_placeholder）。
#[tauri::command]
pub async fn create_tabs_batch(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    urls: Vec<String>,
) -> CommandResult<Vec<Option<Tab>>> {
    log::info!("[tabs] create_tabs_batch: {} 个地址", urls.len());
    let mut out = Vec::with_capacity(urls.len());
    for url in urls {
        match create_tab(
            app.clone(),
            state.clone(),
            url.clone(),
            None,
            None,
            None,
            Some(true),
        )
        .await
        {
            Ok(tab) => out.push(Some(tab)),
            Err(e) => {
                log::warn!(
                    "[tabs] 批量建标签跳过失败项: {} ({})",
                    crate::redact_url(&url),
                    e
                );
                out.push(None);
            }
        }
    }
    Ok(out)
}

/// 关闭标签页（切换到相邻标签）
#[tauri::command]
pub async fn close_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> CommandResult<Option<Tab>> {
    log::info!("[tabs] close_tab: id={}", id);
    // 单写锁内一次改完（状态收敛实施）；id 不存在时返回 None，调用方直接返回不碰 webview
    let next_id: Option<Option<String>> = state.mutate(|s| {
        let was_active = s.active_id == Some(id.clone());
        let removed = s.tabs.remove(&id);
        // 延迟账一起销（关了就不用载了，防泄漏）
        s.deferred_loads.remove(&id);
        // id 不存在：直接返回，调用方不碰 webview（version 照推，无害）
        removed.as_ref()?;
        // 记录被删标签在 order 中的位置（删除后同位置即右邻）
        let removed_pos = s.order.iter().position(|x| x == &id);
        if let Some(pos) = removed_pos {
            s.order.remove(pos);
        }

        let mut next_id: Option<String> = None;
        if was_active {
            // 优先切右边的标签（删除后同位置即右邻），没有则切左边（最后一个）
            next_id = match removed_pos {
                Some(p) if p < s.order.len() => s.order.get(p).cloned(),
                _ => s.order.last().cloned(),
            };
            if let Some(nid) = &next_id {
                if let Some(t) = s.tabs.get_mut(nid) {
                    t.is_active = true;
                }
            }
            s.active_id = next_id.clone();
        }

        Some(next_id)
    });
    let Some(next_id) = next_id else {
        return Ok(None);
    };

    TabManager::destroy_webview(&app, &id);

    // 激活下一个标签的 webview
    if let Some(nid) = &next_id {
        let _ = TabManager::set_webview_visible(&app, nid, true);
    }

    let s = state.read_state();
    // 关完推同步：AI 关的标签前端不知情，不推就留 ghost 标签到下次轮询。
    let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
    Ok(next_id.and_then(|nid| s.tabs.get(&nid).cloned()))
}

/// 关闭全部标签（切换工作区时清空用）
/// 返回清空前的标签快照：前端切换失败时可用快照重建，避免“清空后恢复失败丢标签”。
/// 快照只含内存状态（id/url/标题等），不含 webview 句柄；重建走正常 create 流程。
#[tauri::command]
pub async fn close_all_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> CommandResult<Vec<Tab>> {
    log::info!("[tabs] close_all_tabs");
    // 先快照再清空：快照在读锁内一次性取出，清空不可逆时调用方有料可回滚
    let snapshot: Vec<Tab> = state.all_tabs();
    let ids: Vec<String> = state.mutate(|s| {
        let collected: Vec<String> = s.tabs.keys().cloned().collect();
        s.tabs.clear();
        s.order.clear();
        s.active_id = None;
        s.deferred_loads.clear();
        collected
    });

    for id in ids {
        TabManager::destroy_webview(&app, &id);
    }
    // 清完推同步：工作区切换等批量清空后前端一次跟上。
    let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
    Ok(snapshot)
}

/// 切换标签页
#[tauri::command]
pub async fn switch_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> CommandResult<Option<Tab>> {
    log::info!("[tabs] switch_tab: id={}", id);
    // 先校验标签存在，再写 active_id（否则 active_id 会指向不存在的标签，
    // 导致 go_back/go_forward 报"标签不存在"、UI 显示与实际不符）
    let hibernated_info = {
        let s = state.read_state();
        s.tabs
            .get(&id)
            .map(|t| (t.hibernated, t.url.clone(), t.private))
    };
    let Some((is_hibernated, hibernated_url, hibernated_private)) = hibernated_info else {
        return Ok(None);
    };

    // 休眠标签的恢复：webview 此前已被销毁释放内存，这里按保存的 URL 重建。
    // 重建后需重新注入扩展/隐私防护脚本，并重新计算布局（create_webview 内部处理）。
    if is_hibernated {
        log::info!(
            "[perf] 唤醒休眠标签: id={} url={}",
            id,
            crate::redact_url(&hibernated_url)
        );
        TabManager::create_webview(&app, &id, &hibernated_url, hibernated_private)?;
        state.mutate(|s| {
            if let Some(t) = s.tabs.get_mut(&id) {
                t.hibernated = false;
            }
        });
        // 单页补注：只给唤醒的这页注，别家不动（广播会闪断别的标签脚本）
        crate::extensions::re_inject_one(&app, &format!("tab-{}", id)).await;
    }
    // 隐藏当前活动 webview + 更新激活状态：一次写锁内完成
    let old_active = state.mutate(|s| {
        let old = s.active_id.clone();
        s.active_id = Some(id.clone());
        for t in s.tabs.values_mut() {
            t.is_active = t.id == id;
        }
        old
    });

    // 切换可见性：一次主线程投递里"先显示新标签、再隐藏旧标签"
    // （原实现 hide→show 两次投递且顺序相反，中间白屏空窗 + 双跨线程往返导致切换迟钝）
    if state.split_tab().is_some() {
        // 分屏中：重排两个窗格（活动标签变了，右窗格保持），避免把分屏标签误隐藏
        let _ = apply_split_layout(&app, &state);
    } else if old_active.as_deref() == Some(id.as_str()) {
        // 点的就是当前标签：仅确保可见
        let _ = TabManager::set_webview_visible(&app, &id, true);
    } else {
        let _ = TabManager::switch_visibility(&app, &id, old_active.as_deref());
    }

    // 延迟加载兑现：占位页切出来时载真址并销账（点当前标签同样触发，不只切页）
    let deferred: Option<String> = state.mutate(|s| s.deferred_loads.remove(&id));
    if let Some(real_url) = deferred {
        // 存的 url/title 同步成真址：webview 已经去往真址，存量不跟的话标签栏
        // （标题）与地址栏（存量 url）各说各话，且前端切标签时看到的就是旧数。
        // 标题随后会被页面真标题覆盖（load 信号），这里先对齐地址。
        state.mutate(|s| {
            if let Some(t) = s.tabs.get_mut(&id) {
                t.url = real_url.clone();
                t.title = real_url.clone();
            }
        });
        if let Some(window) = app.get_window("main") {
            if let Some(wv) = window.get_webview(&format!("tab-{}", id)) {
                match real_url.parse::<tauri::Url>() {
                    Ok(target) => {
                        let _ = wv.navigate(target);
                        log::info!("[tabs] 延迟加载已触发: id={}", id);
                    }
                    Err(e) => log::warn!("[tabs] 延迟地址非法 id={}: {}", id, e),
                }
            }
        }
        // 推一次同步已并到函数尾（无条件），这里不再重复推。
    }

    state.touch_active(&id);
    let s = state.read_state();
    // 切完推同步：API 驱使的切换前端不知情，不推就靠 15s 轮询，标签栏高亮/地址栏慢半拍。
    let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
    Ok(s.tabs.get(&id).cloned())
}

/// URL scheme 白名单（create_tab 安全闸，纯函数便于测试）：
/// 只允许 http/https/shift；file 仅限内部页（newtab / worldnav / reader marker），
/// 其余 file 路径拒绝（防本地文件泄露），javascript:/data: 等危险 scheme 拒绝。
/// 例外：阅读模式生成的 file:///.../reader/r*.html（enter_reader_mode 写入 app_data/reader，
/// 内容经 sanitize_html 净化，路径固定）——2026-09-19 修复：此前被白名单误拒，
/// 阅读模式提取成功但新标签打不开
pub(crate) fn validate_create_url(url: &str) -> Result<(), AppError> {
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|e| AppError::validation(format!("URL 解析失败: {}", e)))?;
    match parsed.scheme() {
        "http" | "https" | "shift" => Ok(()),
        "file" => {
            if url.contains(crate::contracts::urls::NEWTAB_MARKER)
                || url.contains(crate::contracts::urls::WORLD_NAV_FILE)
                || url.contains(crate::contracts::urls::READER_MARKER)
            {
                Ok(())
            } else {
                Err(AppError::permission("不允许打开本地文件"))
            }
        }
        other => Err(AppError::permission(format!("不允许的 URL scheme: {}", other))),
    }
}

/// 休眠候选选择（纯函数，便于测试）：非激活、未休眠，且
/// （标签数超阈值 count_pressure → 后台标签直接候选；否则最后激活早于阈值；
/// 无激活记录视为早已空闲，候选）。
pub(crate) fn hibernate_candidates<'a>(
    tabs: impl Iterator<Item = (&'a String, &'a Tab)>,
    active: Option<&str>,
    last_active: &std::collections::HashMap<String, i64>,
    threshold: i64,
    count_pressure: bool,
) -> Vec<String> {
    tabs.filter(|(id, t)| {
            if t.hibernated || Some(id.as_str()) == active {
                return false;
            }
            count_pressure
                || last_active
                    .get(*id)
                    .map(|ts| *ts < threshold)
                    .unwrap_or(true)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

/// 空闲标签休眠：销毁长时间未激活的标签 webview，回收其渲染进程内存
/// （WebKitGTK 默认不主动回收，实测 9 标签合计 3.75GB）。
/// 返回本次休眠的标签数。超时分钟数来自 settings.tab_hibernate_minutes（0 = 关闭，默认 10）。
#[tauri::command]
pub async fn hibernate_idle_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    db: tauri::State<'_, Db>,
) -> CommandResult<u32> {
    // 读取超时配置（默认 10 分钟；0 表示关闭该功能）
    // 性能 P0：走 settings 缓存
    let minutes: i64 = crate::db::settings_cache::get(&db.pool, "tab_hibernate_minutes")
        .await
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(10);
    if minutes <= 0 {
        return Ok(0);
    }
    let threshold = now_ts() - minutes * 60;
    // 读锁内一次取全：标签数（候选选择内部自行取 active，见 hibernate_candidates）
    let tab_count = { state.read_state().tabs.len() };

    // 性能 P1（2026-09-22）：标签数阈值触发——标签数超过 tab_hibernate_count 时，
    // 后台（非激活）标签**切走即可休眠**（不再等 30 分钟无操作），
    // 把常驻渲染进程控制在阈值内（多标签内存主要来源）。
    // 阈值来自 settings.tab_hibernate_count（默认 8；0 = 关闭数量阈值，仅按超时）。
    let count_threshold: usize = crate::db::settings_cache::get(&db.pool, "tab_hibernate_count")
        .await
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8);
    let count_pressure = count_threshold > 0 && tab_count > count_threshold;
    if count_pressure {
        log::info!(
            "[perf] 标签数 {} 超阈值 {}，后台标签将提前休眠",
            tab_count,
            count_threshold
        );
    }

    // 选出候选：非激活、未休眠，且（超时 或 标签数超阈值）——口径见 hibernate_candidates
    let candidates: Vec<String> = {
        let s = state.read_state();
        hibernate_candidates(
            s.tabs.iter(),
            s.active_id.as_deref(),
            &s.last_active,
            threshold,
            count_pressure,
        )
    };
    if candidates.is_empty() {
        return Ok(0);
    }

    let Some(window) = app.get_window("main") else {
        return Ok(0);
    };
    let mut count = 0u32;
    for id in candidates {
        let label = format!("tab-{}", id);
        if let Some(wv) = window.get_webview(&label) {
            // 先隐藏再销毁，避免销毁瞬间的残影
            let _ = wv.hide();
            if let Err(e) = wv.close() {
                log::warn!("[perf] 标签休眠失败（close）: id={} err={}", id, e);
                continue;
            }
            state.mutate(|s| {
                if let Some(t) = s.tabs.get_mut(&id) {
                    t.hibernated = true;
                    // 延迟账一起销：唤醒走 tab.url 真载，不需要占位账了
                    s.deferred_loads.remove(&id);
                }
            });
            count += 1;
            log::info!("[perf] 标签已休眠（释放渲染进程）: id={}", id);
        }
    }
    if count > 0 {
        let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
        log::info!("[perf] 本轮共休眠 {} 个空闲标签", count);
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_url_whitelist_allows_web_and_internal() {
        // http/https/shift 放行
        assert!(validate_create_url("https://example.com").is_ok());
        assert!(validate_create_url("http://example.com/a?b=1").is_ok());
        assert!(validate_create_url("shift://newtab").is_ok());
        assert!(validate_create_url("shift://worldnav").is_ok());
        // file 三类内部页放行（newtab marker / worldnav 文件 / reader 产物——
        // reader 例外是 2026-09-19 修复：此前白名单误拒阅读模式）
        assert!(validate_create_url("file:///appdata/newtab/index.html").is_ok());
        assert!(validate_create_url("file:///appdata/world-nav.html").is_ok());
        assert!(validate_create_url("file:///appdata/reader/r9f2c1.html").is_ok());
    }

    #[test]
    fn create_url_whitelist_rejects_local_and_dangerous() {
        // file 其他路径拒绝（本地文件泄露面）
        assert!(validate_create_url("file:///etc/passwd").is_err());
        assert!(validate_create_url("file:///home/gao/secret.txt").is_err());
        // 危险 scheme 拒绝
        assert!(validate_create_url("javascript:alert(1)").is_err());
        assert!(validate_create_url("data:text/html,<h1>x</h1>").is_err());
        assert!(validate_create_url("vbscript:x").is_err());
        assert!(validate_create_url("ftp://x").is_err());
        // 非法 URL 解析失败拒绝
        assert!(validate_create_url("not a url at all").is_err());
    }

    fn tab(id: &str, hibernated: bool) -> Tab {
        Tab {
            id: id.into(),
            url: "https://x.com".into(),
            title: id.into(),
            favicon: String::new(),
            is_active: false,
            is_pinned: false,
            is_muted: false,
            hibernated,
            private: false,
            origin: "user".into(),
        }
    }

    #[test]
    fn hibernate_candidates_timeout_and_pressure() {
        let tabs = [
            ("t-active".to_string(), tab("t-active", false)),   // 活动 → 永不候选
            ("t-sleepy".to_string(), tab("t-sleepy", true)),    // 已休眠 → 不候选
            ("t-old".to_string(), tab("t-old", false)),          // 早已空闲 → 候选
            ("t-new".to_string(), tab("t-new", false)),          // 刚活跃 → 不候选
            ("t-norec".to_string(), tab("t-norec", false)),      // 无激活记录 → 候选
        ];
        let mut last_active = std::collections::HashMap::new();
        last_active.insert("t-old".to_string(), 100);
        last_active.insert("t-new".to_string(), 10_000); // 晚于阈值
        let threshold = 5_000;
        // 无数量压力：按超时选（t-old 早于阈值；t-norec 无记录视为早已空闲）
        let it = || tabs.iter().map(|(id, t)| (id, t));
        let out = hibernate_candidates(it(), Some("t-active"), &last_active, threshold, false);
        assert_eq!(out, vec!["t-old".to_string(), "t-norec".to_string()]);

        // 数量压力：全部后台未休眠标签直接候选（活动/已休眠仍排除）
        let out = hibernate_candidates(it(), Some("t-active"), &last_active, threshold, true);
        assert_eq!(
            out,
            vec!["t-old".to_string(), "t-new".to_string(), "t-norec".to_string()]
        );

        // active=None（理论不发生，锁死函数语义：无人豁免——t-active 无激活记录
        // 按超时规则同样是候选）
        let out = hibernate_candidates(it(), None, &last_active, threshold, false);
        assert_eq!(out, vec!["t-active".to_string(), "t-old".to_string(), "t-norec".to_string()]);
    }
}
