// 资讯订阅（内置 RSS 源，从原版 ShiftBrowser 迁移）
// 架构：62 个内置源（6 大类）→ tokio 并发抓取 → feed_items 表缓存
// → 启动页"📰 资讯"面板渲染最新条目。30 分钟刷一轮，抓完重新生成启动页。
use crate::db::Db;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

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
    #[serde(default)]
    d: String,
}

fn load_sources() -> Vec<(String, String, String)> {
    let cats: Vec<FeedCategory> = serde_json::from_str(FEEDS_JSON).unwrap_or_default();
    cats.into_iter()
        .flat_map(|c| {
            c.feeds
                .into_iter()
                .map(move |f| (f.t, f.u, c.c.clone()))
        })
        .collect()
}

/// 剥 CDATA 并解码常见 HTML 实体
fn clean_text(s: &str) -> String {
    let s = s.trim();
    let s = if s.starts_with("<![CDATA[") && s.ends_with("]]>") {
        s[9..s.len() - 3].trim()
    } else {
        s
    };
    s.replace("<![CDATA[", "")
        .replace("]]>", "")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#038;", "&")
        .trim()
        .to_string()
}

/// 取 <tag ...>内容</tag> 的内容（找不到返回 None）
fn extract_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let start = block.find(&open)?;
    let gt = block[start..].find('>')? + start;
    // 自闭合（如 atom <link href="..."/>）无内容
    if block[gt - 1..gt + 1].ends_with("/>") {
        return None;
    }
    let close = format!("</{}>", tag);
    let end = block[gt + 1..].find(&close)? + gt + 1;
    Some(block[gt + 1..end].to_string())
}

/// 解析一个 <item>/<entry> 块 → (title, link, published)
fn parse_entry(block: &str) -> Option<(String, String, String)> {
    let title = clean_text(&extract_tag(block, "title")?);
    if title.is_empty() {
        return None;
    }
    // RSS: <link>url</link>；Atom: <link href="url"/>
    let link = extract_tag(block, "link")
        .map(|s| clean_text(&s))
        .filter(|s| s.starts_with("http"))
        .or_else(|| {
            let open = block.find("<link")?;
            let seg = &block[open..block[open..].find('>')? + open + 1];
            let i = seg.find("href=\"")? + 6;
            let j = seg[i..].find('"')? + i;
            let url = clean_text(&seg[i..j]);
            url.starts_with("http").then_some(url)
        })?;
    let published = ["pubDate", "published", "updated", "date"]
        .iter()
        .find_map(|t| extract_tag(block, t).map(|s| clean_text(&s)))
        .unwrap_or_default();
    Some((title, link, published))
}

/// 解析 RSS/Atom 全文 → [(title, link, published)]
pub fn parse_feed(xml: &str) -> Vec<(String, String, String)> {
    let mut items = Vec::new();
    for tag in ["<item>", "<item ", "<entry>", "<entry "] {
        let mut search_from = 0;
        while let Some(rel) = xml[search_from..].find(tag) {
            let start = search_from + rel;
            let close_tag = if tag.starts_with("<item") { "</item>" } else { "</entry>" };
            let Some(rel_end) = xml[start..].find(close_tag) else {
                break;
            };
            let block = &xml[start..rel_end + start];
            if let Some(t) = parse_entry(block) {
                items.push(t);
            }
            search_from = start + close_tag.len();
        }
    }
    items
}

/// 抓单个源并入库；返回条目数
async fn fetch_one(client: &reqwest::Client, db: &Db, source: &str, url: &str, category: &str) -> usize {
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            println!("[feed] {} 请求失败: {}", source, e);
            return 0;
        }
    };
    let xml = match resp.text().await {
        Ok(t) => t,
        Err(e) => {
            println!("[feed] {} 读取失败: {}", source, e);
            return 0;
        }
    };
    let items = parse_feed(&xml);
    let mut count = 0;
    // 单事务批量写：避免 62 源并发逐条 INSERT 抢 SQLite 写锁
    let mut tx = match db.pool.begin().await {
        Ok(t) => t,
        Err(_) => return 0,
    };
    for (title, link, published) in items.iter().take(50) {
        let r = sqlx::query(
            "INSERT INTO feed_items (source, category, title, url, published_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(url) DO UPDATE SET title = excluded.title",
        )
        .bind(source)
        .bind(category)
        .bind(title)
        .bind(link)
        .bind(published)
        .execute(&mut *tx)
        .await;
        if r.is_ok() {
            count += 1;
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
    let _ = tx.commit().await;
    count
}

/// 全量刷新（并发 62 源，单请求 15s 超时）
pub async fn refresh_all(db: &Db) -> usize {
    let sources = load_sources();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("ShiftBrowser/5.6 (RSS reader)")
        .build()
        .unwrap_or_default();
    let mut set = tokio::task::JoinSet::new();
    for (name, url, category) in sources {
        let client = client.clone();
        let pool = db.pool.clone();
        set.spawn(async move {
            let db_owned = Db { pool };
            fetch_one(&client, &db_owned, &name, &url, &category).await
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

/// 启动页渲染用的最新条目（混源，按抓取时间倒序）
pub async fn latest_items(db: &Db, limit: i64) -> Vec<(String, String, String, String, String)> {
    sqlx::query_as(
        "SELECT source, category, title, url, published_at FROM feed_items ORDER BY id DESC LIMIT ?",
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
                println!("[feed] 开始刷新内置资讯源…");
                refresh_all(&db).await
            };
            println!("[feed] 刷新完成，共 {} 条", n);
            // 重新生成新标签页（资讯面板数据更新）
            if let Some(path) = crate::newtab::newtab_path(&app) {
                let db = app.state::<Db>();
                let _ = crate::newtab::generate(&db, &path).await;
            }
            tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
        }
    });
}
