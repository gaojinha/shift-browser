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
        .flat_map(|c| c.feeds.into_iter().map(move |f| (f.t, f.u, c.c.clone())))
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
    // 质量过滤：标题过短（<2 字符）或纯标点/空白（无字母数字）的剔除
    if title.len() < 2 || !title.chars().any(|c| c.is_alphanumeric()) {
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
    // P3：硬上限 200 条——调用方只取 50 条入库，恶意/损坏 feed 塞上千条目
    // 只会白烧 CPU 解析，提前截断
    'outer: for tag in ["<item>", "<item ", "<entry>", "<entry "] {
        let mut search_from = 0;
        while let Some(rel) = xml[search_from..].find(tag) {
            let start = search_from + rel;
            let close_tag = if tag.starts_with("<item") {
                "</item>"
            } else {
                "</entry>"
            };
            let Some(rel_end) = xml[start..].find(close_tag) else {
                break;
            };
            let block = &xml[start..rel_end + start];
            if let Some(t) = parse_entry(block) {
                items.push(t);
                if items.len() >= 200 {
                    break 'outer;
                }
            }
            search_from = start + close_tag.len();
        }
    }
    items
}

/// 解码 feed 响应字节为字符串（自动探测编码）：
/// BOM 优先 → 显式 charset → 严格 UTF-8 → GBK 兜底（中文源最常见）
fn decode_feed_bytes(bytes: &[u8], charset: Option<&str>) -> String {
    // BOM
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).to_string();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE
            .decode_without_bom_handling(&bytes[2..])
            .0
            .to_string();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE
            .decode_without_bom_handling(&bytes[2..])
            .0
            .to_string();
    }
    // 响应头显式 charset
    if let Some(cs) = charset {
        if let Some(enc) = encoding_rs::Encoding::for_label(cs.as_bytes()) {
            let (text, _, _) = enc.decode(bytes);
            return text.to_string();
        }
    }
    // 严格 UTF-8（能过说明是标准 UTF-8）
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // 非 UTF-8：GBK 兜底（中文源最常见）
    let (text, _, _) = encoding_rs::GBK.decode(bytes);
    text.to_string()
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

/// 全量刷新（并发抓取，单请求 15s 超时）
/// 并发限流：信号量压到 16 路——62 源全开 + 失败重试翻倍会与 UI 命令抢 SQLite
/// 连接池（5 连接）和写锁，导致启动后半小时内历史/会话保存偶发卡顿
pub async fn refresh_all(db: &Db) -> usize {
    let sources = load_sources();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
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
        let pool = db.pool.clone();
        let sem = sem.clone();
        set.spawn(async move {
            // 信号量只在被 close 时 acquire 失败（本应用不会发生），失败直接放弃该源
            let Ok(_permit) = sem.acquire_owned().await else {
                return 0;
            };
            // fetch_one 只使用 pool，key_path 用临时空值（密码相关操作不会走到这里）
            let db_owned = Db {
                pool,
                key_path: std::path::PathBuf::new(),
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

    #[test]
    fn parse_feed_caps_at_200_items() {
        // P3：恶意/损坏 feed 塞上千条目时提前截断，防白烧 CPU
        let mut xml = String::from("<rss version=\"2.0\"><channel>");
        for i in 0..250 {
            xml.push_str(&format!(
                "<item><title>条目{}</title><link>https://example.com/{}</link></item>",
                i, i
            ));
        }
        xml.push_str("</channel></rss>");
        let items = parse_feed(&xml);
        assert_eq!(items.len(), 200);
    }

    #[test]
    fn decode_utf8_and_bom() {
        let s = "中文标题";
        assert_eq!(decode_feed_bytes(s.as_bytes(), None), s);
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice(s.as_bytes());
        assert_eq!(decode_feed_bytes(&bom, None), s);
    }

    #[test]
    fn decode_gbk_without_charset() {
        // GBK 编码无 charset 声明 → 严格 UTF-8 失败 → GBK 兜底
        let (gbk, _, _) = encoding_rs::GBK.encode("中文标题");
        assert_eq!(decode_feed_bytes(&gbk, None), "中文标题");
    }

    #[test]
    fn decode_with_explicit_charset() {
        // GBK 字节 + charset=gbk 显式声明
        let (gbk, _, _) = encoding_rs::GBK.encode("标题");
        assert_eq!(decode_feed_bytes(&gbk, Some("gbk")), "标题");
        // charset 大小写/引号容忍
        let (gbk, _, _) = encoding_rs::GBK.encode("测试");
        assert_eq!(decode_feed_bytes(&gbk, Some("GBK")), "测试");
    }
}
