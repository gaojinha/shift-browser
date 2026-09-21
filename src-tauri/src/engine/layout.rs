//! 内容区布局计算（引擎抽象层 · 批次 0.2 第一项迁移）
//!
//! 背景：同一条"内容区该多大、在窗口哪个位置"的公式原先散落在 6 处
//! （窗口缩放 / 切标签 / 标签创建 / 书签栏切换 / AI 分屏 / 几何重应用），
//! 任何一处漏改都会造成布局错位（历史上已多次踩坑）。
//! 这里收敛为唯一事实来源，业务模块只调用 `ContentGeometry`。
//!
//! 布局模型（Linux/GTK）：
//! ```text
//! 窗口
//!  ├─ 顶部 UI（HTML 层，占 top = 标签栏 + 地址栏 + 可选书签栏）
//!  ├─ 内容区（child webview，由 GtkFixed 承载，margin_top = top）
//!  │    · 默认全宽；AI 分屏时右侧让出 AI_PANEL_WIDTH
//!  └─ 底部状态栏（STATUS_BAR_HEIGHT）
//! ```

/// 标签栏高度（与前端 CSS 保持一致）
pub const TAB_BAR_HEIGHT: f64 = 44.0;
/// 地址栏高度
pub const ADDRESS_BAR_HEIGHT: f64 = 42.0;
/// 书签栏高度（可显隐）
pub const BOOKMARKS_BAR_HEIGHT: f64 = 30.0;
/// 底部状态栏高度
pub const STATUS_BAR_HEIGHT: f64 = 22.0;
/// AI 侧边栏占用的右侧宽度：AI 面板打开时内容区让出该宽度
pub const AI_PANEL_WIDTH: f64 = 380.0;
/// 垂直标签栏占用的左侧宽度：垂直标签模式下内容区让出该宽度（阶段 3.1）
pub const VERTICAL_TAB_BAR_WIDTH: f64 = 220.0;
/// 内容区顶部偏移（含书签栏）
pub const CONTENT_TOP_OFFSET: f64 = TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT + BOOKMARKS_BAR_HEIGHT;
/// 内容区顶部偏移（不含书签栏）
pub const CONTENT_TOP_OFFSET_NO_BOOKMARKS: f64 = TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT;
/// 内容区宽度下限（窗口过窄时保底）
pub const MIN_CONTENT_WIDTH: f64 = 400.0;
/// 内容区高度下限
pub const MIN_CONTENT_HEIGHT: f64 = 100.0;

/// 内容区几何（逻辑像素，已换算缩放）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentGeometry {
    /// 内容区左偏移（GtkFixed 的 margin_start；垂直标签时让出左侧栏，否则 0）
    pub left: i32,
    /// 内容区宽度（已让出左侧栏与 AI 分屏右侧）
    pub width: i32,
    /// 内容区高度（已扣除顶部偏移与状态栏）
    pub height: i32,
    /// 顶部偏移（GtkFixed 的 margin_top）
    pub top: i32,
}

impl ContentGeometry {
    /// 依据窗口当前尺寸计算内容区几何。
    /// `top`：内容区顶部偏移（随书签栏显隐 / 垂直标签模式变化，取 `TabManager::top_offset()`）
    /// `ai_active`：AI 分屏是否开启（true 时右侧让出 `AI_PANEL_WIDTH`）
    /// `vertical`：垂直标签模式（true 时左侧让出 `VERTICAL_TAB_BAR_WIDTH`）
    pub fn compute(window: &tauri::Window, top: f64, ai_active: bool, vertical: bool) -> Self {
        let inner = window
            .inner_size()
            .unwrap_or(tauri::PhysicalSize::new(1280, 800));
        let scale = window.scale_factor().unwrap_or(1.0);
        Self::from_size(inner.width as f64 / scale, inner.height as f64 / scale, top, ai_active, vertical)
    }

    /// 分屏：把当前内容区左右对半，返回 (左半, 右半)。
    /// 左半承载活动标签，右半承载分屏标签（阶段 3.1 分屏浏览）。
    pub fn split(self) -> (ContentGeometry, ContentGeometry) {
        let half = self.width / 2;
        let left = ContentGeometry {
            left: self.left,
            width: half,
            height: self.height,
            top: self.top,
        };
        let right = ContentGeometry {
            left: self.left + half,
            width: self.width - half,
            height: self.height,
            top: self.top,
        };
        (left, right)
    }

