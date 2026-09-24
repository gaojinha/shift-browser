//! 标签命令门面（P1-1 拆分 · 2026-09-22）
//!
//! 29 个 #[tauri::command] 从 tabs.rs 迁出。依赖主文件的 TabManager/Tab 等。

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

use super::*;

/// 创建新标签页
#[tauri::command]
pub async fn create_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    url: String,
    private: Option<bool>,
    after_active: Option<bool>,
) -> Result<Tab, String> {
    let is_private = private.unwrap_or(false);
    // 新标签插入位置：true = 插到当前活动标签之后（"在列表页右边打开"）；默认追加末尾
    let insert_after = after_active.unwrap_or(false);
    // 隐私防护：新建标签前清洗跨站追踪参数（只影响 http/https，内部页面不受影响）
    let url = clean_tracking_params(&url);
    log::info!("[tabs] create_tab: url={}", crate::redact_url(&url));
    // P2：URL scheme 白名单——只允许 http/https、内部页面（file://newtab、shift://）。
    // 拒绝 file:// 其他路径（本地文件泄露）与 javascript:/data: 等危险 scheme
    // 例外：阅读模式生成的 file:///.../reader/r*.html（enter_reader_mode 写入 app_data/reader，
    // 内容经 sanitize_html 净化，路径固定）——2026-09-19 修复：此前被白名单误拒，
    // 阅读模式提取成功但新标签打不开
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("URL 解析失败: {}", e))?;
    match parsed.scheme() {
        "http" | "https" | "shift" => {}
        "file" => {
            if !url.contains(crate::contracts::urls::NEWTAB_MARKER)
                && !url.contains(crate::contracts::urls::WORLD_NAV_FILE)
                && !url.contains(crate::contracts::urls::READER_MARKER)
            {
                return Err("不允许打开本地文件".to_string());
            }
        }
        other => return Err(format!("不允许的 URL scheme: {}", other)),
    }
    let id = TabManager::uuid();
    // 本地新标签页（file:// 路径含 /newtab/ 或 shift://newtab/worldnav）标题显示"启动页"，
    // 不显示冗长 URL/host。注意：协议改版后 URL 是 shift://newtab，旧的 contains("/newtab/")
    // 判断匹配不到，必须按 scheme+host 判断
    let title = if is_internal_newtab_url(&url) {
        "启动页".to_string()
    } else {
        url.clone()
    };

    TabManager::create_webview(&app, &id, &url, is_private)?;
    log::info!("[tabs] create_tab: webview 创建成功 id={} private={}", id, is_private);

    let tab = Tab {
        id: id.clone(),
        url,
        title,
        favicon: String::new(),
        is_active: true,
        is_pinned: false,
        is_muted: false,
        hibernated: false,
        private: is_private,
    };
    state.touch_active(&id);

    // 更新状态：新标签激活，旧标签取消激活
    let old_active = {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());

        let prev = (*active).clone();
        for t in tabs.values_mut() {
            t.is_active = false;
        }
        tabs.insert(id.clone(), tab.clone());
        // 插入位置：after_active=true 时插到旧活动标签之后（无活动标签则追加末尾）
        let pos = if insert_after {
            prev.as_ref()
                .and_then(|p| order.iter().position(|x| x == p))
                .map(|p| p + 1)
                .unwrap_or(order.len())
        } else {
            order.len()
        };
        if insert_after {
            log::info!(
                "[tabs] create_tab: 插到活动标签之后 pos={}/{}",
                pos,
                order.len()
            );
        }
        order.insert(pos, id.clone());
        active.replace(id.clone())
    };

    // 新建标签页：立即切换可见性（show 新、hide 旧），避免新 webview 透明期透出旧页面
    if let Some(old) = old_active {
        let _ = TabManager::switch_visibility(&app, &id, Some(&old));
    } else {
        let _ = TabManager::set_webview_visible(&app, &id, true);
    }

    Ok(tab)
}

