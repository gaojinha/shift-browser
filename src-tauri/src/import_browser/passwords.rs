//! 浏览器密码导入（F-92）：源 WAL 快照逐行读 → v10/v11 就地解密 → 复用
//! save_password 链路落库。安全铁律：明文零落盘（只驻内存、用完 drop）、
//! 解密失败/空密码逐条如实计数回执、keyring 不可用整体降级不许猜。
//! F-98 起改用 **insert-only** 落库：同 (domain, username) 已存在则跳过不覆盖。

use serde::Serialize;
use sqlx::Connection;
use sqlx::Row;

use crate::db::passwords::save_password_new_only_inner;
use crate::error::CommandResult;

use super::{BRAVE_PASSWORD_FILES, BRAVE_ROOT, CHROME_PASSWORD_FILES, CHROME_ROOT};

#[derive(Debug, Serialize)]
pub struct ImportPasswordsResult {
    /// 实际新写入条数（F-98 insert-only：同 (domain, username) 已存在不计入）
    pub imported: usize,
    /// 同 (domain, username) 已存在、按 insert-only 跳过条数（F-98）
    pub skipped_existing: usize,
    /// 解密成功但空密码被拒收条数（save_password_inner 守卫口径）
    pub skipped_empty: usize,
    /// 读取/解密/写库失败条数（失败明细进日志，回执只计数）
    pub failed: usize,
    /// 读取失败被跳过的源文件（"profile/文件名"，如 SQLite 打不开）
    pub skipped_sources: Vec<String>,
    /// 实际参与导入的源文件（"profile/文件名"）
    pub used_sources: Vec<String>,
    /// 整体降级原因：Some = keyring 不可用，v11 行解不了（v10 行仍可导）
    pub degraded_reason: Option<String>,
}

/// 从本机浏览器导入密码（Chrome + Brave 一次跑完）
#[tauri::command]
pub async fn import_passwords_from_browser(
    db: tauri::State<'_, crate::db::Db>,
) -> CommandResult<ImportPasswordsResult> {
    import_passwords_inner(&db).await
}

/// 导入内核（与命令同体；命令层只把 `State` 拆成 `&Db` 后转调本函数，
/// 照 `db::passwords` 的 `*_inner` 约定）。F-100 live 测试直接调本函数。
pub async fn import_passwords_inner(db: &crate::db::Db) -> CommandResult<ImportPasswordsResult> {
    let Some(base) = super::config_root() else {
        // 无 HOME：零导入（与扫描口径一致，不报错）
        return Ok(ImportPasswordsResult {
            imported: 0,
            skipped_existing: 0,
            skipped_empty: 0,
            failed: 0,
            skipped_sources: Vec::new(),
            used_sources: Vec::new(),
            degraded_reason: None,
        });
    };
    let mut result = ImportPasswordsResult {
        imported: 0,
        skipped_existing: 0,
        skipped_empty: 0,
        failed: 0,
        skipped_sources: Vec::new(),
        used_sources: Vec::new(),
        degraded_reason: None,
    };

    // 每浏览器探一次 keyring（v11 钥匙）；不可用 → 该浏览器 v11 行降级，
    // v10 行（peanuts 派生）不受影响，回执 degraded_reason 如实标注
    let mut keys: std::collections::HashMap<&'static str, Option<Vec<u8>>> =
        std::collections::HashMap::new();
    for b in ["chrome", "brave"] {
        let k = super::fetch_v11_key(b).await;
        if k.is_err() {
            result.degraded_reason = Some(format!(
                "{b}: keyring 不可用（锁定或无条目），v11 密文无法解密"
            ));
        }
        keys.insert(b, k.ok());
    }

    for (root, browser, files) in [
        (CHROME_ROOT, "chrome", CHROME_PASSWORD_FILES),
        (BRAVE_ROOT, "brave", BRAVE_PASSWORD_FILES),
    ] {
        // 该浏览器 v11 钥匙（None = keyring 不可用，v11 行逐条降级）
        let browser_key: Option<&[u8]> = keys.get(browser).and_then(|k| k.as_deref());
        for prof_dir in super::profile_dirs(&base, root, files) {
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
                // WAL 快照逐行读（复用 F-89 快照原语，源零改动）
                let Some(rows) = snapshot_login_rows(&path).await else {
                    result.skipped_sources.push(label);
                    continue;
                };
                result.used_sources.push(label);
                for (origin, username, blob) in rows {
                    // domain 口径：logins.origin_url 整串（与 P0 验证脚本一致，
                    // 归一化留给展示层；host 提取失败则退整串，不丢数据）
                    let domain = host_of(&origin).unwrap_or(origin.clone());
                    if domain.trim().is_empty() {
                        result.failed += 1;
                        continue;
                    }
                    // 空 blob（长度 0，Chrome 账号库空行实测存在）：无值可导，
                    // 与"解密后空密码"同口径计 skipped_empty，不算 failed
                    if blob.is_empty() {
                        result.skipped_empty += 1;
                        continue;
                    }
                    // 解密：v10 peanuts / v11 keyring 原文派生（F-90 原语）
                    let plain = match decrypt_login_blob(&blob, browser_key) {
                        Ok(p) => p,
                        Err(super::DecryptError::NoKeyringKey) => {
                            // 该浏览器 v11 不可解：逐条计入 failed，degraded_reason 已标注
                            result.failed += 1;
                            continue;
                        }
                        Err(_) => {
                            result.failed += 1;
                            continue;
                        }
                    };
                    // 空密码拒收（save_password_inner 守卫同口径，先计
                    // skipped_empty 而非 failed：解密成功但无值可导）
                    if plain.trim().is_empty() {
                        result.skipped_empty += 1;
                        continue;
                    }
                    match save_password_new_only_inner(db, &domain, username.clone(), &plain, None)
                        .await
                    {
                        Ok(true) => result.imported += 1,
                        // F-98：已存在同 (domain, username)，按跳过语义不覆盖
                        Ok(false) => result.skipped_existing += 1,
                        Err(e) => {
                            // 失败明细进日志（不含明文），回执只计数
                            log::warn!("[import_browser] 密码写入失败 domain={domain}: {e}");
                            result.failed += 1;
                        }
                    }
                    // plain 到此生命周期结束，drop 即灭（zeroize 见 decrypt 原语侧）
                }
            }
        }
    }

    log::info!(
        "[import_browser] 密码导入完成：新写 {} / 已存在跳过 {} / 空密码跳过 {} / 失败 {}（源 {}，跳过源 {}）",
        result.imported,
        result.skipped_existing,
        result.skipped_empty,
        result.failed,
        result.used_sources.len(),
        result.skipped_sources.len()
    );
    Ok(result)
}