    /// 纯数学（与窗口解耦，便于单测）：给定内容区逻辑宽高算几何。
    /// 顺序：先让出左侧（垂直标签）→ 再让出右侧（AI 分屏）→ 下限保底。
    pub fn from_size(
        logical_w: f64,
        logical_h: f64,
        top: f64,
        ai_active: bool,
        vertical: bool,
    ) -> Self {
        let left = if vertical { VERTICAL_TAB_BAR_WIDTH } else { 0.0 };
        let mut w = logical_w - left;
        if ai_active {
            w = (w - AI_PANEL_WIDTH).max(MIN_CONTENT_WIDTH);
        }
        let h = (logical_h - top - STATUS_BAR_HEIGHT).max(MIN_CONTENT_HEIGHT);
        Self {
            left: left as i32,
            width: w as i32,
            height: h as i32,
            top: top as i32,
        }
    }
}

/// 内容区尺寸（浮点，供建标签时直接使用）
pub fn content_size(window: &tauri::Window, top: f64, ai_active: bool, vertical: bool) -> (f64, f64) {
    let geo = ContentGeometry::compute(window, top, ai_active, vertical);
    (geo.width as f64, geo.height as f64)
}

/// 把几何应用到某个标签的 webview：定位并缩放承载它的 GtkFixed 与 webview 自身。
/// 内部会 `queue_resize()`——webview 隐藏期间尺寸变更不触发 GTK 重新分配，
/// 显示时需靠它按最新尺寸分配（否则切回后停留旧尺寸，或输入窗口与实际不符导致"点不动"）。
pub fn apply_to_webview(window: &tauri::Window, tab_id: &str, geo: ContentGeometry) {
    use gtk::prelude::*;
    use tauri::Manager;
    let label = format!("tab-{}", tab_id);
    let Some(wv) = window.get_webview(&label) else {
        return;
    };
    let _ = wv.with_webview(move |pwv| {
        let gtk_wv = pwv.inner();
        if let Some(fixed) = gtk_wv
            .parent()
            .and_then(|p| p.dynamic_cast::<gtk::Fixed>().ok())
        {
            // 顶部偏移用 Fixed 的 margin_top（水平模式已验证有效）；
            // 左侧偏移用 GtkFixed 内部 move_（绝对定位）——不用 margin_start：
            // GtkOverlay 的叠加子件上 margin_start 会把 Fixed 的分配压成 1×1（实测）。
            fixed.set_margin_top(geo.top);
            fixed.set_margin_start(0);
            fixed.move_(&gtk_wv, geo.left, 0);
            fixed.set_size_request(geo.left + geo.width, geo.height);
        }
        gtk_wv.set_size_request(geo.width, geo.height);
        gtk_wv.queue_resize();
    });
}

/// 从 webview 向上找父级 GtkBox（vbox）。
/// 注意：不调 window 方法（避免主线程死锁），直接走 GTK 父链。
pub fn parent_vbox(wv: &webkit2gtk::WebView) -> Option<gtk::Box> {
    use gtk::prelude::*;
    wv.parent().and_then(|p| p.dynamic_cast::<gtk::Box>().ok())
}

/// 从 vbox 查找 GtkOverlay 里的 GtkFixed 容器，不存在则创建并挂到 vbox（兜底）。
/// 该容器承载 child webview，是内容区定位/显隐的关键。
pub fn find_or_create_fixed(vbox: &gtk::Box) -> gtk::Fixed {
    use gtk::prelude::*;
    // 优先：vbox → GtkOverlay → GtkFixed
    if let Some(overlay) = vbox
        .children()
        .iter()
        .find(|c| c.type_().name() == "GtkOverlay")
        .and_then(|c| c.dynamic_cast_ref::<gtk::Overlay>().cloned())
    {
        if let Some(f) = overlay
            .children()
            .iter()
            .find(|c| c.type_().name() == "GtkFixed")
            .and_then(|c| c.dynamic_cast_ref::<gtk::Fixed>().cloned())
        {
            return f;
        }
    }
    // 兜底：vbox 直接找 Fixed
    if let Some(f) = vbox
        .children()
        .iter()
        .find(|c| c.type_().name() == "GtkFixed")
        .and_then(|c| c.dynamic_cast_ref::<gtk::Fixed>().cloned())
    {
        return f;
    }
    // 都没有 → 新建并挂到 vbox（show 后由 GTK 布局）
    let f = gtk::Fixed::new();
    f.set_halign(gtk::Align::Start);
    f.set_valign(gtk::Align::Start);
    vbox.pack_start(&f, true, true, 0);
    f.show();
    f
}