/// 关闭标签页（切换到相邻标签）
#[tauri::command]
pub async fn close_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    log::info!("[tabs] close_tab: id={}", id);
    let mut next_id: Option<String> = None;

    {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());

        let was_active = *active == Some(id.clone());
        let removed = tabs.remove(&id);
        // 记录被删标签在 order 中的位置（删除后同位置即右邻）
        let removed_pos = order.iter().position(|x| x == &id);
        if let Some(pos) = removed_pos {
            order.remove(pos);
        }

        if was_active {
            // 优先切右边的标签（删除后同位置即右邻），没有则切左边（最后一个）
            next_id = match removed_pos {
                Some(p) if p < order.len() => order.get(p).cloned(),
                _ => order.last().cloned(),
            };
            if let Some(nid) = &next_id {
                if let Some(t) = tabs.get_mut(nid) {
                    t.is_active = true;
                }
            }
            *active = next_id.clone();
        }

        if removed.is_none() {
            return Ok(None);
        }
    }

    TabManager::destroy_webview(&app, &id);

    // 激活下一个标签的 webview
    if let Some(nid) = &next_id {
        let _ = TabManager::set_webview_visible(&app, nid, true);
    }

    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(next_id.and_then(|nid| tabs.get(&nid).cloned()))
}

/// 关闭全部标签（切换工作区时清空用）
#[tauri::command]
pub async fn close_all_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    log::info!("[tabs] close_all_tabs");
    let ids: Vec<String> = {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());
        let collected: Vec<String> = tabs.keys().cloned().collect();
        tabs.clear();
        order.clear();
        *active = None;
        collected
    };

    for id in ids {
        TabManager::destroy_webview(&app, &id);
    }
    Ok(())
}

/// 切换标签页
#[tauri::command]
pub async fn switch_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    log::info!("[tabs] switch_tab: id={}", id);
    // 先校验标签存在，再写 active_id（否则 active_id 会指向不存在的标签，
    // 导致 go_back/go_forward 报"标签不存在"、UI 显示与实际不符）
    let hibernated_info = {
        let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        tabs.get(&id).map(|t| (t.hibernated, t.url.clone(), t.private))
    };
    let Some((is_hibernated, hibernated_url, hibernated_private)) = hibernated_info else {
        return Ok(None);
    };

    // 休眠标签的恢复：webview 此前已被销毁释放内存，这里按保存的 URL 重建。
    // 重建后需重新注入扩展/隐私防护脚本，并重新计算布局（create_webview 内部处理）。
    if is_hibernated {
        log::info!("[perf] 唤醒休眠标签: id={} url={}", id, hibernated_url);
        TabManager::create_webview(&app, &id, &hibernated_url, hibernated_private)?;
        if let Some(t) = state
            .tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&id)
        {
            t.hibernated = false;
        }
        crate::extensions::re_inject(&app).await;
    }
    // 隐藏当前活动 webview
    let old_active = {
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());
        let old = active.clone();
        *active = Some(id.clone());
        old
    };

    // 更新激活状态
    {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        for t in tabs.values_mut() {
            t.is_active = t.id == id;
        }
    }

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

    state.touch_active(&id);
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(tabs.get(&id).cloned())
}

