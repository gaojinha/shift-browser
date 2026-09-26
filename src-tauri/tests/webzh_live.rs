//! P3b 真机验收：整页汉化 · 翻译链路
//!
//! 打本机 opencode（4096），用**被测代码本身**（`webzh` 的 prompt 构造 +
//! `webqa::ask_once`）跑通「分段 → prompt → 收答案 → 解析 JSON → 拼回整段」。
//!
//! 关注点跟纯单测不同：单测验的是"给定这段脏输出能不能解析出来"，
//! 这里验的是"真的去问模型，它到底吐什么形状的东西"——LLM 输出格式的
//! 现实脏度只有连上才知道，纯猜是猜不出来的。
//!
//! 跑法：`cargo test --test webzh_live -- --ignored --nocapture --test-threads=1`

use shift_browser_lib::webqa;
use shift_browser_lib::webzh;
use shift_browser_lib::webzh::BatchItem;

/// 从 /proc 扫 opencode serve 进程环境拿凭据。
/// 规则与 `webqa::credentials_from_pid` 一致：cmdline 里同时含 opencode+serve
/// 才算，再用 environ 取账号密码。
fn discover() -> Option<(String, String)> {
    let entries = std::fs::read_dir("/proc").ok()?;
    for entry in entries.flatten() {
        let pid = entry.path();
        // 关键：这里必须 `continue` 不能用 `?`。
        // /proc 下大量进程（其他用户的、内核线程的、权限受限的）我们读不到
        // cmdline/environ，用 `?` 会在第一个读不上的条目就终止**整个遍历**，
        // 后面真正该命中的 serve 进程再也走不到。踩过一次。
        let cmd = std::fs::read_to_string(pid.join("cmdline")).unwrap_or_default();
        if !cmd.contains("opencode") || !cmd.contains("serve") {
            continue;
        }
        let env = std::fs::read_to_string(pid.join("environ")).unwrap_or_default();
        let mut u = None;
        let mut p = None;
        for kv in env.split('\0') {
            if let Some(v) = kv.strip_prefix("OPENCODE_SERVER_USERNAME=") {
                u = Some(v.to_string());
            }
            if let Some(v) = kv.strip_prefix("OPENCODE_SERVER_PASSWORD=") {
                p = Some(v.to_string());
            }
        }
        if let (Some(u), Some(p)) = (u, p) {
            return Some((u, p));
        }
    }
    None
}

fn creds() -> shift_browser_lib::webqa::Credentials {
    let (u, p) = discover().expect(
        "未发现 opencode 凭据：本机需跑着带 OPENCODE_SERVER_PASSWORD 的 `opencode serve`",
    );
    shift_browser_lib::webqa::Credentials { username: u, password: p }
}

async fn ask(items: &[BatchItem], title: &str) -> Vec<BatchItem> {
    let http = webqa::client_pub(&creds()).expect("建 HTTP 客户端");
    let base = webqa::base_url_pub();
    let (sid, _baseline) = webqa::ensure_session_pub(&http, None, "网页汉化live")
        .await
        .expect("建汉化会话");
    let prompt = webzh::build_translate_prompt_pub(items, Some(title));
    let raw = webqa::ask_once(&http, &base, &sid, webqa::DEFAULT_AGENT, &prompt, 120)
        .await
        .expect("发翻译请求");
    println!("---- 模型原始输出（前 600 字）----\n{:.600}\n----", raw);
    webzh::parse_translate_response_pub(&raw, items)
}

#[tokio::test]
#[ignore]
async fn live_真机翻译一批能解析出id和译文() {
    let items = vec![
        BatchItem { id: "t0".into(), text: "Submit".into() },
        BatchItem { id: "t1".into(), text: "Cancel".into() },
        BatchItem { id: "t2".into(), text: "Sign in to your account".into() },
        BatchItem { id: "t3".into(), text: "The server returned an error".into() },
    ];
    let got = ask(&items, "登录页").await;
    println!("解析出 {} 条：", got.len());
    for g in &got {
        println!("  [{}] {}", g.id, g.text);
    }
    assert!(!got.is_empty(), "一条都没解析出来：模型输出格式和预期差太远");
    for g in &got {
        assert!(!g.id.is_empty() && !g.text.is_empty());
        assert!(items.iter().any(|i| i.id == g.id), "出现了不在本批的 id");
    }
    // 至少一半要真的翻成中文
    let zh = got.iter().filter(|g| g.text.chars().any(|c| c as u32 >= 0x4e00 && c as u32 <= 0x9fff)).count();
    assert!(zh * 2 >= got.len(), "只有 {zh}/{} 条有中文，翻译质量存疑", got.len());
}

