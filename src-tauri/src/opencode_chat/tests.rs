use super::*;

fn msg(role: &str, content: &str) -> ChatMessage {
    ChatMessage {
        role: role.to_string(),
        content: content.to_string(),
        tool_call_id: None,
        tool_calls: None,
    }
}

#[test]
fn model_ref_splits_first_slash() {
    assert_eq!(
        parse_model_ref("opencode/big-pickle"),
        Some(("opencode".to_string(), "big-pickle".to_string()))
    );
    // modelID 自身含 '/'：只在第一个斜杠切
    assert_eq!(
        parse_model_ref("cloudflare/@cf/meta/llama"),
        Some(("cloudflare".to_string(), "@cf/meta/llama".to_string()))
    );
    assert_eq!(parse_model_ref("  "), None);
    assert_eq!(parse_model_ref("noslash"), None);
    assert_eq!(parse_model_ref("/onlymodel"), None);
    assert_eq!(parse_model_ref("onlyprovider/"), None);
}

#[test]
fn transcript_single_user_is_verbatim() {
    let (sys, prompt) = render_transcript(&[msg("user", "你好")]);
    assert_eq!(sys, None);
    assert_eq!(prompt, "你好");
}

#[test]
fn transcript_merges_system() {
    let (sys, _) = render_transcript(&[
        msg("system", "你是助手"),
        msg("system", "中文回复"),
        msg("user", "在吗"),
    ]);
    assert_eq!(sys.as_deref(), Some("你是助手\n\n中文回复"));
}

#[test]
fn transcript_multi_turn_is_labeled_and_ordered() {
    let (_, prompt) = render_transcript(&[
        msg("system", "S"),
        msg("user", "Q1"),
        msg("assistant", "A1"),
        msg("user", "Q2"),
    ]);
    let q1 = prompt.find("【用户】Q1").unwrap();
    let a1 = prompt.find("【助手】A1").unwrap();
    let q2 = prompt.find("【用户】Q2").unwrap();
    assert!(q1 < a1 && a1 < q2, "转录顺序错误: {prompt}");
    assert!(prompt.starts_with("以下是此前的对话记录"));
    // system 不混进正文
    assert!(!prompt.contains("S\n"));
}

#[test]
fn transcript_drops_empty_and_unknown_roles() {
    let (_, prompt) = render_transcript(&[
        msg("user", "Q1"),
        msg("assistant", "   "),
        msg("tool", "tool-result"),
        msg("user", "Q2"),
    ]);
    assert!(!prompt.contains("tool-result"));
    assert!(prompt.contains("Q1") && prompt.contains("Q2"));
}

#[test]
fn parse_models_filters_disabled_and_orders_active_first() {
    let v = serde_json::json!({
        "data": [
            {"providerID": "p", "id": "old", "name": "Old", "enabled": true, "status": "deprecated"},
            {"providerID": "p", "id": "off", "name": "Off", "enabled": false, "status": "active"},
            {"providerID": "p", "id": "new", "name": "New", "enabled": true, "status": "active"},
            {"providerID": "cf", "id": "@cf/x", "name": "X"}
        ]
    });
    let list = parse_models(&v);
    let refs: Vec<&str> = list.iter().map(|m| m.model_ref.as_str()).collect();
    // active 组内按名称排序：New < X；deprecated 的 old 沉底
    assert_eq!(refs, vec!["p/new", "cf/@cf/x", "p/old"]);
    assert!(list[0].enabled && !list[0].deprecated);
    assert!(list[2].deprecated);
    // 缺 name 回退 model id
    let v2 = serde_json::json!({"data":[{"providerID":"p","id":"id-only"}]});
    assert_eq!(parse_models(&v2)[0].name, "id-only");
    // 异常形态不炸
    assert!(parse_models(&serde_json::json!({})).is_empty());
}

#[test]
fn transcript_no_turns_is_empty_prompt() {
    let (sys, prompt) = render_transcript(&[
        msg("system", "只有系统消息"),
        msg("user", "   "),
        msg("assistant", ""),
    ]);
    assert!(sys.is_some());
    assert_eq!(prompt, "");
}

#[test]
fn fatal_status_detects_quota_and_far_retry() {
    let quota = serde_json::json!({
        "type": "session.status",
        "properties": {"status": {
            "type": "retry", "message": "Free usage exceeded",
            "action": {"reason": "free_tier_limit"}, "next": 4_000_000_000_000i64
        }}
    });
    assert_eq!(fatal_status(&quota).as_deref(), Some("Free usage exceeded"));

    // 普通瞬时 retry（无 reason、无远期 next）不判死
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let transient = serde_json::json!({
        "type": "session.status",
        "properties": {"status": {"type": "retry", "message": "boop", "next": now + 5_000}}
    });
    assert_eq!(fatal_status(&transient), None);

    let busy = serde_json::json!({
        "type": "session.status",
        "properties": {"status": {"type": "busy"}}
    });
    assert_eq!(fatal_status(&busy), None);

    let err = serde_json::json!({
        "type": "session.status",
        "properties": {"status": {"type": "error", "message": "炸了"}}
    });
    assert_eq!(fatal_status(&err).as_deref(), Some("炸了"));
}

#[test]
fn capture_status_error_picks_retry_message() {
    let ev = serde_json::json!({
        "type": "session.status",
        "properties": {"status": {"type": "retry", "message": "Free usage exceeded"}}
    });
    let mut err = None;
    capture_status_error(&ev, &mut err);
    assert_eq!(err.as_deref(), Some("Free usage exceeded"));
    // busy 不算错误
    let busy = serde_json::json!({
        "type": "session.status",
        "properties": {"status": {"type": "busy"}}
    });
    err = None;
    capture_status_error(&busy, &mut err);
    assert_eq!(err, None);
}
