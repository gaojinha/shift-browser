// LINES-EXEMPT: 翻译记忆的读写、命中统计与淘汰在同一事务域内，拆开需暴露内部连接
// 翻译记忆（整页汉化提速 · 2026-09-27）
//
// 持久化「模型 + 归一化原文 → 译文」。命中缓存的段落不发 opencode 请求
// （每批约 6 秒固定开销，与段数几乎无关——见 webzh.rs 的实测注释），
// 重访页面、常见 UI 文案、二次汉化直接秒回。
//
// 边界：
// - 模型维度隔离：模型换了，旧译文风格可能不一致，宁可重翻；
// - 只存归一化后（压缩空白）的短/中文本，与收集/分批的口径一致；
// - 所有写入都是尽力而为：翻译主流程绝不因缓存故障被打断。

use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::SqlitePool;

use crate::db::Db;

type HmacSha256 = Hmac<Sha256>;

/// 单条原文上限：和翻译分批上限一致，过长的多半是切分边界外的异常内容。
const MAX_SOURCE_CHARS: usize = 6000;
/// IN 查询每块大小：SQLite 变量上限很宽松（新版本 32766），400 一块足够。
const LOOKUP_CHUNK: usize = 400;

/// 算 HMAC-SHA256(translate.key, 归一化原文) 的 hex（C-11：库里不落原文明文）。
///
/// 确定性：同一 key + 同一原文 → 同一 hash，等值查询/主键/裁剪语义与明文列一致；
/// 不可逆：DB 单独泄露时拿不到原文（比加密强——连密文都不存在）。
///
/// `new_from_slice` 对 HMAC 实际不可失败（HMAC 接受任意长度密钥），故就地标注
/// `expect_used`（与 clippy.toml 的生产护栏口径一致），不为不可达分支引入返回值。
#[allow(clippy::expect_used)]
fn source_hash_hex(key: &[u8; 32], source: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(source.as_bytes());
    let tag = mac.finalize().into_bytes();
    hex_encode(&tag)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 取翻译记忆 HMAC 密钥（C-11）：读 translate.key，不存在则生成（权限 600，
/// create_new 原子创建，与密码库 master.key/vault 解锁状态完全解耦——锁库不影响翻译记忆）。
///
/// 文件损坏时**绝不静默重生**（与 master.key 同口径：重生会让旧记忆全部作废且键更换
/// 无从察觉），按记忆模块「尽力而为」语义降级：返回 None = 记忆停用，翻译不受影响。
/// 返回 Some 的进程内缓存挂在 Db.translate_key（tokio Mutex 串行化首次生成）。
pub async fn hmac_key(db: &Db) -> Option<[u8; 32]> {
    let mut cache = db.translate_key.lock().await;
    if let Some(k) = *cache {
        return Some(k);
    }
    let path = &db.translate_key_path;
    let key = if path.exists() {
        match super::password_crypto::read_key_file(path, "翻译记忆") {
            Ok(k) => k,
            Err(e) => {
                log::warn!("[translate_memory] {e}；翻译记忆已停用（翻译主流程不受影响）");
                return None;
            }
        }
    } else {
        let mut k = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut k);
        let encoded = BASE64.encode(k);
        match super::password_crypto::write_key_file_create_new(path, &encoded) {
            Ok(()) => k,
            Err(_) if path.exists() => {
                // 并发窗口：另一调用者已写入，读回其密钥（绝不覆盖）
                match super::password_crypto::read_key_file(path, "翻译记忆") {
                    Ok(existing) => existing,
                    Err(e) => {
                        log::warn!("[translate_memory] {e}；翻译记忆已停用（翻译主流程不受影响）");
                        return None;
                    }
                }
            }
            Err(e) => {
                log::warn!("[translate_memory] {e}；翻译记忆已停用（翻译主流程不受影响）");
                return None;
            }
        }
    };
    *cache = Some(key);
    Some(key)
}