/// 显示/隐藏承载 webview 的 GtkFixed 容器。
/// Fixed 覆盖内容区会拦截弹窗点击，弹窗打开时必须一起隐藏。
pub fn set_fixed_visible(wv: &webkit2gtk::WebView, visible: bool) {
    use gtk::prelude::*;
    if let Some(fixed) = wv
        .parent()
        .and_then(|p| p.dynamic_cast::<gtk::Fixed>().ok())
    {
        if visible {
            fixed.show();
        } else {
            fixed.hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_offsets_match_component_heights() {
        assert_eq!(CONTENT_TOP_OFFSET, 44.0 + 42.0 + 30.0);
        assert_eq!(CONTENT_TOP_OFFSET_NO_BOOKMARKS, 44.0 + 42.0);
        assert_eq!(
            CONTENT_TOP_OFFSET - CONTENT_TOP_OFFSET_NO_BOOKMARKS,
            BOOKMARKS_BAR_HEIGHT
        );
    }

    #[test]
    fn geometry_horizontal_defaults() {
        let g = ContentGeometry::from_size(1280.0, 800.0, CONTENT_TOP_OFFSET, false, false);
        assert_eq!(g.left, 0);
        assert_eq!(g.width, 1280);
        assert_eq!(g.height, (800.0 - CONTENT_TOP_OFFSET - STATUS_BAR_HEIGHT) as i32);
        assert_eq!(g.top, CONTENT_TOP_OFFSET as i32);
    }

    #[test]
    fn geometry_vertical_lets_out_left() {
        let g = ContentGeometry::from_size(
            1280.0,
            800.0,
            CONTENT_TOP_OFFSET_NO_BOOKMARKS,
            false,
            true,
        );
        assert_eq!(g.left, VERTICAL_TAB_BAR_WIDTH as i32);
        assert_eq!(g.width, 1280 - VERTICAL_TAB_BAR_WIDTH as i32);
    }

    #[test]
    fn geometry_vertical_and_ai_both_sides() {
        let g = ContentGeometry::from_size(1280.0, 800.0, CONTENT_TOP_OFFSET, true, true);
        assert_eq!(g.left, VERTICAL_TAB_BAR_WIDTH as i32);
        assert_eq!(
            g.width,
            (1280.0 - VERTICAL_TAB_BAR_WIDTH - AI_PANEL_WIDTH) as i32
        );
    }

    #[test]
    fn geometry_clamps_to_minimums() {
        let g = ContentGeometry::from_size(500.0, 80.0, CONTENT_TOP_OFFSET, true, true);
        assert_eq!(g.width, MIN_CONTENT_WIDTH as i32);
        assert_eq!(g.height, MIN_CONTENT_HEIGHT as i32);
    }

    #[test]
    fn geometry_split_halves_side_by_side() {
        let g = ContentGeometry::from_size(1280.0, 800.0, CONTENT_TOP_OFFSET, false, false);
        let (l, r) = g.split();
        assert_eq!(l.left, 0);
        assert_eq!(l.width, 640);
        assert_eq!(r.left, 640);
        assert_eq!(r.width, 640);
        assert_eq!(l.top, r.top);
        assert_eq!(l.height, r.height);
        // 两块拼起来等于原内容区（无缝隙、无重叠）
        assert_eq!(l.width + r.width, g.width);
        assert_eq!(r.left, l.left + l.width);
    }

    #[test]
    fn geometry_split_keeps_vertical_offset() {
        let g = ContentGeometry::from_size(1280.0, 800.0, 72.0, false, true);
        let (l, r) = g.split();
        assert_eq!(l.left, VERTICAL_TAB_BAR_WIDTH as i32); // 垂直标签：左半从 220 起
        assert_eq!(r.left, VERTICAL_TAB_BAR_WIDTH as i32 + l.width);
        assert_eq!(l.width + r.width, g.width);
    }
}
