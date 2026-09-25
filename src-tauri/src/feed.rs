// 资讯订阅（内置 RSS 源，从原版 ShiftBrowser 迁移）
// 架构：62 个内置源（6 大类）→ tokio 并发抓取 → feed_items 表缓存
// → 启动页"📰 资讯"面板渲染最新条目。30 分钟刷一轮，抓完重新生成启动页。
use crate::db::Db;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

// RSS/Atom 解析纯函数（R1 拆分 2026-09-25）
#[path = "feed/parse.rs"]
mod parse;
use parse::{decode_feed_bytes, parse_feed};

/// 内置源列表（原版精选）
const FEEDS_JSON: &str = include_str!("../resources/feeds.json");

#[derive(Deserialize)]
struct FeedCategory {
    c: String,
    feeds: Vec<FeedSource>,
}

#[derive(Deserialize)]
struct FeedSource {
    t: String,
    u: String,
}

fn load_sources() -> Vec<(String, String, String)> {
    let cats: Vec<FeedCategory> = serde_json::from_str(FEEDS_JSON).unwrap_or_default();
    cats.into_iter()
        .flat_map(|c| c.feeds.into_iter().map(move |f| (f.t, f.u, c.c.clone())))
        .collect()
}

/// 抓单个源并入库；返回条目数
async fn fetch_one(
    client: &reqwest::Client,
    db: &Db,
    source: &str,
    url: &str,
    category: &str,
) -> usize {
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            log::error!("[feed] {} 请求失败: {}", source, e);
            return 0;
        }
    };
    // 先取响应头 charset（bytes() 会消费 resp）
    let charset = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| {
            ct.split(';').find_map(|p| {
                let p = p.trim();
                p.strip_prefix("charset=")
                    .map(|c| c.trim().trim_matches('"').to_string())
            })
        });
    // P2：流式读取 + 边读边计数——响应体上限 4MB。
    // 不能用 resp.bytes() 先读全再检查：恶意/被攻破的 feed 服务器配合 15s 超时
    // 窗口可高速灌入数百 MB~GB 数据（内存炸弹），事后检查挡不住下载过程的内存峰值。
    // 边读边计数，超限立即中止读取并断开。
    use futures_util::StreamExt;
    let max_bytes = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(c) => {
                bytes.extend_from_slice(&c);
                if bytes.len() > max_bytes {
                    log::warn!("[feed] {} 响应体超过 4MB，中止读取", source);
                    return 0;
                }
            }
            Err(e) => {
                log::error!("[feed] {} 读取失败: {}", source, e);
                return 0;
            }
        }
    }
    let xml = decode_feed_bytes(&bytes, charset.as_deref());
    let items = parse_feed(&xml);
    let mut count = 0;
    // 单事务批量写：避免 62 源并发逐条 INSERT 抢 SQLite 写锁
    let mut tx = match db.pool.begin().await {
        Ok(t) => t,
        Err(_) => return 0,
    };
    // 性能 P1：批量 INSERT（QueryBuilder 一次多值），替代逐条 execute
    // 原 50 条 × 62 源 ≈ 3100 次往返 → 现每源 1 次
    {
        use sqlx::QueryBuilder;
        let batch: Vec<_> = items.iter().take(50).collect();
        if !batch.is_empty() {
            let mut qb: QueryBuilder<sqlx::Sqlite> = QueryBuilder::new(
                "INSERT INTO feed_items (source, category, title, url, published_at) ",
            );
            qb.push_values(batch.iter(), |mut b, (title, link, published)| {
                b.push_bind(source)
                    .push_bind(category)
                    .push_bind(title)
                    .push_bind(link)
                    .push_bind(published);
            });
            qb.push(" ON CONFLICT(url) DO UPDATE SET title = excluded.title");
            if qb.build().execute(&mut *tx).await.is_ok() {
                count = batch.len();
            }
        }
    }
    // 每源只留最新 50 条
    let _ = sqlx::query(
        "DELETE FROM feed_items WHERE source = ? AND id NOT IN (SELECT id FROM feed_items WHERE source = ? ORDER BY id DESC LIMIT 50)",
    )
    .bind(source)
    .bind(source)
    .execute(&mut *tx)
    .await;
    // 提交失败要如实上报，不能把未落库的数据当成功（2026-09-19 修复）。
    // commit 失败时事务已结束、连接归还池自动回滚，无需（也无法）再显式 rollback
    if let Err(e) = tx.commit().await {
        log::error!("[feed] {} 事务提交失败: {}", source, e);
        return 0;
    }
    count
}

/// 全量刷新（并发抓取，单请求 15s 超时）
/// 并发限流：信号量压到 16 路——62 源全开 + 失败重试翻倍会与 UI 命令抢 SQLite
/// 连接池（5 连接）和写锁，导致启动后半小时内历史/会话保存偶发卡顿
pub async fn refresh_all(db: &Db) -> usize {
    let sources = load_sources();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(crate::contracts::timeout::FEED_FETCH_SECS))
        .user_agent(format!(
            "ShiftBrowser/{} (RSS reader)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .unwrap_or_default();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
    let mut set = tokio::task::JoinSet::new();
    for (name, url, category) in sources {
        let client = client.clone();
        let feed_pool = db.feed_pool.clone();
        let sem = sem.clone();
        set.spawn(async move {
            // 信号量只在被 close 时 acquire 失败（本应用不会发生），失败直接放弃该源
            let Ok(_permit) = sem.acquire_owned().await else {
                return 0;
            };
            // 性能 P1：fetch_one 用独立 feed_pool（与 UI 命令的 pool 隔离，避免抢锁）
            let db_owned = Db {
                pool: feed_pool.clone(),
                feed_pool,
                key_path: std::path::PathBuf::new(),
                vault_path: std::path::PathBuf::new(),
                master_key: tokio::sync::Mutex::new(None),
            };
            let n = fetch_one(&client, &db_owned, &name, &url, &category).await;
            if n == 0 {
                // 失败重试一次：anyfeeder 偶发限流/网络抖动会让单个源整轮丢失，
                // 等 30 分钟下一轮太久，延迟 2 秒补抓一次
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                log::warn!("[feed] {} 首次抓取失败，2 秒后重试", name);
                fetch_one(&client, &db_owned, &name, &url, &category).await
            } else {
                n
            }
        });
    }
    let mut total = 0;
    while let Some(r) = set.join_next().await {
        if let Ok(n) = r {
            total += n;
        }
    }
    total
}

/// 启动页渲染用的最新条目（混源，按抓取时间倒序，按 url 去重——转载文章不重复显示）
pub async fn latest_items(db: &Db, limit: i64) -> Vec<(String, String, String, String, String)> {
    sqlx::query_as(
        "SELECT source, category, title, url, published_at FROM feed_items
         WHERE id IN (SELECT MAX(id) FROM feed_items GROUP BY url)
         ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&db.pool)
    .await
    .unwrap_or_default()
}

/// 调度器：启动 6 秒后首轮，之后每 30 分钟一轮；每轮完成后重新生成新标签页
pub fn start_scheduler(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        loop {
            let n = {
                let db = app.state::<Db>();
                log::info!("[feed] 开始刷新内置资讯源…");
                refresh_all(&db).await
            };
            log::info!("[feed] 刷新完成，共 {} 条", n);
            // 重新生成新标签页（资讯面板数据更新）
            if let Some(path) = crate::newtab::newtab_path(&app) {
                let db = app.state::<Db>();
                let _ = crate::newtab::generate(&db, &path).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
        }
    });
}

