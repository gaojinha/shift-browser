//! 浏览器书签导入（F-91）：多源合并 → 危险 scheme 过滤 → 复用写入内核落库
//!
//! 入口 [`import_bookmarks_from_browser`]：拉 Chrome / Brave 全部 profile，
//! 按 v3 §3.2 优先级序逐源解析（明文 JSON 直读；Encrypted*2 经 F-90 原语，
//! keyring 不可用则该源降级跳过并如实记入 skipped_sources），URL 合并去重
//!（多源同 URL 后者让位前者，大小写不敏感），危险 scheme 过滤复用
//! [`crate::newtab::quick_sites::is_safe_web_url`]（http/https 白名单，与
//! 快捷访问同一份判定），最后交给 [`crate::db::bookmarks_html::do_import_items`]
//! ——与「从 HTML 文件导入」完全同一套写入内核（单事务/文件夹复用/URL 去重）。

use serde::Serialize;

use crate::db::bookmarks::bookmarks_html::{do_import_items, BookmarkItem};
use crate::error::CommandResult;
use crate::newtab::quick_sites::is_safe_web_url;

use super::{BRAVE_BOOKMARK_FILES, BRAVE_ROOT, CHROME_BOOKMARK_FILES, CHROME_ROOT};

#[derive(Debug, Serialize)]
pub struct ImportBookmarksResult {
    /// 实际写入（落库去重后新增）条数
    pub imported: usize,
    /// 源内合并被跳过条数（多源间 URL 重复，后者让位前者）
    pub merged_away: usize,
    /// 危险 scheme（javascript:/data:/file:/vbscript:/blob: 等，白名单外一切）拒收条数
    pub rejected_unsafe: usize,
    /// 读取/解析失败被跳过的源文件（"profile/文件名"，如解密降级、坏 JSON）
    pub skipped_sources: Vec<String>,
    /// 实际参与合并的源文件（"profile/文件名"，优先级序）
    pub used_sources: Vec<String>,
}

/// 从本机浏览器导入书签（Chrome + Brave 一次跑完，多源合并去重）
#[tauri::command]
pub async fn import_bookmarks_from_browser(
    db: tauri::State<'_, crate::db::Db>,
) -> CommandResult<ImportBookmarksResult> {
    let Some(base) = super::config_root() else {
        // 无 HOME：零导入（与扫描口径一致，不报错）
        return Ok(ImportBookmarksResult {
            imported: 0,
            merged_away: 0,
            rejected_unsafe: 0,
            skipped_sources: Vec::new(),
            used_sources: Vec::new(),
        });
    };
    let (raw, used, skipped) = collect_raw_bookmarks(&base).await;
    let total = raw.len();
    // 危险 scheme 过滤（白名单外一律拒收；计数如实回报）
    let safe: Vec<BookmarkItem> = raw
        .into_iter()
        .filter(|b| is_safe_web_url(&b.url))
        .map(|b| BookmarkItem {
            url: b.url,
            title: b.title,
            folder_name: b.folder_name,
        })
        .collect();
    let rejected_unsafe = total - safe.len();
    let imported = do_import_items(&db.pool, &safe)
        .await
        .map_err(crate::error::AppError::Db)?;
    log::info!(
        "[import_browser] 书签导入完成：写入 {} / 合并去重 {} / 拒收不安全 {}（源 {} 个，跳过 {} 个）",
        imported,
        total - rejected_unsafe - imported,
        rejected_unsafe,
        used.len(),
        skipped.len()
    );
    Ok(ImportBookmarksResult {
        imported,
        // 源内合并去重数 = 进了写入内核的条数 − 实写条数（DB 已有同 URL 的差值）
        merged_away: (total - rejected_unsafe).saturating_sub(imported),
        rejected_unsafe,
        skipped_sources: skipped,
        used_sources: used,
    })
}