/// 空闲标签休眠：销毁长时间未激活的标签 webview，回收其渲染进程内存
/// （WebKitGTK 默认不主动回收，实测 9 标签合计 3.75GB）。
/// 返回本次休眠的标签数。超时分钟数来自 settings.tab_hibernate_minutes（0 = 关闭）。
#[tauri::command]
pub async fn hibernate_idle_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    db: tauri::State<'_, Db>,
) -> Result<u32, String> {
    // 读取超时配置（默认 30 分钟；0 表示关闭该功能）
    // 性能 P0：走 settings 缓存
    let minutes: i64 = crate::db::settings_cache::get(&db.pool, "tab_hibernate_minutes")
        .await
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(30);
    if minutes <= 0 {
        return Ok(0);
    }
    let threshold = now_ts() - minutes * 60;
    let active = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    // 性能 P1（2026-09-22）：标签数阈值触发——标签数超过 tab_hibernate_count 时，
    // 后台（非激活）标签**切走即可休眠**（不再等 30 分钟无操作），
    // 把常驻渲染进程控制在阈值内（多标签内存主要来源）。
    // 阈值来自 settings.tab_hibernate_count（默认 8；0 = 关闭数量阈值，仅按超时）。
    let count_threshold: usize = crate::db::settings_cache::get(&db.pool, "tab_hibernate_count")
        .await
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8);
    let tab_count = state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .len();
    let count_pressure = count_threshold > 0 && tab_count > count_threshold;
    if count_pressure {
        log::info!(
            "[perf] 标签数 {} 超阈值 {}，后台标签将提前休眠",
            tab_count,
            count_threshold
        );
    }

    // 选出候选：非激活、未休眠，且（超时 或 标签数超阈值）
    let candidates: Vec<String> = {
        let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let last = state.last_active.lock().unwrap_or_else(|e| e.into_inner());
        tabs.iter()
            .filter(|(id, t)| {
                if t.hibernated || Some((*id).clone()) == active {
                    return false;
                }
                // 超阈值：后台标签直接候选；否则按"最后激活时间早于阈值"
                count_pressure || last.get(*id).map(|ts| *ts < threshold).unwrap_or(true)
            })
            .map(|(id, _)| id.clone())
            .collect()
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
            if let Some(t) = state
                .tabs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&id)
            {
                t.hibernated = true;
            }
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

/// 翻译当前活动标签的整个页面（供前端「翻译当前页面」按钮调用）
/// 复用既有整页翻译脚本：脚本自包含（进度浮层 + 分批调 shift://__translate__/batch + 回填 DOM），
/// 放在 tabs.rs 是为了直接访问 TabManager 的 active_id（该字段不对外暴露）
#[tauri::command]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub async fn translate_current_page(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    let id = state
        .active_id
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "当前没有活动标签".to_string())?;
    let label = format!("tab-{}", id);
    let wv = app
        .get_webview(&label)
        .ok_or_else(|| "活动标签的 webview 不存在".to_string())?;
    wv.eval(crate::proto_shift::WHOLE_PAGE_TRANSLATE_JS)
        .map_err(|e| format!("注入整页翻译脚本失败: {}", e))?;
    log::info!("[translate] 已对标签 {} 注入整页翻译脚本", id);
    Ok(())
}

/// 导航到新 URL
#[tauri::command]
pub async fn navigate_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
    url: String,
) -> Result<Option<Tab>, String> {
    // 隐私防护：导航前清洗跨站追踪参数（utm_*/fbclid/gclid/msclkid/spm 等）
    let url = clean_tracking_params(&url);
    log::info!("[tabs] navigate_tab: id={} url={}", id, url);
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;

    let parsed_url = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("URL 解析失败: {}", e))?;
    // P2：导航同样做 scheme 白名单（与 create_tab 一致），防止注入的脚本导航到 file://
    match parsed_url.scheme() {
        "http" | "https" | "shift" => {}
        "file" => {
            if !url.contains(crate::contracts::urls::NEWTAB_MARKER) && !url.contains(crate::contracts::urls::WORLD_NAV_FILE) {
                return Err("不允许导航到本地文件".to_string());
            }
        }
        other => return Err(format!("不允许的 URL scheme: {}", other)),
    }
    webview
        .navigate(parsed_url)
        .map_err(|e| format!("导航失败: {}", e))?;

    let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    let mut tab = tabs.get_mut(&id);
    if let Some(ref mut t) = tab {
        t.url = url.clone();
        t.title = url;
    }
    Ok(tab.cloned())
}

/// 刷新当前标签（Ctrl+R）
#[tauri::command]
pub async fn reload_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    log::info!("[tabs] reload_tab: id={}", id);
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
    webview
        .with_webview(|pwv| {
            crate::engine::webview::reload(&pwv.inner());
        })
        .map_err(|e| format!("刷新失败: {}", e))?;
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(tabs.get(&id).cloned())
}

