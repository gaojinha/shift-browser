// LINES-EXEMPT: 同步包导出/合并导入，拆分排期 R4（2026-10-14 红线卡标注）
//! 同步包导出/合并导入（R1 拆分 · 2026-09-24）
//!
//! 导出：password 列**原样密文**（服务器零知识的关键：绝不解密），
//! 外加书签/会话（≤50）/历史（≤300）。
//! 导入：按 domain+username UPSERT，仅当远端更新时覆盖；绝不删除本地独有。

use crate::db::{Db, DbError, DbResult};

use super::store::now_str;

/// 导出分页大小：密码/书签逐片拉取，避免万级库单次全表查询卡死同步超时.
/// 协议不变（仍合成单个包一次 PUT），分片只约束单次 SQL 规模。
const EXPORT_PAGE: i64 = 500;

/// 导出密码库：password 列**原样密文**（服务器零知识的关键：绝不解密）
pub(super) async fn export_payload(db: &Db) -> DbResult<Vec<u8>> {
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut offset = 0i64;
    loop {
        let rows = sqlx::query_as::<_, (String, Option<String>, String, Option<String>, String, String)>(
            "SELECT domain, username, password, notes, created_at, updated_at FROM passwords ORDER BY id LIMIT ? OFFSET ?",
        )
        .bind(EXPORT_PAGE)
        .bind(offset)
        .fetch_all(&db.pool)
        .await?;
        let n = rows.len();
        entries.extend(rows.into_iter().map(
            |(domain, username, password, notes, created_at, updated_at)| {
                serde_json::json!({
                    "domain": domain,
                    "username": username,
                    "password": password,
                    "notes": notes,
                    "created_at": created_at,
                    "updated_at": updated_at,
                })
            },
        ));
        offset += n as i64;
        if n < EXPORT_PAGE as usize {
            break;
        }
    }

    // 书签（阶段4 多端协同）：跨端同步；文件夹按名字保留，导入时 find-or-create
    let mut bookmarks: Vec<serde_json::Value> = Vec::new();
    let mut bm_offset = 0i64;
    loop {
        let bm_rows = sqlx::query_as::<_, (String, String, Option<String>, i64, String)>(
            "SELECT b.title, b.url, f.name, b.sort_order, b.created_at
             FROM bookmarks b LEFT JOIN bookmark_folders f ON f.id = b.folder_id
             ORDER BY b.id LIMIT ? OFFSET ?",
        )
        .bind(EXPORT_PAGE)
        .bind(bm_offset)
        .fetch_all(&db.pool)
        .await?;
        let n = bm_rows.len();
        bookmarks.extend(bm_rows.into_iter().map(
            |(title, url, folder, sort_order, created_at)| {
                serde_json::json!({
                    "title": title,
                    "url": url,
                    "folder": folder,
                    "sort_order": sort_order,
                    "created_at": created_at,
                })
            },
        ));
        bm_offset += n as i64;
        if n < EXPORT_PAGE as usize {
            break;
        }
    }

    // 会话（阶段4 多端协同）：标签集跨端；按 (name, created_at) 去重，最多带 50 条
    let se_rows = sqlx::query_as::<_, (String, String, String)>(
        "SELECT name, data, created_at FROM sessions ORDER BY id DESC LIMIT 50",
    )
    .fetch_all(&db.pool)
    .await?;
    let sessions: Vec<serde_json::Value> = se_rows
        .into_iter()
        .map(|(name, data, created_at)| {
            serde_json::json!({
                "name": name,
                "data": data,
                "created_at": created_at,
            })
        })
        .collect();

    // 历史（阶段4 多端协同）：最近 300 条跨端；按 url 合并（取最大 visit_count / 最新时间）
    let hi_rows = sqlx::query_as::<_, (String, Option<String>, i64, String)>(
        "SELECT url, title, visit_count, last_visit_at FROM history
         ORDER BY last_visit_at DESC LIMIT 300",
    )
    .fetch_all(&db.pool)
    .await?;
    let history: Vec<serde_json::Value> = hi_rows
        .into_iter()
        .map(|(url, title, visit_count, last_visit_at)| {
            serde_json::json!({
                "url": url,
                "title": title,
                "visit_count": visit_count,
                "last_visit_at": last_visit_at,
            })
        })
        .collect();

    let exported_at = now_str(db).await;
    let payload = serde_json::json!({
        "version": 4,
        "exported_at": exported_at,
        "entries": entries,
        "bookmarks": bookmarks,
        "sessions": sessions,
        "history": history,
    });
    serde_json::to_vec(&payload).map_err(|e| DbError::Other(format!("导出序列化失败: {}", e)))
}