/// 归一化原文口径与 page_collect.js 的 norm() 保持一致：所有空白压成单空格并 trim。
///
/// 口径不一致的后果是"存的时候一个 key、查的时候另一个 key"，缓存永远miss——
/// 这种 bug 不会报错、只会静默变慢，所以单测把两边的典型输入钉死。
pub fn normalize_source(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 引擎翻译的记忆命名空间键（C-9）：把「引擎 + 目标语言」编进 `model` 隔离维度。
///
/// 复用同一张 `translate_memory` 表，用前缀命名空间与 AI 模型路径区隔：
/// - AI 整页汉化：`model` = 模型名（如 `opencode/big-pickle`）
/// - 引擎翻译：  `model` = `engine:google:zh-CN` / `engine:bing:zh-Hans`
///
/// 为什么把目标语言也编进去：同一原文翻成不同语言是不同结果，只按引擎隔离会
/// 出现「换了目标语言却命中旧语言译文」的脏命中。前缀 `engine:` 保证不会与
/// 无前缀的 AI 模型名撞键。
pub fn engine_scope(engine: &str, target: &str) -> String {
    format!("engine:{}:{}", engine.trim(), target.trim())
}

fn storable(model: &str, source: &str, target: &str) -> bool {
    !model.trim().is_empty()
        && !source.is_empty()
        && !target.trim().is_empty()
        && source.chars().count() <= MAX_SOURCE_CHARS
}

/// 批量查缓存：source（原始未归一化）→ target。查不到的 key 不出现在 map 里。
///
/// 模型名为空时直接返回空 map（宁可全量重翻，也不能在"未知模型"键下错命中）。
/// C-11：source 归一化后走 HMAC 查询键，库里不落原文明文。
pub async fn lookup(
    pool: &SqlitePool,
    model: &str,
    sources: &[String],
    key: &[u8; 32],
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if model.trim().is_empty() || sources.is_empty() {
        return out;
    }
    // 去重 + 归一化，别让同一段文本在一条 SQL 里占两个绑定位
    let mut uniq: Vec<String> = sources
        .iter()
        .map(|s| normalize_source(s))
        .filter(|s| !s.is_empty() && s.chars().count() <= MAX_SOURCE_CHARS)
        .collect();
    uniq.sort_unstable();
    uniq.dedup();

    for window in uniq.chunks(LOOKUP_CHUNK) {
        let placeholders = window.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT source_hash, target FROM translate_memory WHERE model = ? AND source_hash IN ({placeholders})"
        );
        let mut q = sqlx::query_as::<_, (String, String)>(&sql).bind(model);
        for s in window {
            q = q.bind(source_hash_hex(key, s));
        }
        match q.fetch_all(pool).await {
            Ok(rows) => {
                for (source_hash, target) in rows {
                    if let Some(source) = window.iter().find(|s| source_hash_hex(key, s) == source_hash) {
                        out.entry(source.clone()).or_insert(target);
                    }
                }
            }
            Err(e) => log::warn!("[translate_memory] 查询失败（本次翻译不受影响）: {e}"),
        }
    }
    out
}