/// 更新标签顺序（拖拽排序）；前端传新顺序，缺失的现存标签兜底追加到末尾
#[tauri::command]
pub async fn reorder_tabs(
    state: tauri::State<'_, TabManager>,
    ids: Vec<String>,
) -> Result<(), String> {
    let existing_set: std::collections::HashSet<String> = {
        let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        tabs.keys().cloned().collect()
    };
    let mut new_order: Vec<String> = ids
        .iter()
        .filter(|id| existing_set.contains(*id))
        .cloned()
        .collect();
    let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
    for id in order.iter() {
        if !new_order.contains(id) {
            new_order.push(id.clone());
        }
    }
    *order = new_order;
    log::info!("[tabs] reorder_tabs: 新顺序 {:?}", order);
    Ok(())
}

/// 获取所有标签页（按顺序）
#[tauri::command]
pub async fn get_tabs(state: tauri::State<'_, TabManager>) -> Result<Vec<Tab>, String> {
    let tabs = state.all_tabs();
    // 诊断"前端标签列表为空/标签不显示"问题：记录每次同步的标签数（事件驱动 + 8s 轮询）
    log::info!("[tabs] get_tabs: {} 个标签", tabs.len());
    Ok(tabs)
}

/// 隐藏/显示所有标签页 webview（弹窗/菜单等 Vue UI 覆盖层需要）
/// source: 调用来源标识（前端调试用，可选）
/// seq: 单调递增令牌（P1-9）——前端快速开关面板时多次调用并发乱序，
/// 只让最新 seq 生效，过期指令直接丢弃，避免"最后完成的是旧指令"导致
/// webview 显隐状态与面板实际状态相反。
#[tauri::command]
pub async fn set_webview_visible(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    visible: bool,
    seq: u64,
    source: Option<String>,
) -> Result<(), String> {
    // 过期指令丢弃：并发乱序时只应用最大 seq
    {
        let mut last = state
            .webview_visible_seq
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if seq <= *last {
            log::info!(
                "[tabs] set_webview_visible 过期指令丢弃 seq={} last={} source={:?}",
                seq,
                *last,
                source
            );
            return Ok(());
        }
        *last = seq;
    }
    // 覆盖层锁：隐藏 webview（visible=false）视为"有全屏覆盖层"，布局重排不得再显示，
    // 否则证书警告页会被 GTK 层 webview 盖掉；恢复显示时解锁。
    UI_OVERLAY_ACTIVE.store(!visible, std::sync::atomic::Ordering::Relaxed);
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    // 注意：child webview 不在 window.webviews() 列表里（只有 main），
    // 必须从 TabManager 拿已知标签逐个 get_webview
    let ids: Vec<String> = state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    let mut count = 0usize;
    for id in ids {
        let label = format!("tab-{}", id);
        if let Some(wv) = window.get_webview(&label) {
            let wv2 = wv.clone();
            let _ = wv.with_webview(move |pwv| {
                let gtk_wv = pwv.inner();
                // 同时隐藏/显示 Fixed 容器：Fixed 覆盖内容区会拦截弹窗点击，必须一起隐藏
                crate::engine::layout::set_fixed_visible(&gtk_wv, visible);
                if visible {
                    let _ = wv2.show();
                } else {
                    let _ = wv2.hide();
                }
            });
            count += 1;
        }
    }
    // 恢复显示后重新应用几何：隐藏期间（弹窗打开时）可能改过书签栏显隐/AI 分屏/窗口尺寸，
    // 不重设会导致 webview 尺寸或位置滞后，WebKit 输入窗口不跟随 → 页面点不动。
    // 注意：必须走 apply_split_layout（先全部隐藏、再只显示活动标签）——
    // 若直接对全部 webview show+apply，非活动标签会叠在活动标签上面（z-order），
    // 把点击事件吃掉 → 表现"设置弹窗关闭后页面按钮点不动"。
    if visible {
        apply_split_layout(&app, &state)?;
    }
    log::info!(
        "[tabs] set_webview_visible: visible={} count={} source={:?}",
        visible,
        count,
        source
    );
    Ok(())
}

