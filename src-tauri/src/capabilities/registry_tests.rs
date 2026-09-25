//! 能力注册表防漂移测试（R1 外置 · 2026-09-25）

use super::*;

#[test]
fn tool_defs_wellformed() {
  let defs = tool_defs();
  assert_eq!(
    defs.len(),
    41,
    "工具数应为 41（读 24 + 写 17：含 tabs/close_ai、action/dismiss、action/wait_for、action/scroll、action/click、action/fill、action/back）"
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
  // 不进 MCP 清单的高危命令也必须被闸门覆盖
  for name in ["eval", "tool_call", "plugins/set_enabled"] {
    assert_eq!(risk_of(name), Some(Risk::Write), "{name} 应为写命令");
    assert!(is_write(name), "{name} 必须被只读闸门拦截");
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
  assert!(!is_write("read_page"), "read_page 是只读采集，不得收紧为写命令");
  assert!(!is_write("status"));
}
