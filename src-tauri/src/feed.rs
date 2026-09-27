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

/// 单跳建连：先解析校验公网，再把地址 pin 死（防校验后 DNS 翻转）。
///
/// - IP 字面量：判公网后直连（无 DNS 面可翻转）
/// - 域名：只解析这一次，全部结果校验后 pin 给 reqwest，建连不再二次解析
///
/// 与 reader/fetch.rs 的 curl --resolve 同口径，ai 网关的 resolve_to_addrs 同机制。
async fn pinned_client_for(url: &str) -> Result<reqwest::Client, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("只支持 http/https 订阅源".to_string());
    }
    let parsed = url::Url::parse(url).map_err(|_| "URL 不合法".to_string())?;
    let raw = parsed
        .host_str()
        .ok_or_else(|| "URL 缺少主机名".to_string())?;
    let host = raw
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::FEED_FETCH_SECS,
        ))
        .user_agent(format!(
            "ShiftBrowser/{} (RSS reader)",
            env!("CARGO_PKG_VERSION")
        ))
        .redirect(reqwest::redirect::Policy::none());
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if crate::ai::is_private_ip(ip) {
            return Err(format!("订阅源指向内网/保留地址，已拦截：{}", host));
        }
    } else {
        let sock: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| format!("DNS 解析失败: {} ({})", host, e))?
            .collect();
        let addrs = crate::ai::check_resolved_ips(&host, sock.iter().map(|s| s.ip()), port)?;
        builder = builder.resolve_to_addrs(&host, &addrs);
    }
    builder
        .build()
        .map_err(|e| format!("HTTP 客户端创建失败: {}", e))
}

/// 抓单个源并入库；返回条目数
async fn fetch_one(db: &Db, source: &str, url: &str, category: &str) -> usize {
    // 手动跟随重定向（最多 5 跳）：reqwest 自动跟随会在复核前把请求打到内网，
    // 这里每跳先建 pin 死的连接再发请求，内网跳转连第一个字节都发不出去
    let mut cur = url.to_string();
    let mut hops = 0u8;
    let resp = loop {
        let hop_client = match pinned_client_for(&cur).await {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[feed] {} SSRF 拦截: {}", source, e);
                return 0;
            }
        };
        let r = match hop_client.get(&cur).send().await {
            Ok(r) => r,
            Err(e) => {
                log::error!("[feed] {} 请求失败: {}", source, e);
                return 0;
            }
        };
        let status = r.status().as_u16();
        if (300..400).contains(&status) {
            let loc = r
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            if loc.is_empty() {
                log::warn!("[feed] {} 重定向无 Location: HTTP {}", source, status);
                return 0;
            }
            match url::Url::parse(&cur)
                .ok()
                .and_then(|base| base.join(&loc).ok())
            {
                Some(next) => {
                    hops += 1;
                    if hops > 5 {
                        log::warn!("[feed] {} 重定向过多，中止", source);
                        return 0;
                    }
                    cur = next.to_string();
                    continue;
                }
                None => {
                    log::warn!("[feed] {} 重定向地址非法: {}", source, loc);
                    return 0;
                }
            }
        }
        if !(200..300).contains(&status) {
            log::error!("[feed] {} 抓取失败: HTTP {}", source, status);
            return 0;
        }
        break r;
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
/// 并发 6 路：feed_pool 仅 2 连接 + SQLite 单写锁，16 路排队超 10 秒直接报错回零；
/// 降到 6 路后排队可消化。失败重试带 2-5 秒抖动，避免多源同时失败同时重试打群架。
/// 连接说明：每跳自建 pin 死的客户端（见 pinned_client_for），无共享连接池——
/// 订阅 30 分钟一轮，跨轮复用池子早已空闲回收，同轮同 host 复用收益极小，不值得为此留自动跟随
pub async fn refresh_all(db: &Db) -> usize {
    let sources = load_sources();
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(6));
    let mut set = tokio::task::JoinSet::new();
    for (name, url, category) in sources {
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
            let n = fetch_one(&db_owned, &name, &url, &category).await;
            if n == 0 {
                // 失败重试一次：anyfeeder 偶发限流/网络抖动会让单个源整轮丢失，
                // 等 30 分钟下一轮太久，补抓一次；延迟 2-5 秒抖动（纳秒取模，
                // 不为抖动引入 rand 依赖），避免多源同时失败同时重试打群架
                let jitter = 2
                    + (std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() as u64 % 4)
                        .unwrap_or(0));
                tokio::time::sleep(std::time::Duration::from_secs(jitter)).await;
                log::warn!("[feed] {} 首次抓取失败，{} 秒后重试", name, jitter);
                fetch_one(&db_owned, &name, &url, &category).await
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
        tokio::time::sleep(std::time::Duration::from_secs(
            crate::contracts::timeout::FEED_INITIAL_DELAY_SECS,
        ))
        .await;
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
            tokio::time::sleep(std::time::Duration::from_secs(
                crate::contracts::timeout::FEED_LOOP_SECS,
            ))
            .await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// pin 建连内网直接拒（无网络也测：字面量走纯本地判定）
    #[tokio::test]
    async fn pinned_client_rejects_intranet_literals() {
        for url in [
            "http://127.0.0.1/",
            "http://10.0.0.5/rss",
            "http://192.168.1.1:8080/feed",
            "http://169.254.169.254/latest",
            "http://[::1]/",
        ] {
            assert!(pinned_client_for(url).await.is_err(), "{url} 必须被拦截");
        }
    }

    /// 非 http/https 直接拒
    #[tokio::test]
    async fn pinned_client_rejects_non_http() {
        assert!(pinned_client_for("ftp://example.com/rss").await.is_err());
        assert!(pinned_client_for("file:///etc/passwd").await.is_err());
        assert!(pinned_client_for("not-a-url").await.is_err());
    }
}
