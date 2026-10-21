// LINES-EXEMPT: 浏览器导入门面：扫描 / 解密 / 落库三段共用中间结构与 keyring 映射表，拆开需大量 pub(crate) 暴露
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

pub(crate) mod bookmarks;
pub(crate) mod passwords;

/// profile 子目录列表（含任一目标源文件才算 profile；排除名单同 F-89 扫描）。
/// 书签（F-91）与密码（F-92）导入共用。
pub(crate) fn profile_dirs(
    base: &std::path::Path,
    rel_root: &str,
    files: &[&str],
) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(base.join(rel_root)) else {
        return Vec::new();
    };
    let mut dirs: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| !EXCLUDED_PROFILE_DIRS.contains(&n))
        })
        .filter(|p| files.iter().any(|f| p.join(f).is_file()))
        .collect();
    dirs.sort();
    dirs
}
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
    Ok(scan_browser_import_inner().await)
}

/// 扫描内核（与命令同体；命令层只做签名包装，照 `db::passwords` 的 `*_inner` 约定）。
/// F-100 真机 live 测试（`tests/import_browser_live.rs`）直接调本函数，不重抄逻辑。
pub async fn scan_browser_import_inner() -> Vec<SourceReport> {
    let Some(base) = config_root() else {
        // 无 HOME（非常规环境）：空结果而非报错，前端按"未发现浏览器"展示
        return Vec::new();
    };
    // F-90 已接线：加密书签源就地解密计数、密码源补 decryptable（keyring
    // 不可用时逐源降级，不影响扫描本身）。probe_keys=true 生产路径。
    scan_from(&base, true).await
}

/// 导入预览（F-99）：逐行站点/用户名 + 书签按文件夹分组。
/// **结构体严禁任何密码/明文相关字段**——预览只到 domain / username 层。
#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub passwords: Vec<PasswordPreviewItem>,
    pub bookmark_groups: Vec<BookmarkPreviewGroup>,
    /// 读取/解密失败被跳过的源（"profile/文件"）
    pub skipped_sources: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PasswordPreviewItem {
    /// "chrome" | "brave"
    pub browser: String,
    pub profile: String,
    /// 站点 host（logins.origin_url 提取；提取失败退整串）
    pub domain: String,
    pub username: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BookmarkPreviewGroup {
    /// 文件夹名；书签栏散书签归 "书签栏"
    pub folder: String,
    pub items: Vec<BookmarkPreviewItem>,
}

#[derive(Debug, Serialize)]
pub struct BookmarkPreviewItem {
    pub title: String,
    pub url: String,
}

/// 导入前预览（只读、无副作用、不动源库、不解密密码值）：
/// 供前端在导入按钮旁展示"将要导入什么"。
#[tauri::command]
pub async fn preview_browser_import() -> CommandResult<ImportPreview> {
    Ok(preview_browser_import_inner().await)
}

/// 预览内核（与命令同体；命令层只做签名包装）。F-100 live 测试直接调本函数。
pub async fn preview_browser_import_inner() -> ImportPreview {
    let Some(base) = config_root() else {
        return ImportPreview {
            passwords: Vec::new(),
            bookmark_groups: Vec::new(),
            skipped_sources: Vec::new(),
        };
    };
    let (passwords, mut skipped_sources) = passwords::collect_password_previews(&base).await;
    let (bookmark_groups, bm_skipped) = bookmarks::collect_bookmark_previews(&base).await;
    skipped_sources.extend(bm_skipped);
    ImportPreview {
        passwords,
        bookmark_groups,
        skipped_sources,
    }
}

/// `~/.config`（本期仅 Linux；HOME 缺失返回 None → 空结果）
fn config_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

async fn scan_from(base: &Path, probe_keys: bool) -> Vec<SourceReport> {
    let mut out = Vec::new();
    scan_browser(
        base,
        CHROME_ROOT,
        "chrome",
        CHROME_BOOKMARK_FILES,
        CHROME_PASSWORD_FILES,
        probe_keys,
        &mut out,
    )
    .await;
    scan_browser(
        base,
        BRAVE_ROOT,
        "brave",
        BRAVE_BOOKMARK_FILES,
        BRAVE_PASSWORD_FILES,
        probe_keys,
        &mut out,
    )
    .await;
    out
}

/// 扫一个浏览器的全部 profile：含目标源文件的子目录才算 profile
///（自然滤除 Brave 下的 32 位扩展缓存目录）；排除名单精确匹配目录名。
/// `probe_keys=false`（单测路径）不碰 keyring/DBus：Encrypted* 恒 count=None /
/// decryptable=None，保证测试确定性。
#[allow(clippy::too_many_arguments)]
async fn scan_browser(
    base: &Path,
    rel_root: &str,
    browser: &'static str,
    bookmark_files: &[&str],
    password_files: &[&str],
    probe_keys: bool,
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
                out.push(inspect_bookmark_file(&p, browser, &name, probe_keys).await);
            }
        }
        for f in password_files {
            let p = prof.join(f);
            if p.is_file() {
                out.push(inspect_password_file(&p, browser, &name, probe_keys).await);
            }
        }
    }
}

