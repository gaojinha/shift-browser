//! RSS/Atom 解析纯函数（R1 拆分：从 feed.rs 抽出，逻辑零改动）
//!
//! 手写 XML 解析：CDATA 剥离、HTML 实体解码、RSS/Atom 双风格 link 提取、
//! 编码自动探测（BOM → 显式 charset → 严格 UTF-8 → GBK 兜底）。

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
pub(crate) fn parse_feed(xml: &str) -> Vec<(String, String, String)> {
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
pub(crate) fn decode_feed_bytes(bytes: &[u8], charset: Option<&str>) -> String {
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

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;
