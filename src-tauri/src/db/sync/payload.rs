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

    #[tokio::test]
    async fn export_import_roundtrip_preserves_all_rows() {
        // 远端库塞满四类数据（含 NULL 字段），导出后导入全新空库，逐行比对
        let (_rd, remote) = temp_db().await;
        sqlx::query(
            "INSERT INTO passwords (domain, username, password, notes, created_at, updated_at)
             VALUES ('x.com', 'a', 'enc1', 'note1', '2026-09-01 10:00:00', '2026-09-20 10:00:00')",
        )
        .execute(&remote.pool)
        .await
        .unwrap();
        // 注：username 不用 NULL——UNIQUE(domain, username) 在 SQLite 里 NULL 互不相等，
        // ON CONFLICT 对 NULL username 永不命中，重复导入会插重复行（已知怪癖，回执已上报）
        sqlx::query(
            "INSERT INTO passwords (domain, username, password, notes, created_at, updated_at)
             VALUES ('y.com', 'b', 'enc2', NULL, '2026-09-02 10:00:00', '2026-09-21 09:00:00')",
        )
        .execute(&remote.pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO bookmark_folders (name) VALUES ('工作')")
            .execute(&remote.pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO bookmarks (title, url, folder_id, sort_order, created_at)
             SELECT 't1', 'https://x.com', id, 3, '2026-09-03 10:00:00' FROM bookmark_folders WHERE name='工作'",
        )
        .execute(&remote.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (name, data, created_at) VALUES ('s1', '[]', '2026-09-04 10:00:00')",
        )
        .execute(&remote.pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO history (url, title, visit_count, last_visit_at)
             VALUES ('https://x.com', 'H', 7, '2026-09-05 10:00:00')",
        )
        .execute(&remote.pool)
        .await
        .unwrap();

        let raw = export_payload(&remote).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["version"].as_u64(), Some(4), "导出协议版本锚点");
        assert!(v["exported_at"].as_str().unwrap().len() >= 19);

        let (_ld, local) = temp_db().await;
        assert_eq!(
            import_payload(&local, &raw).await.unwrap(),
            5,
            "密码 2 + 书签 1 + 会话 1 + 历史 1"
        );
        let (pw, notes, u): (String, Option<String>, Option<String>) =
            sqlx::query_as("SELECT password, notes, username FROM passwords WHERE domain='x.com'")
                .fetch_one(&local.pool)
                .await
                .unwrap();
        assert_eq!(pw, "enc1", "密文原样透传");
        assert_eq!(notes.as_deref(), Some("note1"));
        assert_eq!(u.as_deref(), Some("a"));
        // 远端 NULL notes → 导入落库为 NULL，不变成空串
        let (u2, n2): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT username, notes FROM passwords WHERE domain='y.com'")
                .fetch_one(&local.pool)
                .await
                .unwrap();
        assert_eq!(u2.as_deref(), Some("b"));
        assert!(n2.is_none());
        let bm: (i64, Option<String>) = sqlx::query_as(
            "SELECT b.sort_order, f.name FROM bookmarks b
             LEFT JOIN bookmark_folders f ON f.id = b.folder_id WHERE b.url='https://x.com'",
        )
        .fetch_one(&local.pool)
        .await
        .unwrap();
        assert_eq!(bm.0, 3, "sort_order 保留");
        assert_eq!(
            bm.1.as_deref(),
            Some("工作"),
            "文件夹 find-or-create 同名复用"
        );
        let (vc, lv): (i64, String) = sqlx::query_as(
            "SELECT visit_count, last_visit_at FROM history WHERE url='https://x.com'",
        )
        .fetch_one(&local.pool)
        .await
        .unwrap();
        assert_eq!(vc, 7);
        assert_eq!(lv, "2026-09-05 10:00:00");

        // 重复导入幂等：密码按条计数但 updated_at 相同不覆盖（WHERE 不成立）、
        // 书签/会话按 url/(name,created_at) 去重跳过不计数、历史 UPSERT 计次
        let n2 = import_payload(&local, &raw).await.unwrap();
        assert_eq!(n2, 3, "密码 2（计数不覆盖）+ 历史 1（UPSERT）");
        let pc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM passwords")
            .fetch_one(&local.pool)
            .await
            .unwrap();
        let bc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM bookmarks")
            .fetch_one(&local.pool)
            .await
            .unwrap();
        let sc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&local.pool)
            .await
            .unwrap();
        let hc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM history")
            .fetch_one(&local.pool)
            .await
            .unwrap();
        assert_eq!((pc.0, bc.0, sc.0, hc.0), (2, 1, 1, 1));
    }

    #[tokio::test]
    async fn export_empty_db_produces_wellformed_empty_payload() {
        let (_dir, db) = temp_db().await;
        let raw = export_payload(&db).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["version"].as_u64(), Some(4));
        assert!(v["exported_at"].as_str().is_some());
        // 四类集合必须是空数组而非缺字段/null——导入侧按 as_array 判定
        for key in ["entries", "bookmarks", "sessions", "history"] {
            assert_eq!(
                v[key].as_array().map(|a| a.len()),
                Some(0),
                "{key} 应为空数组"
            );
        }
        // 空包导入空库：0 处理、零行
        let (_dir2, db2) = temp_db().await;
        assert_eq!(import_payload(&db2, &raw).await.unwrap(), 0);
        let c: (i64,) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM passwords)
                + (SELECT COUNT(*) FROM bookmarks)
                + (SELECT COUNT(*) FROM sessions)
                + (SELECT COUNT(*) FROM history)",
        )
        .fetch_one(&db2.pool)
        .await
        .unwrap();
        assert_eq!(c.0, 0);
    }

    #[tokio::test]
    async fn import_rejects_malformed_payloads() {
        let (_dir, db) = temp_db().await;
        // 非 JSON / 截断负载：解析失败即整包拒绝
        for bad in [b"\xff\xfe\xfdgarbage".to_vec(), b"[1, 2".to_vec()] {
            let e = import_payload(&db, &bad).await.unwrap_err();
            assert!(
                e.to_string().contains("同步包解析失败"),
                "截断/非法字节应报解析失败，实际: {e}"
            );
        }
        // JSON 但缺 entries：格式非法
        let missing = serde_json::to_vec(&serde_json::json!({"version": 4})).unwrap();
        let e = import_payload(&db, &missing).await.unwrap_err();
        assert!(e.to_string().contains("缺少 entries"));
        // 拒绝后库内零写入、无半包状态
        let c: (i64,) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM passwords)
                + (SELECT COUNT(*) FROM bookmarks)
                + (SELECT COUNT(*) FROM sessions)
                + (SELECT COUNT(*) FROM history)",
        )
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(c.0, 0);
        // 恢复能力：同一库随后导入合法包不受前次失败影响
        let ok = serde_json::to_vec(&serde_json::json!({
            "version": 4,
            "entries": [{"domain": "z.com", "password": "e", "updated_at": "2026-09-22 00:00:00"}]
        }))
        .unwrap();
        assert_eq!(import_payload(&db, &ok).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn import_skips_entries_missing_required_fields() {
        let (_dir, db) = temp_db().await;
        let payload = serde_json::json!({
            "version": 4,
            "entries": [
                {"password": "no-domain"},
                {"domain": "", "password": "empty-domain"},
                {"domain": "ok.com", "username": "u", "password": "enc"}
            ],
            "sessions": [
                {"name": "", "data": "[]", "created_at": "2026-09-22 00:00:00"},
                {"name": "s2", "data": "", "created_at": "2026-09-22 01:00:00"}
            ],
            "history": [
                {"title": "no-url"},
                {"url": "https://ok.com", "visit_count": 2}
            ]
        });
        let plain = serde_json::to_vec(&payload).unwrap();
        assert_eq!(
            import_payload(&db, &plain).await.unwrap(),
            2,
            "仅 ok.com 密码 + ok.com 历史"
        );
        let pc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM passwords")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let sc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        let hc: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM history")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!((pc.0, sc.0, hc.0), (1, 0, 1));
        // 空字段条目重复导入仍被跳过，无脏数据累积
        assert_eq!(
            import_payload(&db, &plain).await.unwrap(),
            2,
            "密码同 updated_at 跳过 + 历史 UPSERT 计次"
        );
        assert_eq!(
            sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM passwords")
                .fetch_one(&db.pool)
                .await
                .unwrap()
                .0,
            1
        );
    }
}
