//! 启动查版本提示（R4-4 先行版 · 2026-10-14）
//!
//! 只提示不安装：启动后前端调用一次，查 GitCode Release 最新版本号，
//! 有新版返回提示信息（含 Release 页链接），由前端 toast 展示。
//!
//! 两条硬约束：
//!
//! 1. **fail-silent**：本机实测 GitCode API 会被 CloudWAF 拦截（返回 HTML），
//!    网络失败/WAF/解析失败一律返回 None 静默降级——查版本是尽力而为，
//!    绝不能弹错误打扰用户、更不能拖慢启动。
//! 2. **默认开、显式关**：设置键 `update_check_enabled` 缺省即开启
//!    （安全修复触达是公共利益），只有显式写 "false" 才关闭。

use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::error::CommandResult;

/// GitCode Release API（公开仓库免 token；owner/repo 与 about.rs repo_url 同源）
const RELEASE_API: &str =
    "https://api.gitcode.com/api/v5/repos/gaojinha/shiftbrowser-tauri/releases/latest";
/// Release 浏览页（提示里指向这里，用户自行下载）
const RELEASE_PAGE: &str = "https://gitcode.com/gaojinha/shiftbrowser-tauri/releases";

/// 有新版时的提示信息（无新版 / 查询失败 = 前端收到 null）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
}

#[derive(serde::Deserialize)]
struct LatestRelease {
    tag_name: String,
}

/// 启动查版本主体（fail-silent：任何失败返回 None 静默降级，绝不打扰用户）
async fn check_inner(db: &Db) -> Option<UpdateInfo> {
    // 显式关闭才跳过（缺省开启：安全修复触达优先）
    let enabled: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'update_check_enabled'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    if enabled.map(|(v,)| v == "false").unwrap_or(false) {
        return None;
    }

    let client = reqwest::Client::builder()
        .user_agent(concat!("ShiftBrowser/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::CONNECT_TIMEOUT_SECS,
        ))
        .timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::UPDATE_CHECK_SECS,
        ))
        .build()
        .ok()?;
    let release: LatestRelease = client
        .get(RELEASE_API)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;

    let current = env!("CARGO_PKG_VERSION");
    if !is_newer_release(&release.tag_name, current) {
        return None;
    }
    Some(UpdateInfo {
        current_version: current.to_string(),
        latest_version: release.tag_name.trim_start_matches('v').to_string(),
        release_url: RELEASE_PAGE.to_string(),
    })
}

/// 启动查版本（只提示不安装；fail-silent —— 正常路径恒 Ok，Err 仅为满足
/// Tauri async 命令签名约束，前端按 null 处理静默降级）
#[tauri::command]
pub async fn check_for_update(db: State<'_, Db>) -> CommandResult<Option<UpdateInfo>> {
    Ok(check_inner(&db).await)
}

/// "v5.9.0" / "5.8.3" → [5, 9, 0]（非数字段丢弃；解析不出返回 None）
fn parse_version(s: &str) -> Option<Vec<u64>> {
    let parts: Vec<u64> = s
        .trim()
        .trim_start_matches(['v', 'V'])
        .split('.')
        .map(|p| p.trim().parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

/// latest 是否比 current 新（逐段数值比较，短版本右侧补 0；相等/更旧/解析失败都算不新）
fn is_newer_release(latest_tag: &str, current: &str) -> bool {
    match (parse_version(latest_tag), parse_version(current)) {
        (Some(l), Some(c)) => {
            let n = l.len().max(c.len());
            for i in 0..n {
                let a = l.get(i).copied().unwrap_or(0);
                let b = c.get(i).copied().unwrap_or(0);
                if a != b {
                    return a > b;
                }
            }
            false
        }
        _ => false, // 任一解析失败 → 不提示（宁可不提示，不误报）
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parse_handles_v_prefix_and_garbage() {
        assert_eq!(parse_version("v5.8.3"), Some(vec![5, 8, 3]));
        assert_eq!(parse_version("5.8.3"), Some(vec![5, 8, 3]));
        assert_eq!(parse_version("V6"), Some(vec![6]));
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version("5.x.3"), None);
    }

    /// 防漂移：比较语义（数值段、补零、宁可不提示）
    #[test]
    fn newer_release_comparison() {
        assert!(is_newer_release("v5.9.0", "5.8.3"));
        assert!(is_newer_release("v6", "5.8.3")); // 短版本补零
        assert!(is_newer_release("v5.8.10", "5.8.9")); // 数值比较非字典序
        assert!(!is_newer_release("v5.8.3", "5.8.3")); // 相等不提示
        assert!(!is_newer_release("v5.8.2", "5.8.3"));
        assert!(!is_newer_release("garbage", "5.8.3")); // 解析失败不提示
    }
}
