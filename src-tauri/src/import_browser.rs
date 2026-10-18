//! 从本机浏览器导入（只读源 + 解密 + 落库）
//! 安全：源库只读（只 copy 不打开）；明文零落盘；keyring label 硬映射不猜
//!
//! F-89 范围：只读扫描 `scan_browser_import`——Chrome / Brave 全部 profile ×
//! 逐源文件返回 {源文件, 条数, 是否加密}，**返回结构不含任何明文字段**。
//! v11/v10 解密原语（F-90）与书签/密码导入命令（F-91/F-92）在后续卡中追加到本文件。
//!
//! 三条铁律（v1 翻车逐条防死，v3 方案 §5.3）：
//! 1. SQLite 源必须把 `<db>` + `-wal` + `-shm` 一起拷进 0700 临时目录再读副本——
//!    只读主库会漏掉未 checkpoint 的 WAL 页（v1 实测 69 条的库只读出 0）；
//! 2. 排除无数据/测试残留 profile 目录（brave-anon / google-chrome-headless / ms-playwright-mcp）；
//! 3. 临时目录 0700、读完即删（TempDir drop 兜底，早退也不留）。

use std::path::{Path, PathBuf};

use serde::Serialize;
use sqlx::Connection;

use crate::error::CommandResult;

/// 排除的 profile 目录名（无数据/测试残留，方案 §3.2）
const EXCLUDED_PROFILE_DIRS: &[&str] = &[
    "brave-anon",
    "google-chrome-headless",
    "ms-playwright-mcp",
];

/// 浏览器根目录（相对 `~/.config`）
const CHROME_ROOT: &str = "google-chrome";
const BRAVE_ROOT: &str = "BraveSoftware/Brave-Browser";

/// Chrome 书签源（v3 §3.2 优先级序：Account 明文最新 → bak → 空壳 Bookmarks → v11 密文兜底）
const CHROME_BOOKMARK_FILES: &[&str] = &[
    "AccountBookmarks",
    "Bookmarks.bak",
    "Bookmarks",
    "EncryptedAccountBookmarks2",
    "EncryptedBookmarks2",
];
/// Chrome 密码源（SQLite，`logins` 表；Account 版优先）
const CHROME_PASSWORD_FILES: &[&str] = &["Login Data For Account", "Login Data"];
/// Brave 书签源
const BRAVE_BOOKMARK_FILES: &[&str] = &["Bookmarks"];
/// Brave 密码源
const BRAVE_PASSWORD_FILES: &[&str] = &["Login Data"];

/// 单个源文件的扫描报告（全是计数与标记，严禁出现任何明文字段）
#[derive(Debug, Serialize)]
pub struct SourceReport {
    /// "chrome" | "brave"
    pub browser: &'static str,
    /// profile 目录名（如 "Default"）
    pub profile: String,
    /// "bookmarks" | "passwords"
    pub kind: &'static str,
    /// 源文件名（profile 内）
    pub file: String,
    /// 条数：Some(0)=空壳（空壳文件也必须扫到并标记，如实测 Bookmarks=0）；
    /// None=读取/解析失败，或加密源（解密后才有条数，F-90 接线）
    pub count: Option<usize>,
    /// 源文件本体是否加密（Encrypted*2 是 v11 密文，需 F-90 原语）
    pub encrypted: bool,
    /// 加密源可否解密：明文源不适用（None）；加密源待 F-90 keyring 探测接线（本卡恒 None）
    pub decryptable: Option<bool>,
}

/// 扫描本机 Chrome / Brave 全部 profile 的数据源。
/// 只读：源文件零改动（SQLite 只 copy 不打开，验收做 mtime/hash 校验）。
#[tauri::command]
pub async fn scan_browser_import() -> CommandResult<Vec<SourceReport>> {
    let Some(base) = config_root() else {
        // 无 HOME（非常规环境）：空结果而非报错，前端按"未发现浏览器"展示
        return Ok(Vec::new());
    };
    Ok(scan_from(&base).await)
}

/// `~/.config`（本期仅 Linux；HOME 缺失返回 None → 空结果）
fn config_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