/// 书签源：
/// - 明文 JSON：直接递归数 `type == "url"` 节点，decryptable 不适用（None）
/// - `Encrypted*`（v11 密文）：F-90 原语就地解密计数——keyring 可用则给条数
///   （decryptable=Some(true)）；keyring 锁着/不可达则降级（count=None、
///   decryptable=Some(false)），绝不猜 label、绝不弹解锁
async fn inspect_bookmark_file(
    path: &Path,
    browser: &'static str,
    profile: &str,
    probe_keys: bool,
) -> SourceReport {
    let file = file_name_of(path);
    if file.starts_with("Encrypted") {
        // 单测路径不碰 DBus（确定性）：没探测过 → decryptable=None，不冒充探测结果
        if !probe_keys {
            return SourceReport {
                browser,
                profile: profile.to_string(),
                kind: "bookmarks",
                file,
                count: None,
                encrypted: true,
                decryptable: None,
            };
        }
        // 生产路径：解密计数，keyring 失败降级 Some(false)
        match fetch_v11_key(browser).await {
            Ok(key) => {
                // key = keyring 条目值原文（口令），decrypt_os_crypt 内部做 PBKDF2
                let count = std::fs::read(path)
                    .ok()
                    .and_then(|blob| decrypt_os_crypt(&blob, Some(&key)).ok())
                    .and_then(|json| {
                        serde_json::from_str::<serde_json::Value>(&json)
                            .ok()
                            .map(|v| count_url_nodes(&v))
                    });
                SourceReport {
                    browser,
                    profile: profile.to_string(),
                    kind: "bookmarks",
                    file,
                    count,
                    encrypted: true,
                    decryptable: Some(true),
                }
            }
            Err(_) => SourceReport {
                browser,
                profile: profile.to_string(),
                kind: "bookmarks",
                file,
                count: None,
                encrypted: true,
                decryptable: Some(false),
            },
        }
    } else {
        SourceReport {
            browser,
            profile: profile.to_string(),
            kind: "bookmarks",
            file,
            count: count_bookmark_json(path),
            encrypted: false,
            decryptable: None,
        }
    }
}

