// 全局常量单一事实来源（SSOT）
//
// 注意：本文件用普通注释而非 `//!` 文档注释——因为 build.rs 会 `include!` 它，
// 展开后 `//!` 不在文件开头会导致 "expected outer doc comment" 编译错误。
//
// **本文件必须自包含**——不引用 crate 内其他模块。原因：`build.rs` 会 `include!`
// 本文件来生成前端镜像 `src/constants.ts`，任何 crate 依赖都会让构建脚本编译失败。
//
// 前端用法：`import { LAYOUT, SEARCH_ENGINES, UI } from '../constants'`。
// **禁止在前端手写同义常量**——历史问题：布局高度 38/36、搜索引擎 URL 曾 4 处重复、
// 默认 favicon 2 处重复，改一处漏一处（见 docs/架构-能力与常量统一治理方案）。

/// 布局尺寸（与前端 CSS 一一对应；改这里前后端同时生效）
pub mod layout {
    /// 标签栏高度
    pub const TAB_BAR_HEIGHT: f64 = 38.0;
    /// 地址栏高度
    pub const ADDRESS_BAR_HEIGHT: f64 = 36.0;
    /// 书签栏高度（可显隐）
    pub const BOOKMARKS_BAR_HEIGHT: f64 = 26.0;
    /// 底部状态栏高度
    pub const STATUS_BAR_HEIGHT: f64 = 22.0;
    /// AI 侧边栏占用的右侧宽度
    pub const AI_PANEL_WIDTH: f64 = 380.0;
    /// 垂直标签栏占用的左侧宽度
    pub const VERTICAL_TAB_BAR_WIDTH: f64 = 220.0;
    /// 分屏左栏占比（左窄右宽）
    pub const SPLIT_LEFT_RATIO: f64 = 0.25;
    /// 内容区顶部偏移（含书签栏）
    pub const CONTENT_TOP_OFFSET: f64 = TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT + BOOKMARKS_BAR_HEIGHT;
    /// 内容区顶部偏移（不含书签栏）
    pub const CONTENT_TOP_OFFSET_NO_BOOKMARKS: f64 = TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT;
    /// 内容区宽度下限（窗口过窄时保底）
    pub const MIN_CONTENT_WIDTH: f64 = 400.0;
    /// 内容区高度下限
    pub const MIN_CONTENT_HEIGHT: f64 = 100.0;
}

/// 搜索引擎（唯一来源：地址栏、设置页下拉、新标签页、开放 API 都从这里取）
pub mod search {
    /// (id, 显示名, URL 模板)
    pub const ENGINES: &[(&str, &str, &str)] = &[
        ("bing", "必应", "https://www.bing.com/search?q="),
        ("baidu", "百度", "https://www.baidu.com/s?wd="),
        ("google", "谷歌", "https://www.google.com/search?q="),
    ];
    /// 默认引擎 id
    pub const DEFAULT_ENGINE: &str = "bing";

    /// 按 id 取 URL 模板（未知 id 回退默认引擎）
    pub fn url_template(id: &str) -> &'static str {
        ENGINES
            .iter()
            .find(|(i, _, _)| *i == id)
            .or_else(|| ENGINES.iter().find(|(i, _, _)| *i == DEFAULT_ENGINE))
            .map(|(_, _, u)| *u)
            .unwrap_or("")
    }

    /// 拼完整搜索 URL
    pub fn url_for(id: &str, query: &str) -> String {
        format!("{}{}", url_template(id), query)
    }
}