async fn scan_from(base: &Path) -> Vec<SourceReport> {
    let mut out = Vec::new();
    scan_browser(
        base,
        CHROME_ROOT,
        "chrome",
        CHROME_BOOKMARK_FILES,
        CHROME_PASSWORD_FILES,
        &mut out,
    )
    .await;
    scan_browser(
        base,
        BRAVE_ROOT,
        "brave",
        BRAVE_BOOKMARK_FILES,
        BRAVE_PASSWORD_FILES,
        &mut out,
    )
    .await;
    out
}

/// 扫一个浏览器的全部 profile：含目标源文件的子目录才算 profile
///（自然滤除 Brave 下的 32 位扩展缓存目录）；排除名单精确匹配目录名。
async fn scan_browser(
    base: &Path,
    rel_root: &str,
    browser: &'static str,
    bookmark_files: &[&str],
    password_files: &[&str],
    out: &mut Vec<SourceReport>,
) {
    let root = base.join(rel_root);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let mut profiles: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| !EXCLUDED_PROFILE_DIRS.contains(&n))
        })
        .filter(|p| {
            bookmark_files
                .iter()
                .chain(password_files)
                .any(|f| p.join(f).is_file())
        })
        .collect();
    profiles.sort();
    for prof in profiles {
        let name = prof
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        for f in bookmark_files {
            let p = prof.join(f);
            if p.is_file() {
                out.push(inspect_bookmark_file(&p, browser, &name));
            }
        }
        for f in password_files {
            let p = prof.join(f);
            if p.is_file() {
                out.push(inspect_password_file(&p, browser, &name).await);
            }
        }
    }
}

/// 书签源：明文 JSON 递归数 `type == "url"` 节点；`Encrypted*` 是 v11 密文，
/// 条数与可解密性待 F-90 原语接线后填充（本卡 count=None / decryptable=None）。
fn inspect_bookmark_file(path: &Path, browser: &'static str, profile: &str) -> SourceReport {
    let file = file_name_of(path);
    let encrypted = file.starts_with("Encrypted");
    let count = if encrypted {
        None
    } else {
        count_bookmark_json(path)
    };
    SourceReport {
        browser,
        profile: profile.to_string(),
        kind: "bookmarks",
        file,
        count,
        encrypted,
        decryptable: None,
    }
}

/// 密码源：SQLite `logins` 表行数（WAL 快照，见 [`count_logins_snapshot`]）。
/// 文件级 `encrypted=false`（库本体不加密，行内 blob 才是 v10/v11——F-90 处理）。
async fn inspect_password_file(path: &Path, browser: &'static str, profile: &str) -> SourceReport {
    SourceReport {
        browser,
        profile: profile.to_string(),
        kind: "passwords",
        file: file_name_of(path),
        count: count_logins_snapshot(path).await,
        encrypted: false,
        decryptable: None,
    }
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// 明文书签 JSON 计数：失败（读不到/非法 JSON）→ None；合法但无 url 节点 → Some(0)（空壳）
fn count_bookmark_json(path: &Path) -> Option<usize> {
    let data = std::fs::read(path).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&data).ok()?;
    Some(count_url_nodes(&v))
}

fn count_url_nodes(v: &serde_json::Value) -> usize {
    match v {
        serde_json::Value::Object(m) => {
            let self_count = usize::from(m.get("type").and_then(serde_json::Value::as_str) == Some("url"));
            m.values().map(count_url_nodes).sum::<usize>() + self_count
        }
        serde_json::Value::Array(a) => a.iter().map(count_url_nodes).sum(),
        _ => 0,
    }
}