/// 密码源：SQLite `logins` 表行数（WAL 快照，见 [`count_logins_snapshot`]）。
/// 文件级 `encrypted=false`（库本体不加密，行内 blob 才是 v10/v11）；
/// decryptable = 该浏览器 keyring 密钥可取（每文件探一次，命令频度低可接受）。
async fn inspect_password_file(
    path: &Path,
    browser: &'static str,
    profile: &str,
    probe_keys: bool,
) -> SourceReport {
    // 单测路径不碰 DBus；生产路径探一次 keyring 得 decryptable
    let decryptable = if probe_keys {
        fetch_v11_key(browser).await.is_ok()
    } else {
        false
    };
    SourceReport {
        browser,
        profile: profile.to_string(),
        kind: "passwords",
        file: file_name_of(path),
        count: count_logins_snapshot(path).await,
        encrypted: false,
        decryptable: Some(decryptable),
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
        crate::harden::restrict_mode(tmp.path(), 0o700);
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

// ===== F-90：Chromium os_crypt（Linux）v11/v10 解密原语 =====
// 算法照 Chromium os_crypt_linux.cc（P0 实测 83/83 全解）：
//   v11：key = PBKDF2-HMAC-SHA1(keyring 条目值本身（24 字符串，不解 base64）,
//         salt="saltysalt", iter=1, dklen=16)
//   v10：同算法，password 硬编码 "peanuts"（无 keyring 档）
//   密文去 3 字节前缀 → AES-128-CBC，IV=16×0x20 → PKCS7 去填充
// 判据铁律（.learnings/2026-10-18-brave-v11-password-decrypt.md）：
//   解密成败必须在 PKCS7 去填充【之后】判（UTF-8/填充合法性），不得拿原始块判——
//   否则正确密钥也会被判失败。填充非法一律 Err，绝不 panic。
// 安全：明文只在内存驻留、密钥与含填充副本用完即 zeroize；
//   错误信息不含任何明文/密钥；keyring label 硬映射不猜不试。

use zeroize::Zeroize;

#[derive(Debug, thiserror::Error, PartialEq)]
enum DecryptError {
    #[error("密文过短")]
    TooShort,
    #[error("未知前缀（非 v10/v11）")]
    UnknownPrefix,
    #[error("密文长度非块对齐")]
    BadLength,
    #[error("PKCS7 填充非法")]
    BadPadding,
    #[error("明文非 UTF-8")]
    NotUtf8,
    #[error("keyring 无可用密钥（该浏览器无法解密）")]
    NoKeyringKey,
}

/// v10（无 keyring 档）的硬编码口令（Chromium 源码常量）
const V10_PASSWORD: &[u8] = b"peanuts";
/// PBKDF2 盐（Chromium os_crypt_linux.cc 常量）
const PBKDF2_SALT: &[u8] = b"saltysalt";
/// AES-128-CBC IV：16 个空格（0x20）
const CBC_IV: [u8; 16] = [0x20; 16];

/// keyring label 硬映射：只认这两个精确 label，其余一律 None（不许猜、不许硬试）
fn keyring_label(browser: &str) -> Option<&'static str> {
    match browser {
        "chrome" => Some("Chrome Safe Storage"),
        "brave" => Some("Brave Safe Storage"),
        _ => None,
    }
}

/// PBKDF2-HMAC-SHA1(salt="saltysalt", iter=1, dklen=16) → 16 字节 AES-128 key
fn derive_key(password: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA1,
        std::num::NonZeroU32::MIN, // iter=1
        PBKDF2_SALT,
        password,
        &mut key,
    );
    key
}

/// AES-128-CBC 解密（IV=CBC_IV，倒序原地逐块：块 i 解完与前一块密文异或）
fn aes_cbc_decrypt(body: &[u8], key: &[u8; 16]) -> Result<Vec<u8>, DecryptError> {
    use aes::cipher::{BlockDecrypt, KeyInit};
    if body.is_empty() || body.len() % 16 != 0 {
        return Err(DecryptError::BadLength);
    }
    let cipher = aes::Aes128::new_from_slice(key).map_err(|_| DecryptError::BadLength)?;
    let mut buf = body.to_vec();
    let n = buf.len() / 16;
    let mut prev_ct = [0u8; 16];
    let mut block = [0u8; 16];
    for i in (1..n).rev() {
        prev_ct.copy_from_slice(&buf[(i - 1) * 16..i * 16]);
        block.copy_from_slice(&buf[i * 16..(i + 1) * 16]);
        cipher.decrypt_block(aes::Block::from_mut_slice(&mut block));
        for j in 0..16 {
            buf[i * 16 + j] = block[j] ^ prev_ct[j];
        }
    }
    block.copy_from_slice(&buf[0..16]);
    cipher.decrypt_block(aes::Block::from_mut_slice(&mut block));
    for j in 0..16 {
        buf[j] = block[j] ^ CBC_IV[j];
    }
    Ok(buf)
}

/// PKCS7 去填充：末尾 n 字节必须全等于 n（1..=16），否则填充非法
fn pkcs7_unpad(buf: &[u8]) -> Result<&[u8], DecryptError> {
    let len = buf.len();
    let pad = *buf.last().ok_or(DecryptError::BadPadding)? as usize;
    if pad == 0 || pad > 16 || pad > len || buf[len - pad..].iter().any(|&b| b as usize != pad) {
        return Err(DecryptError::BadPadding);
    }
    Ok(&buf[..len - pad])
}