/// AI 面板占用右侧空间（分屏对话）：active=true 时 child webview 宽度让出
/// AI_PANEL_WIDTH，右侧留给 HTML 层 AI 侧边栏；active=false 恢复全宽。
/// 与 set_webview_visible 不同：这里不隐藏 webview，页面保持可见可交互。
#[tauri::command]
pub async fn set_webview_ai_panel(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    active: bool,
) -> Result<(), String> {
    *state
        .ai_panel_active
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = active;
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let top = state.top_offset();
    let vertical = state.vertical_tabs();
    let geo = crate::engine::layout::ContentGeometry::compute(&window, top, active, vertical);
    log::info!(
        "[tabs] set_webview_ai_panel: active={} content={}x{}",
        active,
        geo.width,
        geo.height
    );
    // 关键：不能只对每个 webview apply_to_webview 改尺寸——WebKit 输入窗口
    // 不跟随 widget 尺寸变化（已知问题，见 apply_to_webview 注释），AI 面板打开时
    // 右侧 380px 仍是旧输入窗口的命中区，点击被 webview 吞掉 → ✕ 点不动。
    // 统一走 apply_split_layout（先全部隐藏、再按活动标签/分屏重新应用几何并显示），
    // hide→show 强制 GTK 重新分配，输入窗口跟随新尺寸，右侧让给 HTML 层 AI 面板；
    // 关闭时同理：hide→show 让 webview 输入窗口恢复全宽（避免关面板后点不动）。
    apply_split_layout(&app, &state)?;
    // 焦点切换：AI 面板打开时把 GTK 键盘焦点交给主 webview（HTML 层），
    // 否则焦点留在 child webview（页面）上，AI 输入框收不到键盘事件。
    // 关闭时把焦点还给当前活动标签的 webview。
    if let Some(main_wv) = window.get_webview("main") {
        if active {
            let _ = main_wv.set_focus();
        } else {
            let active_id = state
                .active_id
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(id) = active_id {
                if let Some(wv) = window.get_webview(&format!("tab-{}", id)) {
                    let _ = wv.set_focus();
                }
            }
        }
    }
    Ok(())
}

/// 后退（对当前活动标签执行 history.back()）
#[tauri::command]
pub async fn go_back(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
    let id = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(id) = id else {
        return Ok(None);
    };
    log::info!("[tabs] go_back: tab={}", id);
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
    // 修复（2026-09-22）：改用 WebKit 原生导航，不再 eval("history.back()")——
    // 后者受页面 CSP / 页面脚本重写 history 影响，会"点了没反应"
    webview
        .with_webview(|pwv| {
            webkit2gtk::WebViewExt::go_back(&pwv.inner());
        })
        .map_err(|e| format!("后退失败: {}", e))?;
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(tabs.get(&id).cloned())
}

/// 前进（对当前活动标签执行原生 go_forward()）
#[tauri::command]
pub async fn go_forward(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
    let id = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(id) = id else {
        return Ok(None);
    };
    log::info!("[tabs] go_forward: tab={}", id);
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
    // 修复（2026-09-22）：同 go_back，改用 WebKit 原生导航
    webview
        .with_webview(|pwv| {
            webkit2gtk::WebViewExt::go_forward(&pwv.inner());
        })
        .map_err(|e| format!("前进失败: {}", e))?;
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(tabs.get(&id).cloned())
}

/// 更新标签页元数据（标题/图标等，供前端同步）
#[tauri::command]
pub async fn update_tab(
    state: tauri::State<'_, TabManager>,
    id: String,
    title: Option<String>,
    favicon: Option<String>,
) -> Result<Option<Tab>, String> {
    let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    let mut tab = tabs.get_mut(&id);
    if let Some(ref mut t) = tab {
        if let Some(new_title) = title {
            t.title = new_title;
        }
        if let Some(new_favicon) = favicon {
            t.favicon = new_favicon;
        }
    }
    Ok(tab.cloned())
}

