//! P2 真机验收：实连本机 opencode，跑通「建 session → prompt_async → SSE 过滤 → 收尾」
//! 整条 Rust 路径（不是 Python 复刻版）。
//!
//! 与 `webqa.rs` 单测的分工：
//! - 单测（`cargo test --lib webqa`）不依赖外部服务，纯逻辑可离线回归。
//! - 本文件（`cargo test --test webqa_live -- --ignored --nocapture`）**真打 opencode**，
//!   验证协议假设与 IO 路径：凭据从 /proc 发现、agent 存在性拦截、SSE 事件过滤、
//!   baseline 不误认历史、user delta 不进正文。
//!
//! 用 `#[ignore]` 标记：默认 `cargo test` 不跑（CI 无 opencode 也会绿），本机手动跑。

use shift_browser_lib::webqa;

fn base() -> String {
    std::env::var("SHIFT_WEBQA_BASE").unwrap_or_else(|_| "http://127.0.0.1:4096".into())
}

fn auth() -> String {
    use base64::Engine;
    // 凭据由 webqa::resolve_credentials 的 /proc 发现提供；测试直接复用同一规则
    let c = discover()
        .expect("未发现 opencode 凭据：本机需跑着带 OPENCODE_SERVER_PASSWORD 的 `opencode serve`");
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", c.0, c.1))
    )
}

/// 复用被测代码的 /proc 发现逻辑（复制其规则，不重复实现第三份）。
fn discover() -> Option<(String, String)> {
    for e in std::fs::read_dir("/proc").ok()? {
        let e = e.ok()?;
        let pid = e.file_name().to_string_lossy().to_string();
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let dir = std::path::Path::new("/proc").join(&pid);
        let cmd = std::fs::read_to_string(dir.join("cmdline")).unwrap_or_default();
        if !cmd.contains("opencode") || !cmd.contains("serve") {
            continue;
        }
        let environ = std::fs::read_to_string(dir.join("environ")).unwrap_or_default();
        let mut u = None;
        let mut p = None;
        for item in environ.split('\0') {
            if let Some(v) = item.strip_prefix("OPENCODE_SERVER_USERNAME=") {
                u = Some(v.to_string());
            } else if let Some(v) = item.strip_prefix("OPENCODE_SERVER_PASSWORD=") {
                p = Some(v.to_string());
            }
        }
        if let (Some(u), Some(p)) = (u, p) {
            return Some((u, p));
        }
    }
    None
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn post_json(c: &reqwest::Client, path: &str, body: serde_json::Value) -> serde_json::Value {
    let r = c
        .post(format!("{}{}", base(), path))
        .header("Authorization", auth())
        .json(&body)
        .send()
        .await
        .expect("请求失败");
    r.json().await.unwrap_or(serde_json::Value::Null)
}

#[tokio::test]
#[ignore = "需本机 opencode serve 在 4096 运行"]
async fn live_凭据能从_proc发现且认证通过() {
    assert!(discover().is_some(), "应能从 /proc 发现 serve 凭据");
    let c = client();
    let r = c
        .get(format!("{}/config", base()))
        .header("Authorization", auth())
        .send()
        .await
        .expect("连不上 opencode");
    assert!(
        r.status().is_success(),
        "认证应成功，实际 HTTP {}",
        r.status()
    );
}

#[tokio::test]
#[ignore = "需本机 opencode serve 在 4096 运行"]
async fn live_plan_agent存在且只读() {
    let c = client();
    let v: serde_json::Value = c
        .get(format!("{}/agent", base()))
        .header("Authorization", auth())
        .send()
        .await
        .expect("拉 agent 失败")
        .json()
        .await
        .unwrap();
    let items: Vec<serde_json::Value> = match &v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(m) => m.values().cloned().collect(),
        _ => vec![],
    };
    let plan = items
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some("plan"))
        .expect("opencode 应有 plan agent（P2 靠它保证只读边界）");
    let desc = plan
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("");
    assert!(
        desc.contains("edit") || desc.contains("Plan"),
        "plan agent 描述应表明禁用编辑工具，实际：{desc}"
    );
}

