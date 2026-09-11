// 书签管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

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
    .map(|(id, title, url, folder_id, sort_order, created_at)| Bookmark {
      id,
      title,
      url,
      folder_id,
      sort_order,
      created_at,
    })
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
    .map(|(id, name, parent_id, sort_order, created_at)| BookmarkFolder {
      id,
      name,
      parent_id,
      sort_order,
      created_at,
    })
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
  .bind(&input.folder_id)
  .bind(&input.folder_id)
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
  .bind(&input.parent_id)
  .bind(&input.parent_id)
  .execute(&db.pool)
  .await?;

  Ok(result.last_insert_rowid())
}

/// 更新书签
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
  if let Some(folder_id) = folder_id {
    sqlx::query("UPDATE bookmarks SET folder_id = ? WHERE id = ?")
      .bind(folder_id)
      .bind(id)
      .execute(&db.pool)
      .await?;
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
    let rows: Vec<(i64,)> = sqlx::query_as("SELECT id FROM bookmark_folders WHERE parent_id = ?")
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
pub async fn search_bookmarks(
  db: tauri::State<'_, Db>,
  query: String,
) -> DbResult<Vec<Bookmark>> {
  let like = format!("%{}%", query);
  let rows = sqlx::query_as::<_, (i64, String, String, Option<i64>, i64, String)>(
    "SELECT id, title, url, folder_id, sort_order, created_at FROM bookmarks
     WHERE title LIKE ? OR url LIKE ? ORDER BY title LIMIT 50",
  )
  .bind(&like)
  .bind(&like)
  .fetch_all(&db.pool)
  .await?;

  Ok(rows
    .into_iter()
    .map(|(id, title, url, folder_id, sort_order, created_at)| Bookmark {
      id,
      title,
      url,
      folder_id,
      sort_order,
      created_at,
    })
    .collect())
}