/// 切换书签栏显隐，返回新的 top_offset
#[tauri::command]
pub async fn set_bookmarks_bar_visible(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    visible: bool,
) -> Result<f64, String> {
    let new_top = state.set_show_bookmarks_bar(visible);
    log::info!(
        "[tabs] 书签栏显隐切换: visible={} 新 top_offset={}",
        visible,
        new_top
    );

    // 即时重算：不依赖下次窗口 Resize，直接按新的 top_offset 重算几何并应用
    // （几何计算统一走引擎层，见 engine/layout.rs）
    if let Some(window) = app.get_window("main") {
        let ai_active = *state
            .ai_panel_active
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let vertical = state.vertical_tabs();
        let geo =
            crate::engine::layout::ContentGeometry::compute(&window, new_top, ai_active, vertical);
        let ids: Vec<String> = state
            .tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            crate::engine::layout::apply_to_webview(&window, &id, geo);
        }
    }

    Ok(new_top)
}

/// 切换垂直标签模式（阶段 3.1），返回新的 top_offset
#[tauri::command]
pub async fn set_vertical_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    active: bool,
) -> Result<f64, String> {
    state.set_vertical_tabs(active);
    let new_top = state.top_offset();
    log::info!(
        "[tabs] 垂直标签模式切换: active={} 新 top_offset={}",
        active,
        new_top
    );

    // 即时重算：标签栏从顶部移到左侧，影响内容区 top 与 left，直接重算几何并应用到所有标签
    if let Some(window) = app.get_window("main") {
        let ai_active = *state
            .ai_panel_active
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let geo =
            crate::engine::layout::ContentGeometry::compute(&window, new_top, ai_active, active);
        let ids: Vec<String> = state
            .tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            crate::engine::layout::apply_to_webview(&window, &id, geo);
        }
    }

    Ok(new_top)
}

/// 开启分屏：右侧窗格显示指定标签（阶段 3.1 分屏浏览）
#[tauri::command]
pub async fn set_split_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<(), String> {
    if !state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&id)
    {
        return Err("标签不存在".to_string());
    }
    state.set_split_tab(Some(id.clone()));
    log::info!("[tabs] set_split_tab: {}", id);
    apply_split_layout(&app, &state)
}

/// 关闭分屏
#[tauri::command]
pub async fn clear_split(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    state.set_split_tab(None);
    log::info!("[tabs] clear_split");
    apply_split_layout(&app, &state)
}

/// 查询当前分屏标签（None = 未分屏）
#[tauri::command]
pub async fn get_split_tab(state: tauri::State<'_, TabManager>) -> Result<Option<String>, String> {
    Ok(state.split_tab())
}

/// 获取当前活动标签的导航状态（可后退/可前进）


/// 打开当前活动标签的开发者工具（Web Inspector）。
/// 依赖创建 webview 时设置的 `Settings.enable-developer-extras`（本文件 create 流程中已开启）。
/// 快捷键入口在 GTK 窗口层统一拦截（F12 / Ctrl+Shift+I），见 `setup_gtk_layout`：
/// 页面（child webview）持有焦点时 HTML 层收不到按键，必须在 GTK 层兜底。
#[tauri::command]
pub async fn open_devtools(
    app: AppHandle,
    _state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    open_devtools_for_active(&app);
    Ok(())
}

/// 固定/取消固定标签（原版 tab-pin IPC）
#[tauri::command]
pub async fn toggle_pin(
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    let mut tab = tabs.get_mut(&id);
    if let Some(ref mut t) = tab {
        t.is_pinned = !t.is_pinned;
        log::info!("[tabs] toggle_pin: id={} pinned={}", id, t.is_pinned);
    }
    Ok(tab.cloned())
}

