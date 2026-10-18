//! 书签 Netscape HTML 导入/导出（R1 拆分 · 2026-09-25）
//!
//! 导出命令与 HTML 序列化/解析纯逻辑同域居住；导入命令在 `bookmarks.rs` 门面
//! （其主体是 DB 写入），经 `parse_bookmarks_html` 复用解析器。

use crate::db::Db;

/// 导出书签为 Netscape HTML，写入下载目录（统一走 download_dir 设置键），返回文件路径。
#[tauri::command]
pub async fn export_bookmarks_html(db: tauri::State<'_, Db>) -> Result<String, String> {
    // 书签（含文件夹归属）
    let bookmarks = sqlx::query_as::<_, (i64, String, String, Option<i64>, i64, String)>(
    "SELECT id, title, url, folder_id, sort_order, created_at FROM bookmarks ORDER BY sort_order, id",
  )
  .fetch_all(&db.pool)
  .await
  .map_err(|e| e.to_string())?;
    let folders = sqlx::query_as::<_, (i64, String, Option<i64>, i64, String)>(
    "SELECT id, name, parent_id, sort_order, created_at FROM bookmark_folders ORDER BY sort_order, id",
  )
  .fetch_all(&db.pool)
  .await
  .map_err(|e| e.to_string())?;

    // 生成 Netscape 书签 HTML
    let mut html = String::new();
    html.push_str("<!DOCTYPE NETSCAPE-Bookmark-file-1>\n");
    html.push_str("<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n");
    html.push_str("<TITLE>Bookmarks</TITLE>\n");
    html.push_str("<H1>Bookmarks</H1>\n<DL><p>\n");

    // 顶层文件夹（P1-5：递归渲染子文件夹，visited 防环——bookmark_folders 无外键约束，
    // 理论上可能被手工写成环，死循环会把导出卡死）
    let top_folders: Vec<_> = folders.iter().filter(|f| f.2.is_none()).collect();
    let mut visited: Vec<i64> = Vec::new();
    for folder in &top_folders {
        html.push_str(&format!(
            "    <DT><H3>{}</H3>\n    <DL><p>\n",
            escape_html(&folder.1)
        ));
        render_folder_tree(&mut html, &folders, &bookmarks, folder.0, 1, &mut visited);
        html.push_str("    </DL><p>\n");
    }
    // 未分类书签（无文件夹）
    for bm in &bookmarks {
        if bm.3.is_none() {
            html.push_str(&format!(
                "    <DT><A HREF=\"{}\">{}</A>\n",
                escape_html(&bm.2),
                escape_html(&bm.1)
            ));
        }
    }
    html.push_str("</DL><p>\n");

    // 写入下载目录（统一走 download_dir 设置键：三级优先级 覆盖 > XDG > ~/Downloads，
    // 与 GTK 下载回调同链路；同名去重绝不覆盖）
    let dir = crate::engine::platform::resolve_download_dir();
    let file = crate::tabs::backend::unique_download_path(&dir, "bookmarks.html");
    if let Some(parent) = std::path::Path::new(&file).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&file, html).map_err(|e| format!("写入导出文件失败: {}", e))?;
    log::info!("[bookmarks] 导出书签 {} 个 → {}", bookmarks.len(), file);
    Ok(file)
}

/// 导入书签（Netscape HTML 格式，Chrome/Firefox 导出标准）。
/// 解析 <DT><A HREF> 书签项与 <H3> 文件夹层级，同名文件夹复用，返回导入数量。
#[tauri::command]
pub async fn import_bookmarks_html(
    db: tauri::State<'_, Db>,
    html: String,
) -> Result<usize, String> {
    do_import(&db, &html).await
}

/// 选择书签 HTML 文件并导入（系统文件对话框，XDG portal）
#[tauri::command]
pub async fn import_bookmarks_from_file(db: tauri::State<'_, Db>) -> Result<usize, String> {
    let file = rfd::AsyncFileDialog::new()
        .set_title("选择书签文件（Chrome/Firefox 导出的 HTML）")
        .add_filter("书签文件", &["html", "htm"])
        .pick_file()
        .await
        .ok_or_else(|| "已取消".to_string())?;
    let html = std::fs::read_to_string(file.path()).map_err(|e| format!("读取文件失败: {}", e))?;
    let count = do_import(&db, &html).await?;
    log::info!("[bookmarks] 文件导入书签 {} 个", count);
    Ok(count)
}

