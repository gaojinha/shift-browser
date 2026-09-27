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
) -> CommandResult<Tab> {
    let is_private = private.unwrap_or(false);
    // 新标签插入位置：true = 插到当前活动标签之后（"在列表页右边打开"）；默认追加末尾
    let insert_after = after_active.unwrap_or(false);
    let background = background.unwrap_or(false);
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
        .map_err(|e| AppError::validation(format!("URL 解析失败: {}", e)))?;
    match parsed.scheme() {
        "http" | "https" | "shift" => {}
        "file" => {
            if !url.contains(crate::contracts::urls::NEWTAB_MARKER)
                && !url.contains(crate::contracts::urls::WORLD_NAV_FILE)
                && !url.contains(crate::contracts::urls::READER_MARKER)
            {
                return Err(AppError::permission("不允许打开本地文件").into());
            }
        }
        other => {
            return Err(AppError::permission(format!("不允许的 URL scheme: {}", other)).into())
        }
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
        is_active: !background,
        is_pinned: false,
        is_muted: false,
        hibernated: false,
        private: is_private,
        origin: "user".to_string(),
    };
    state.touch_active(&id);

    // 更新状态：新标签激活，旧标签取消激活（background 时保持当前活动标签不变）
    let old_active = {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());

        let prev = (*active).clone();
        if !background {
            for t in tabs.values_mut() {
                t.is_active = false;
            }
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
        if background {
            prev
        } else {
            active.replace(id.clone())
        }
    };

    // 新建标签页可见性：前台标签立即切换（show 新、hide 旧），避免新 webview 透明期透出旧页面；
    // 后台标签（J6）保持隐藏，用户当前页面继续显示，webview 仍在后台加载。
    if background {
        let _ = TabManager::set_webview_visible(&app, &id, false);
    } else if let Some(old) = old_active {
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
) -> CommandResult<Option<Tab>> {
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
/// 返回清空前的标签快照：前端切换失败时可用快照重建，避免“清空后恢复失败丢标签”。
/// 快照只含内存状态（id/url/标题等），不含 webview 句柄；重建走正常 create 流程。
#[tauri::command]
pub async fn close_all_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> CommandResult<Vec<Tab>> {
    log::info!("[tabs] close_all_tabs");
    // 先快照再清空：快照在锁内一次性取出，清空不可逆时调用方有料可回滚
    let snapshot: Vec<Tab> = {
        let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        order
            .iter()
            .filter_map(|id| tabs.get(id).cloned())
            .collect()
    };
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