/// 拉全浏览器书签源（优先级序）并解析为待导入列表。
/// 多源合并规则：同一 URL 以优先靠前的源为准（Account 明文 > bak > 空壳 > 加密）；
/// 返回（条目, 实际使用的源, 读取失败跳过的源）。
async fn collect_raw_bookmarks(
    base: &std::path::Path,
) -> (Vec<RawBookmark>, Vec<String>, Vec<String>) {
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<RawBookmark> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (root, browser, files) in [
        (CHROME_ROOT, "chrome", CHROME_BOOKMARK_FILES),
        (BRAVE_ROOT, "brave", BRAVE_BOOKMARK_FILES),
    ] {
        for prof_dir in profile_dirs(base, root, files) {
            let prof_name = prof_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            for file in files {
                let path = prof_dir.join(file);
                if !path.is_file() {
                    continue;
                }
                let label = format!("{prof_name}/{file}");
                let Some(json) = read_source_json(&path, browser).await else {
                    skipped.push(label); // 读取/解密失败：如实记录，不静默
                    continue;
                };
                let items = parse_chrome_bookmark_json(&json);
                used.push(label);
                for item in items {
                    let key = item.url.trim().to_ascii_lowercase();
                    if !seen_urls.insert(key) {
                        continue; // 多源合并：URL 已见（大小写不敏感）后者让位前者
                    }
                    out.push(item);
                }
            }
        }
    }
    (out, used, skipped)
}

/// profile 子目录列表（含书签源文件才算 profile；排除名单同 F-89 扫描）
fn profile_dirs(
    base: &std::path::Path,
    rel_root: &str,
    files: &[&str],
) -> Vec<std::path::PathBuf> {
    super::profile_dirs(base, rel_root, files)
}

/// 读一个书签源为 JSON 字符串：明文直读；Encrypted*2 走 F-90 原语
///（keyring 不可用 → None 降级跳过，绝不猜、绝不弹解锁）。
async fn read_source_json(path: &std::path::Path, browser: &str) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if name.starts_with("Encrypted") {
        let key = super::fetch_v11_key(browser).await.ok()?;
        let blob = std::fs::read(path).ok()?;
        super::decrypt_os_crypt(&blob, Some(&key)).ok()
    } else {
        std::fs::read_to_string(path).ok()
    }
}

/// 单条待导入书签（解析产物；层级由 folder_name 交给写入内核维护）
#[derive(Debug, Clone, PartialEq)]
struct RawBookmark {
    url: String,
    title: String,
    /// None = 浏览器书签栏（bookmarks/）直下：并入书签栏；
    /// Some = 其余目录最近一层文件夹名（保持层级）
    folder_name: Option<String>,
}

/// 解析 Chrome/Brave 书签 JSON（两者 roots 结构同构）：
/// - `roots.bookmark_bar`（Chrome）/`bookmarks`（Brave，书签栏）下的散书签：
///   folder_name=None（用户预期：书签栏书签进书签栏）；其内子文件夹保持层级
/// - 其余根（other/synced 等）：folder_name=最近一层文件夹名
fn parse_chrome_bookmark_json(json: &str) -> Vec<RawBookmark> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Some(roots) = v.get("roots").and_then(|r| r.as_object()) else {
        return out;
    };
    for (root_name, node) in roots {
        let in_bar = root_name == "bookmark_bar" || root_name == "bookmarks";
        walk_bookmark_node(node, in_bar, None, &mut out);
    }
    out
}

