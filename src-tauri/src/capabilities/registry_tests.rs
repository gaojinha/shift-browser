//! 能力注册表防漂移测试（R1 外置 · 2026-09-25）

use super::*;

#[test]
fn tool_defs_wellformed() {
    let defs = tool_defs();
    assert_eq!(
    defs.len(),
    48,
    "工具数应为 48（读 27 + 写 21：page/content/audit 读 + 标签治理/交互写；手部升级 +action/find 读 +action/type 写；P1 上下文采集 +read_selection 读；P3b 整页汉化 +page_collect_translatable 读 +page_apply_translation 写）"
  );
    for d in &defs {
        assert!(d.get("name").and_then(|v| v.as_str()).is_some(), "缺 name");
        assert!(d.get("description").is_some(), "缺 description");
        let schema = d.get("inputSchema").expect("缺 inputSchema");
        assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));
        // risk 是内部安全元数据，绝不能通过 MCP 视图泄露
        assert!(d.get("risk").is_none(), "MCP 视图不得暴露 risk");
    }
}

#[test]
fn list_json_carries_schema_version() {
    // 调用方（MCP bin / 外部脚本）感知工具集演进的锚点：字段必须在，
    // 值与常量同源（防两处手写漂移）
    let v = list_json();
    assert_eq!(
        v.get("schema_version").and_then(|x| x.as_u64()),
        Some(SCHEMA_VERSION as u64),
        "list_json 的 schema_version 必须与常量一致"
    );
    assert_eq!(v.get("count").and_then(|x| x.as_u64()), Some(48));
    assert!(
        v.get("instructions").and_then(|x| x.as_str()).is_some(),
        "缺 instructions"
    );
}

#[test]
fn ids_unique() {
    let mut ids = ids();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "能力 id 必须唯一");
}

/// 防漂移（P0-2 核心）：注册表 Risk 分类必须与只读闸门判定同源一致。
/// 历史问题：api.rs 手写第二份写命令名单，新增写命令漏改即静默绕过只读模式。
#[test]
fn readonly_gate_matches_registry_risk() {
    for d in registry() {
        assert_eq!(
            is_write(d.name),
            d.risk == Risk::Write,
            "命令 {} 的只读闸门判定与注册表 Risk::{:?} 不一致",
            d.name,
            d.risk
        );
    }
    // 不进 MCP 清单的命令（EXTRA_RISK 旁路）必须全部有正确风险分类：
    // 写被只读闸门拦截、读放行；清单直接派生自 EXTRA_RISK 表，
    // 新增一条旁路命令即自动进入本断言（此前 tools 漏在断言外）
    for (name, risk) in extra_risk_pairs() {
        assert_eq!(
            risk_of(name),
            Some(*risk),
            "命令 {name} 的 extra_risk 分类漂移"
        );
        assert_eq!(
            is_write(name),
            *risk == Risk::Write,
            "命令 {name} 的只读闸门判定与风险分类不一致"
        );
    }
    // 未知命令不享有写身份（match 分发本身会拒未知命令，这里双保险）
    assert!(!is_write("definitely-not-a-command"));
    assert_eq!(risk_of("definitely-not-a-command"), None);
}

#[test]
fn write_cmd_set_unchanged_snapshot() {
    // 行为基线：与重构前 api.rs 手写名单完全一致（screenshot 历史上不拦截）
    let expected = [
        "open_url",
        "new_tab",
        "close_tab",
        "switch_tab",
        "reload_tab",
        "navigate",
        "search",
        "pin_tab",
        "mute_tab",
        "minimize_window",
        "maximize_window",
        "eval",
        "tool_call",
        "plugins/set_enabled",
    ];
    for name in expected {
        assert!(is_write(name), "基线写命令 {name} 丢失");
    }
    assert!(
        !is_write("screenshot"),
        "screenshot 历史行为为只读，不得收紧"
    );
    assert!(
        !is_write("read_page"),
        "read_page 是只读采集，不得收紧为写命令"
    );
    assert!(!is_write("status"));
}