/// 从 logins.origin_url 提 host（"https://sub.example.com/path?x" →
/// "sub.example.com"）。非 http(s) origin（chrome:// 等不该出现在 logins，
/// 防御性拒收）与空 host 返回 None（调用方计 failed），不产出垃圾 domain。
fn host_of(origin: &str) -> Option<String> {
    let lower = origin.trim().to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    let rest = origin.split_once("://").map(|(_, r)| r).unwrap_or(origin);
    let authority = rest.split(['/', '?', '#']).next()?;
    // 剥 userinfo（u:p@h:port → h:port，对齐 F-68 host_of 修复口径）
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    // 端口剥不剥：保留（与 P0 验证脚本展示一致，唯一性靠 (domain, username) UPSERT）
    if host_port.is_empty() {
        None
    } else {
        Some(host_port.to_string())
    }
}

/// 解一条 login blob：v10 peanuts / v11 keyring 原文（F-90 原语透传）
fn decrypt_login_blob(blob: &[u8], v11_key: Option<&[u8]>) -> Result<String, super::DecryptError> {
    super::decrypt_os_crypt(blob, v11_key)
}

/// WAL 快照读 Login Data 副本的 logins 表：返回 (origin_url, username_value,
/// password_value blob) 列表。快照铁律同 F-89（0700 临时目录、db+wal+shm、
/// 读完即删、源零改动）。
async fn snapshot_login_rows(
    db_path: &std::path::Path,
) -> Option<Vec<(String, Option<String>, Vec<u8>)>> {
    let tmp = tempfile::Builder::new()
        .prefix("shift-import-pw-")
        .tempdir()
        .ok()?;
    #[cfg(unix)]
    {
        crate::harden::restrict_mode(tmp.path(), 0o700);
    }
    let base_name = db_path.file_name()?.to_string_lossy().into_owned();
    let dst = tmp.path().join(&base_name);
    std::fs::copy(db_path, &dst).ok()?;
    for ext in ["-wal", "-shm"] {
        let src_side = std::path::PathBuf::from(format!("{}{}", db_path.display(), ext));
        if src_side.is_file() {
            let _ = std::fs::copy(&src_side, tmp.path().join(format!("{base_name}{ext}")));
        }
    }
    let mut conn = sqlx::sqlite::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&dst),
    )
    .await
    .ok()?;
    let rows = sqlx::query("SELECT origin_url, username_value, password_value FROM logins")
        .fetch_all(&mut conn)
        .await
        .ok()?;
    Some(
        rows.into_iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<Option<String>, _>(1).unwrap_or(None),
                    r.try_get::<Vec<u8>, _>(2).unwrap_or_default(),
                )
            })
            .collect(),
    )
}

