// LINES-EXEMPT: 书签门面：CRUD 命令与导入导出编排同域，主体逻辑已拆到 bookmarks_html.rs，剩余为命令声明与事务编排
// 书签管理（R1 拆分 2026-09-25：Netscape HTML 导入导出命令与解析见 db/bookmarks_html.rs）
// 普通 file module 的子模块默认在同名子目录找，这里文件是兄弟路径，需显式 #[path]
#[path = "bookmarks_html.rs"]
pub(crate) mod bookmarks_html;

use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ts_rs::TS;

#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Bookmark {
    #[ts(type = "number")]
    pub id: i64,
    pub title: String,
    pub url: String,
    #[ts(type = "number | null")]
    pub folder_id: Option<i64>,
    #[ts(type = "number")]
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct BookmarkFolder {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    #[ts(type = "number")]
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

// ===== 核心逻辑（与 Tauri 解耦，直接吃连接池，便于集成测试）=====
// 约定：命令层只负责把 tauri::State 拆成 &pool 后转调 *_inner；
// 所有 SQL 与业务规则只在 *_inner 里写一次，测试直接调它们（不再重抄 SQL）。

/// 获取所有书签（按 sort_order, id 排序）
pub(crate) async fn get_bookmarks_inner(pool: &SqlitePool) -> DbResult<Vec<Bookmark>> {
    let rows = sqlx::query_as::<_, (i64, String, String, Option<i64>, i64, String)>(
        "SELECT id, title, url, folder_id, sort_order, created_at FROM bookmarks ORDER BY sort_order, id",
    )
    .fetch_all(pool)
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

/// 获取所有书签文件夹（按 sort_order, id 排序）
pub(crate) async fn get_bookmark_folders_inner(pool: &SqlitePool) -> DbResult<Vec<BookmarkFolder>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<i64>, i64, String)>(
        "SELECT id, name, parent_id, sort_order, created_at FROM bookmark_folders ORDER BY sort_order, id",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, parent_id, sort_order, created_at)| BookmarkFolder {
            id,
            name,
            parent_id,
            sort_order,
            created_at,
        })
        .collect())
}

/// 添加书签，返回新行 id
pub(crate) async fn create_bookmark_inner(
    pool: &SqlitePool,
    input: &CreateBookmarkInput,
) -> DbResult<i64> {
    let result = sqlx::query(
        "INSERT INTO bookmarks (title, url, folder_id, sort_order) VALUES (?, ?, ?, 
     COALESCE((SELECT MAX(sort_order) + 1 FROM bookmarks WHERE folder_id IS ?), 0))",
    )
    .bind(&input.title)
    .bind(&input.url)
    .bind(input.folder_id)
    .bind(input.folder_id)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

/// 添加书签文件夹，返回新行 id
pub(crate) async fn create_bookmark_folder_inner(
    pool: &SqlitePool,
    input: &CreateFolderInput,
) -> DbResult<i64> {
    let result = sqlx::query(
        "INSERT INTO bookmark_folders (name, parent_id, sort_order) VALUES (?, ?, 
     COALESCE((SELECT MAX(sort_order) + 1 FROM bookmark_folders WHERE parent_id IS ?), 0))",
    )
    .bind(&input.name)
    .bind(input.parent_id)
    .bind(input.parent_id)
    .execute(pool)
    .await?;

    Ok(result.last_insert_rowid())
}