/// 递归走书签节点树；`current_folder` 记最近一层非书签栏文件夹名
fn walk_bookmark_node(
    node: &serde_json::Value,
    in_bar: bool,
    current_folder: Option<String>,
    out: &mut Vec<RawBookmark>,
) {
    let Some(obj) = node.as_object() else { return };
    match obj.get("type").and_then(|t| t.as_str()) {
        Some("url") => {
            let url = obj.get("url").and_then(|u| u.as_str()).unwrap_or("");
            if url.is_empty() {
                return;
            }
            let title = obj
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            out.push(RawBookmark {
                url: url.to_string(),
                title: if title.is_empty() { url.to_string() } else { title },
                folder_name: if in_bar { None } else { current_folder },
            });
        }
        Some("folder") => {
            let name = obj
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            // 书签栏直接子层的散书签仍归书签栏（folder=None）；
            // 子文件夹内/其余根的散书签归最近一层具名文件夹
            let child_folder = if in_bar {
                None
            } else if name.is_empty() {
                current_folder
            } else {
                Some(name)
            };
            if let Some(children) = obj.get("children").and_then(|c| c.as_array()) {
                for c in children {
                    walk_bookmark_node(c, false, child_folder.clone(), out);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod f91_tests {
    use super::*;

    // ---- 解析：书签栏散书签 → None；子文件夹与 other 保持层级 ----

    #[test]
    fn bookmark_bar_urls_have_no_folder() {
        let json = r#"{"roots":{"bookmark_bar":{"type":"folder","name":"书签栏","children":[
            {"type":"url","url":"https://a.com","name":"A"},
            {"type":"folder","name":"Inner","children":[
                {"type":"url","url":"https://b.com","name":"B"}
            ]}
        ]},"other":{"type":"folder","name":"其他书签","children":[
            {"type":"url","url":"https://c.com","name":"C"}
        ]}}}"#;
        let items = parse_chrome_bookmark_json(json);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].url, "https://a.com");
        assert_eq!(items[0].folder_name, None, "书签栏散书签进书签栏");
        assert_eq!(items[1].url, "https://b.com");
        assert_eq!(items[1].folder_name.as_deref(), Some("Inner"), "书签栏子文件夹保持层级");
        assert_eq!(items[2].url, "https://c.com");
        assert_eq!(items[2].folder_name.as_deref(), Some("其他书签"), "other 根不混进书签栏");
    }

    #[test]
    fn brave_bookmarks_root_treated_as_bar() {
        // Brave 书签栏根名是 "bookmarks"（与 Chrome "bookmark_bar" 不同）
        let json = r#"{"roots":{"bookmarks":{"type":"folder","name":"书签栏","children":[
            {"type":"url","url":"https://brave.com","name":"B"}
        ]},"other":{"type":"folder","name":"其他","children":[]}}}"#;
        let items = parse_chrome_bookmark_json(json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].folder_name, None, "Brave 书签栏同样直进书签栏");
    }

    #[test]
    fn nested_folder_uses_deepest_name() {
        let json = r#"{"roots":{"other":{"type":"folder","name":"其他书签","children":[
            {"type":"folder","name":"L1","children":[
                {"type":"folder","name":"L2","children":[
                    {"type":"url","url":"https://deep.com","name":"D"}
                ]}
            ]}
        ]}}}"#;
        let items = parse_chrome_bookmark_json(json);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].folder_name.as_deref(), Some("L2"), "层级取最近一层");
    }

    #[test]
    fn empty_title_falls_back_to_url_and_empty_url_skipped() {
        let json = r#"{"roots":{"bookmark_bar":{"type":"folder","name":"书签栏","children":[
            {"type":"url","url":"https://t.com","name":""},
            {"type":"url","url":"","name":"ghost"}
        ]}}}"#;
        let items = parse_chrome_bookmark_json(json);
        assert_eq!(items.len(), 1, "空 URL 跳过");
        assert_eq!(items[0].title, "https://t.com", "空标题回退 URL");
    }

    #[test]
    fn malformed_json_yields_empty() {
        assert!(parse_chrome_bookmark_json("not json{").is_empty());
        assert!(parse_chrome_bookmark_json(r#"{"no_roots":1}"#).is_empty());
    }

    // ---- 危险 scheme 过滤：复用 is_safe_web_url（白名单口径） ----

    #[test]
    fn unsafe_schemes_rejected_by_shared_check() {
        for url in [
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "file:///etc/passwd",
            "vbscript:x",
            "blob:https://a.com/uuid",
            "chrome://settings",
        ] {
            assert!(!is_safe_web_url(url), "{url} 必须被白名单拒收");
        }
        assert!(is_safe_web_url("https://ok.com"));
        assert!(is_safe_web_url("http://ok.com"));
    }

    // ---- 多源合并：同 URL 后源让位前源（大小写不敏感） ----

    #[test]
    fn dedupe_keeps_first_source_order() {
        let mk = |u: &str| RawBookmark {
            url: u.to_string(),
            title: u.to_string(),
            folder_name: None,
        };
        let mut seen = std::collections::HashSet::new();
        let mut out: Vec<RawBookmark> = Vec::new();
        // 大小写不敏感合并；尾斜杠变体是不同 URL（与写入内核精确匹配口径一致）
        for item in [
            mk("https://dup.com"),
            mk("https://uniq.com"),
            mk("https://DUP.com"),
            mk("https://dup.com/"),
        ] {
            let key = item.url.trim().to_ascii_lowercase();
            if seen.insert(key) {
                out.push(item);
            }
        }
        assert_eq!(out.len(), 3, "同 URL（大小写不敏感）后者让位前者");
        assert_eq!(out[0].url, "https://dup.com");
        assert_eq!(out[1].url, "https://uniq.com");
        assert_eq!(out[2].url, "https://dup.com/", "尾斜杠变体视为不同 URL");
    }
}