/// 解一条 v10/v11 密文 → 明文字符串。
/// `v11_key` 是 keyring 条目值**原文**（24 字符串的字节，不解 base64）——
/// 内部整体进 PBKDF2 派生出 AES key；None → NoKeyringKey 降级，绝不猜。
fn decrypt_os_crypt(blob: &[u8], v11_key: Option<&[u8]>) -> Result<String, DecryptError> {
    if blob.len() < 3 {
        return Err(DecryptError::TooShort);
    }
    let (prefix, body) = blob.split_at(3);
    let mut key: [u8; 16] = match prefix {
        b"v10" => derive_key(V10_PASSWORD),
        // keyring 条目值整体做口令（P0 实测：不解 base64、不截断）
        b"v11" => derive_key(v11_key.ok_or(DecryptError::NoKeyringKey)?),
        _ => return Err(DecryptError::UnknownPrefix),
    };
    let mut plain = aes_cbc_decrypt(body, &key)?;
    key.zeroize(); // 密钥用完即清
    let out = pkcs7_unpad(&plain)?;
    let s = std::str::from_utf8(out).map_err(|_| DecryptError::NotUtf8)?.to_string();
    plain.zeroize(); // 含填充的明文副本清零
    Ok(s)
}

// ---- keyring 取钥匙（org.freedesktop.secrete，与 P0 Python 验证算法一一对应）----
// 说明：keyring crate 只能按 service/username 属性检索，检索不到 Chromium 条目
//（其属性是 application=chrome，唯一稳定特征是 label）→ 按"不许猜"铁律，这里用
// keyring 底层同一套 Secret Service 协议（zbus）做 label 精确匹配。
// 锁定条目不弹解锁、不硬试 → 直接降级。返回值是密钥本体：调用方用完必须
// zeroize，绝不写日志/错误串。

/// GetSecret 回执结构。**字段序按线上实测签名 `(oayays)` 定**：
/// `(o session, ay parameters, ay value, s content_type)`。
/// 注意：本机 gnome-keyring 的实际回包把 `value`(ay) 排在 `content_type`(s) **之前**，
/// 与文档里常见的 (…, s content_type, ay value) 相反——2026-10-19 F-100 真机探针
/// 实测钉死（原实现的 (o, ay, s, ay) 会 SignatureMismatch → 取钥匙恒失败）。
#[derive(serde::Deserialize, zbus::zvariant::Type)]
struct SecretPayload {
    #[allow(dead_code)]
    session: zbus::zvariant::OwnedObjectPath,
    #[allow(dead_code)]
    parameters: Vec<u8>,
    /// 密钥本体（value）
    value: Vec<u8>,
    #[allow(dead_code)]
    content_type: String,
}