/// 静音/取消静音标签。
/// 真实现：翻转内存状态 + 异步派发到主线程调 WebKit `set_is_muted`
/// （WebKitGTK 2.36+ 支持 per-webview 静音，绑定里 `WebViewExt::set_is_muted` 存在）。
#[tauri::command]
pub async fn toggle_mute(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    let result = {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut tab = tabs.get_mut(&id);
        if let Some(ref mut t) = tab {
            t.is_muted = !t.is_muted;
            log::info!("[tabs] toggle_mute: id={} muted={}", id, t.is_muted);
        }
        tab.cloned()
    };
    let muted = result.as_ref().map(|t| t.is_muted).unwrap_or(false);

    // 真静音：异步派发到主线程（从 GTK 信号/异步上下文直接调 with_webview 有主线程重入风险）
    let app2 = app.clone();
    let id2 = id.clone();
    tauri::async_runtime::spawn(async move {
        let Some(window) = app2.get_window("main") else {
            return;
        };
        let label = format!("tab-{}", id2);
        if let Some(wv) = window.get_webview(&label) {
            let _ = wv.with_webview(move |pwv| {
                crate::engine::webview::set_muted(&pwv.inner(), muted);
            });
        }
    });

    Ok(result)
}

/// 最大化/还原主窗口（双击标签栏空白处调用）
#[tauri::command]
pub async fn toggle_maximize(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let maximized = window.is_maximized().unwrap_or(false);
    if maximized {
        window.unmaximize().map_err(|e| e.to_string())?;
    } else {
        window.maximize().map_err(|e| e.to_string())?;
    }
    Ok(!maximized)
}

/// 最小化主窗口（标题栏/标签栏合并后的窗口控制按钮）
#[tauri::command]
pub async fn minimize_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.minimize().map_err(|e| e.to_string())
}

/// 关闭主窗口（标题栏/标签栏合并后的窗口控制按钮）
#[tauri::command]
pub async fn close_window(app: AppHandle) -> Result<(), String> {
    // 前端「退出」按钮是明确要退出，先置真退出标志，
    // 否则会被「关窗=缩托盘」的 delete-event 拦截，变成只隐藏。
    crate::tray::REAL_QUIT.store(true, std::sync::atomic::Ordering::SeqCst);
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.close().map_err(|e| e.to_string())
}

/// 开始拖动主窗口（无边框窗口标题栏拖拽，标签栏空白处按住调用）
#[tauri::command]
pub async fn start_drag_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.start_dragging().map_err(|e| e.to_string())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_newtab_url_detection() {
        // shift:// 协议（当前实际使用）
        assert!(is_internal_newtab_url("shift://newtab"));
        assert!(is_internal_newtab_url("shift://newtab/"));
        assert!(is_internal_newtab_url("shift://worldnav"));
        assert!(is_internal_newtab_url("shift://worldnav/world-nav.html"));
        // 旧 file:// 路径（兼容）
        assert!(is_internal_newtab_url(
            "file:///home/gao/.local/share/com.shiftbrowser.app/newtab/index.html"
        ));
        assert!(is_internal_newtab_url(
            "file:///home/gao/.local/share/com.shiftbrowser.app/newtab/world-nav.html"
        ));
        // 普通网页/其他协议不是启动页
        assert!(!is_internal_newtab_url("https://example.com"));
        assert!(!is_internal_newtab_url("http://example.com/newtab/"));
        assert!(!is_internal_newtab_url("file:///etc/passwd"));
        assert!(!is_internal_newtab_url("shift://__translate__/text"));
    }

    #[test]
    fn unique_path_adds_suffix_when_exists() {
        let dir = std::env::temp_dir().join("sb-unique-test");
        let _ = std::fs::create_dir_all(&dir);
        let dir = dir.to_str().unwrap();

        // 不存在 → 原文件名
        let p1 = unique_download_path(dir, "a.pdf");
        assert_eq!(p1, format!("{}/a.pdf", dir));

        // 创建后 → a(1).pdf
        std::fs::write(&p1, "x").unwrap();
        let p2 = unique_download_path(dir, "a.pdf");
        assert_eq!(p2, format!("{}/a(1).pdf", dir));

        // 再创建 → a(2).pdf
        std::fs::write(&p2, "x").unwrap();
        let p3 = unique_download_path(dir, "a.pdf");
        assert_eq!(p3, format!("{}/a(2).pdf", dir));

        // 无扩展名
        let p4 = unique_download_path(dir, "noext");
        assert_eq!(p4, format!("{}/noext", dir));
        std::fs::write(&p4, "x").unwrap();
        let p5 = unique_download_path(dir, "noext");
        assert_eq!(p5, format!("{}/noext(1)", dir));

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("sb-unique-test"));
    }

    #[test]
    fn sanitize_filename_blocks_path_traversal() {
        // 路径分隔符剥离
        assert_eq!(sanitize_filename("a/../b.txt"), "a..b.txt");
        assert_eq!(sanitize_filename("..\\evil.txt"), "evil.txt");
        // 纯穿越段 → 默认名
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename("."), "download");
        assert_eq!(sanitize_filename(""), "download");
        // 空字节/控制字符剥离
        assert_eq!(sanitize_filename("a\0b.txt"), "ab.txt");
        assert_eq!(sanitize_filename("a\nb.txt"), "ab.txt");
        // 正常名保留
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        // 首尾点段剥离（防隐藏文件）
        assert_eq!(sanitize_filename(".secret"), "secret");
        assert_eq!(sanitize_filename("..hidden"), "hidden");
        // 超长截断
        let long = "x".repeat(500);
        assert_eq!(sanitize_filename(&long).len(), 200);
    }
}