/// SQLite 源 `logins` 表行数。铁律 ①③：`<db>`+`-wal`+`-shm` 一起拷进 0700 一次性
/// 临时目录再读**副本**（绝不打开源文件，源零改动）；TempDir drop 即删。
/// 表不存在/副本打不开 → None（读取失败）；表在但 0 行 → Some(0)（空壳标记）。
async fn count_logins_snapshot(db_path: &Path) -> Option<usize> {
    let tmp = tempfile::Builder::new()
        .prefix("shift-import-scan-")
        .tempdir()
        .ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700));
    }
    let base_name = db_path.file_name()?.to_string_lossy().into_owned();
    let dst = tmp.path().join(&base_name);
    std::fs::copy(db_path, &dst).ok()?;
    for ext in ["-wal", "-shm"] {
        let src_side = PathBuf::from(format!("{}{}", db_path.display(), ext));
        if src_side.is_file() {
            let _ = std::fs::copy(&src_side, tmp.path().join(format!("{base_name}{ext}")));
        }
    }
    let mut conn = sqlx::sqlite::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&dst),
    )
    .await
    .ok()?;
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM logins")
        .fetch_one(&mut conn)
        .await
        .ok()?;
    Some(usize::try_from(n.max(0)).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 造数：Chrome 书签 JSON ----

    fn url_node(url: &str) -> serde_json::Value {
        serde_json::json!({ "type": "url", "url": url, "name": "t" })
    }

    fn folder_node(name: &str, children: Vec<serde_json::Value>) -> serde_json::Value {
        serde_json::json!({ "type": "folder", "name": name, "children": children })
    }

    /// 合法书签 JSON（n 个 url，含嵌套文件夹）
    fn bookmarks_json(urls: &[&str]) -> String {
        let (a, b) = urls.split_at(urls.len() / 2);
        serde_json::json!({
            "checksum": "c00193",
            "roots": {
                "bookmark_bar": folder_node("bar", a.iter().map(|u| url_node(u)).collect()),
                "other": folder_node("other", vec![folder_node(
                    "nested",
                    b.iter().map(|u| url_node(u)).collect(),
                )]),
                "synced": folder_node("synced", vec![]),
            }
        })
        .to_string()
    }

    /// 在 base 下造一个浏览器 profile，写入若干源文件
    fn make_profile(base: &Path, browser_root: &str, profile: &str, files: &[(&str, Vec<u8>)]) {
        let dir = base.join(browser_root).join(profile);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, data) in files {
            std::fs::write(dir.join(name), data).unwrap();
        }
    }

    fn reports_for<'a>(out: &'a [SourceReport], profile: &str) -> Vec<&'a SourceReport> {
        out.iter().filter(|r| r.profile == profile).collect()
    }

    // ---- 排除目录不入结果（铁律 ②） ----

    #[tokio::test]
    async fn excluded_profile_dirs_never_scanned() {
        let base = tempfile::tempdir().unwrap();
        make_profile(
            base.path(),
            CHROME_ROOT,
            "Default",
            &[("AccountBookmarks", bookmarks_json(&["https://a.com", "https://b.com"]).into_bytes())],
        );
        // 测试残留目录：即使有数据也不入结果
        make_profile(
            base.path(),
            CHROME_ROOT,
            "ms-playwright-mcp",
            &[("Bookmarks", bookmarks_json(&["https://x.com"]).into_bytes())],
        );
        let out = scan_from(base.path()).await;
        assert!(out.iter().all(|r| r.profile == "Default"), "排除目录不得入结果: {out:?}");
        let default = reports_for(&out, "Default");
        assert_eq!(default.len(), 1);
        assert_eq!(default[0].file, "AccountBookmarks");
        assert_eq!(default[0].count, Some(2));
    }

    // ---- 空壳源标记（实测 Bookmarks=0 空壳必须扫到） ----

    #[tokio::test]
    async fn empty_shell_marked_as_zero() {
        let base = tempfile::tempdir().unwrap();
        // 空壳：合法 JSON 但 0 个 url 节点（对齐实测 1005 字节 Bookmarks）
        let shell = serde_json::json!({
            "checksum": "x",
            "roots": {
                "bookmark_bar": folder_node("bar", vec![]),
                "other": folder_node("other", vec![]),
                "synced": folder_node("synced", vec![]),
            }
        })
        .to_string();
        make_profile(
            base.path(),
            CHROME_ROOT,
            "Default",
            &[("Bookmarks", shell.into_bytes())],
        );
        let out = scan_from(base.path()).await;
        let r = reports_for(&out, "Default");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].count, Some(0), "空壳必须扫到并标记为 0，不得静默丢失");
        assert!(!r[0].encrypted);
    }

    // ---- 多源优先级顺序 + 加密源标记 ----

    #[tokio::test]
    async fn multi_source_priority_order_and_encrypted_flag() {
        let base = tempfile::tempdir().unwrap();
        make_profile(
            base.path(),
            CHROME_ROOT,
            "Default",
            &[
                ("AccountBookmarks", bookmarks_json(&["https://a.com", "https://b.com", "https://c.com"]).into_bytes()),
                ("Bookmarks.bak", bookmarks_json(&["https://old.com"]).into_bytes()),
                ("Bookmarks", serde_json::json!({"roots": {"bookmark_bar": folder_node("bar", vec![])}}).to_string().into_bytes()),
                ("EncryptedAccountBookmarks2", b"v11\xb7\xfb\xf6g\x74\xab".to_vec()),
                ("EncryptedBookmarks2", b"v10\xb7\xfb\xf6g\x74\xab".to_vec()),
            ],
        );
        let out = scan_from(base.path()).await;
        let r = reports_for(&out, "Default");
        let files: Vec<&str> = r.iter().map(|x| x.file.as_str()).collect();
        assert_eq!(
            files,
            ["AccountBookmarks", "Bookmarks.bak", "Bookmarks", "EncryptedAccountBookmarks2", "EncryptedBookmarks2"],
            "报告必须按 v3 §3.2 优先级序输出"
        );
        let counts: Vec<Option<usize>> = r.iter().map(|x| x.count).collect();
        assert_eq!(counts, [Some(3), Some(1), Some(0), None, None]);
        let encrypted: Vec<bool> = r.iter().map(|x| x.encrypted).collect();
        assert_eq!(encrypted, [false, false, false, true, true]);
    }

    // ---- 非法 JSON → None（读取失败），不影响其余源 ----

    #[tokio::test]
    async fn malformed_bookmark_json_counts_none() {
        let base = tempfile::tempdir().unwrap();
        make_profile(
            base.path(),
            BRAVE_ROOT,
            "Default",
            &[
                ("Bookmarks", b"not json{{{".to_vec()),
                ("Login Data", Vec::new()), // 0 字节文件：非 SQLite → None
            ],
        );
        let out = scan_from(base.path()).await;
        let r = reports_for(&out, "Default");
        assert_eq!(r.len(), 2);
        assert_eq!(r.iter().find(|x| x.kind == "bookmarks").unwrap().count, None);
        assert_eq!(r.iter().find(|x| x.kind == "passwords").unwrap().count, None);
    }

    // ---- WAL 快照（铁律 ①，v1 翻车回归）+ 源文件零改动 ----

    #[tokio::test]
    async fn wal_snapshot_reads_what_main_only_misses_and_never_touches_source() {
        use sqlx::Connection;

        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("Login Data For Account");

        // 写库：WAL 模式 + 关自动 checkpoint → 最新写入只留在 -wal（模拟运行中的浏览器）
        // create_if_missing：夹具首连时文件尚不存在（生产路径拷的是已存在副本，无需此参）
        let mut conn = sqlx::sqlite::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&db)
                .create_if_missing(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
        .await
        .unwrap();
        sqlx::query("PRAGMA wal_autocheckpoint=0")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE logins (id INTEGER PRIMARY KEY, origin_url TEXT)")
            .execute(&mut conn)
            .await
            .unwrap();
        for i in 0..3 {
            sqlx::query("INSERT INTO logins (origin_url) VALUES (?)")
                .bind(format!("https://x{i}.com"))
                .execute(&mut conn)
                .await
                .unwrap();
        }
        // 连接保持打开（不 checkpoint），此刻 -wal 持有全部数据页

        let main_before = std::fs::read(&db).unwrap();
        let wal_path = format!("{}-wal", db.display());
        let wal_before = std::fs::read(&wal_path).ok();

        // v1 翻车复现：只拷主库 → schema/数据都在 -wal 里，读出 0
        let tmp2 = tempfile::tempdir().unwrap();
        let lone = tmp2.path().join("lone.db");
        std::fs::copy(&db, &lone).unwrap();
        let mut lone_conn = sqlx::sqlite::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(&lone),
        )
        .await
        .unwrap();
        let main_only = sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM logins")
            .fetch_one(&mut lone_conn)
            .await
            .map(|(n,)| n as usize)
            .unwrap_or(0);
        assert_eq!(main_only, 0, "v1 翻车根因必须可复现：未 checkpoint 的 WAL 页只读主库不可见");

        // 铁律：db + -wal + -shm 一起快照 → 读全 3 行
        assert_eq!(count_logins_snapshot(&db).await, Some(3));

        // 源文件零改动（验收红线：mtime/hash 不变——内容字节级不变）
        assert_eq!(main_before, std::fs::read(&db).unwrap(), "主库源文件被改动");
        assert_eq!(wal_before, std::fs::read(&wal_path).ok(), "-wal 源文件被改动");
    }
}