#[tokio::test]
#[ignore]
async fn live_超长段切分后能翻回来() {
    // 高哥 2026-09-26 提的"超大文件可以分段翻译"：验切分→翻译→拼接闭环
    let long = "The quick brown fox jumps over the lazy dog. ".repeat(60); // ~2640 字符
    println!("原段 {} 字符", long.chars().count());
    let pieces = webzh::split_long_text_pub(&long, 1000);
    println!("切成 {} 块：", pieces.len());
    for (i, p) in pieces.iter().enumerate() {
        println!("  [{i}] {} 字符，末尾 {:?}", p.chars().count(), p.chars().last());
    }
    assert!(pieces.len() >= 3, "应切成多块");
    assert_eq!(pieces.concat(), long, "切分拼回必须等于原文");

    // 只翻第一块（省时间），验子块 id 与译文对应
    let items: Vec<BatchItem> = pieces
        .iter()
        .enumerate()
        .map(|(i, p)| BatchItem { id: format!("t0.{i}"), text: p.clone() })
        .collect();
    let got = ask(&items, "测试文档").await;
    assert!(!got.is_empty(), "子块翻译一条都没出来");
    assert!(got.iter().all(|g| g.id.starts_with("t0.")));
    println!("子块翻译 {} / {} 条", got.len(), items.len());
}

#[test]
#[ignore]
fn live_脏输出能救回来() {
    // 直接喂真实常见的脏形状，确认解析层顶得住
    let its = vec![
        BatchItem { id: "t0".into(), text: "Save".into() },
        BatchItem { id: "t1".into(), text: "Open".into() },
    ];
    let cases = vec![
        ("裸数组", r#"[{"id":"t0","t":"保存"},{"id":"t1","t":"打开"}]"#),
        ("裹代码块", "```json\n[{\"id\":\"t0\",\"t\":\"保存\"},{\"id\":\"t1\",\"t\":\"打开\"}]\n```"),
        ("带寒暄", "好的，翻译如下：\n[{\"id\":\"t0\",\"t\":\"保存\"},{\"id\":\"t1\",\"t\":\"打开\"}]\n完毕。"),
        ("整段 JSON", r#"{"translations":[{"id":"t0","t":"保存"},{"id":"t1","t":"打开"}]}"#),
    ];
    for (name, raw) in cases {
        let got = webzh::parse_translate_response_pub(raw, &its);
        println!("{name}: 解析 {} 条", got.len());
        assert_eq!(got.len(), 2, "{name} 应解析出 2 条");
    }
}

/// 真机全链路：读真页面的待译片段 → 真调 opencode 翻 → 落盘给外部写回。
/// （脚本层与写回由 src/utils/webzh_dom.test.ts 和 api.sock 驱动各验一半，
///   这里只验"真模型的译文质量与可解析性"这一段。）
#[tokio::test]
#[ignore]
async fn live_真页面片段翻译落盘() {
    let raw = std::fs::read_to_string("/tmp/opencode/items.json").expect("先跑收集脚本生成 items.json");
    let items: Vec<BatchItem> = serde_json::from_str(&raw).expect("解析 items.json");
    assert!(!items.is_empty());
    for i in &items {
        println!("  待翻 [{}] {}", i.id, i.text);
    }
    let got = ask(&items, "HTTP Semantics").await;
    println!("  解析 {} / {} 条", got.len(), items.len());
    assert!(!got.is_empty());
    // 落盘给 python 写回用
    let out = serde_json::to_string(&got).unwrap();
    std::fs::write("/tmp/opencode/translated.json", &out).expect("落盘");
    println!("  已写 /tmp/opencode/translated.json");
}