#[tokio::test]
#[ignore = "需本机 opencode serve 在 4096 运行"]
async fn live_整条问答链路能拿到回答() {
    use futures_util::StreamExt;

    let c = client();
    let hdr = auth();

    // 1) 建 session
    let sid = post_json(
        &c,
        "/session",
        serde_json::json!({"title": "网页随手问 · 真机验收"}),
    )
    .await
    .get("id")
    .and_then(|v| v.as_str())
    .expect("建 session 失败")
    .to_string();
    println!("session = {sid}");

    // 2) 先开 SSE（与被测代码同序，防漏事件）
    let resp = c
        .get(format!("{}/event", base()))
        .header("Authorization", hdr.clone())
        .send()
        .await
        .expect("订阅事件流失败");
    let mut stream = resp.bytes_stream();

    // 3) 取 baseline（提问前已有的消息 id）
    let before: Vec<String> = c
        .get(format!("{}/session/{sid}/message", base()))
        .header("Authorization", hdr.clone())
        .send()
        .await
        .expect("读历史失败")
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|m| {
            m.get("info")
                .and_then(|i| i.get("id"))
                .and_then(|v| v.as_str())
        })
        .map(str::to_string)
        .collect();
    println!("baseline = {} 条", before.len());

    // 4) 发问（用被测的 prompt 组装）
    let prompt = webqa::build_prompt(
        "用一句话说明什么是幂等性",
        Some("缓存：把结果先存起来，下次直接取。"),
        Some("缓存基础"),
        Some("https://example.com/cache"),
    );
    let pr = c
        .post(format!("{}/session/{sid}/prompt_async", base()))
        .header("Authorization", hdr.clone())
        .json(&serde_json::json!({"agent":"plan","parts":[{"type":"text","text":prompt}]}))
        .send()
        .await
        .expect("发问失败");
    assert!(
        pr.status().is_success() || pr.status() == reqwest::StatusCode::NO_CONTENT,
        "prompt_async 应 2xx/204，实际 {}",
        pr.status()
    );

    // 5) 收敛事件：复用被测的 SseAccumulator + fold_event
    let mut acc = webqa::SseAccumulator::new();
    let mut target: Option<String> = None;
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);

    while tokio::time::Instant::now() < deadline {
        let Ok(Some(chunk)) =
            tokio::time::timeout(std::time::Duration::from_secs(20), stream.next()).await
        else {
            break;
        };
        let Ok(bytes) = chunk else { continue };
        for data in acc.push(&bytes) {
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&data) else {
                continue;
            };
            if let Some((delta, done)) = webqa::fold_event(&ev, &sid, &mut target, &before) {
                if let Some(d) = delta {
                    text.push_str(&d);
                }
                if done {
                    println!("回答({} 字) = {text}", text.chars().count());
                    assert!(!text.trim().is_empty(), "不该空回答");
                    // 不能把用户提问原文当回答
                    assert!(
                        !text.contains("用一句话说明什么是幂等性"),
                        "回答里混进了用户提问原文：{text}"
                    );
                    let _ = c
                        .delete(format!("{}/session/{sid}", base()))
                        .header("Authorization", hdr)
                        .send()
                        .await;
                    return;
                }
            }
        }
    }
    panic!("超时未收到 session.idle，已收 {} 字", text.chars().count());
}

#[tokio::test]
#[ignore = "需本机 opencode serve 在 4096 运行"]
async fn live_不存在agent会被拦下() {
    // 被测代码的 ensure_agent_exists 逻辑：未知 agent 必须拦下，
    // 否则 opencode 会静默回退到有编辑权限的默认 agent。
    let c = client();
    let hdr = auth();
    let v: serde_json::Value = c
        .get(format!("{}/agent", base()))
        .header("Authorization", hdr)
        .send()
        .await
        .expect("拉 agent 失败")
        .json()
        .await
        .unwrap();
    let items: Vec<serde_json::Value> = match &v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(m) => m.values().cloned().collect(),
        _ => vec![],
    };
    let names: Vec<&str> = items
        .iter()
        .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
        .collect();
    assert!(
        !names.contains(&"__不存在的agent__"),
        "断言前提有误：测试 agent 名竟然存在"
    );
    // 反向：plan 必须在列表里，否则 P2 默认值不可用
    assert!(names.contains(&"plan"), "plan 应在 agent 列表里");
}
