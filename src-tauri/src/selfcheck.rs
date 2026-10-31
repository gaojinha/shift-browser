// LINES-EXEMPT: 自检项清单，集中一处便于与运维手册逐条对照
//! 自检模块（战略阶段 0 · 可观测性）
//!
//! 一键体检：把"出问题时最需要知道的那些事实"汇总成结构化 JSON，
//! 既能给用户看（设置页/自检入口），也能进日志给排查用。
//!
//! 设计原则（对齐战略"能力显性化"）：
//! - 只读，绝不修改任何状态
//! - 单项失败不影响整体（每项独立兜底，标注 error 字段）
//! - 版本/路径/计数这类"环境事实"必须报，避免用户描述问题时说不清环境

use serde::Serialize;
use tauri::Manager;

use crate::db::Db;
use crate::tabs::TabManager;

/// 单项自检结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    /// 检查项名称
    pub name: String,
    /// 是否正常
    pub ok: bool,
    /// 人类可读的结论
    pub detail: String,
}

impl CheckItem {
    fn ok(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            ok: true,
            detail: detail.into(),
        }
    }
    fn bad(name: &str, detail: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            ok: false,
            detail: detail.into(),
        }
    }
}

/// 完整自检报告
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfCheckReport {
    /// 整体是否健康（所有必检项通过）
    pub healthy: bool,
    /// 生成时间（Unix 秒）
    pub timestamp: i64,
    /// 版本与环境
    pub versions: Vec<CheckItem>,
    /// 数据层
    pub storage: Vec<CheckItem>,
    /// 运行时
    pub runtime: Vec<CheckItem>,
    /// 能力状态
    pub capabilities: Vec<CheckItem>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 解析 /proc/net/if_inet6 文本，返回（全局单播地址数，总地址数）。
/// 行格式：32位hex地址 + 接口索引hex + 前缀长hex + 范围scope hex + 标志hex + 网卡名。
/// scope 0x00=全局，0x10=本机回环(::1)、0x20=链路本地(fe80::/10)，只数全局。
fn parse_if_inet6(text: &str) -> (usize, usize) {
    let mut global = 0usize;
    let mut total = 0usize;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 6 {
            continue;
        }
        if f[0].len() != 32 || !f[0].chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        total += 1;
        if u8::from_str_radix(f[3], 16).unwrap_or(0xff) == 0x00 {
            global += 1;
        }
    }
    (global, total)
}

/// IPv6 本地栈自检项（建议性，永不报 bad、不影响 healthy）。
#[cfg(target_os = "linux")]
fn ipv6_stack_item() -> CheckItem {
    match std::fs::read_to_string("/proc/net/if_inet6") {
        Ok(text) => {
            let (global, _) = parse_if_inet6(&text);
            if global > 0 {
                CheckItem::ok("IPv6", format!("就绪（全局地址 {} 个）", global))
            } else {
                CheckItem::ok("IPv6", "本机无全局地址，出网走 IPv4".to_string())
            }
        }
        Err(_) => CheckItem::ok("IPv6", "未启用（出网走 IPv4）".to_string()),
    }
}

#[cfg(not(target_os = "linux"))]
fn ipv6_stack_item() -> CheckItem {
    CheckItem::ok("IPv6", "当前平台不探测".to_string())
}

/// 版本与环境信息
fn check_versions(app: &tauri::AppHandle) -> Vec<CheckItem> {
    let mut items = Vec::new();

    items.push(CheckItem::ok(
        "ShiftBrowser",
        format!("v{}", app.package_info().version),
    ));

    // WebKitGTK 运行时版本（引擎升级/兼容问题排查的第一手信息）
    let (major, minor, micro) = unsafe {
        (
            webkit2gtk::ffi::webkit_get_major_version(),
            webkit2gtk::ffi::webkit_get_minor_version(),
            webkit2gtk::ffi::webkit_get_micro_version(),
        )
    };
    items.push(CheckItem::ok(
        "WebKitGTK",
        format!("{}.{}.{}", major, minor, micro),
    ));

    items.push(CheckItem::ok(
        "操作系统",
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    ));

    // IPv6 本地栈（2026-09-29 丁钉后台 web.js 加载失败复盘：单边地址族卡住是常因）。
    // 纯本地读取（/proc，不出网、零遥测），只报事实：本机有无全局 IPv6 地址。
    // 建议性，不影响 healthy——无 v6 时出网走 IPv4 本就是正道；有 v6 但远端抖动时，
    // 建连上限（contracts::timeout::CONNECT_TIMEOUT_SECS）负责快速切换地址族。
    items.push(ipv6_stack_item());

    // 应用数据目录（权限/路径问题排查）
    match app.path().app_data_dir() {
        Ok(dir) => {
            let exists = dir.exists();
            items.push(if exists {
                CheckItem::ok("数据目录", dir.display().to_string())
            } else {
                CheckItem::bad("数据目录", format!("不存在: {}", dir.display()))
            });
        }
        Err(e) => items.push(CheckItem::bad("数据目录", format!("获取失败: {}", e))),
    }

    items
}

