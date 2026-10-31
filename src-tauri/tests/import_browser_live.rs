//! F-100 真机验收：从浏览器导入 · 后端全链 live 测试（王二狗，2026-10-19）
//!
//! 跑法：`cargo test --test import_browser_live -- --ignored --nocapture`
//!
//! 前置：本机真实 Chrome / Brave profile（`~/.config/google-chrome`、
//! `~/.config/BraveSoftware/Brave-Browser`）；图形会话（v11 keyring 走 D-Bus
//! session，需 `DBUS_SESSION_BUS_ADDRESS` 指向会话总线，否则 v11 行整体降级）。
//!
//! **铁律**：
//! 1. 靶子是本机**真实** profile，**不许 fixture 造数**（造数归单测）；
//! 2. 源库**只读**：导入前后 `Bookmarks` / `Login Data*` 的 sha256 + mtime
//!    必须逐字节一致（本模块只 copy 到 0700 临时目录再读副本）；
//! 3. 落库用**临时 app 数据目录**（`Db::init(tempdir)`），绝不碰高哥真机应用库。
//!
//! 全部用例 `#[ignore]`：默认 `cargo test` 不跑（CI 无真机 profile 也会绿）。

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use shift_browser_lib::import_live_bridge::{
    import_bookmarks_inner, import_passwords_inner, preview_browser_import_inner,
    scan_browser_import_inner, Db, ImportBookmarksResult, ImportPasswordsResult, ImportPreview,
    SourceReport,
};

// ── 真机锚 ──────────────────────────────────────────────────────────
// 卡面（2026-10-19 06:14）记 Chrome `AccountBookmarks`=120；同日 08:2x 复测为
// **122**（真机书签确有变动，非代码问题）——按实测值更新，差异已在 F-100 回执如实记。
const CHROME_ACCOUNT_BOOKMARKS: usize = 122;
const CHROME_BOOKMARKS_SHELL: usize = 0;
const CHROME_LOGIN_FOR_ACCOUNT: usize = 69;
const BRAVE_BOOKMARKS: usize = 127;
const BRAVE_LOGIN: usize = 20;
/// 真机数据里的空密码锚（解密成功但无值 → skipped_empty）
const PASSWORD_EMPTY_ANCHOR: usize = 6;

/// 排除目录（F-89 铁律 ②）：任何情况下都不得出现在结果里
const EXCLUDED_PROFILE_DIRS: &[&str] =
    &["brave-anon", "google-chrome-headless", "ms-playwright-mcp"];

/// 被监视的源文件（导入前后指纹必须一致）
const WATCHED_FILES: &[&str] = &[
    "AccountBookmarks",
    "Bookmarks",
    "Bookmarks.bak",
    "Login Data",
    "Login Data For Account",
];

fn config_root() -> PathBuf {
    PathBuf::from(std::env::var("HOME").expect("live 测试需要 HOME")).join(".config")
}

/// 临时 app 数据目录上的独立 Db（不碰高哥真机应用库）
async fn temp_db() -> (tempfile::TempDir, Db) {
    let tmp = tempfile::tempdir().expect("建临时目录失败");
    let db = Db::init(tmp.path().join("appdata"))
        .await
        .expect("Db::init 失败（临时 app 数据目录）");
    (tmp, db)
}

// ── 源库指纹（sha256 + mtime 纳秒）──────────────────────────────────

fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("读源文件失败");
    let mut h = Sha256::new();
    h.update(&bytes);
    hex::encode(h.finalize())
}