/// 批量写入（upsert）。返回实际写入条数；任何失败只记日志、不打断翻译。
///
/// 同一原文再次出现（新译文）时覆盖 target 并累加 hits——以最新一次模型输出
/// 为准，比"首译永远生效"更能反映模型/prompt 改进后的质量。
///
/// 记忆保留上限：总量超限时优先保留高命中、最近使用的条目
pub const MEMORY_KEEP: i64 = 5000;
pub async fn remember(
    pool: &SqlitePool,
    model: &str,
    pairs: &[(String, String)],
    key: &[u8; 32],
) -> usize {
    if model.trim().is_empty() || pairs.is_empty() {
        return 0;
    }
    let mut written = 0usize;
    // 先归一化过滤，再分片批量写：每片 200 行（600 绑定位，兼容新老 SQLite 变量上限），
    // 一片一次往返；原逐条 execute 几百次往返，与 feed 的批量做法对齐。
    // C-11：归一化原文只算 HMAC 查询键落库，明文不出现在任何列
    let clean: Vec<(String, &str)> = pairs
        .iter()
        .map(|(raw_source, raw_target)| (normalize_source(raw_source), raw_target.trim()))
        .filter(|(source, target)| storable(model, source, target))
        .map(|(source, target)| (source_hash_hex(key, &source), target))
        .collect();
    // 单事务批量提交：翻译一页要写几百条，逐条 autocommit 会让 WAL fsync 刷屏。
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            log::warn!("[translate_memory] 开事务失败（本次翻译不受影响）: {e}");
            return 0;
        }
    };
    {
        use sqlx::QueryBuilder;
        for window in clean.chunks(200) {
            let mut qb: QueryBuilder<sqlx::Sqlite> =
                QueryBuilder::new("INSERT INTO translate_memory (model, source_hash, target) ");
            qb.push_values(window.iter(), |mut b, (source_hash, target)| {
                b.push_bind(model).push_bind(source_hash).push_bind(target);
            });
            qb.push(
                " ON CONFLICT(model, source_hash) DO UPDATE SET \
                 target = excluded.target, hits = hits + 1, updated_at = datetime('now')",
            );
            match qb.build().execute(&mut *tx).await {
                Ok(_) => written += window.len(),
                Err(e) => log::warn!("[translate_memory] 写入失败（本次翻译不受影响）: {e}"),
            }
        }
    }
    // 总量裁剪：优先保留高命中、最近使用的条目，其余淘汰（防无限增长）
    if let Err(e) = sqlx::query(
        "DELETE FROM translate_memory WHERE (model, source_hash) NOT IN \
         (SELECT model, source_hash FROM translate_memory ORDER BY hits DESC, updated_at DESC LIMIT ?)",
    )
    .bind(MEMORY_KEEP)
    .execute(&mut *tx)
    .await
    {
        log::warn!("[translate_memory] 裁剪失败（本次翻译不受影响）: {e}");
    }
    if let Err(e) = tx.commit().await {
        log::warn!("[translate_memory] 提交失败（本次翻译不受影响）: {e}");
        return 0;
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用 HMAC 密钥（各用例共用即可：隔离靠 model 命名空间 + 独立临时库）
    const KEY: [u8; 32] = [0x5a; 32];

    async fn test_pool(dir: &std::path::Path) -> SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        let options = SqliteConnectOptions::new()
            .filename(dir.join("tm_test.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20261015000000_translate_memory_hmac.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[test]
    fn 归一化压缩所有空白() {
        assert_eq!(normalize_source("  Hello   world\n\t"), "Hello world");
        // 全角空格也算 unicode whitespace，split_whitespace 能处理
        assert_eq!(normalize_source("Sign\u{3000}in"), "Sign in");
    }

    #[test]
    fn 引擎命名空间键按引擎与目标语言隔离() {
        assert_eq!(engine_scope("google", "zh-CN"), "engine:google:zh-CN");
        // 引擎不同 → 键不同
        assert_ne!(
            engine_scope("google", "zh-CN"),
            engine_scope("bing", "zh-CN")
        );
        // 目标语言不同 → 键不同（防旧语言脏命中）
        assert_ne!(engine_scope("google", "zh-CN"), engine_scope("google", "ja"));
        // 前缀保证与无前缀的 AI 模型名不撞键
        assert!(engine_scope("google", "zh-CN").starts_with("engine:"));
    }

    #[tokio::test]
    async fn 引擎与模型命名空间互不命中() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        let engine_key = engine_scope("google", "zh-CN");
        remember(
            &pool,
            &engine_key,
            &[("Home".to_string(), "首页".to_string())],
            &KEY,
        )
        .await;
        remember(
            &pool,
            "opencode/big-pickle",
            &[("Home".to_string(), "主页".to_string())],
            &KEY,
        )
        .await;
        let engine_hit = lookup(&pool, &engine_key, &["Home".to_string()], &KEY).await;
        let model_hit = lookup(&pool, "opencode/big-pickle", &["Home".to_string()], &KEY).await;
        assert_eq!(engine_hit.get("Home"), Some(&"首页".to_string()));
        assert_eq!(model_hit.get("Home"), Some(&"主页".to_string()));
    }

    #[tokio::test]
    async fn 写入后按归一化key命中() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        let n = remember(
            &pool,
            "model-a",
            &[
                ("  Sign   in ".to_string(), "登录".to_string()),
                ("Settings".to_string(), "设置".to_string()),
            ],
            &KEY,
        )
        .await;
        assert_eq!(n, 2);
        let got = lookup(
            &pool,
            "model-a",
            &[
                "Sign\n in".to_string(),
                "Settings".to_string(),
                "Missing".to_string(),
            ],
            &KEY,
        )
        .await;
        assert_eq!(got.get("Sign in"), Some(&"登录".to_string()));
        assert_eq!(got.get("Settings"), Some(&"设置".to_string()));
        assert_eq!(got.len(), 2, "没翻到的不该出现");
    }

    #[tokio::test]
    async fn 模型隔离不串用() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        remember(
            &pool,
            "model-a",
            &[("Save".to_string(), "保存".to_string())],
            &KEY,
        )
        .await;
        let other = lookup(&pool, "model-b", &["Save".to_string()], &KEY).await;
        assert!(other.is_empty(), "别的模型的记忆不能命中");
        // 空模型名直接 miss，不允许落到 "" 这种脏键
        assert!(lookup(&pool, "", &["Save".to_string()], &KEY).await.is_empty());
        assert_eq!(
            remember(
                &pool,
                "",
                &[("Save".to_string(), "保存".to_string())],
                &KEY
            )
            .await,
            0
        );
    }

    /// C-11 核心断言：写入后库里不能有任何列残留原文明文
    #[tokio::test]
    async fn 原文不落明文只有hmac查询键() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        remember(
            &pool,
            "m",
            &[("Secret Browsing Text".to_string(), "译文".to_string())],
            &KEY,
        )
        .await;
        // 1. source_hash 列 = HMAC(key, 归一化原文) 的 hex，且不等于明文
        let (source_hash, target): (String, String) = sqlx::query_as(
            "SELECT source_hash, target FROM translate_memory WHERE model='m'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(target, "译文");
        assert_eq!(source_hash, source_hash_hex(&KEY, "Secret Browsing Text"));
        assert_ne!(source_hash, "Secret Browsing Text");
        // 2. 整表逐列扫一遍：明文不得出现在任何列（防未来加列回归）
        let rows: Vec<(String, String, String)> =
            sqlx::query_as("SELECT model, source_hash, target FROM translate_memory")
                .fetch_all(&pool)
                .await
                .unwrap();
        for (model, hash, tgt) in rows {
            for col in [model, hash, tgt] {
                assert!(
                    !col.contains("Secret Browsing Text") && !col.contains("Secret"),
                    "原文明文泄漏到库：{col}"
                );
            }
        }
        // 3. 换密钥后同原文不得命中（键控 HMAC 的隔离语义）
        let wrong_key = [0u8; 32];
        assert!(
            lookup(&pool, "m", &["Secret Browsing Text".to_string()], &wrong_key)
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn 重复写入覆盖译文并累加命中() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        remember(&pool, "m", &[("Open".to_string(), "打开".to_string())], &KEY).await;
        remember(&pool, "m", &[("Open".to_string(), "开启".to_string())], &KEY).await;
        let got = lookup(&pool, "m", &["Open".to_string()], &KEY).await;
        assert_eq!(
            got.get("Open"),
            Some(&"开启".to_string()),
            "新译文覆盖旧译文"
        );
        let hits: i64 = sqlx::query_scalar(
            "SELECT hits FROM translate_memory WHERE model='m' AND source_hash=?",
        )
        .bind(source_hash_hex(&KEY, "Open"))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(hits, 2);
    }

    #[tokio::test]
    async fn 超限裁剪保留高命中条目() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        // 直插超限旧条目，其中一条刷高命中
        for i in 0..(super::MEMORY_KEEP + 5) {
            sqlx::query(
                "INSERT INTO translate_memory (model, source_hash, target) VALUES ('m', ?, 'x')",
            )
            .bind(source_hash_hex(&KEY, &format!("old-{i}")))
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("UPDATE translate_memory SET hits = 99 WHERE source_hash = ?")
            .bind(source_hash_hex(&KEY, "old-0"))
            .execute(&pool)
            .await
            .unwrap();
        // 记一条触发裁剪
        remember(&pool, "m", &[("New".to_string(), "新".to_string())], &KEY).await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM translate_memory")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, super::MEMORY_KEEP);
        // 高命中与新条目保留
        let kept: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM translate_memory WHERE source_hash IN (?, ?)",
        )
        .bind(source_hash_hex(&KEY, "old-0"))
        .bind(source_hash_hex(&KEY, "New"))
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(kept, 2);
    }

    #[tokio::test]
    async fn 空译文与超长原文不入库() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        let long = "a".repeat(MAX_SOURCE_CHARS + 1);
        let n = remember(
            &pool,
            "m",
            &[
                ("Empty".to_string(), "   ".to_string()),
                (long, "译文".to_string()),
            ],
            &KEY,
        )
        .await;
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn 大批查询自动分块不丢项() {
        let dir = tempfile_like_dir();
        let pool = test_pool(&dir).await;
        let pairs: Vec<(String, String)> = (0..(LOOKUP_CHUNK * 2 + 7))
            .map(|i| (format!("Text {i}"), format!("译文{i}")))
            .collect();
        remember(&pool, "m", &pairs, &KEY).await;
        let keys: Vec<String> = pairs.iter().map(|(s, _)| s.clone()).collect();
        let got = lookup(&pool, "m", &keys, &KEY).await;
        assert_eq!(got.len(), keys.len(), "分块查询后条数必须守恒");
    }

    /// 测试用临时目录（每个用例唯一，避免串库）。测试结束靠 /tmp 定期清理，
    /// 与项目里其他 db 测试的做法一致。
    fn tempfile_like_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sb_tm_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