/// 一键清除浏览数据（隐私 · 2026-09-22）
/// scopes: history / cookies / cache / storage / downloads / sessions / all
#[tauri::command]
pub async fn clear_browsing_data(
    app: AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    scopes: Vec<String>,
) -> Result<Vec<String>, String> {
    let has = |k: &str| scopes.iter().any(|s| s == k || s == "all");
    let mut done: Vec<String> = Vec::new();

    // ---- 数据库侧 ----
    if has("history") {
        let _ = sqlx::query("DELETE FROM history").execute(&db.pool).await;
        done.push("history".into());
    }
    if has("downloads") {
        let _ = sqlx::query("DELETE FROM downloads").execute(&db.pool).await;
        done.push("downloads".into());
    }
    if has("sessions") {
        let _ = sqlx::query("DELETE FROM sessions").execute(&db.pool).await;
        done.push("sessions".into());
    }

    // ---- WebKit 侧（GTK 对象只能在主线程操作）----
    let need_cookies = has("cookies");
    let need_cache = has("cache");
    let need_storage = has("storage");
    if need_cookies || need_cache || need_storage {
        app.run_on_main_thread(move || {
            // P0-2：WebKit 数据清理下沉到 engine::platform
            crate::engine::platform::clear_web_data(need_cookies, need_cache, need_storage);
            log::info!(
                "[privacy] 清除浏览数据: cookies={} cache={} storage={}",
                need_cookies, need_cache, need_storage
            );
        })
        .map_err(|e| e.to_string())?;
        if need_cookies {
            done.push("cookies".into());
        }
        if need_cache {
            done.push("cache".into());
        }
        if need_storage {
            done.push("storage".into());
        }
    }

    log::info!("[privacy] 清除浏览数据完成: {:?}", done);
    Ok(done)
}


#[tauri::command]
pub async fn get_nav_state(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: Option<String>,
) -> Result<NavState, String> {
    let id = match id {
        Some(i) => i,
        None => state
            .active_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or("无活动标签")?,
    };
    let window = app.get_window("main").ok_or("主窗口不存在")?;
    let label = format!("tab-{}", id);
    let webview = window.get_webview(&label).ok_or("webview 不存在")?;
    let (tx, rx) = tokio::sync::oneshot::channel::<(bool, bool)>();
    webview
        .with_webview(move |pwv| {
            let _ = tx.send(crate::engine::webview::navigation_state(&pwv.inner()));
        })
        .map_err(|e| e.to_string())?;
    let (b, f) = match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
        Ok(Ok(v)) => v,
        _ => (false, false),
    };
    Ok(NavState { can_go_back: b, can_go_forward: f })
}