/// 合并导入：按 domain+username UPSERT，仅当远端更新时覆盖；绝不删除本地独有
pub(super) async fn import_payload(db: &Db, plain: &[u8]) -> DbResult<usize> {
    let v: serde_json::Value = serde_json::from_slice(plain)
        .map_err(|e| DbError::Other(format!("同步包解析失败: {}", e)))?;
    let entries = v
        .get("entries")
        .and_then(|e| e.as_array())
        .ok_or_else(|| DbError::Other("同步包格式非法：缺少 entries".into()))?;

    let mut count = 0usize;
    for e in entries {
        let domain = e.get("domain").and_then(|x| x.as_str()).unwrap_or("");
        if domain.is_empty() {
            continue;
        }
        let username = e.get("username").and_then(|x| x.as_str());
        let password = e.get("password").and_then(|x| x.as_str()).unwrap_or("");
        let notes = e.get("notes").and_then(|x| x.as_str());
        let created_at = e.get("created_at").and_then(|x| x.as_str()).unwrap_or("");
        let updated_at = e.get("updated_at").and_then(|x| x.as_str()).unwrap_or("");

        sqlx::query(
            "INSERT INTO passwords (domain, username, password, notes, created_at, updated_at)
             VALUES (?, ?, ?, ?, COALESCE(NULLIF(?, ''), datetime('now')), COALESCE(NULLIF(?, ''), datetime('now')))
             ON CONFLICT(domain, username) DO UPDATE SET
               password = excluded.password,
               notes = excluded.notes,
               updated_at = excluded.updated_at
             WHERE excluded.updated_at > passwords.updated_at",
        )
        .bind(domain)
        .bind(username)
        .bind(password)
        .bind(notes)
        .bind(created_at)
        .bind(updated_at)
        .execute(&db.pool)
        .await
        .map_err(|e| DbError::Other(format!("合并写入失败: {}", e)))?;
        count += 1;
    }

    // 书签合并（阶段4 · 版本 2 起）：按 url 去重——本地已有同 url 则跳过（不覆盖本地）；
    // 文件夹按名字 find-or-create。旧版同步包（无 bookmarks 字段）自然跳过。
    if let Some(bms) = v.get("bookmarks").and_then(|b| b.as_array()) {
        for b in bms {
            let url = b.get("url").and_then(|x| x.as_str()).unwrap_or("");
            if url.is_empty() {
                continue;
            }
            let exists: Option<(i64,)> =
                sqlx::query_as("SELECT id FROM bookmarks WHERE url = ? LIMIT 1")
                    .bind(url)
                    .fetch_optional(&db.pool)
                    .await?;
            if exists.is_some() {
                continue;
            }
            let title = b.get("title").and_then(|x| x.as_str()).unwrap_or(url);
            let sort_order = b.get("sort_order").and_then(|x| x.as_i64()).unwrap_or(0);
            let created_at = b.get("created_at").and_then(|x| x.as_str()).unwrap_or("");
            let folder_id: Option<i64> = match b.get("folder").and_then(|x| x.as_str()) {
                Some(n) if !n.is_empty() => {
                    let f: Option<(i64,)> =
                        sqlx::query_as("SELECT id FROM bookmark_folders WHERE name = ? LIMIT 1")
                            .bind(n)
                            .fetch_optional(&db.pool)
                            .await?;
                    match f {
                        Some((id,)) => Some(id),
                        None => {
                            let r = sqlx::query(
                                "INSERT INTO bookmark_folders (name, sort_order) VALUES (?, 0)",
                            )
                            .bind(n)
                            .execute(&db.pool)
                            .await?;
                            Some(r.last_insert_rowid())
                        }
                    }
                }
                _ => None,
            };
            sqlx::query(
                "INSERT INTO bookmarks (title, url, folder_id, sort_order, created_at)
                 VALUES (?, ?, ?, ?, COALESCE(NULLIF(?, ''), datetime('now')))",
            )
            .bind(title)
            .bind(url)
            .bind(folder_id)
            .bind(sort_order)
            .bind(created_at)
            .execute(&db.pool)
            .await
            .map_err(|e| DbError::Other(format!("书签合并失败: {}", e)))?;
            count += 1;
        }
    }

    // 会话合并（阶段4 · 版本 3 起）：按 (name, created_at) 去重；workspace 不跨端（置空）。
    if let Some(ss) = v.get("sessions").and_then(|s| s.as_array()) {
        for s in ss {
            let name = s.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let created_at = s.get("created_at").and_then(|x| x.as_str()).unwrap_or("");
            let data = s.get("data").and_then(|x| x.as_str()).unwrap_or("");
            if name.is_empty() || created_at.is_empty() || data.is_empty() {
                continue;
            }
            let exists: Option<(i64,)> =
                sqlx::query_as("SELECT id FROM sessions WHERE name = ? AND created_at = ? LIMIT 1")
                    .bind(name)
                    .bind(created_at)
                    .fetch_optional(&db.pool)
                    .await?;
            if exists.is_some() {
                continue;
            }
            sqlx::query(
                "INSERT INTO sessions (name, workspace_id, data, created_at) VALUES (?, NULL, ?, ?)",
            )
            .bind(name)
            .bind(data)
            .bind(created_at)
            .execute(&db.pool)
            .await
            .map_err(|e| DbError::Other(format!("会话合并失败: {}", e)))?;
            count += 1;
        }
    }

    // 历史合并（阶段4 · 版本 4 起）：按 url UPSERT——取最大 visit_count + 最新 last_visit_at，
    // 标题仅在本地为空时补（不覆盖本地）。旧版包（无 history 字段）自然跳过。
    if let Some(hs) = v.get("history").and_then(|h| h.as_array()) {
        for h in hs {
            let url = h.get("url").and_then(|x| x.as_str()).unwrap_or("");
            if url.is_empty() {
                continue;
            }
            let title = h.get("title").and_then(|x| x.as_str());
            let visit_count = h.get("visit_count").and_then(|x| x.as_i64()).unwrap_or(1);
            let last_visit_at = h
                .get("last_visit_at")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            sqlx::query(
                "INSERT INTO history (url, title, visit_count, last_visit_at)
                 VALUES (?, ?, ?, COALESCE(NULLIF(?, ''), datetime('now')))
                 ON CONFLICT(url) DO UPDATE SET
                   title = COALESCE(history.title, excluded.title),
                   visit_count = MAX(history.visit_count, excluded.visit_count),
                   last_visit_at = MAX(history.last_visit_at, excluded.last_visit_at)",
            )
            .bind(url)
            .bind(title)
            .bind(visit_count)
            .bind(last_visit_at)
            .execute(&db.pool)
            .await
            .map_err(|e| DbError::Other(format!("历史合并失败: {}", e)))?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("t.db");
        let opts = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE passwords (id INTEGER PRIMARY KEY AUTOINCREMENT, domain TEXT NOT NULL, username TEXT, password TEXT NOT NULL, notes TEXT, created_at TEXT, updated_at TEXT, UNIQUE(domain, username))",
            "CREATE TABLE bookmark_folders (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, parent_id INTEGER, sort_order INTEGER DEFAULT 0, created_at TEXT DEFAULT (datetime('now')))",
            "CREATE TABLE bookmarks (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, url TEXT NOT NULL, folder_id INTEGER, sort_order INTEGER DEFAULT 0, created_at TEXT DEFAULT (datetime('now')))",
            "CREATE TABLE sessions (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, workspace_id INTEGER, data TEXT NOT NULL, created_at TEXT DEFAULT (datetime('now')))",
            "CREATE TABLE history (id INTEGER PRIMARY KEY AUTOINCREMENT, url TEXT NOT NULL, title TEXT, visit_count INTEGER DEFAULT 1, last_visit_at TEXT DEFAULT (datetime('now')), created_at TEXT DEFAULT (datetime('now')))",
            "CREATE UNIQUE INDEX idx_history_url_uniq ON history(url)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        let db = Db {
            pool: pool.clone(),
            feed_pool: pool,
            key_path: dir.path().join("master.key"),
            vault_path: dir.path().join("vault.key"),
            translate_key_path: dir.path().join("translate.key"),
            master_key: tokio::sync::Mutex::new(None),
            translate_key: tokio::sync::Mutex::new(None),
        };
        (dir, db)
    }

    #[tokio::test]
    async fn import_merges_bookmarks_by_url_and_creates_folder() {
        let (_d, db) = temp_db().await;
        // 本地先有一条书签（同 url 的远端条目应被跳过、不覆盖本地）
        sqlx::query("INSERT INTO bookmarks (title, url) VALUES ('本地', 'https://a.com')")
            .execute(&db.pool)
            .await
            .unwrap();
        // 本地历史（visit_count=2）——导入的 visit_count=5 应取最大、标题不覆盖本地
        sqlx::query(
            "INSERT INTO history (url, title, visit_count, last_visit_at)
             VALUES ('https://a.com', '本地标题', 2, '2026-09-21 09:00:00')",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        let payload = serde_json::json!({
            "version": 3,
            "entries": [],
            "bookmarks": [
                {"title": "远端A", "url": "https://a.com", "folder": null},
                {"title": "远端B", "url": "https://b.com", "folder": "工作"}
            ],
            "sessions": [
                {"name": "自动保存", "data": "[]", "created_at": "2026-09-21 10:00:00"},
                {"name": "自动保存", "data": "[]", "created_at": "2026-09-21 11:00:00"}
            ],
            "history": [
                {"url": "https://a.com", "title": "远端标题", "visit_count": 5, "last_visit_at": "2026-09-21 12:00:00"}
            ]
        });
        let plain = serde_json::to_vec(&payload).unwrap();
        let n = import_payload(&db, &plain).await.unwrap();
        assert_eq!(n, 4, "b.com + 两个会话 + 一条历史");
        // 历史按 url 合并：visit_count 取最大、last_visit_at 取最新、标题不覆盖本地
        let (vc, lv, t): (i64, String, String) = sqlx::query_as(
            "SELECT visit_count, last_visit_at, title FROM history WHERE url='https://a.com'",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(vc, 5);
        assert_eq!(lv, "2026-09-21 12:00:00");
        assert_eq!(t, "本地标题");
        let cnt: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM bookmarks")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(cnt.0, 2);
        let f: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM bookmark_folders WHERE name='工作'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(f.0, 1, "文件夹应按名字 find-or-create");
        let sc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(sc.0, 2, "两个会话应按 (name, created_at) 合并入库");
        // 重复导入：书签/会话幂等跳过；历史是 UPSERT（数据不变，但计一次"处理"）
        let n2 = import_payload(&db, &plain).await.unwrap();
        assert_eq!(n2, 1, "仅历史被重新 UPSERT");
        let hc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM history")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(hc.0, 1, "历史仍只有一行（UPSERT 不重复插入）");
        // 旧版包（无 bookmarks 字段）不应报错
        let v1 = serde_json::to_vec(&serde_json::json!({"version": 1, "entries": []})).unwrap();
        assert_eq!(import_payload(&db, &v1).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn export_pages_cover_all_rows() {
        let (_dir, db) = temp_db().await;
        // 超出一页（500）的密码与书签：分片导出必须一根不少
        for i in 0..505 {
            sqlx::query("INSERT INTO passwords (domain, username, password) VALUES (?, ?, 'x')")
                .bind(format!("d{i}.example.com"))
                .bind(format!("u{i}"))
                .execute(&db.pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO bookmarks (title, url) VALUES (?, ?)")
                .bind(format!("t{i}"))
                .bind(format!("https://b{i}.example.com"))
                .execute(&db.pool)
                .await
                .unwrap();
        }
        let raw = export_payload(&db).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["entries"].as_array().unwrap().len(), 505);
        assert_eq!(v["bookmarks"].as_array().unwrap().len(), 505);
    }
}