#[zbus::proxy(
    interface = "org.freedesktop.Secret.Service",
    default_service = "org.freedesktop.secrets",
    default_path = "/org/freedesktop/secrets"
)]
trait SecretService {
    fn open_session(
        &self,
        algorithm: &str,
        input: zbus::zvariant::Value<'_>,
    ) -> zbus::Result<(
        // 注意：gnome-keyring 声明的出参序是 `out v output, out o result`
        //（与 freedesktop 文档的 (o, v) 相反）——按**线上实际签名**为准，
        // 排错见 F-100 live（SignatureMismatch(expected (o,v), got (v,o))）。
        zbus::zvariant::OwnedValue,
        zbus::zvariant::OwnedObjectPath,
    )>;
    fn search_items(
        &self,
        attributes: std::collections::HashMap<&str, &str>,
    ) -> zbus::Result<(
        Vec<zbus::zvariant::OwnedObjectPath>,
        Vec<zbus::zvariant::OwnedObjectPath>,
    )>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Secret.Item",
    default_service = "org.freedesktop.secrets"
)]
trait SecretItem {
    #[zbus(property)]
    fn label(&self) -> zbus::Result<String>;
    #[zbus(property)]
    fn locked(&self) -> zbus::Result<bool>;
    fn get_secret(&self, session: zbus::zvariant::ObjectPath<'_>) -> zbus::Result<SecretPayload>;
}

/// 任何 keyring 侧失败统一降级（不许把 DBus 细节泄进用户错误）
fn nokey<T>(_: T) -> DecryptError {
    DecryptError::NoKeyringKey
}

/// 取某浏览器 v11 keyring 密钥（原始字节）。失败一律 Err 降级（keyring 锁着 /
/// DBus 不可达 / label 无条目），不猜 label、不硬试。空查询 = 全量枚举后按
/// label 精确匹配（与 P0 Python 逐项 load_secret 的行为一致）。
async fn fetch_v11_key(browser: &str) -> Result<Vec<u8>, DecryptError> {
    let label = keyring_label(browser).ok_or(DecryptError::NoKeyringKey)?;
    let conn = zbus::Connection::session().await.map_err(nokey)?;
    let svc = SecretServiceProxy::new(&conn).await.map_err(nokey)?;
    let (_, session) = svc
        // plain 会话的 input 必须是**空字符串**变体（`s`）。传字节数组（`ay`）会被
        // gnome-keyring 判 InvalidArgs("The session algorithm input argument (plain)
        // was invalid")，取钥匙第一步就失败 → 所有密码行降级 NoKeyringKey。
        // 2026-10-19 真机实测钉死（F-100 live 抓起）：gnome-keyring 条目本身是好的
        //（`SearchItems` 返回 22 条、label 命中且 Locked=false），错的是这个入参类型。
        .open_session("plain", zbus::zvariant::Value::from(""))
        .await
        .map_err(nokey)?;
    let (unlocked, locked) = svc
        .search_items(std::collections::HashMap::new())
        .await
        .map_err(nokey)?;
    for path in unlocked.into_iter().chain(locked) {
        let item = SecretItemProxy::builder(&conn)
            .path(path.as_str())
            .map_err(nokey)?
            .build()
            .await
            .map_err(nokey)?;
        if item.label().await.as_deref() != Ok(label) {
            continue;
        }
        // 锁定 → 不弹解锁 → 降级
        if item.locked().await.unwrap_or(true) {
            return Err(DecryptError::NoKeyringKey);
        }
        let session_path: zbus::zvariant::ObjectPath<'_> =
            session.as_str().try_into().map_err(nokey)?;
        let secret = item.get_secret(session_path).await.map_err(nokey)?;
        if secret.value.is_empty() {
            return Err(DecryptError::NoKeyringKey);
        }
        return Ok(secret.value);
    }
    Err(DecryptError::NoKeyringKey)
}

// 扫描结果 decryptable 已在 inspect_*_file 就地填充（F-90）。

#[cfg(test)]
mod f90_tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    // 黄金向量：openssl enc -aes-128-cbc + hashlib.pbkdf2_hmac(sha1) 生成（2026-10-18）
    const GOLDEN_V10_KEY_HEX: &str = "fd621fe5a2b402539dfa147ca9272778";
    const GOLDEN_V10_CT_HEX: &str = "8a2f0711b2b584b62bd4f134bd2d425636de3c42f4b17b617ba3fa739ad827a7";
    const GOLDEN_V11_SECRET: &str = "q7b7aG9hbm9yaXNlaGVpZGVp"; // 24 字符，不解 base64
    const GOLDEN_V11_KEY_HEX: &str = "07481d5306203a61a6bebdaf489f49d9";
    const GOLDEN_V11_CT_HEX: &str = "035436fe29e7cbbcf01298db6336bf4f";
    const GOLDEN_NONUTF8_CT_HEX: &str = "64e5b5699c3fb63b966a08e8e17bb45c"; // 明文 ff fe fd

