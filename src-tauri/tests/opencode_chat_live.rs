//! 侧边栏 opencode Provider 真机验收：实连本机 serve，跑通
//! 「OpencodeProvider → 建会话 → prompt_async（可带 model/system）→ SSE 收敛」。
//!
//! 默认不跑（无 serve / 无额度的环境会失败）：
//! `cargo test --test opencode_chat_live -- --ignored --nocapture`

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use shift_browser_lib::opencode_chat::{
    parse_models, AiProvider, ChatMessage, OcModelInfo, OpencodeProvider, ProviderRequest,
};
use shift_browser_lib::webqa::Credentials;

fn base() -> String {
    std::env::var("SHIFT_WEBQA_BASE").unwrap_or_else(|_| "http://127.0.0.1:4096".into())
}

/// 复用 webqa 的 /proc 凭据发现规则（与 webqa_live 同款，不引第三份实现）。
fn discover() -> Option<Credentials> {
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
        if let (Some(username), Some(password)) = (u, p) {
            return Some(Credentials { username, password });
        }
    }
    None
}

fn auth_header(c: &Credentials) -> String {
    use base64::Engine;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD
            .encode(format!("{}:{}", c.username, c.password))
    )
}

#[tokio::test]
#[ignore = "需本机 opencode serve 在 4096 运行"]
async fn live_模型列表端点能解析且含active项() {
    let creds = discover().expect("未发现 opencode 凭据");
    let v: serde_json::Value = reqwest::Client::new()
        .get(format!("{}/api/model", base()))
        .header("Authorization", auth_header(&creds))
        .send()
        .await
        .expect("请求 /api/model 失败")
        .json()
        .await
        .unwrap();
    let list: Vec<OcModelInfo> = parse_models(&v);
    assert!(!list.is_empty(), "应解析出模型");
    assert!(
        list.iter().all(|m| m.enabled),
        "enabled=false 的模型不应出现"
    );
    // active 必须排在 deprecated 前面
    let first_deprecated = list.iter().position(|m| m.deprecated).unwrap_or(list.len());
    assert!(
        list[..first_deprecated].iter().all(|m| !m.deprecated),
        "active 模型必须排在 deprecated 之前"
    );
    // model_ref 可被 parse_model_ref 还原（在 lib 单测已覆盖切分规则，这里抽查形状）
    assert!(list[0].model_ref.contains('/'));
    println!(
        "解析出 {} 个模型，前 3：{:?}",
        list.len(),
        list.iter().take(3).map(|m| &m.model_ref).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ignore = "需本机 opencode serve 在 4096 运行；免费额度耗尽时会走显式错误分支"]
async fn live_侧边栏问答链路_有回答或显式上游错误() {
    let creds = discover().expect("未发现 opencode 凭据");
    // 不指定模型：跟随 serve /config 默认
    let provider = OpencodeProvider::new(creds, "");
    let req = ProviderRequest {
        model: String::new(),
        messages: vec![
            ChatMessage::new("system", "你是测试助手，用一句话回答。"),
            ChatMessage::new("user", "侧边栏链路自检：用一句话说你好"),
        ],
        stream: true,
        tools: None,
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let deltas = Arc::new(AtomicUsize::new(0));
    let counter = deltas.clone();
    let result = provider
        .chat_stream(
            &req,
            cancel,
            Box::new(move |_| {
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }),
        )
        .await;

    match result {
        Ok(full) => {
            assert!(!full.trim().is_empty(), "正常路径不应空回复");
            assert!(deltas.load(Ordering::Relaxed) > 0, "正常路径应有流式增量");
            println!("✅ 拿到回答（{} 块增量）：{full}", deltas.load(Ordering::Relaxed));
        }
        Err(e) => {
            // 上游故障（如免费额度耗尽）必须显式抛出，绝不允许"静默空回复"
            println!("⚠️ 上游错误（可接受）：{}", e.message);
            assert!(
                e.message.contains("opencode 未返回内容")
                    || e.message.contains("超时")
                    || e.message.contains("事件流")
                    || e.message.contains("plan"),
                "意外的错误类型：{}",
                e.message
            );
        }
    }
}

#[tokio::test]
#[ignore = "需本机 opencode serve 在 4096 运行"]
async fn live_空对话被前置校验拒绝() {
    let creds = discover().expect("未发现 opencode 凭据");
    let provider = OpencodeProvider::new(creds, "");
    let req = ProviderRequest {
        model: String::new(),
        messages: vec![ChatMessage::new("user", "   ")],
        stream: false,
        tools: None,
    };
    let err = provider
        .chat(&req)
        .await
        .expect_err("空内容必须在发 HTTP 前被拒绝");
    assert!(err.message.contains("没有可发送的对话内容"));
}
