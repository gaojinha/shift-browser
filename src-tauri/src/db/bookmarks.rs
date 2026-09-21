// 书签管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};
use tauri::Manager;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Bookmark {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub folder_id: Option<i64>,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookmarkFolder {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateBookmarkInput {
    pub title: String,
    pub url: String,
    pub folder_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateFolderInput {
    pub name: String,
    pub parent_id: Option<i64>,
}

/// 获取所有书签
#[tauri::command]
pub async fn get_bookmarks(db: tauri::State<'_, Db>) -> DbResult<Vec<Bookmark>> {
    let rows = sqlx::query_as::<_, (i64, String, String, Option<i64>, i64, String)>(
    "SELECT id, title, url, folder_id, sort_order, created_at FROM bookmarks ORDER BY sort_order, id",
  )
  .fetch_all(&db.pool)
  .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, title, url, folder_id, sort_order, created_at)| Bookmark {
                id,
                title,
                url,
                folder_id,
                sort_order,
                created_at,
            },
        )
        .collect())
}

/// 获取所有书签文件夹
#[tauri::command]
pub async fn get_bookmark_folders(db: tauri::State<'_, Db>) -> DbResult<Vec<BookmarkFolder>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<i64>, i64, String)>(
    "SELECT id, name, parent_id, sort_order, created_at FROM bookmark_folders ORDER BY sort_order, id",
  )
  .fetch_all(&db.pool)
  .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, name, parent_id, sort_order, created_at)| BookmarkFolder {
                id,
                name,
                parent_id,
                sort_order,
                created_at,
            },
        )
        .collect())
}

/// 添加书签
#[tauri::command]
pub async fn create_bookmark(
    db: tauri::State<'_, Db>,
    input: CreateBookmarkInput,
) -> DbResult<i64> {
    let result = sqlx::query(
        "INSERT INTO bookmarks (title, url, folder_id, sort_order) VALUES (?, ?, ?, 
     COALESCE((SELECT MAX(sort_order) + 1 FROM bookmarks WHERE folder_id IS ?), 0))",
    )
    .bind(&input.title)
    .bind(&input.url)
    .bind(input.folder_id)
    .bind(input.folder_id)
    .execute(&db.pool)
    .await?;

    Ok(result.last_insert_rowid())
}

/// 添加书签文件夹
#[tauri::command]
pub async fn create_bookmark_folder(
    db: tauri::State<'_, Db>,
    input: CreateFolderInput,
) -> DbResult<i64> {
    let result = sqlx::query(
        "INSERT INTO bookmark_folders (name, parent_id, sort_order) VALUES (?, ?, 
     COALESCE((SELECT MAX(sort_order) + 1 FROM bookmark_folders WHERE parent_id IS ?), 0))",
    )
    .bind(&input.name)
    .bind(input.parent_id)
    .bind(input.parent_id)
    .execute(&db.pool)
    .await?;

    Ok(result.last_insert_rowid())
}

/// 更新书签
/// 更新书签
/// folder_id：Some(id) 移到指定文件夹；Some(0) 移出文件夹（置 NULL）；None 不修改
#[tauri::command]
pub async fn update_bookmark(
    db: tauri::State<'_, Db>,
    id: i64,
    title: Option<String>,
    url: Option<String>,
    folder_id: Option<i64>,
) -> DbResult<()> {
    if let Some(title) = title {
        sqlx::query("UPDATE bookmarks SET title = ? WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    if let Some(url) = url {
        sqlx::query("UPDATE bookmarks SET url = ? WHERE id = ?")
            .bind(url)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    if let Some(fid) = folder_id {
        if fid == 0 {
            // 移出文件夹（置 NULL）
            sqlx::query("UPDATE bookmarks SET folder_id = NULL WHERE id = ?")
                .bind(id)
                .execute(&db.pool)
                .await?;
        } else {
            sqlx::query("UPDATE bookmarks SET folder_id = ? WHERE id = ?")
                .bind(fid)
                .bind(id)
                .execute(&db.pool)
                .await?;
        }
    }
    Ok(())
}

/// 删除书签
#[tauri::command]
pub async fn delete_bookmark(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM bookmarks WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// 删除书签文件夹（级联删除子文件夹和书签）
#[tauri::command]
pub async fn delete_bookmark_folder(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    // 递归收集子文件夹 id
    async fn collect_child_ids(
        pool: &sqlx::SqlitePool,
        parent_id: i64,
        ids: &mut Vec<i64>,
    ) -> DbResult<()> {
        let rows: Vec<(i64,)> =
            sqlx::query_as("SELECT id FROM bookmark_folders WHERE parent_id = ?")
                .bind(parent_id)
                .fetch_all(pool)
                .await?;
        for (child_id,) in rows {
            ids.push(child_id);
            Box::pin(collect_child_ids(pool, child_id, ids)).await?;
        }
        Ok(())
    }

    let mut ids = vec![id];
    collect_child_ids(&db.pool, id, &mut ids).await?;

    let mut tx = db.pool.begin().await?;
    for fid in &ids {
        sqlx::query("DELETE FROM bookmarks WHERE folder_id = ?")
            .bind(fid)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM bookmark_folders WHERE id = ?")
            .bind(fid)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 搜索书签
#[tauri::command]
pub async fn search_bookmarks(db: tauri::State<'_, Db>, query: String) -> DbResult<Vec<Bookmark>> {
    // LIKE 通配符转义：% 和 _ 是 SQL LIKE 的通配符，用户输入含这些字符时按字面量匹配
    // （先转义反斜杠本身，再转义 % 和 _，配合 ESCAPE '\' 使用，与 search_history 保持一致）
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let like = format!("%{}%", escaped);
    let rows = sqlx::query_as::<_, (i64, String, String, Option<i64>, i64, String)>(
        "SELECT id, title, url, folder_id, sort_order, created_at FROM bookmarks
     WHERE title LIKE ? ESCAPE '\\' OR url LIKE ? ESCAPE '\\' ORDER BY title LIMIT 50",
    )
    .bind(&like)
    .bind(&like)
    .fetch_all(&db.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, title, url, folder_id, sort_order, created_at)| Bookmark {
                id,
                title,
                url,
                folder_id,
                sort_order,
                created_at,
            },
        )
        .collect())
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
    // P1-6：整个导入放一个事务——中途失败整体回滚，不留半截数据；
    // 文件夹名 → id 用 HashMap 缓存，避免同名文件夹重复查询（原实现 N+1）
    let mut tx = db.pool.begin().await.map_err(|e| e.to_string())?;
    let mut folder_cache: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut count = 0usize;
    for item in &items {
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

/// 导出书签为 Netscape HTML，写入用户下载目录，返回文件路径。
#[tauri::command]
pub async fn export_bookmarks_html(
    db: tauri::State<'_, Db>,
    app: tauri::AppHandle,
) -> Result<String, String> {
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

    // 写入下载目录
    let dir = app
        .path()
        .download_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let file = dir.join("bookmarks.html");
    std::fs::write(&file, html).map_err(|e| format!("写入导出文件失败: {}", e))?;
    log::info!("[bookmarks] 导出书签 {} 个 → {:?}", bookmarks.len(), file);
    Ok(file.to_string_lossy().to_string())
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
fn render_folder_tree(
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
fn parse_bookmarks_html(html: &str) -> Vec<BookmarkItem> {
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
struct BookmarkItem {
    url: String,
    title: String,
    folder_name: Option<String>,
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
