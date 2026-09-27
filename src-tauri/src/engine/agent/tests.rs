//! engine/agent.rs 单测（R1 拆分：外置，用例零改动）

use super::*;

#[test]
fn click_js_escapes_selector() {
    // 选择器含引号/反斜杠，JSON 序列化后必须安全嵌入
    let p = AgentActionParams {
        action: "click".to_string(),
        selector: Some("\"; alert(1); \"".to_string()),
        value: None,
        direction: None,
        url: None,
        ms: None,
    };
    let js = build_action_js(&p).unwrap().unwrap();
    assert!(js.contains("\\\"; alert(1); \\\""));
    assert!(!js.contains("alert(1);\""), "选择器未转义");
}

#[test]
fn fill_rejects_password_field() {
    let p = AgentActionParams {
        action: "fill".to_string(),
        selector: Some("input[name=pwd]".to_string()),
        value: Some("secret".to_string()),
        direction: None,
        url: None,
        ms: None,
    };
    let js = build_action_js(&p).unwrap().unwrap();
    assert!(js.contains("password"), "必须拒绝密码框");
}

#[test]
fn unknown_action_rejected() {
    let p = AgentActionParams {
        action: "eval".to_string(),
        selector: None,
        value: None,
        direction: None,
        url: None,
        ms: None,
    };
    assert!(build_action_js(&p).is_err(), "白名单外动作必须拒绝");
}

#[test]
fn backend_actions_no_js() {
    for a in ["wait", "navigate", "back", "snapshot"] {
        let p = AgentActionParams {
            action: a.to_string(),
            selector: None,
            value: None,
            direction: None,
            url: Some("https://example.com".to_string()),
            ms: Some(100),
        };
        assert!(build_action_js(&p).unwrap().is_none(), "{} 应为后端动作", a);
    }
}

#[test]
fn parse_js_result_ok_and_err() {
    assert_eq!(
        parse_js_result(r#"{"ok":true,"text":"你好"}"#).unwrap(),
        "你好"
    );
    assert!(parse_js_result(r#"{"ok":false,"error":"元素未找到"}"#).is_err());
}

#[test]
fn describe_action_human_readable() {
    let p = AgentActionParams {
        action: "click".to_string(),
        selector: Some("#submit".to_string()),
        value: None,
        direction: None,
        url: None,
        ms: None,
    };
    assert_eq!(describe_action(&p), "点击元素「#submit」");
}

#[test]
fn failed_action_params_collects_failures() {
    let p = AgentActionParams {
        action: "click".to_string(),
        selector: Some("#nope".to_string()),
        value: None,
        direction: None,
        url: None,
        ms: None,
    };
    let step = record_action(&p, "失败: 元素未找到");
    let pairs = failed_action_params().expect("应有失败步");
    assert!(
        pairs.iter().any(|(s, _)| *s == step),
        "整批重建应含刚记录的失败步"
    );
}

#[test]
fn batch_retry_plan_shape() {
    let p = AgentActionParams {
        action: "fill".to_string(),
        selector: Some("#q".to_string()),
        value: Some("你好".to_string()),
        direction: None,
        url: None,
        ms: None,
    };
    let pairs = vec![(7u64, p)];
    let plan = batch_retry_plan(&pairs);
    assert_eq!(plan.steps.len(), 1);
    assert_eq!(plan.steps[0].action, "fill");
    assert_eq!(plan.steps[0].value.as_deref(), Some("你好"));
    assert!(plan.description.contains('1'));
}

#[test]
fn agent_tool_defs_include_both_tools() {
    let defs = agent_tool_defs();
    let names: Vec<String> = defs
        .iter()
        .filter_map(|d| d["function"]["name"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        names.contains(&"browser_action".to_string()),
        "缺 browser_action"
    );
    assert!(
        names.contains(&"browser_action_plan".to_string()),
        "缺 browser_action_plan（多步计划工具必须注册进 AI 工具清单）"
    );
}
