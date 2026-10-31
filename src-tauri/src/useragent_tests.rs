use super::*;

/// F-CEF-08：品牌表必须与「引擎实际发出的 UA」版本一致。
/// CEF 走内核原生 154、WebKit 走应用层 142；混用会造成 UA/brands 版本矛盾。
#[test]
fn brands_for_engine_matches_kernel_ua() {
    let brands = brands_json_for_engine();
    assert!(
        brands.contains("Google Chrome"),
        "品牌表必须包含 Google Chrome（真 Chrome 三项之一）：{brands}"
    );
    #[cfg(feature = "cef-engine")]
    assert!(
        brands.contains("\"154\""),
        "CEF 构建 brands 必须是内核 154，不能是应用层 142：{brands}"
    );
    #[cfg(not(feature = "cef-engine"))]
    assert!(
        brands.contains("\"142\""),
        "WebKit 构建沿用应用层 142：{brands}"
    );
}

/// 有全局静态可变状态的测试统一串行（GLOBAL_UA / SITE_OVERRIDES 进程级共享）；
/// 用 tokio 互斥锁：状态类测试均 async，锁跨 await 不中毒
static STATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// 复位全局静态到默认态（每个状态类测试开头调用，防用例间状态泄漏）
fn reset_shared_state() {
    *global_ua().lock().unwrap_or_else(|e| e.into_inner()) = DEFAULT_UA.to_string();
    overrides()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// 建一个跑完整迁移的真实库（settings 表齐全），同 newtab.rs temp_db 模式
async fn temp_db() -> (tempfile::TempDir, crate::db::Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Db::init(dir.path().to_path_buf()).await.unwrap();
    (dir, db)
}

/// 直写一行 settings（测试造数用；绕开命令层）
async fn seed_setting(pool: &SqlitePool, key: &str, value: &str) {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .unwrap();
}

// ── 纯函数：chrome_major ────────────────────────────────────────

#[test]
fn chrome_major_parses_version_from_ua() {
    assert_eq!(chrome_major(DEFAULT_UA), 142);
    assert_eq!(
        chrome_major("Mozilla/5.0 (X11; Linux x86_64) Chrome/131.0.0.0 Safari/537.36"),
        131
    );
}

#[test]
fn chrome_major_falls_back_to_142_on_missing_or_malformed() {
    // 缺 Chrome 标记（WebKitGTK 真身）
    assert_eq!(
        chrome_major("Mozilla/5.0 (X11) AppleWebKit/605.1.15 Safari/605"),
        142
    );
    // 畸变输入：非数字 / 空串
    assert_eq!(chrome_major("Chrome/abc"), 142);
    assert_eq!(chrome_major(""), 142);
}

// ── 纯函数：brands_json ─────────────────────────────────────────

#[test]
fn brands_json_is_valid_and_aligned_with_chrome_major() {
    let json = brands_json("Chrome/131.0.0.0 Safari/537.36");
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("brands_json 必须是合法 JSON");
    let brands = parsed.as_array().unwrap();
    assert_eq!(brands.len(), 3);
    // Chromium 与 Google Chrome 两项版本都要对齐 chrome_major
    for brand in brands {
        let name = brand["brand"].as_str().unwrap();
        if name == "Chromium" || name == "Google Chrome" {
            assert_eq!(brand["version"].as_str().unwrap(), "131");
        }
    }
    // 默认 UA → 142 对齐
    assert!(brands_json(DEFAULT_UA).contains(r#""version":"142""#));
}

// ── 状态函数：ua_for / overrides_json（串行） ───────────────────

#[tokio::test]
async fn ua_for_prefers_site_override_then_global_and_skips_empty_value() {
    let _g = STATE_LOCK.lock().await;
    reset_shared_state();
    assert_eq!(ua_for(""), DEFAULT_UA); // 空 host 回落全局
                                        // 站点覆盖优先于全局
    overrides()
        .lock()
        .unwrap()
        .insert("example.com".into(), "TestUA/9.0".into());
    assert_eq!(ua_for("example.com"), "TestUA/9.0");
    assert_eq!(ua_for("other.com"), DEFAULT_UA);
    // 覆盖值为空串 → 视为无覆盖，回落全局
    overrides()
        .lock()
        .unwrap()
        .insert("empty.com".into(), String::new());
    assert_eq!(ua_for("empty.com"), DEFAULT_UA);
}

#[tokio::test]
async fn overrides_json_empty_table_returns_empty_object() {
    let _g = STATE_LOCK.lock().await;
    reset_shared_state();
    let json = overrides_json();
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("overrides_json 必须是合法 JSON");
    assert!(parsed.as_object().unwrap().is_empty());
}

// ── 抽出的命令核心：normalized_global_ua / apply_site_override ──

#[test]
fn normalized_global_ua_trims_and_falls_back_to_default_on_empty() {
    assert_eq!(normalized_global_ua(""), DEFAULT_UA);
    assert_eq!(normalized_global_ua("   "), DEFAULT_UA);
    assert_eq!(normalized_global_ua("  Chrome/77.0  "), "Chrome/77.0");
}

#[test]
fn apply_site_override_rejects_empty_host() {
    let mut m = HashMap::new();
    assert!(apply_site_override(&mut m, "", "SomeUA/1").is_err());
    assert!(apply_site_override(&mut m, "   ", "SomeUA/1").is_err());
    assert!(m.is_empty());
}

#[test]
fn apply_site_override_lowercases_host_and_empty_ua_removes() {
    let mut m = HashMap::new();
    apply_site_override(&mut m, "  Example.COM ", "SomeUA/1").unwrap();
    assert_eq!(m.get("example.com").map(String::as_str), Some("SomeUA/1"));
    // 空 ua = 删除覆盖
    apply_site_override(&mut m, "example.com", "   ").unwrap();
    assert!(!m.contains_key("example.com"));
}

// ── DB 命令层：persist_setting + load（真实库） ─────────────────

#[tokio::test]
async fn persist_setting_upserts_and_load_reads_back() {
    let _g = STATE_LOCK.lock().await;
    reset_shared_state();
    let (_dir, db) = temp_db().await;
    // 首次插入
    persist_setting(&db.pool, "user_agent", "Chrome/100.0.0.0 persist")
        .await
        .unwrap();
    // 冲突更新
    persist_setting(&db.pool, "user_agent", "Chrome/101.0.0.0 persist")
        .await
        .unwrap();
    // 命令层语义：全局 UA 空串落库为空串（命令写 trim 后的值）
    persist_setting(&db.pool, "ua_site_overrides", r#"{"db.com":"DbUA/1"}"#)
        .await
        .unwrap();
    // load 读回并生效到内存状态
    load(&db.pool).await;
    assert_eq!(ua_for(""), "Chrome/101.0.0.0 persist");
    assert_eq!(ua_for("db.com"), "DbUA/1");
    assert!(overrides_json().contains("db.com"));
}

#[tokio::test]
async fn load_keeps_default_when_user_agent_blank_or_absent() {
    let _g = STATE_LOCK.lock().await;
    reset_shared_state();
    let (_dir, db) = temp_db().await;
    // 不写任何 settings → 全部缺省
    load(&db.pool).await;
    assert_eq!(ua_for(""), DEFAULT_UA);
    // 空串 / 纯空白 → 不采纳，保持默认
    seed_setting(&db.pool, "user_agent", "   ").await;
    load(&db.pool).await;
    assert_eq!(ua_for(""), DEFAULT_UA);
    // 覆盖表畸变 JSON → 静默忽略，不炸
    seed_setting(&db.pool, "ua_site_overrides", "not-json").await;
    load(&db.pool).await;
    assert!(overrides_json().starts_with('{'));
}
