// 标签页纯数据状态机（R2-1 拆分：从 tabs.rs 抽出）
//
// 本文件只含不依赖 GTK/webview 的标签元数据与状态操作：
// - Tab / TabManager 结构定义（字段 pub(crate)，允许 tabs.rs / commands.rs 直接访问，
//   封装收敛是 R2-5 状态治理的活，本步只做文件拆分、不动行为）；
// - 纯数据方法（增删查、激活、书签栏/垂直标签/分屏等状态读写）；
// - NavState（导航状态查询结果）。
//
// 所有 GTK/webview 相关方法（create_webview / fix_gtk_layout / setup_gtk_layout /
// apply_split_layout 等）保留在 tabs.rs。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::engine::layout::{
    BOOKMARKS_BAR_HEIGHT, CONTENT_TOP_OFFSET_NO_BOOKMARKS, TAB_BAR_HEIGHT,
};

/// 标签页元数据（运行时状态，不持久化；会话保存走 sessions 表）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Tab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub favicon: String,
    pub is_active: bool,
    pub is_pinned: bool,
    pub is_muted: bool,
    /// 是否已休眠（webview 已销毁以释放内存；点击标签时按 url 重建）
    #[serde(default)]
    pub hibernated: bool,
    /// 无痕标签（2026-09-22）：incognito → ephemeral WebContext，
    /// Cookie/缓存/存储不落盘、与常规标签隔离；不计历史
    #[serde(default)]
    pub private: bool,
    /// 标签来源（J 类治理）：`user` 用户自开 / `ai` 经开放 API 打开。
    /// AI 标签不写浏览历史、可被 `tabs/close_ai` 批量回收、受 AI 标签上限约束。
    #[serde(default = "default_origin")]
    pub origin: String,
}

/// Tab.origin 缺省值（旧数据/旧构造点为 user）
fn default_origin() -> String {
    "user".to_string()
}

/// 标签管理器（Tauri State）
pub struct TabManager {
    pub(crate) tabs: Mutex<HashMap<String, Tab>>,
    pub(crate) order: Mutex<Vec<String>>,
    pub(crate) active_id: Mutex<Option<String>>,
    /// 书签栏是否显示（影响 child webview 的 top 偏移：112 vs 82）
    pub(crate) show_bookmarks_bar: Mutex<bool>,
    /// set_webview_visible 已应用的最近 seq（P1-9：丢弃并发乱序的过期指令）
    pub(crate) webview_visible_seq: Mutex<u64>,
    /// AI 面板是否占用右侧（child webview 宽度让出 AI_PANEL_WIDTH，方便边看页面边对话）
    pub(crate) ai_panel_active: Mutex<bool>,
    /// 垂直标签模式（true 时左侧让出 VERTICAL_TAB_BAR_WIDTH，顶部不再含标签栏）
    pub(crate) vertical_tabs: Mutex<bool>,
    /// 分屏：右侧窗格承载的标签 id（None = 未分屏；左侧窗格始终是活动标签）
    pub(crate) split_tab: Mutex<Option<String>>,
    /// 每个标签最后一次激活时间（Unix 秒），用于"空闲休眠"判定
    pub(crate) last_active: Mutex<HashMap<String, i64>>,
}

