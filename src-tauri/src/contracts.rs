// 跨端契约单一事实来源（设置键 / 事件名 / 内部 URL / 超时 / 快捷键）
//
// 注意：本文件用普通注释而非 `//!` 文档注释——`build.rs` 会 `include!` 它，
// 展开后 `//!` 不在文件开头会导致 "expected outer doc comment" 编译错误。
//
// **本文件必须自包含**——不引用 crate 内其他模块（build.rs 独立编译）。
//
// 用途：Rust 侧为源 → build.rs 生成前端 `src/constants.ts` 的契约部分，
// 让"后端白名单 / 前端默认值 / 事件名 / 快捷键"只有一份定义，拼错或漏改编译期/构建期即暴露。
// 见 docs/架构-能力与常量统一治理方案_2026-09-23.md

/// 设置项（键 / 默认值 / 类型 / 敏感标记 / 说明）
pub mod settings {
    /// 设置值类型（前端用于表单渲染，后端用于校验提示）
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        Str,
        Bool,
        Int,
    }

    impl Kind {
        /// 生成 TS 用的类型标签
        pub fn as_ts(&self) -> &'static str {
            match self {
                Kind::Str => "str",
                Kind::Bool => "bool",
                Kind::Int => "int",
            }
        }
    }

    /// 单个设置项定义
    pub struct SettingDef {
        pub key: &'static str,
        pub default: &'static str,
        pub kind: Kind,
        pub sensitive: bool,
        pub desc: &'static str,
    }

    /// 敏感键（即使误加进 SETTINGS 也必须拒绝返回/写入前端）
    pub const SENSITIVE_KEYS: &[&str] = &["master_key"];

    /// 全部设置键（**唯一来源**：后端白名单与前端默认值都从这里派生）
    pub const SETTINGS: &[SettingDef] = &[
        // ---- 基础 ----
        SettingDef { key: "homepage", default: "newtab", kind: Kind::Str, sensitive: false, desc: "启动页（'newtab' 或 URL）" },
        SettingDef { key: "search_engine", default: "bing", kind: Kind::Str, sensitive: false, desc: "搜索引擎（bing/baidu/google）" },
        SettingDef { key: "theme", default: "dark", kind: Kind::Str, sensitive: false, desc: "主题（dark/light）" },
        SettingDef { key: "theme_color", default: "", kind: Kind::Str, sensitive: false, desc: "主题色（空=默认）" },
        SettingDef { key: "wallpaper", default: "", kind: Kind::Str, sensitive: false, desc: "启动页壁纸（空=Bing 每日图）" },
        SettingDef { key: "show_bookmarks_bar", default: "true", kind: Kind::Bool, sensitive: false, desc: "显示书签栏" },
        SettingDef { key: "vertical_tabs", default: "false", kind: Kind::Bool, sensitive: false, desc: "垂直标签栏" },
        SettingDef { key: "record_history", default: "false", kind: Kind::Bool, sensitive: false, desc: "记录浏览历史（隐私优先默认关）" },
        SettingDef { key: "font_size", default: "16", kind: Kind::Int, sensitive: false, desc: "启动页字号" },
        SettingDef { key: "show_bg", default: "true", kind: Kind::Bool, sensitive: false, desc: "启动页显示壁纸" },
        SettingDef { key: "show_lunar", default: "true", kind: Kind::Bool, sensitive: false, desc: "启动页显示农历" },
        SettingDef { key: "show_quote", default: "true", kind: Kind::Bool, sensitive: false, desc: "启动页显示每日一句" },
        SettingDef { key: "show_weather", default: "true", kind: Kind::Bool, sensitive: false, desc: "启动页显示天气" },
        SettingDef { key: "weather_city", default: "", kind: Kind::Str, sensitive: false, desc: "天气城市（空=自动定位）" },
        SettingDef { key: "lock_enabled", default: "true", kind: Kind::Bool, sensitive: false, desc: "启动锁屏" },
        // ---- AI ----
        SettingDef { key: "ai_model", default: "agnes-2.0-flash", kind: Kind::Str, sensitive: false, desc: "AI 模型" },
        SettingDef { key: "ai_base_url", default: "https://apihub.agnes-ai.com/v1", kind: Kind::Str, sensitive: false, desc: "AI API 地址" },
        SettingDef { key: "ai_api_key", default: "", kind: Kind::Str, sensitive: false, desc: "AI API Key（留空用环境变量；掩码存储）" },
        // ---- 隐私 / 拦截 ----
        SettingDef { key: "adblock_enabled", default: "true", kind: Kind::Bool, sensitive: false, desc: "广告拦截" },
        SettingDef { key: "adblock_keywords", default: "", kind: Kind::Str, sensitive: false, desc: "自定义拦截关键词（换行分隔）" },
        SettingDef { key: "privacy_shield", default: "true", kind: Kind::Bool, sensitive: false, desc: "隐私防护（旧布尔开关，兼容）" },
        SettingDef { key: "privacy_shield_level", default: "standard", kind: Kind::Str, sensitive: false, desc: "隐私防护档位（off/standard/strict）" },
        SettingDef { key: "privacy_whitelist", default: "", kind: Kind::Str, sensitive: false, desc: "隐私防护白名单域名" },
        SettingDef { key: "gpc_header", default: "true", kind: Kind::Bool, sensitive: false, desc: "发送 GPC 请求头（Sec-GPC: 1）" },
        SettingDef { key: "net_block_rules", default: "", kind: Kind::Str, sensitive: false, desc: "网络层拦截规则（每行一个 URL 子串）" },
        SettingDef { key: "net_headers", default: "", kind: Kind::Str, sensitive: false, desc: "自定义请求头（每行 Name: Value）" },
        // ---- 性能 ----
        SettingDef { key: "tab_hibernate_minutes", default: "30", kind: Kind::Int, sensitive: false, desc: "空闲标签休眠超时（分钟，0=关）" },
        SettingDef { key: "tab_hibernate_count", default: "0", kind: Kind::Int, sensitive: false, desc: "空闲标签休眠数量阈值（0=关）" },
        // ---- 翻译 ----
        SettingDef { key: "translate_engine", default: "google", kind: Kind::Str, sensitive: false, desc: "翻译源（google/bing）" },
        SettingDef { key: "translate_target", default: "zh-CN", kind: Kind::Str, sensitive: false, desc: "翻译目标语言" },
        SettingDef { key: "translate_timeout", default: "15", kind: Kind::Int, sensitive: false, desc: "翻译超时（秒，5-120）" },
        // ---- 自动化 ----
        SettingDef { key: "auto_clear_history_days", default: "0", kind: Kind::Int, sensitive: false, desc: "自动清理历史（天，0=关）" },
        SettingDef { key: "auto_refresh_feeds_minutes", default: "0", kind: Kind::Int, sensitive: false, desc: "自动刷新资讯（分钟，0=关）" },
        SettingDef { key: "auto_agent_minutes", default: "0", kind: Kind::Int, sensitive: false, desc: "定时 Agent（分钟，0=关）" },
        SettingDef { key: "auto_agent_prompt", default: "", kind: Kind::Str, sensitive: false, desc: "定时 Agent 提示词" },
        // ---- 开放 API ----
        SettingDef { key: "api_allow_eval", default: "false", kind: Kind::Bool, sensitive: false, desc: "允许执行 JS（危险，默认关）" },
        SettingDef { key: "api_http_port", default: "19701", kind: Kind::Int, sensitive: false, desc: "开放 API HTTP 端口" },
        SettingDef { key: "api_bind_lan", default: "false", kind: Kind::Bool, sensitive: false, desc: "API 绑局域网（0.0.0.0）" },
        SettingDef { key: "api_model_gateway", default: "false", kind: Kind::Bool, sensitive: false, desc: "模型网关" },
        SettingDef { key: "api_readonly", default: "false", kind: Kind::Bool, sensitive: false, desc: "API 只读模式（禁写命令）" },
        SettingDef { key: "api_allowed_ips", default: "", kind: Kind::Str, sensitive: false, desc: "API IP 白名单（逗号分隔前缀）" },
    ];

    /// 是否为敏感设置键
    pub fn is_sensitive(key: &str) -> bool {
        SENSITIVE_KEYS.contains(&key)
    }

    /// 是否为允许读写的设置键
    pub fn is_allowed(key: &str) -> bool {
        SETTINGS.iter().any(|s| s.key == key)
    }

    /// 取某键的默认值
    pub fn default_of(key: &str) -> Option<&'static str> {
        SETTINGS.iter().find(|s| s.key == key).map(|s| s.default)
    }
}