/// 更新书签
/// folder_id：Some(id) 移到指定文件夹；Some(0) 移出文件夹（置 NULL）；None 不修改
pub(crate) async fn update_bookmark_inner(
    pool: &SqlitePool,
    id: i64,
    title: Option<String>,
    url: Option<String>,
    folder_id: Option<i64>,
) -> DbResult<()> {
    if let Some(title) = title {
        sqlx::query("UPDATE bookmarks SET title = ? WHERE id = ?")
            .bind(title)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(url) = url {
        sqlx::query("UPDATE bookmarks SET url = ? WHERE id = ?")
            .bind(url)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(fid) = folder_id {
        if fid == 0 {
            // 移出文件夹（置 NULL）
            sqlx::query("UPDATE bookmarks SET folder_id = NULL WHERE id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        } else {
            sqlx::query("UPDATE bookmarks SET folder_id = ? WHERE id = ?")
                .bind(fid)
                .bind(id)
                .execute(pool)
                .await?;
        }
    }
    Ok(())
}

/// 删除书签
pub(crate) async fn delete_bookmark_inner(pool: &SqlitePool, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM bookmarks WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 删除书签文件夹（级联删除子文件夹和书签）
pub(crate) async fn delete_bookmark_folder_inner(pool: &SqlitePool, id: i64) -> DbResult<()> {
    // 递归收集子文件夹 id
    async fn collect_child_ids(
        pool: &SqlitePool,
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
    collect_child_ids(pool, id, &mut ids).await?;

    let mut tx = pool.begin().await?;
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

/// 搜索书签（标题或 URL 模糊匹配，最多 50 条）
pub(crate) async fn search_bookmarks_inner(
    pool: &SqlitePool,
    query: &str,
) -> DbResult<Vec<Bookmark>> {
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
    .fetch_all(pool)
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

// ===== Tauri 命令层（薄壳：拆 State → 转调 *_inner）=====

/// 获取所有书签
#[tauri::command]
pub async fn get_bookmarks(db: tauri::State<'_, Db>) -> DbResult<Vec<Bookmark>> {
    get_bookmarks_inner(&db.pool).await
}

/// 获取所有书签文件夹
#[tauri::command]
pub async fn get_bookmark_folders(db: tauri::State<'_, Db>) -> DbResult<Vec<BookmarkFolder>> {
    get_bookmark_folders_inner(&db.pool).await
}

/// 添加书签
#[tauri::command]
pub async fn create_bookmark(
    db: tauri::State<'_, Db>,
    input: CreateBookmarkInput,
) -> DbResult<i64> {
    create_bookmark_inner(&db.pool, &input).await
}

/// 添加书签文件夹
#[tauri::command]
pub async fn create_bookmark_folder(
    db: tauri::State<'_, Db>,
    input: CreateFolderInput,
) -> DbResult<i64> {
    create_bookmark_folder_inner(&db.pool, &input).await
}

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
    update_bookmark_inner(&db.pool, id, title, url, folder_id).await
}

/// 删除书签
#[tauri::command]
pub async fn delete_bookmark(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    delete_bookmark_inner(&db.pool, id).await
}

/// 删除书签文件夹（级联删除子文件夹和书签）
#[tauri::command]
pub async fn delete_bookmark_folder(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    delete_bookmark_folder_inner(&db.pool, id).await
}

/// 搜索书签
#[tauri::command]
pub async fn search_bookmarks(db: tauri::State<'_, Db>, query: String) -> DbResult<Vec<Bookmark>> {
    search_bookmarks_inner(&db.pool, &query).await
}

// 导入/导出书签（import_bookmarks_html / import_bookmarks_from_file /
// export_bookmarks_html）三个命令随 Netscape 解析与渲染同居 bookmarks_html.rs；
// lib.rs 注册路径为 bookmarks::bookmarks_html::*，lib_tests 盘点清单已纳入该文件。

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个跑完整迁移的真实库（含 bookmarks / bookmark_folders 表）
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    fn bm(title: &str, url: &str, folder_id: Option<i64>) -> CreateBookmarkInput {
        CreateBookmarkInput {
            title: title.into(),
            url: url.into(),
            folder_id,
        }
    }

    #[tokio::test]
    async fn create_and_list_bookmarks_orders_by_sort_order() {
        let (_dir, db) = temp_db().await;
        create_bookmark_inner(&db.pool, &bm("B", "https://b.com", None))
            .await
            .unwrap();
        create_bookmark_inner(&db.pool, &bm("A", "https://a.com", None))
            .await
            .unwrap();

        let all = get_bookmarks_inner(&db.pool).await.unwrap();
        assert_eq!(all.len(), 2);
        // 同文件夹内按写入顺序递增 sort_order（先插的在前），而非按标题
        assert_eq!(all[0].url, "https://b.com");
        assert_eq!(all[0].sort_order, 0);
        assert_eq!(all[1].sort_order, 1);
    }

    #[tokio::test]
    async fn sort_order_is_scoped_per_folder() {
        let (_dir, db) = temp_db().await;
        let f1 = create_bookmark_folder_inner(
            &db.pool,
            &CreateFolderInput {
                name: "F1".into(),
                parent_id: None,
            },
        )
        .await
        .unwrap();

        // 根目录一条、文件夹内两条：各自从 0 起算，互不影响
        create_bookmark_inner(&db.pool, &bm("root", "https://root.com", None))
            .await
            .unwrap();
        create_bookmark_inner(&db.pool, &bm("f1-a", "https://a.com", Some(f1)))
            .await
            .unwrap();
        create_bookmark_inner(&db.pool, &bm("f1-b", "https://b.com", Some(f1)))
            .await
            .unwrap();

        let in_folder: Vec<_> = get_bookmarks_inner(&db.pool)
            .await
            .unwrap()
            .into_iter()
            .filter(|b| b.folder_id == Some(f1))
            .collect();
        assert_eq!(in_folder.len(), 2);
        assert_eq!(in_folder[0].sort_order, 0);
        assert_eq!(in_folder[1].sort_order, 1);
    }

    #[tokio::test]
    async fn folders_support_nesting() {
        let (_dir, db) = temp_db().await;
        let root = create_bookmark_folder_inner(
            &db.pool,
            &CreateFolderInput {
                name: "根".into(),
                parent_id: None,
            },
        )
        .await
        .unwrap();
        let child = create_bookmark_folder_inner(
            &db.pool,
            &CreateFolderInput {
                name: "子".into(),
                parent_id: Some(root),
            },
        )
        .await
        .unwrap();

        let folders = get_bookmark_folders_inner(&db.pool).await.unwrap();
        assert_eq!(folders.len(), 2);
        let child_row = folders.iter().find(|f| f.id == child).unwrap();
        assert_eq!(child_row.parent_id, Some(root));
    }

    #[tokio::test]
    async fn update_bookmark_partial_and_folder_move() {
        let (_dir, db) = temp_db().await;
        let f = create_bookmark_folder_inner(
            &db.pool,
            &CreateFolderInput {
                name: "F".into(),
                parent_id: None,
            },
        )
        .await
        .unwrap();
        let id = create_bookmark_inner(&db.pool, &bm("旧标题", "https://old.com", None))
            .await
            .unwrap();

        // 只改标题：url/folder_id 不动
        update_bookmark_inner(&db.pool, id, Some("新标题".into()), None, None)
            .await
            .unwrap();
        let b = &get_bookmarks_inner(&db.pool).await.unwrap()[0];
        assert_eq!(b.title, "新标题");
        assert_eq!(b.url, "https://old.com");
        assert_eq!(b.folder_id, None);

        // 移入文件夹
        update_bookmark_inner(&db.pool, id, None, None, Some(f))
            .await
            .unwrap();
        assert_eq!(
            get_bookmarks_inner(&db.pool).await.unwrap()[0].folder_id,
            Some(f)
        );

        // folder_id=Some(0) → 移出（置 NULL）
        update_bookmark_inner(&db.pool, id, None, None, Some(0))
            .await
            .unwrap();
        assert_eq!(
            get_bookmarks_inner(&db.pool).await.unwrap()[0].folder_id,
            None
        );

        // 只改 url
        update_bookmark_inner(&db.pool, id, None, Some("https://new.com".into()), None)
            .await
            .unwrap();
        assert_eq!(
            get_bookmarks_inner(&db.pool).await.unwrap()[0].url,
            "https://new.com"
        );
    }

    #[tokio::test]
    async fn delete_bookmark_removes_row() {
        let (_dir, db) = temp_db().await;
        let id = create_bookmark_inner(&db.pool, &bm("X", "https://x.com", None))
            .await
            .unwrap();
        delete_bookmark_inner(&db.pool, id).await.unwrap();
        assert!(get_bookmarks_inner(&db.pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn delete_folder_cascades_to_children_and_bookmarks() {
        let (_dir, db) = temp_db().await;
        let root = create_bookmark_folder_inner(
            &db.pool,
            &CreateFolderInput {
                name: "根".into(),
                parent_id: None,
            },
        )
        .await
        .unwrap();
        let child = create_bookmark_folder_inner(
            &db.pool,
            &CreateFolderInput {
                name: "子".into(),
                parent_id: Some(root),
            },
        )
        .await
        .unwrap();
        // 两处各放一条书签，外加一条根目录书签（不应被删）
        let b_root = create_bookmark_inner(&db.pool, &bm("r", "https://r.com", Some(root)))
            .await
            .unwrap();
        let b_child = create_bookmark_inner(&db.pool, &bm("c", "https://c.com", Some(child)))
            .await
            .unwrap();
        let b_out = create_bookmark_inner(&db.pool, &bm("out", "https://out.com", None))
            .await
            .unwrap();

        delete_bookmark_folder_inner(&db.pool, root).await.unwrap();

        assert!(get_bookmark_folders_inner(&db.pool).await.unwrap().is_empty());
        let ids: Vec<i64> = get_bookmarks_inner(&db.pool)
            .await
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert!(!ids.contains(&b_root), "根文件夹内的书签应被级联删除");
        assert!(!ids.contains(&b_child), "子文件夹内的书签应被级联删除");
        assert_eq!(ids, vec![b_out], "根目录书签不受影响");
    }

    #[tokio::test]
    async fn search_matches_title_or_url_and_escapes_wildcards() {
        let (_dir, db) = temp_db().await;
        create_bookmark_inner(&db.pool, &bm("100%优惠", "https://sale.com", None))
            .await
            .unwrap();
        create_bookmark_inner(&db.pool, &bm("100xdone", "https://other.com", None))
            .await
            .unwrap();
        create_bookmark_inner(&db.pool, &bm("Rust 指南", "https://rust-lang.org", None))
            .await
            .unwrap();

        // 按 URL 子串命中
        let by_url = search_bookmarks_inner(&db.pool, "rust-lang").await.unwrap();
        assert_eq!(by_url.len(), 1);
        assert_eq!(by_url[0].title, "Rust 指南");

        // 按标题命中
        let by_title = search_bookmarks_inner(&db.pool, "指南").await.unwrap();
        assert_eq!(by_title.len(), 1);

        // % 按字面量匹配：只命中标题含「100%」的那条，而不是把 % 当通配符全命中
        let literal = search_bookmarks_inner(&db.pool, "100%").await.unwrap();
        assert_eq!(literal.len(), 1);
        assert_eq!(literal[0].title, "100%优惠");

        // _ 同样按字面量：无标题含「100_」，应为空
        let underscore = search_bookmarks_inner(&db.pool, "100_").await.unwrap();
        assert!(underscore.is_empty());
    }
}