/// 当前 Unix 时间戳（秒）
pub(crate) fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            tabs: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            active_id: Mutex::new(None),
            show_bookmarks_bar: Mutex::new(true),
            webview_visible_seq: Mutex::new(0),
            ai_panel_active: Mutex::new(false),
            vertical_tabs: Mutex::new(false),
            split_tab: Mutex::new(None),
            last_active: Mutex::new(HashMap::new()),
        }
    }

    /// 记录标签激活时间（休眠判定的输入）
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    pub fn touch_active(&self, id: &str) {
        self.last_active
            .lock()
            .unwrap()
            .insert(id.to_string(), now_ts());
    }

    pub fn set_show_bookmarks_bar(&self, visible: bool) -> f64 {
        *self
            .show_bookmarks_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = visible;
        self.top_offset()
    }

    /// 运行时快照（自检/可观测性用）：标签数、活动标签、AI 分屏状态
    pub fn runtime_snapshot(&self) -> (usize, Option<String>, bool) {
        let count = self.tabs.lock().unwrap_or_else(|e| e.into_inner()).len();
        let active = self
            .active_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let ai = *self
            .ai_panel_active
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        (count, active, ai)
    }

    /// 活动标签的 URL（GPC 站点声明检测等用）
    pub fn active_url(&self) -> Option<String> {
        let id = self
            .active_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()?;
        let tabs = self.tabs.lock().unwrap_or_else(|e| e.into_inner());
        tabs.get(&id).map(|t| t.url.clone())
    }

    /// 是否垂直标签模式（阶段 3.1）
    pub fn vertical_tabs(&self) -> bool {
        *self
            .vertical_tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 设置垂直标签模式
    pub fn set_vertical_tabs(&self, on: bool) {
        *self
            .vertical_tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = on;
    }

    /// 分屏：右侧窗格承载的标签 id（None = 未分屏）
    pub fn split_tab(&self) -> Option<String> {
        self.split_tab
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 设置/清除分屏标签
    pub fn set_split_tab(&self, id: Option<String>) {
        *self
            .split_tab
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = id;
    }

    /// 当前内容区顶部偏移（随书签栏显隐 + 垂直标签模式变化）
    pub fn top_offset(&self) -> f64 {
        let bookmarks = *self
            .show_bookmarks_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 基准：标签栏 + 地址栏
        let mut t = CONTENT_TOP_OFFSET_NO_BOOKMARKS;
        if self.vertical_tabs() {
            // 垂直标签：标签栏移到左侧，顶部不再占标签栏高度
            t -= TAB_BAR_HEIGHT;
        }
        if bookmarks {
            t += BOOKMARKS_BAR_HEIGHT;
        }
        t
    }

    /// 生成 UUID v4（真随机，crypto 级）
    pub(crate) fn uuid() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    pub fn all_tabs(&self) -> Vec<Tab> {
        let tabs = self.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let order = self.order.lock().unwrap_or_else(|e| e.into_inner());
        order
            .iter()
            .filter_map(|id| tabs.get(id).cloned())
            .collect()
    }

    /// 设置标签来源（J 类治理）：`user` / `ai`。标签不存在时静默。
    pub(crate) fn set_origin(&self, id: &str, origin: &str) {
        let mut tabs = self.tabs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = tabs.get_mut(id) {
            t.origin = origin.to_string();
        }
    }

    /// AI 打开且仍在的标签 id 列表（按 order 顺序）
    pub(crate) fn ai_tab_ids(&self) -> Vec<String> {
        self.all_tabs()
            .into_iter()
            .filter(|t| t.origin == "ai")
            .map(|t| t.id)
            .collect()
    }

    /// 当前 AI 标签数量（受 `open_and_read` 上限约束）
    pub(crate) fn ai_tab_count(&self) -> usize {
        let tabs = self.tabs.lock().unwrap_or_else(|e| e.into_inner());
        tabs.values().filter(|t| t.origin == "ai").count()
    }
}

/// 查询活动标签的真实导航状态（修复 2026-09-22：前端 canGoBack/canGoForward
/// 原为本地猜测，切标签后不更新 → 前进/后退按钮 disabled 卡住、点了没反应）
#[derive(serde::Serialize)]
pub struct NavState {
    #[serde(rename = "canGoBack")]
    pub can_go_back: bool,
    #[serde(rename = "canGoForward")]
    pub can_go_forward: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_manager_is_empty() {
        let m = TabManager::new();
        assert!(m.all_tabs().is_empty());
        assert_eq!(m.runtime_snapshot().0, 0);
        assert!(m.active_url().is_none());
        assert!(m.split_tab().is_none());
    }

    #[test]
    fn top_offset_follows_bookmarks_bar_and_vertical() {
        let m = TabManager::new();
        // 默认书签栏显示
        let with_bookmarks = m.top_offset();
        let without = m.set_show_bookmarks_bar(false);
        assert!(without < with_bookmarks);
        // 垂直标签模式：顶部再让出标签栏高度
        m.set_vertical_tabs(true);
        let vertical = m.set_show_bookmarks_bar(true);
        assert!(vertical < with_bookmarks);
    }

    /// J 类治理：origin 标记 + AI 标签计数/列举（close_ai 依赖）。
    #[test]
    fn ai_tab_origin_tracking() {
        let m = TabManager::new();
        let mk = |id: &str| Tab {
            id: id.to_string(),
            url: format!("https://{id}.example"),
            title: id.to_string(),
            favicon: String::new(),
            is_active: false,
            is_pinned: false,
            is_muted: false,
            hibernated: false,
            private: false,
            origin: "user".to_string(),
        };
        {
            let mut tabs = m.tabs.lock().unwrap();
            let mut order = m.order.lock().unwrap();
            tabs.insert("u1".into(), mk("u1"));
            tabs.insert("a1".into(), mk("a1"));
            tabs.insert("a2".into(), mk("a2"));
            order.extend(["u1".into(), "a1".into(), "a2".into()]);
        }
        assert_eq!(m.ai_tab_count(), 0, "默认全为 user");
        m.set_origin("a1", "ai");
        m.set_origin("a2", "ai");
        assert_eq!(m.ai_tab_count(), 2);
        assert_eq!(m.ai_tab_ids(), vec!["a1".to_string(), "a2".to_string()]);
        // 用户标签不受影响
        assert!(m
            .all_tabs()
            .iter()
            .any(|t| t.id == "u1" && t.origin == "user"));
        // 不存在的 id 静默
        m.set_origin("nope", "ai");
        assert_eq!(m.ai_tab_count(), 2);
    }

    #[test]
    fn split_tab_roundtrip() {
        let m = TabManager::new();
        assert!(m.split_tab().is_none());
        m.set_split_tab(Some("t1".to_string()));
        assert_eq!(m.split_tab().as_deref(), Some("t1"));
        m.set_split_tab(None);
        assert!(m.split_tab().is_none());
    }

    #[test]
    fn all_tabs_follows_order() {
        let m = TabManager::new();
        // 直接操纵字段（纯数据状态机，测试允许）
        let mut tabs = m.tabs.lock().unwrap();
        let mut order = m.order.lock().unwrap();
        tabs.insert(
            "a".to_string(),
            Tab {
                id: "a".to_string(),
                url: "https://a.example".to_string(),
                title: "A".to_string(),
                favicon: String::new(),
                is_active: false,
                is_pinned: false,
                is_muted: false,
                hibernated: false,
                private: false,
                origin: "user".to_string(),
            },
        );
        tabs.insert(
            "b".to_string(),
            Tab {
                id: "b".to_string(),
                url: "https://b.example".to_string(),
                title: "B".to_string(),
                favicon: String::new(),
                is_active: true,
                is_pinned: false,
                is_muted: false,
                hibernated: false,
                private: false,
                origin: "user".to_string(),
            },
        );
        order.push("b".to_string());
        order.push("a".to_string());
        drop(order);
        let mut active = m.active_id.lock().unwrap();
        *active = Some("b".to_string());
        drop(active);
        drop(tabs);
        let all = m.all_tabs();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "b");
        assert_eq!(all[1].id, "a");
        assert_eq!(m.runtime_snapshot().0, 2);
        assert_eq!(m.active_url().as_deref(), Some("https://b.example"));
    }
}