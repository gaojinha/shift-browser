//! Agent 浏览器动作（AI 执行层的动作白名单，单一来源，R1 拆分 · 2026-09-25）。
//!
//! 与 `registry::tool_defs()`（MCP/API 工具）区别：动作是 AI 在**当前标签页**上的原子操作，
//! 每步需人工确认（见 `engine/agent.rs`）。这里集中声明，
//! 供 function calling 的 `enum` 生成、白名单校验、以及后续的 AI 能力自述。
// 动作注册表待 AI 执行层（engine/agent.rs）接线后消费，接线前允许死代码
#![allow(dead_code)]

/// 一个动作
pub struct Action {
    pub id: &'static str,
    pub desc: &'static str,
    /// 必填参数（供 Agent 校验 / 提示词生成）
    pub required: &'static [&'static str],
}

/// 全部动作（唯一来源）
pub const ACTIONS: &[Action] = &[
    Action {
        id: "click",
        desc: "点击页面元素",
        required: &["selector"],
    },
    Action {
        id: "fill",
        desc: "填写输入框",
        required: &["selector", "value"],
    },
    Action {
        id: "scroll",
        desc: "滚动页面",
        required: &["direction"],
    },
    Action {
        id: "read",
        desc: "读取页面文本",
        required: &[],
    },
    Action {
        id: "wait",
        desc: "等待若干毫秒",
        required: &["ms"],
    },
    Action {
        id: "navigate",
        desc: "跳转到指定 URL",
        required: &["url"],
    },
    Action {
        id: "back",
        desc: "后退一页",
        required: &[],
    },
    Action {
        id: "snapshot",
        desc: "采集页面语义快照",
        required: &[],
    },
    Action {
        id: "submit",
        desc: "提交表单",
        required: &["selector"],
    },
];

/// 动作 id 数组（供 function calling 的 `enum` / 白名单校验直接用）
pub const IDS: [&str; 9] = [
    "click", "fill", "scroll", "read", "wait", "navigate", "back", "snapshot", "submit",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_actions() {
        // IDS 与 ACTIONS 必须一致（防两处漂移）
        assert_eq!(IDS.len(), ACTIONS.len());
        for (id, a) in IDS.iter().zip(ACTIONS.iter()) {
            assert_eq!(id, &a.id);
            assert!(!a.desc.is_empty());
        }
    }

    #[test]
    fn required_params_sane_and_ids_unique() {
        // required 供 Agent 校验与提示词生成：关键原子操作的必填参数必须锁死
        let need = |id: &str| ACTIONS.iter().find(|a| a.id == id).unwrap().required;
        assert_eq!(need("fill"), &["selector", "value"]);
        assert_eq!(need("click"), &["selector"]);
        assert_eq!(need("submit"), &["selector"]);
        assert_eq!(need("navigate"), &["url"]);
        assert_eq!(need("wait"), &["ms"]);
        for id in ["read", "back", "snapshot"] {
            assert!(need(id).is_empty(), "{id} 应为无参动作");
        }
        // id 唯一（enum 生成 / 白名单查重依赖唯一性）
        let mut sorted = IDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), IDS.len(), "动作 id 必须唯一");
    }
}
