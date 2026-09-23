//! 构建脚本：生成前端常量镜像 `src/constants.ts`
//!
//! 常量单一来源在 `src/constants.rs`（自包含、无 crate 依赖），这里 `include!`
//! 后序列化成 TypeScript，保证前后端常量永远一致——彻底消除"注释写'与后端一致'"
//! 的人肉同步（历史问题：布局高度、搜索引擎 URL、默认 favicon 都曾多处重复）。
//!
//! 见 docs/架构-能力与常量统一治理方案_2026-09-23.md

include!("src/constants.rs");

fn main() {
    generate_ts_constants();
    tauri_build::build();
}

/// 把 constants.rs 的常量写成前端 `src/constants.ts`
fn generate_ts_constants() {
    let mut s = String::new();
    s.push_str("// 本文件由 src-tauri/build.rs 自动生成，请勿手改！\n");
    s.push_str("// 单一来源：src-tauri/src/constants.rs\n\n");

    // ---- 布局尺寸 ----
    s.push_str("export const LAYOUT = {\n");
    s.push_str(&format!("  tabBar: {},\n", layout::TAB_BAR_HEIGHT as i32));
    s.push_str(&format!("  addressBar: {},\n", layout::ADDRESS_BAR_HEIGHT as i32));
    s.push_str(&format!("  bookmarksBar: {},\n", layout::BOOKMARKS_BAR_HEIGHT as i32));
    s.push_str(&format!("  statusBar: {},\n", layout::STATUS_BAR_HEIGHT as i32));
    s.push_str(&format!("  aiPanelWidth: {},\n", layout::AI_PANEL_WIDTH as i32));
    s.push_str(&format!(
        "  verticalTabBarWidth: {},\n",
        layout::VERTICAL_TAB_BAR_WIDTH as i32
    ));
    s.push_str(&format!("  splitLeftRatio: {},\n", layout::SPLIT_LEFT_RATIO));
    s.push_str(&format!(
        "  contentTopOffset: {},\n",
        layout::CONTENT_TOP_OFFSET as i32
    ));
    s.push_str(&format!(
        "  contentTopOffsetNoBookmarks: {},\n",
        layout::CONTENT_TOP_OFFSET_NO_BOOKMARKS as i32
    ));
    s.push_str("} as const\n\n");

    // ---- 搜索引擎（地址栏 / 设置页 / 新标签页共用）----
    s.push_str("export const SEARCH_ENGINES = [\n");
    for (id, label, url) in search::ENGINES {
        s.push_str(&format!("  {{ id: '{id}', label: '{label}', url: '{url}' }},\n"));
    }
    s.push_str("] as const\n\n");
    s.push_str("export const SEARCH_URLS: Record<string, string> = Object.fromEntries(\n");
    s.push_str("  SEARCH_ENGINES.map((e) => [e.id, e.url]),\n");
    s.push_str(")\n\n");

    // ---- UI 色板 / 默认图标 ----
    s.push_str("export const UI = {\n");
    s.push_str(&format!("  groupColors: {},\n", ts_str_array(ui::GROUP_COLORS)));
    s.push_str(&format!("  spaceColors: {},\n", ts_str_array(ui::SPACE_COLORS)));
    s.push_str(&format!("  groupPalette: {},\n", ts_str_array(ui::GROUP_PALETTE)));
    s.push_str(&format!("  defaultFavicon: '{}',\n", ui::DEFAULT_FAVICON));
    s.push_str("} as const\n");

    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/constants.ts");
    std::fs::write(&out, s).expect("生成 src/constants.ts 失败");
    println!("cargo:rerun-if-changed=src/constants.rs");
}

/// `&["a","b"]` → `['a', 'b']`
fn ts_str_array(items: &[&str]) -> String {
    format!(
        "[{}]",
        items
            .iter()
            .map(|c| format!("'{c}'"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}