/// UI 常量（色板、默认图标）
pub mod ui {
    /// 标签分组自动配色池（新建分组按序取色）
    pub const GROUP_COLORS: &[&str] = &[
        "#4a9eff", "#f06292", "#ffb74d", "#66bb6a", "#ab47bc", "#26c6da", "#ec407a", "#7e57c2",
        "#8d6e63", "#9ccc65",
    ];
    /// 工作区（空间）配色池
    pub const SPACE_COLORS: &[&str] = &[
        "#4a9eff", "#43a047", "#e53935", "#fb8c00", "#8e24aa", "#00acc1", "#f06292", "#5e35b1",
    ];
    /// 分组色板（用户手动选择用）
    pub const GROUP_PALETTE: &[&str] = &[
        "var(--accent)",
        "#e53935",
        "#43a047",
        "#fb8c00",
        "#8e24aa",
        "#00acc1",
        "#6d4c41",
        "#546e7a",
    ];
    /// 默认 favicon（地球版，横向/垂直标签栏统一使用；内联 SVG data URL）
    pub const DEFAULT_FAVICON: &str = "data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20viewBox%3D%220%200%2016%2016%22%3E%3Ccircle%20cx%3D%228%22%20cy%3D%228%22%20r%3D%226.5%22%20fill%3D%22var(--accent)%22%2F%3E%3Cellipse%20cx%3D%228%22%20cy%3D%228%22%20rx%3D%223%22%20ry%3D%226.5%22%20fill%3D%22none%22%20stroke%3D%22%23fff%22%20stroke-width%3D%220.8%22%2F%3E%3Cpath%20d%3D%22M1.5%208h13%22%20stroke%3D%22%23fff%22%20stroke-width%3D%220.8%22%2F%3E%3C%2Fsvg%3E";
}

#[cfg(test)]
mod tests {
    use super::*;

    // 编译期锁死常量不变量（运行期 assert!(常量) 会被 clippy::assertions_on_constants 拒）
    const _: () = assert!(layout::SPLIT_LEFT_RATIO > 0.0 && layout::SPLIT_LEFT_RATIO < 1.0);
    const _: () = assert!(layout::MIN_CONTENT_WIDTH > 0.0 && layout::MIN_CONTENT_HEIGHT > 0.0);

    #[test]
    fn search_url_template_known_and_fallback() {
        assert_eq!(search::url_template("bing"), "https://www.bing.com/search?q=");
        assert_eq!(search::url_template("baidu"), "https://www.baidu.com/s?wd=");
        assert_eq!(
            search::url_template("google"),
            "https://www.google.com/search?q="
        );
        // 未知/空 id 回退默认引擎（newtab generate 消费此行为，F-22 测试亦依赖）
        assert_eq!(
            search::url_template("duckduckgo"),
            "https://www.bing.com/search?q="
        );
        assert_eq!(search::url_template(""), "https://www.bing.com/search?q=");
    }

    #[test]
    fn search_url_for_concatenates_and_default_listed() {
        assert_eq!(
            search::url_for("bing", "rust"),
            "https://www.bing.com/search?q=rust"
        );
        // 默认引擎必须列在 ENGINES 表里（否则回退链失效，url_template 落到空串）
        assert!(
            search::ENGINES
                .iter()
                .any(|(i, _, _)| *i == search::DEFAULT_ENGINE),
            "DEFAULT_ENGINE 必须在 ENGINES 表内"
        );
        // 所有模板都以协议开头（防止改成相对路径悄悄破坏跳转）
        for (_, _, u) in search::ENGINES {
            assert!(u.starts_with("https://"), "引擎模板必须 https: {u}");
        }
    }

    #[test]
    fn layout_derived_offsets_match_sum() {
        use layout::*;
        // 两个顶部偏移是派生值（和必须与分量一致，改分量漏改派生即红）
        assert_eq!(
            CONTENT_TOP_OFFSET,
            TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT + BOOKMARKS_BAR_HEIGHT
        );
        assert_eq!(
            CONTENT_TOP_OFFSET_NO_BOOKMARKS,
            TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT
        );
    }

    #[test]
    fn ui_palettes_are_valid_colors() {
        let ok = |c: &str| c.starts_with("var(") || (c.starts_with('#') && c.len() == 7);
        for pool in [ui::GROUP_COLORS, ui::SPACE_COLORS, ui::GROUP_PALETTE] {
            assert!(!pool.is_empty(), "配色池不得为空");
            for c in pool {
                assert!(ok(c), "非法色值: {c}");
            }
        }
        assert!(ui::DEFAULT_FAVICON.starts_with("data:image/svg+xml"));
    }
}