/// 数据层：数据库可读写、表数量、关键表行数
async fn check_storage(app: &tauri::AppHandle) -> Vec<CheckItem> {
    let mut items = Vec::new();
    // 软着陆：Db 尚未注册（启动极早/单测）时报不健康，不崩进程（与 check_runtime 同口径）
    let Some(db) = app.try_state::<Db>() else {
        items.push(CheckItem::bad("数据库", "状态未注册"));
        return items;
    };

    // 连通性（最基础，先查）
    match sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&db.pool)
        .await
    {
        Ok(_) => items.push(CheckItem::ok("数据库", "连接正常")),
        Err(e) => {
            items.push(CheckItem::bad("数据库", format!("连接失败: {}", e)));
            return items; // 连不上后面不用查了
        }
    }

    // 表数量
    match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&db.pool)
    .await
    {
        Ok(n) => items.push(CheckItem::ok("数据表", format!("{} 张", n))),
        Err(e) => items.push(CheckItem::bad("数据表", format!("统计失败: {}", e))),
    }

    // 关键表行数（书签/历史/密码 —— 用户最关心的数据）
    for (label, sql) in [
        ("书签", "SELECT COUNT(*) FROM bookmarks"),
        ("历史", "SELECT COUNT(*) FROM history"),
        ("密码", "SELECT COUNT(*) FROM passwords"),
    ] {
        match sqlx::query_scalar::<_, i64>(sql).fetch_one(&db.pool).await {
            Ok(n) => items.push(CheckItem::ok(label, format!("{} 条", n))),
            Err(e) => items.push(CheckItem::bad(label, format!("查询失败: {}", e))),
        }
    }

    // 密码主密钥文件（密码能否解密的关键，缺失=密码全废）
    let key_path = db.key_path.clone();
    if key_path.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_path)
                .map(|m| m.permissions().mode() & 0o777)
                .unwrap_or(0);
            if mode == 0o600 {
                items.push(CheckItem::ok("密码主密钥", "存在（权限 600）"));
            } else {
                items.push(CheckItem::bad(
                    "密码主密钥",
                    format!("权限过宽: {:o}（应为 600）", mode),
                ));
            }
        }
        #[cfg(not(unix))]
        items.push(CheckItem::ok("密码主密钥", "存在"));
    } else {
        items.push(CheckItem::ok(
            "密码主密钥",
            "尚未生成（未保存过密码时正常）",
        ));
    }

    items
}

/// 运行时：标签数、AI 分屏、书签栏、焦点
fn check_runtime(app: &tauri::AppHandle) -> Vec<CheckItem> {
    let mut items = Vec::new();
    let Some(state) = app.try_state::<TabManager>() else {
        items.push(CheckItem::bad("标签管理器", "状态未注册"));
        return items;
    };

    let (count, active, ai_active) = state.runtime_snapshot();
    items.push(CheckItem::ok(
        "标签",
        match active {
            Some(id) => format!("{} 个（活动: {}）", count, &id[..id.len().min(8)]),
            None => format!("{} 个（无活动标签）", count),
        },
    ));

    // 主窗口存在性 + 尺寸
    match app.get_window("main") {
        Some(win) => {
            let size = win
                .inner_size()
                .map(|s| format!("{}x{}", s.width, s.height))
                .unwrap_or_else(|_| "未知".to_string());
            items.push(CheckItem::ok("主窗口", size));
        }
        None => items.push(CheckItem::bad("主窗口", "不存在")),
    }

    items.push(CheckItem::ok(
        "AI 分屏",
        if ai_active { "已开启" } else { "未开启" },
    ));

    items
}