fn mtime_nanos(path: &Path) -> u128 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// 真机源文件清单（Chrome + Brave 全部 profile，排除名单目录）
fn watched_sources() -> Vec<PathBuf> {
    let base = config_root();
    let mut out = Vec::new();
    for rel in ["google-chrome", "BraveSoftware/Brave-Browser"] {
        let Ok(entries) = std::fs::read_dir(base.join(rel)) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| !EXCLUDED_PROFILE_DIRS.contains(&n))
            })
            .collect();
        dirs.sort();
        for d in dirs {
            for f in WATCHED_FILES {
                let p = d.join(f);
                if p.is_file() {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn fingerprint_sources() -> BTreeMap<String, (String, u128)> {
    watched_sources()
        .into_iter()
        .map(|p| {
            let key = p.to_string_lossy().into_owned();
            let v = (sha256_of(&p), mtime_nanos(&p));
            (key, v)
        })
        .collect()
}

/// 源库零改动断言（F-100 断言 ⑤）
fn assert_sources_untouched(before: &BTreeMap<String, (String, u128)>) {
    let after = fingerprint_sources();
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "源文件清单变了（导入过程不应新增/删除源文件）"
    );
    for (path, (h_before, m_before)) in before {
        let (h_after, m_after) = &after[path];
        assert_eq!(h_after, h_before, "源文件被改动（sha256 变了）：{path}");
        assert_eq!(m_after, m_before, "源文件被改动（mtime 变了）：{path}");
    }
    println!(
        "源库零改动 ✔ 共 {} 个源文件（sha256 + mtime 逐项一致）",
        before.len()
    );
}

// ── 回执结构无明文字段（F-100 断言 ⑥）────────────────────────────────

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

/// 断言序列化结果里不出现任何明文/密文字段名
fn assert_no_plaintext_leaves<T: serde::Serialize>(v: &T, what: &str) {
    let json = serde_json::to_value(v).expect("序列化失败");
    let mut keys = Vec::new();
    collect_keys(&json, &mut keys);
    for banned in ["password", "password_value", "plain", "plaintext", "cipher"] {
        assert!(
            !keys.iter().any(|k| k == banned),
            "{what} 不得含 `{banned}` 叶子字段（keys={keys:?}）"
        );
    }
}

fn scan_find<'a>(out: &'a [SourceReport], browser: &str, file: &str) -> Option<&'a SourceReport> {
    out.iter().find(|r| r.browser == browser && r.file == file)
}

fn print_scan(out: &[SourceReport]) {
    println!("── 真机扫描结果（{} 条源记录）──", out.len());
    for r in out {
        println!(
            "  {:<6} {:<20} {:<28} count={:<6?} encrypted={:<5} decryptable={:?}",
            r.browser, r.profile, r.file, r.count, r.encrypted, r.decryptable
        );
    }
}

// ═════════════════════ 用例 ═════════════════════

/// ① 扫描数字对齐真机 + 排除目录不入结果 + 空壳必须出现且标 Some(0)
#[tokio::test]
#[ignore = "真机 live：需要本机 Chrome/Brave profile"]
async fn scan_matches_real_profiles() {
    let out = scan_browser_import_inner().await;
    print_scan(&out);
    assert!(
        !out.is_empty(),
        "真机必须扫到源（空结果说明 HOME/.config 路径没找对）"
    );

    // Chrome：AccountBookmarks 条数
    let ab = scan_find(&out, "chrome", "AccountBookmarks").unwrap_or_else(|| {
        panic!("Chrome `AccountBookmarks` 必须扫到（实测应为 {CHROME_ACCOUNT_BOOKMARKS} 条）")
    });
    assert_eq!(
        ab.count,
        Some(CHROME_ACCOUNT_BOOKMARKS),
        "Chrome AccountBookmarks 条数对不上（None=读取失败，与 0 语义不同）"
    );

    // 空壳：必须**出现**且为 Some(0)——不许缺行、不许 None
    let shell = scan_find(&out, "chrome", "Bookmarks")
        .unwrap_or_else(|| panic!("Chrome 空壳 `Bookmarks` 必须出现在结果里（缺行=扫描漏源）"));
    assert_eq!(
        shell.count,
        Some(CHROME_BOOKMARKS_SHELL),
        "空壳必须标 Some(0)；None 是「读取失败」语义，两者不许混"
    );
    assert!(!shell.encrypted, "明文 Bookmarks 不应标 encrypted");

    // Chrome 密码源
    let lfa = scan_find(&out, "chrome", "Login Data For Account").unwrap_or_else(|| {
        panic!("Chrome `Login Data For Account` 必须扫到（实测应为 {CHROME_LOGIN_FOR_ACCOUNT} 行）")
    });
    assert_eq!(
        lfa.count,
        Some(CHROME_LOGIN_FOR_ACCOUNT),
        "Chrome 密码源行数对不上"
    );

    // Brave：书签 + 密码
    let bb = scan_find(&out, "brave", "Bookmarks")
        .unwrap_or_else(|| panic!("Brave `Bookmarks` 必须扫到（实测应为 {BRAVE_BOOKMARKS} 条）"));
    assert_eq!(bb.count, Some(BRAVE_BOOKMARKS), "Brave 书签条数对不上");

    let bl = scan_find(&out, "brave", "Login Data")
        .unwrap_or_else(|| panic!("Brave `Login Data` 必须扫到（实测应为 {BRAVE_LOGIN} 行）"));
    assert_eq!(bl.count, Some(BRAVE_LOGIN), "Brave 密码行数对不上");

    // 排除目录（铁律 ②）
    for banned in EXCLUDED_PROFILE_DIRS {
        assert!(
            out.iter().all(|r| r.profile != *banned),
            "排除目录 `{banned}` 不得出现在结果里"
        );
    }
    println!("① 扫描断言全过 ✔");
}