    #[test]
    fn v10_peanuts_golden_vector() {
        assert_eq!(&derive_key(V10_PASSWORD)[..], &hex(GOLDEN_V10_KEY_HEX)[..]);
        let blob = [b"v10".as_slice(), &hex(GOLDEN_V10_CT_HEX)].concat();
        assert_eq!(
            decrypt_os_crypt(&blob, None).unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn v11_keyring_value_used_raw_no_base64_decode() {
        // 条目值按原始 24 字符串进 PBKDF2（不解 base64）——向量对上即证明
        assert_eq!(
            &derive_key(GOLDEN_V11_SECRET.as_bytes())[..],
            &hex(GOLDEN_V11_KEY_HEX)[..]
        );
        let blob = [b"v11".as_slice(), &hex(GOLDEN_V11_CT_HEX)].concat();
        // 传 keyring 条目值原文（口令）；derive 在 decrypt_os_crypt 内部完成
        // 明文含非 ASCII（é / 中文）：去填充后判 UTF-8 的判据铁律回归
        assert_eq!(
            decrypt_os_crypt(&blob, Some(GOLDEN_V11_SECRET.as_bytes())).unwrap(),
            "p@ssw0rd-é中"
        );
    }

    #[test]
    fn bad_padding_fails_not_panics() {
        // 教训文档回归：判据必须"去填充后再判"；坏填充 → Err，绝不 panic
        let mut ct = hex(GOLDEN_V10_CT_HEX);
        let last = ct.len() - 1;
        ct[last] ^= 0xff;
        let blob = [b"v10".as_slice(), &ct].concat();
        assert_eq!(decrypt_os_crypt(&blob, None), Err(DecryptError::BadPadding));
    }

    #[test]
    fn non_utf8_plaintext_fails_cleanly() {
        // 填充合法但明文非 UTF-8（ff fe fd）→ NotUtf8，不 panic 不给半截数据
        let blob = [b"v11".as_slice(), &hex(GOLDEN_NONUTF8_CT_HEX)].concat();
        assert_eq!(
            decrypt_os_crypt(&blob, Some(GOLDEN_V11_SECRET.as_bytes())),
            Err(DecryptError::NotUtf8)
        );
    }

    #[test]
    fn malformed_blobs_rejected() {
        assert_eq!(decrypt_os_crypt(b"v1", None), Err(DecryptError::TooShort));
        assert_eq!(decrypt_os_crypt(b"v99xxxx", None), Err(DecryptError::UnknownPrefix));
        // v11 无 key → 降级（keyring 锁着/不可用的命令级映射就是这条）
        let blob = [b"v11".as_slice(), &hex(GOLDEN_V11_CT_HEX)].concat();
        assert_eq!(decrypt_os_crypt(&blob, None), Err(DecryptError::NoKeyringKey));
        // 块长不对齐
        assert_eq!(
            decrypt_os_crypt(b"v10short", None),
            Err(DecryptError::BadLength)
        );
    }

    #[test]
    fn keyring_label_hard_mapping_exact() {
        assert_eq!(keyring_label("chrome"), Some("Chrome Safe Storage"));
        assert_eq!(keyring_label("brave"), Some("Brave Safe Storage"));
        // 不许猜：未知浏览器一律 None
        assert_eq!(keyring_label("chromium"), None);
        assert_eq!(keyring_label(""), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- F-99 预览：结构里绝不许出现密码/明文叶子字段 ----

    fn collect_keys(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, val) in m {
                    out.push(k.clone());
                    collect_keys(val, out);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| collect_keys(x, out)),
            _ => {}
        }
    }

    #[test]
    fn preview_struct_never_carries_plaintext_leaf_fields() {
        let pv = ImportPreview {
            passwords: vec![PasswordPreviewItem {
                browser: "chrome".into(),
                profile: "Default".into(),
                domain: "example.com".into(),
                username: Some("alice".into()),
            }],
            bookmark_groups: vec![BookmarkPreviewGroup {
                folder: "书签栏".into(),
                items: vec![BookmarkPreviewItem {
                    title: "A".into(),
                    url: "https://a.com/".into(),
                }],
            }],
            skipped_sources: vec!["Default/Login Data".into()],
        };
        let json = serde_json::to_value(&pv).unwrap();
        let mut keys = Vec::new();
        collect_keys(&json, &mut keys);
        for banned in ["password", "password_value", "plain", "plaintext", "secret", "cipher"] {
            assert!(
                !keys.iter().any(|k| k == banned),
                "预览结构不得含 `{banned}` 叶子字段（keys={keys:?}）"
            );
        }
        // 正向锚：站点/用户名/书签分组都在
        assert!(keys.iter().any(|k| k == "domain"));
        assert!(keys.iter().any(|k| k == "username"));
        assert!(keys.iter().any(|k| k == "bookmark_groups"));
    }

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
        let out = scan_from(base.path(), false).await;
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
        let out = scan_from(base.path(), false).await;
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
        // probe_keys=false：不碰 DBus，加密源 count=None / decryptable=None
        let out = scan_from(base.path(), false).await;
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
        let decryptable: Vec<Option<bool>> = r.iter().map(|x| x.decryptable).collect();
        assert_eq!(decryptable, [None, None, None, None, None]);
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
        let out = scan_from(base.path(), false).await;
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