/// 能力状态：广告拦截（内容过滤器）、隐私档位、扩展
async fn check_capabilities(app: &tauri::AppHandle) -> Vec<CheckItem> {
    let mut items = Vec::new();
    // 内容过滤器状态不依赖 Db，先报；Db 未注册时后半段软着陆
    // 网络层内容过滤器（广告拦截 2.0）
    let cf = crate::engine::content_filter::status();
    let ready = cf["ready"].as_bool().unwrap_or(false);
    let enabled = cf["enabled"].as_bool().unwrap_or(false);
    let rule_count = cf["ruleCount"].as_i64().unwrap_or(0);
    items.push(if !enabled {
        CheckItem::ok("内容过滤器", "已关闭（广告拦截关闭）")
    } else if ready {
        CheckItem::ok(
            "内容过滤器",
            format!("已就绪（内置规则 {} 条）", rule_count),
        )
    } else {
        CheckItem::bad("内容过滤器", "已开启但尚未编译完成")
    });

    // Db 未注册时后半段查不了，报不健康不崩进程
    let Some(db) = app.try_state::<Db>() else {
        items.push(CheckItem::bad("设置存储", "状态未注册"));
        return items;
    };

    // 广告拦截开关
    let adblock: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key='adblock_enabled'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    let adblock_on = adblock.map(|v| v == "true").unwrap_or(true);
    items.push(CheckItem::ok(
        "广告拦截",
        if adblock_on { "开启" } else { "关闭" },
    ));

    // 隐私防护档位
    let level: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key='privacy_shield_level'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    items.push(CheckItem::ok(
        "隐私防护",
        level.unwrap_or_else(|| "standard（默认）".to_string()),
    ));

    // 已启用扩展数
    match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM extensions WHERE enabled=1")
        .fetch_one(&db.pool)
        .await
    {
        Ok(n) => items.push(CheckItem::ok("启用扩展", format!("{} 个", n))),
        Err(_) => items.push(CheckItem::ok("启用扩展", "未知")),
    }

    items
}

/// 执行完整自检
pub async fn run(app: &tauri::AppHandle) -> SelfCheckReport {
    let versions = check_versions(app);
    let storage = check_storage(app).await;
    let runtime = check_runtime(app);
    let capabilities = check_capabilities(app).await;

    let healthy = versions
        .iter()
        .chain(storage.iter())
        .chain(runtime.iter())
        .chain(capabilities.iter())
        .all(|i| i.ok);

    SelfCheckReport {
        healthy,
        timestamp: now_secs(),
        versions,
        storage,
        runtime,
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_item_helpers() {
        let a = CheckItem::ok("x", "正常");
        assert!(a.ok);
        assert_eq!(a.name, "x");
        assert_eq!(a.detail, "正常");

        let b = CheckItem::bad("y", "坏了");
        assert!(!b.ok);
        assert_eq!(b.name, "y");
        assert_eq!(b.detail, "坏了");

        // 接受 String 与 Into<String> 类型
        let c = CheckItem::ok("z", String::from("动态生成信息"));
        assert!(c.ok);
        assert_eq!(c.detail, "动态生成信息");
    }

    #[test]
    fn parse_if_inet6_counts_global_only() {
        // 回环(::1 scope 10) + 链路本地(fe80 scope 20) + 全局(2001 scope 00)各一
        let text = "00000000000000000000000000000001 01 80 10 80 lo\n\
                    fe800000000000000000000000000001 02 40 20 80 eth0\n\
                    240887260000001d0003000000000011 02 40 00 00 eth0\n";
        assert_eq!(super::parse_if_inet6(text), (1, 3));
        // 残行/非法行跳过不计数
        assert_eq!(super::parse_if_inet6("garbage line\n\n"), (0, 0));
        assert_eq!(super::parse_if_inet6(""), (0, 0));
    }

    #[test]
    fn parse_if_inet6_边界格式与无效字段过滤() {
        // 字段少于 6 个跳过（前 5 个字段存在但缺少网卡名）
        let missing_col = "240887260000001d0003000000000011 02 40 00 00\n";
        assert_eq!(super::parse_if_inet6(missing_col), (0, 0));

        // 地址长度不等于 32 位（31 位与 33 位 hex）跳过
        let short_addr = "240887260000001d000300000000001 02 40 00 00 eth0\n";
        let long_addr = "240887260000001d00030000000000111 02 40 00 00 eth0\n";
        assert_eq!(super::parse_if_inet6(short_addr), (0, 0));
        assert_eq!(super::parse_if_inet6(long_addr), (0, 0));

        // 地址包含非 16 进制字符（例如包含 g）跳过
        let non_hex_addr = "240887260000001d000300000000001g 02 40 00 00 eth0\n";
        assert_eq!(super::parse_if_inet6(non_hex_addr), (0, 0));

        // scope 为非 16 进制（如 xx）时进入 unwrap_or(0xff)，计入 total 但不计入 global
        let invalid_scope = "240887260000001d0003000000000011 02 40 xx 00 eth0\n";
        assert_eq!(super::parse_if_inet6(invalid_scope), (0, 1));

        // 多行混合：仅合法行计入，全局 scope (00) 准确识别
        let mixed = "\
            # 注释或空行\n\
            \n\
            240887260000001d0003000000000011 02 40 00 00 eth0\n\
            fe800000000000000000000000000002 02 40 20 80 eth0\n\
            invalid row\n\
            20010db8000000000000000000000001 03 40 00 00 wlan0\n";
        assert_eq!(super::parse_if_inet6(mixed), (2, 3));
    }

    #[test]
    fn ipv6_probe_never_fails_health() {
        // 建议性探针：任何情况下不得报 bad（healthy 语义不受影响）
        assert!(super::ipv6_stack_item().ok);
    }
}