/// ② 预览：逐行 domain/username + 书签按文件夹分组 + 无明文字段
#[tokio::test]
#[ignore = "真机 live：需要本机 Chrome/Brave profile"]
async fn preview_rows_grouped_bookmarks_and_no_plaintext() {
    let pv = preview_browser_import_inner().await;
    println!(
        "预览：密码 {} 行 / 书签 {} 组 / 跳过源 {} 个",
        pv.passwords.len(),
        pv.bookmark_groups.len(),
        pv.skipped_sources.len()
    );
    for g in &pv.bookmark_groups {
        println!("  组「{}」：{} 条", g.folder, g.items.len());
    }

    // 密码预览：逐行 domain / username，domain 不许空
    assert!(
        !pv.passwords.is_empty(),
        "真机应有密码预览行（空=源库/keyring 异常；skipped_sources={:?}）",
        pv.skipped_sources
    );
    for it in &pv.passwords {
        assert!(!it.domain.trim().is_empty(), "预览行 domain 不许为空");
        assert!(
            !it.browser.is_empty() && !it.profile.is_empty(),
            "预览行须带来源标识"
        );
    }
    assert!(
        pv.passwords.iter().any(|it| it.username.is_some()),
        "真机应至少有带用户名的密码行"
    );

    // 书签预览：按文件夹分组渲染是非空分组构成的列表
    assert!(
        !pv.bookmark_groups.is_empty(),
        "书签预览必须按文件夹分组（不得为空）"
    );
    let total_items: usize = pv.bookmark_groups.iter().map(|g| g.items.len()).sum();
    assert!(
        total_items >= BRAVE_BOOKMARKS,
        "书签预览行数 {total_items} 少于 Brave 单源基线 {BRAVE_BOOKMARKS}（Chrome 120 + Brave 127 去重后应更多）"
    );
    assert!(
        pv.bookmark_groups.iter().all(|g| !g.folder.is_empty()),
        "分组名不许为空"
    );

    // 序列化后无明文字段（F-100 断言 ② 后半 + ⑥）
    assert_no_plaintext_leaves(&pv, "ImportPreview");
    println!("② 预览断言全过 ✔（共 {total_items} 条书签预览行）");
}

/// ③ 书签导入：真机写入 >0，二次导入 imported=0（URL 去重）+ 源库零改动
#[tokio::test]
#[ignore = "真机 live：需要本机 Chrome/Brave profile"]
async fn bookmark_import_writes_then_dedupes_and_leaves_sources_alone() {
    let (_tmp, db) = temp_db().await;
    let before = fingerprint_sources();

    let r1 = import_bookmarks_inner(&db)
        .await
        .expect("首次书签导入不得报错");
    println!("首次：{}", serde_json::to_string(&r1).unwrap());
    assert!(r1.imported > 0, "首次导入必须真写入 >0 条（real profile）");
    assert!(
        r1.imported + r1.merged_away + r1.rejected_unsafe > 0,
        "回执计数全零 = 源没读到"
    );
    assert!(
        !r1.used_sources.is_empty(),
        "必须报告实际参与合并的源文件（skipped={:?}）",
        r1.skipped_sources
    );
    assert_no_plaintext_leaves(&r1, "ImportBookmarksResult");

    // 二次导入：同 URL 已落库 → imported 必须 0（不许重复插入）
    let r2 = import_bookmarks_inner(&db)
        .await
        .expect("二次书签导入不得报错");
    println!("二次：{}", serde_json::to_string(&r2).unwrap());
    assert_eq!(r2.imported, 0, "二次导入 imported 必须为 0（URL 去重）");

    assert_sources_untouched(&before);
    println!("③ 书签导入断言全过 ✔（首次写入 {} 条）", r1.imported);
}

