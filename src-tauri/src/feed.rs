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
    // 自闭合（如 <link href="..."/>、<link href="..." />）无内容
    if block[..gt].trim_end().ends_with('/') {
        return None;
    }
    let close = format!("</{}>", tag);
    let end = block[gt + 1..].find(&close)? + gt + 1;
    Some(block[gt + 1..end].to_string())
}

/// 从 <link .../> 标签段提取 href 属性（支持单/双引号）
fn extract_href(seg: &str) -> Option<String> {
    for q in ['"', '\''] {
        let pat = format!("href={}", q);
        if let Some(i) = seg.find(&pat) {
            let val_start = i + pat.len();
            if let Some(j) = seg[val_start..].find(q) {
                return Some(seg[val_start..val_start + j].to_string());
            }
        }
    }
    None
}

/// 提取条目链接：RSS <link>url</link> 或 Atom <link href="url"/>。
/// 块内可能有多个 <link>（rel="self"/rel="alternate"/正文链接），
/// 遍历取第一个 http 开头的，避免拿到 feed 自身地址。
fn extract_link(block: &str) -> Option<String> {
    let mut rest = block;
    while let Some(rel) = rest.find("<link") {
        let start = rel;
        let Some(gt_rel) = rest[start..].find('>') else {
            break;
        };
        let gt = start + gt_rel;
        if rest[..gt].trim_end().ends_with('/') {
            // Atom 风格：<link href="..."/>
            let seg = &rest[start..gt + 1];
            // 跳过 rel="self"（feed 自身地址），继续找正文链接
            if seg.contains("rel=\"self\"") || seg.contains("rel='self'") {
                rest = &rest[gt + 1..];
                continue;
            }
            if let Some(href) = extract_href(seg) {
                let clean = clean_text(&href);
                if clean.starts_with("http") {
                    return Some(clean);
                }
            }
        } else {
            // RSS 风格：<link>url</link>
            let Some(close_rel) = rest[gt + 1..].find("</link>") else {
                break;
            };
            let end = gt + 1 + close_rel;
            let url = clean_text(&rest[gt + 1..end]);
            if url.starts_with("http") {
                return Some(url);
            }
        }
        rest = &rest[gt + 1..];
    }
    None
}

/// 解析一个 <item>/<entry> 块 → (title, link, published)
fn parse_entry(block: &str) -> Option<(String, String, String)> {
    let title = clean_text(&extract_tag(block, "title")?);
    if title.is_empty() {
        return None;
    }
    let link = extract_link(block)?;
    // 时间字段兼容多种命名空间：RSS pubDate / Atom published/updated / DC date
    let published = ["pubDate", "published", "updated", "date", "dc:date"]
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
    // 用 bytes + lossy 解码：部分源是 GBK/其他编码，text() 会直接失败，
    // lossy 至少能解析出结构（非 UTF-8 字符显示为 �，比整源失败强）
    let xml = match resp.bytes().await {
        Ok(b) => String::from_utf8_lossy(&b).to_string(),
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
            // fetch_one 只使用 pool，key_path 用临时空值（密码相关操作不会走到这里）
            let db_owned = Db { pool, key_path: std::path::PathBuf::new() };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rss_item() {
        let xml = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
<item>
  <title>测试标题 &amp; 实体</title>
  <link>https://example.com/a?x=1</link>
  <pubDate>Mon, 01 Jan 2026 00:00:00 GMT</pubDate>
</item>
<item><title>第二条</title><link>https://example.com/b</link><pubDate>2026-01-02</pubDate></item>
</channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "测试标题 & 实体");
        assert_eq!(items[0].1, "https://example.com/a?x=1");
        assert_eq!(items[0].2, "Mon, 01 Jan 2026 00:00:00 GMT");
        assert_eq!(items[1].1, "https://example.com/b");
    }

    #[test]
    fn parse_atom_entry_with_self_link() {
        // 第一个 link 是 rel="self"（feed 自身地址），应跳过取正文链接
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
<entry>
  <title><![CDATA[Atom 标题]]></title>
  <link rel="self" href="https://example.com/feed.xml"/>
  <link rel="alternate" href="https://example.com/post/1"/>
  <updated>2026-03-01T10:00:00Z</updated>
</entry>
</feed>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, "Atom 标题");
        assert_eq!(items[0].1, "https://example.com/post/1");
        assert_eq!(items[0].2, "2026-03-01T10:00:00Z");
    }

    #[test]
    fn parse_dc_date_and_single_quote_href() {
        let xml = r#"<rss version="2.0"><channel>
<item>
  <title>DC 日期源</title>
  <link href='https://example.com/c' />
  <dc:date>2026-05-05T08:00:00+08:00</dc:date>
</item>
</channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1, "https://example.com/c");
        assert_eq!(items[0].2, "2026-05-05T08:00:00+08:00");
    }

    #[test]
    fn parse_rss_link_with_attributes() {
        // RSS 里带属性的 <link rel="alternate" type="text/html">url</link>
        let xml = r#"<rss version="2.0"><channel>
<item>
  <title>带属性 link</title>
  <link rel="alternate" type="text/html">https://example.com/d</link>
</item>
</channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].1, "https://example.com/d");
    }

    #[test]
    fn skip_empty_and_self_closed_items() {
        let xml = r#"<rss version="2.0"><channel>
<item><title>正常</title><link>https://example.com/e</link></item>
<item/>
<item><title></title><link>https://example.com/f</link></item>
</channel></rss>"#;
        let items = parse_feed(xml);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].0, "正常");
    }
}