/// 预览：逐条 (browser, profile, domain, username)，**不取 password_value、不解密**。
/// 与导入共用源发现逻辑；返回（预览条目, 读取失败的源）。
pub(super) async fn collect_password_previews(
    base: &std::path::Path,
) -> (Vec<super::PasswordPreviewItem>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for (root, browser, files) in [
        (CHROME_ROOT, "chrome", CHROME_PASSWORD_FILES),
        (BRAVE_ROOT, "brave", BRAVE_PASSWORD_FILES),
    ] {
        for prof_dir in super::profile_dirs(base, root, files) {
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
                let Some(rows) = snapshot_login_meta(&path).await else {
                    skipped.push(format!("{prof_name}/{file}"));
                    continue;
                };
                for (origin, username) in rows {
                    let domain = host_of(&origin).unwrap_or(origin.clone());
                    if domain.trim().is_empty() {
                        continue;
                    }
                    out.push(super::PasswordPreviewItem {
                        browser: browser.to_string(),
                        profile: prof_name.clone(),
                        domain,
                        username,
                    });
                }
            }
        }
    }
    (out, skipped)
}

/// WAL 快照读 logins 的 (origin_url, username_value)——**不 SELECT password_value**，
/// 预览专用（快照铁律同 [`snapshot_login_rows`]：0700 临时目录、db+wal+shm、读完即删）。
async fn snapshot_login_meta(db_path: &std::path::Path) -> Option<Vec<(String, Option<String>)>> {
    let tmp = tempfile::Builder::new()
        .prefix("shift-import-pwpreview-")
        .tempdir()
        .ok()?;
    #[cfg(unix)]
    {
        crate::harden::restrict_mode(tmp.path(), 0o700);
    }
    let base_name = db_path.file_name()?.to_string_lossy().into_owned();
    let dst = tmp.path().join(&base_name);
    std::fs::copy(db_path, &dst).ok()?;
    for ext in ["-wal", "-shm"] {
        let src_side = std::path::PathBuf::from(format!("{}{}", db_path.display(), ext));
        if src_side.is_file() {
            let _ = std::fs::copy(&src_side, tmp.path().join(format!("{base_name}{ext}")));
        }
    }
    let mut conn = sqlx::sqlite::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&dst),
    )
    .await
    .ok()?;
    let rows = sqlx::query("SELECT origin_url, username_value FROM logins")
        .fetch_all(&mut conn)
        .await
        .ok()?;
    Some(
        rows.into_iter()
            .map(|r| {
                (
                    r.try_get::<String, _>(0).unwrap_or_default(),
                    r.try_get::<Option<String>, _>(1).unwrap_or(None),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod f92_tests {
    use super::*;

    // ---- host_of：origin_url → host（对齐 F-68 host_of 口径） ----

    #[test]
    fn host_of_extracts_host_and_strips_userinfo() {
        assert_eq!(
            host_of("https://example.com/login").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            host_of("http://sub.example.com:8443/x?y=1").as_deref(),
            Some("sub.example.com:8443")
        );
        assert_eq!(
            host_of("https://u:p@host.example.com/").as_deref(),
            Some("host.example.com"),
            "userinfo 剥离"
        );
        // 非 http(s) origin 防御性拒收（logins 里不该有，防垃圾 domain）
        assert_eq!(host_of("chrome://settings"), None);
        assert_eq!(host_of("ftp://x.com"), None);
        assert_eq!(host_of(""), None);
    }

    // ---- decrypt_login_blob：透传 F-90 原语（黄金向量语义在此再锁一层） ----

    const GOLDEN_V10_CT_HEX: &str =
        "8a2f0711b2b584b62bd4f134bd2d425636de3c42f4b17b617ba3fa739ad827a7";
    const GOLDEN_V11_SECRET: &str = "q7b7aG9hbm9yaXNlaGVpZGVp";
    const GOLDEN_V11_CT_HEX: &str = "035436fe29e7cbbcf01298db6336bf4f";

    fn hex(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    #[test]
    fn v10_blob_decrypts_without_keyring() {
        let blob = [b"v10".as_slice(), &hex(GOLDEN_V10_CT_HEX)].concat();
        assert_eq!(
            decrypt_login_blob(&blob, None).unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn v11_blob_needs_keyring_key() {
        let blob = [b"v11".as_slice(), &hex(GOLDEN_V11_CT_HEX)].concat();
        assert_eq!(
            decrypt_login_blob(&blob, None),
            Err(super::super::DecryptError::NoKeyringKey),
            "v11 无 keyring key → 降级 Err（不猜）"
        );
        assert_eq!(
            decrypt_login_blob(&blob, Some(GOLDEN_V11_SECRET.as_bytes())).unwrap(),
            "p@ssw0rd-é中"
        );
    }
}
