//! feed/parse.rs 单测（R1 拆分：从 feed.rs 外置，用例零改动）

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