/// ④ 密码导入：首次条数守恒、二次 insert-only 不覆盖（F-98）+ 源库零改动
#[tokio::test]
#[ignore = "真机 live：需要本机 Chrome/Brave profile"]
async fn password_import_is_insert_only_on_second_run() {
    let (_tmp, db) = temp_db().await;
    let before = fingerprint_sources();

    // 真机密码总行数：从扫描结果取（与断言 ① 的锚互为印证）
    let scan = scan_browser_import_inner().await;
    let total: usize = scan
        .iter()
        .filter(|r| r.kind == "passwords")
        .filter_map(|r| r.count)
        .sum();
    assert_eq!(
        total,
        CHROME_LOGIN_FOR_ACCOUNT + BRAVE_LOGIN,
        "真机密码源总行数与卡面锚不符（Chrome {CHROME_LOGIN_FOR_ACCOUNT} + Brave {BRAVE_LOGIN}）"
    );

    let r1 = import_passwords_inner(&db)
        .await
        .expect("首次密码导入不得报错");
    println!("首次：{}", serde_json::to_string(&r1).unwrap());
    assert_eq!(
        r1.imported + r1.skipped_existing + r1.skipped_empty + r1.failed,
        total,
        "条数守恒：imported + skipped_existing + skipped_empty + failed 必须等于真机总行数 {total}"
    );
    assert_eq!(
        r1.skipped_empty, PASSWORD_EMPTY_ANCHOR,
        "空密码锚对不上（卡面实测 {PASSWORD_EMPTY_ANCHOR} 条）"
    );
    assert_eq!(
        r1.failed, 0,
        "keyring 可用时不应有解密/写库失败（degraded_reason={:?}）",
        r1.degraded_reason
    );
    assert!(r1.imported > 0, "首次必须真写入 >0 条");
    assert_no_plaintext_leaves(&r1, "ImportPasswordsResult");

    // 二次导入：F-98 insert-only —— imported=0 且 skipped_existing = 首次 imported（不覆盖）
    let r2 = import_passwords_inner(&db)
        .await
        .expect("二次密码导入不得报错");
    println!("二次：{}", serde_json::to_string(&r2).unwrap());
    assert_eq!(
        r2.imported, 0,
        "F-98 insert-only：二次导入不得新写（更不许覆盖）"
    );
    assert_eq!(
        r2.skipped_existing,
        r1.imported + r1.skipped_existing,
        "二次 skipped_existing 必须等于首次导入总存量（imported + skipped_existing，同 (domain,username) 全命中）"
    );

    assert_sources_untouched(&before);
    println!(
        "④ 密码导入断言全过 ✔（首次写入 {} 条，空密码 {} 条）",
        r1.imported, r1.skipped_empty
    );
}

/// ⑥ 回执结构无明文字段（纯结构，不需真机，但一并放这里做留痕）
#[test]
#[ignore = "真机 live：与 ③④ 同批跑，留痕用"]
fn receipt_structs_carry_no_plaintext_leaves() {
    let bm = ImportBookmarksResult {
        imported: 1,
        merged_away: 2,
        rejected_unsafe: 3,
        skipped_sources: vec!["Default/Bookmarks".into()],
        used_sources: vec!["Default/AccountBookmarks".into()],
    };
    let pw = ImportPasswordsResult {
        imported: 1,
        skipped_existing: 2,
        skipped_empty: 3,
        failed: 4,
        skipped_sources: vec!["Default/Login Data".into()],
        used_sources: vec!["Default/Login Data For Account".into()],
        degraded_reason: None,
    };
    let pv = ImportPreview {
        passwords: vec![],
        bookmark_groups: vec![],
        skipped_sources: vec![],
    };
    assert_no_plaintext_leaves(&bm, "ImportBookmarksResult");
    assert_no_plaintext_leaves(&pw, "ImportPasswordsResult");
    assert_no_plaintext_leaves(&pv, "ImportPreview");
    println!("⑥ 回执结构断言全过 ✔");
}
