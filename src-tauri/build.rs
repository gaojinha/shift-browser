//! 构建脚本：生成前端常量镜像 `src/constants.ts`
//!
//! 常量单一来源在 `src/constants.rs`（自包含、无 crate 依赖），这里 `include!`
//! 后序列化成 TypeScript，保证前后端常量永远一致——彻底消除"注释写'与后端一致'"
//! 的人肉同步（历史问题：布局高度、搜索引擎 URL、默认 favicon 都曾多处重复）。
//!
//! 见 docs/架构-能力与常量统一治理方案_2026-09-23.md

// 构建脚本在编译期运行，IO/生成失败只能让编译立即中止（panic 是 build.rs 的
// 标准错误通道，无法像运行时代码那样向用户优雅降级），整文件豁免 expect_used。
#![allow(clippy::expect_used)]

include!("src/constants.rs");
include!("src/contracts.rs");

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
    s.push_str("} as const\n\n");

    // ---- 设置契约（键 / 默认值 / 类型 / 敏感标记）----
    // 后端白名单与前端默认值同源，杜绝"前端缺默认值 → get() 空串失真"
    s.push_str("export const SETTINGS_SCHEMA = {\n");
    for d in settings::SETTINGS {
        s.push_str(&format!(
            "  {}: {{ default: '{}', kind: '{}', sensitive: {}, desc: '{}' }},\n",
            d.key,
            escape_single(d.default),
            d.kind.as_ts(),
            d.sensitive,
            escape_single(d.desc),
        ));
    }
    s.push_str("} as const\n\n");
    s.push_str("export const DEFAULT_SETTINGS: Record<string, string> = Object.fromEntries(\n");
    s.push_str("  Object.entries(SETTINGS_SCHEMA).map(([k, v]) => [k, v.default]),\n");
    s.push_str(")\n\n");

    // ---- IPC 事件名（后端 emit / 前端 listen 共用）----
    s.push_str("export const EVENTS = {\n");
    for (name, value) in events::ALL {
        s.push_str(&format!("  {name}: '{value}',\n"));
    }
    s.push_str("} as const\n\n");

    // ---- 内部 URL / 路由 ----
    s.push_str("export const URLS = {\n");
    for (name, value) in urls::ALL {
        s.push_str(&format!("  {name}: '{value}',\n"));
    }
    s.push_str("} as const\n\n");

    // ---- 超时 / 间隔（语义命名）----
    s.push_str("export const TIMEOUT = {\n");
    for (name, value) in timeout::ALL {
        s.push_str(&format!("  {name}: {value},\n"));
    }
    s.push_str("} as const\n\n");

    // ---- 快捷键（前端 keydown 匹配用；Rust GTK 兜底表同源）----
    s.push_str("// 组合键字符串，形如 'ctrl+shift+t'（与前端 matchShortcut 约定一致）\n");
    s.push_str("export const SHORTCUTS = {\n");
    for (name, value) in shortcuts::ALL {
        s.push_str(&format!("  {name}: '{value}',\n"));
    }
    s.push_str("} as const\n\n");

    // ---- AI 技能（前端快捷入口数据驱动）----
    s.push_str("export const SKILLS = [\n");
    for (id, name, icon, desc) in skills::ALL {
        s.push_str(&format!(
            "  {{ id: '{id}', name: '{name}', icon: '{icon}', desc: '{desc}' }},\n"
        ));
    }
    s.push_str("] as const\n");

    // ---- AI 行为约定（AI 面板 system 消息；MCP initialize 的 instructions 同源）----
    s.push_str("export const AI_BEHAVIOR_RULES = ");
    s.push_str(&format!(
        "{:?}\n",
        ai_behavior::BEHAVIOR_RULES.replace('\r', "\\r").replace('\n', "\\n")
    ));

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

/// 转义字符串里的单引号（描述文案里可能有 `'newtab'` 这类写法）
fn escape_single(s: &str) -> String {
    s.replace('\'', "\\'")
}