async fn do_import(db: &Db, html: &str) -> Result<usize, String> {
    let items = parse_bookmarks_html(html);
    do_import_items(&db.pool, &items).await
}

/// 写入内核（HTML 文件导入与浏览器导入 F-91 共用）：单事务、同名文件夹
/// 复用缓存、URL 去重，返回实写条数（DB 已有同 URL 的不计入）。
pub(crate) async fn do_import_items(
    pool: &sqlx::SqlitePool,
    items: &[BookmarkItem],
) -> Result<usize, String> {
    // P1-6：整个导入放一个事务——中途失败整体回滚，不留半截数据；
    // 文件夹名 → id 用 HashMap 缓存，避免同名文件夹重复查询（原实现 N+1）
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let mut folder_cache: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut count = 0usize;
    for item in items {
        let mut folder_id: Option<i64> = None;
        if let Some(folder_name) = &item.folder_name {
            folder_id = match folder_cache.get(folder_name) {
                Some(&fid) => Some(fid),
                None => {
                    // 找同名文件夹，没有则新建
                    let existing: Option<(i64,)> = sqlx::query_as(
                        "SELECT id FROM bookmark_folders WHERE name = ? ORDER BY id LIMIT 1",
                    )
                    .bind(folder_name)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
                    match existing {
                        Some((fid,)) => {
                            folder_cache.insert(folder_name.clone(), fid);
                            Some(fid)
                        }
                        None => {
                            let res = sqlx::query(
                "INSERT INTO bookmark_folders (name, parent_id, sort_order) VALUES (?, NULL, 0)",
              )
              .bind(folder_name)
              .execute(&mut *tx)
              .await
              .map_err(|e| e.to_string())?;
                            let fid = res.last_insert_rowid();
                            folder_cache.insert(folder_name.clone(), fid);
                            Some(fid)
                        }
                    }
                }
            };
        }
        // 去重：同 URL 已存在则跳过
        let dup: Option<(i64,)> =
            sqlx::query_as("SELECT id FROM bookmarks WHERE url = ? ORDER BY id LIMIT 1")
                .bind(&item.url)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        if dup.is_some() {
            continue;
        }
        sqlx::query(
            "INSERT INTO bookmarks (title, url, folder_id, sort_order) VALUES (?, ?, ?, 0)",
        )
        .bind(&item.title)
        .bind(&item.url)
        .bind(folder_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        count += 1;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    log::info!("[bookmarks] 导入书签 {} 个", count);
    Ok(count)
}

/// 转义 HTML 特殊字符
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 书签文件夹行（id, name, parent_id, sort_order, created_at）
type FolderRow = (i64, String, Option<i64>, i64, String);
/// 书签行（id, title, url, folder_id, sort_order, created_at）
type BookmarkRow = (i64, String, String, Option<i64>, i64, String);

/// 递归渲染文件夹树（书签 + 子文件夹），供导出使用
/// - depth 控制缩进，0 为顶层
/// - visited 记录已访问文件夹 id，防环（bookmark_folders 无外键约束）
pub(super) fn render_folder_tree(
    html: &mut String,
    folders: &[FolderRow],
    bookmarks: &[BookmarkRow],
    folder_id: i64,
    depth: usize,
    visited: &mut Vec<i64>,
) {
    // 防环 + 深度上限（防御纵深，正常数据不会触发）
    if depth > 32 || visited.contains(&folder_id) {
        return;
    }
    visited.push(folder_id);
    let pad = "  ".repeat(depth + 1);
    // 子文件夹（递归）
    for f in folders.iter().filter(|f| f.2 == Some(folder_id)) {
        html.push_str(&format!(
            "{}<DT><H3>{}</H3>\n{}<DL><p>\n",
            pad,
            escape_html(&f.1),
            pad
        ));
        render_folder_tree(html, folders, bookmarks, f.0, depth + 1, visited);
        html.push_str(&format!("{}</DL><p>\n", pad));
    }
    // 文件夹内书签
    for bm in bookmarks.iter().filter(|b| b.3 == Some(folder_id)) {
        html.push_str(&format!(
            "{}<DT><A HREF=\"{}\">{}</A>\n",
            pad,
            escape_html(&bm.2),
            escape_html(&bm.1)
        ));
    }
}

/// 解析 Netscape 书签 HTML（Chrome/Firefox 导出标准格式）
pub(super) fn parse_bookmarks_html(html: &str) -> Vec<BookmarkItem> {
    let mut items: Vec<BookmarkItem> = Vec::new();
    let mut folder_stack: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(idx) = rest.find("<DT>") {
        let seg = &rest[idx..];
        let end = seg[4..].find("<DT>").map(|i| i + 4).unwrap_or(seg.len());
        process_segment(&seg[..end], &mut items, &mut folder_stack);
        rest = &seg[end..];
        if end >= seg.len() {
            break;
        }
    }
    items
}

#[derive(Debug, Clone)]
pub(crate) struct BookmarkItem {
    pub url: String,
    pub title: String,
    pub folder_name: Option<String>,
}

fn process_segment(seg: &str, items: &mut Vec<BookmarkItem>, folder_stack: &mut Vec<String>) {
    // 文件夹开始：<DT><H3 ...>名称</H3>
    if let Some(h3) = find_h3(seg) {
        let name = strip_tags(&h3).trim().to_string();
        if !name.is_empty() {
            folder_stack.push(name);
        }
        return;
    }
    // 书签项：<DT><A HREF="...">标题</A>
    if let Some((url, title)) = find_anchor(seg) {
        if !url.is_empty() {
            items.push(BookmarkItem {
                url: url.clone(),
                title: {
                    let t = strip_tags(&title).trim().to_string();
                    if t.is_empty() {
                        url.clone()
                    } else {
                        t
                    }
                },
                folder_name: folder_stack.last().cloned(),
            });
        }
    }
    // 文件夹结束：</DL> 弹出
    let closes = seg.matches("</DL>").count();
    for _ in 0..closes {
        if !folder_stack.is_empty() {
            folder_stack.pop();
        }
    }
}

fn find_h3(seg: &str) -> Option<String> {
    let start = seg.find("<H3")?;
    let gt = seg[start..].find('>')? + start + 1;
    let end = seg[gt..].find("</H3>")? + gt;
    Some(seg[gt..end].to_string())
}

fn find_anchor(seg: &str) -> Option<(String, String)> {
    let start = seg.find("<A ")?;
    let href_start = seg[start..].find("HREF=\"")? + start + 6;
    let href_end = seg[href_start..].find('"')? + href_start;
    let url = seg[href_start..href_end].to_string();
    let gt = seg[start..].find('>')? + start + 1;
    let end = seg[gt..].find("</A>")? + gt;
    let title = seg[gt..end].to_string();
    Some((url, title))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::platform::{resolve_download_dir, set_download_dir_override};
    use crate::tabs::backend::unique_download_path;

    /// 覆盖态是全局 RwLock，测试串行化（cargo test 默认并行跑）。
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 导出落盘路径走 download_dir 覆盖态：覆盖值 > XDG > ~/Downloads，
    /// 与 GTK 下载回调行为一致（resolve_download_dir 三级优先级）。
    #[test]
    fn export_path_uses_download_dir_override() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "bm-export-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let dir_str = dir.to_string_lossy().to_string();
        set_download_dir_override(&dir_str);
        let resolved = resolve_download_dir();
        assert_eq!(resolved, dir_str, "覆盖目录必须被解析优先");
        let path = unique_download_path(&resolved, "bookmarks.html");
        assert!(
            path.starts_with(&dir_str),
            "导出文件必须落在覆盖目录下: {}",
            path
        );
        // 同名去重验证
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, b"test");
        let path2 = unique_download_path(&resolved, "bookmarks.html");
        assert_ne!(path, path2, "同名应去重");
        set_download_dir_override(""); // 还原
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 未设覆盖时 resolve_download_dir 返回非空路径
    #[test]
    fn export_path_fallback_non_empty() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_download_dir_override("");
        let dir = resolve_download_dir();
        assert!(!dir.is_empty(), "回退目录不得为空");
        let path = unique_download_path(&dir, "bookmarks.html");
        assert!(path.contains("bookmarks.html"), "路径应包含文件名");
    }

    #[test]
    fn escape_html_basic() {
        assert_eq!(escape_html("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }

    #[test]
    fn parse_roundtrip() {
        let html = "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
                    <DT><H3>Work</H3>\n<DL><p>\n\
                    <DT><A HREF=\"https://a.com\">A</A>\n\
                    </DL><p>\n\
                    <DT><A HREF=\"https://b.com\">B</A>\n";
        let items = parse_bookmarks_html(html);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://a.com");
        assert_eq!(items[0].folder_name.as_deref(), Some("Work"));
        assert_eq!(items[1].url, "https://b.com");
        assert!(items[1].folder_name.is_none());
    }

    #[test]
    fn parse_nested_folders_and_empty_title_fallback() {
        let html = "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
                    <DT><H3>Level 1</H3>\n<DL><p>\n\
                    <DT><H3>Level 2</H3>\n<DL><p>\n\
                    <DT><A HREF=\"https://sub.org\"><span>Sub Item</span></A>\n\
                    <DT><A HREF=\"https://notitle.org\"></A>\n\
                    </DL><p>\n\
                    <DT><A HREF=\"https://level1.org\">Level 1 Item</A>\n\
                    </DL><p>\n";
        let items = parse_bookmarks_html(html);
        assert_eq!(items.len(), 3);
        // Level 2
        assert_eq!(items[0].url, "https://sub.org");
        assert_eq!(items[0].title, "Sub Item");
        assert_eq!(items[0].folder_name.as_deref(), Some("Level 2"));
        // 空标题应回退为 url
        assert_eq!(items[1].url, "https://notitle.org");
        assert_eq!(items[1].title, "https://notitle.org");
        assert_eq!(items[1].folder_name.as_deref(), Some("Level 2"));
        // 弹出 Level 2 后的 Level 1
        assert_eq!(items[2].url, "https://level1.org");
        assert_eq!(items[2].title, "Level 1 Item");
        assert_eq!(items[2].folder_name.as_deref(), Some("Level 1"));
    }

    #[test]
    fn render_folder_tree_nested_and_cycle_prevention() {
        let folders = vec![
            (1, "Folder A".to_string(), None, 0, "2026-01-01".to_string()),
            (2, "Folder B".to_string(), Some(1), 0, "2026-01-01".to_string()),
            (3, "Cycle Folder".to_string(), Some(3), 0, "2026-01-01".to_string()), // 自环
        ];
        let bookmarks = vec![
            (10, "Title 1 & Special".to_string(), "https://a.com?x=1&y=2".to_string(), Some(1), 0, "2026-01-01".to_string()),
            (11, "Title 2".to_string(), "https://b.com".to_string(), Some(2), 0, "2026-01-01".to_string()),
        ];

        let mut html = String::new();
        let mut visited = Vec::new();

        // 渲染 Folder A
        render_folder_tree(&mut html, &folders, &bookmarks, 1, 1, &mut visited);
        assert!(html.contains("<DT><H3>Folder B</H3>"));
        assert!(html.contains("https://a.com?x=1&amp;y=2"));
        assert!(html.contains("Title 1 &amp; Special"));
        assert!(html.contains("https://b.com"));
        assert!(visited.contains(&1));
        assert!(visited.contains(&2));

        // 循环引用防御测试
        let mut cycle_html = String::new();
        let mut cycle_visited = Vec::new();
        // 传入自环文件夹，必须正常退出不栈溢出
        render_folder_tree(&mut cycle_html, &folders, &bookmarks, 3, 1, &mut cycle_visited);
        assert!(cycle_visited.contains(&3));
    }

    #[test]
    fn render_and_parse_roundtrip_consistency() {
        let folders = vec![
            (1, "Work".to_string(), None, 0, "2026-01-01".to_string()),
        ];
        let bookmarks = vec![
            (10, "Search Engine".to_string(), "https://google.com".to_string(), Some(1), 0, "2026-01-01".to_string()),
        ];

        let mut html = String::new();
        html.push_str("    <DT><H3>Work</H3>\n    <DL><p>\n");
        let mut visited = Vec::new();
        render_folder_tree(&mut html, &folders, &bookmarks, 1, 1, &mut visited);
        html.push_str("    </DL><p>\n");

        let items = parse_bookmarks_html(&html);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].folder_name.as_deref(), Some("Work"));
        assert_eq!(items[0].url, "https://google.com");
        assert_eq!(items[0].title, "Search Engine");
    }
}
