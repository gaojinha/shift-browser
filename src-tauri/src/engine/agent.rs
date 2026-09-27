//! Agent 执行层（阶段 1.4）
//!
//! 把浏览器动作封装成 function calling 工具（`browser_action`），AI 在对话中
//! 请求动作，经**每步人工确认**后执行在**当前活动标签**。
//!
//! 安全铁律（strategy-2026.md 1.4）：
//! - **动作白名单**：只有下面定义的固定动作，不允许任意 JS
//! - **每步人工确认**：所有动作执行前必须经前端确认（见 agent_cmd 的确认流程）
//! - **仅当前标签**：动作只作用于 TabManager 的活动标签
//! - **凭据永不外泄**：快照提取排除密码框；read 动作不读 input[type=password]
//! - **注入防护**：selector/value 等参数一律 JSON 序列化后嵌入 JS 模板（杜绝引号逃逸）

use serde_json::{json, Value};

/// 动作白名单（AI 只能请求这些动作）——单一来源见 `capabilities::agent_actions`
pub use crate::capabilities::agent_actions::IDS as AGENT_ACTIONS;

// 动作历史（R1 拆分 2026-09-25）：record/rebuild/失败重试 → agent/history.rs
#[path = "agent/history.rs"]
mod history;
pub use history::{
    action_history_list, batch_retry_plan, failed_action_params, last_failed_step, rebuild_params,
    record_action, ActionHistoryItem,
};

/// 多步计划中的单步（AI 编排提示词的最小单元）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentStep {
    pub step: u32,
    pub action: String,
    pub selector: Option<String>,
    pub value: Option<String>,
    pub direction: Option<String>,
    pub url: Option<String>,
    pub ms: Option<u64>,
}

/// 多步任务计划（AI 生成的编排提示词）：一步确认、按序执行
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MultiStepPlan {
    pub steps: Vec<AgentStep>,
    pub description: String,
}

/// 动作描述（前端确认卡片展示用，多步版）
pub fn describe_multi_step(plan: &MultiStepPlan) -> String {
    format!("{}（共 {} 步）", plan.description, plan.steps.len())
}

/// OpenAI function calling 工具定义（`browser_action_plan`：多步编排提示词）
pub fn browser_action_plan_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "browser_action_plan",
            "description": "生成多步任务编排提示词（需用户一次性确认整个计划后按序执行）。steps 为有序动作列表，每步同 browser_action 参数。",
            "parameters": {
                "type": "object",
                "properties": {
                    "description": { "type": "string", "description": "计划描述（给用户看）" },
                    "steps": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": { "type": "string", "enum": AGENT_ACTIONS },
                                "selector": { "type": "string" },
                                "value": { "type": "string" },
                                "direction": { "type": "string", "enum": ["up", "down"] },
                                "url": { "type": "string" },
                                "ms": { "type": "integer" }
                            },
                            "required": ["action"]
                        }
                    }
                },
                "required": ["description", "steps"]
            }
        }
    })
}

/// OpenAI function calling 工具定义（`browser_action`）
pub fn browser_action_tool() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "browser_action",
            "description": "在浏览器当前标签页执行动作（需要用户确认）。动作列表：click=点击元素(selector)；fill=填写输入框(selector,value)；scroll=滚动页面(direction=up/down)；read=读取元素文本(selector)；wait=等待(ms)；navigate=跳转网址(url)；back=后退；snapshot=获取当前页面语义快照；submit=提交表单(selector)。",
            "parameters": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": AGENT_ACTIONS },
                    "selector": { "type": "string", "description": "CSS 选择器（click/fill/read/submit 用）" },
                    "value": { "type": "string", "description": "填写值（fill 用）" },
                    "direction": { "type": "string", "enum": ["up", "down"], "description": "滚动方向（scroll 用）" },
                    "url": { "type": "string", "description": "目标地址（navigate 用）" },
                    "ms": { "type": "integer", "description": "等待毫秒（wait 用）" }
                },
                "required": ["action"]
            }
        }
    })
}

/// Agent 工具定义集合（AI 工具循环注册用）：单步动作 + 多步计划。
///
/// 单独抽出来是为了**防止"定义了工具却没注册"**（历史 bug：browser_action_plan
/// 有定义有处理器，但工具清单里漏了它，模型永远调不到）。
pub fn agent_tool_defs() -> Vec<Value> {
    vec![browser_action_tool(), browser_action_plan_tool()]
}

/// 动作参数（来自 AI 的 tool_call arguments）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentActionParams {
    pub action: String,
    pub selector: Option<String>,
    pub value: Option<String>,
    pub direction: Option<String>,
    pub url: Option<String>,
    pub ms: Option<u64>,
}

/// 生成动作的 JS（参数 JSON 序列化嵌入，防注入）。
/// 返回 Ok(Some(js)) = 需在页面执行；Ok(None) = 后端动作（wait/navigate/back/snapshot）。
pub fn build_action_js(params: &AgentActionParams) -> Result<Option<String>, String> {
    let action = params.action.as_str();
    if !AGENT_ACTIONS.contains(&action) {
        return Err(format!("动作 '{}' 不在白名单内", action));
    }
    match action {
        "click" => {
            let sel = json!(params.selector.as_deref().unwrap_or("")).to_string();
            Ok(Some(format!(
                "(() => {{ const el = document.querySelector({sel}); if (!el) return JSON.stringify({{ok:false,error:'元素未找到'}}); el.click(); return JSON.stringify({{ok:true,text:(el.innerText||el.textContent||'').slice(0,200)}}); }})()",
                sel = sel
            )))
        }
        "fill" => {
            let sel = json!(params.selector.as_deref().unwrap_or("")).to_string();
            let val = json!(params.value.as_deref().unwrap_or("")).to_string();
            Ok(Some(format!(
                "(() => {{ const el = document.querySelector({sel}); if (!el) return JSON.stringify({{ok:false,error:'元素未找到'}}); if (el.tagName==='INPUT' && el.type==='password') return JSON.stringify({{ok:false,error:'拒绝操作密码框'}}); el.value = {val}; el.dispatchEvent(new Event('input',{{bubbles:true}})); el.dispatchEvent(new Event('change',{{bubbles:true}})); return JSON.stringify({{ok:true}}); }})()",
                sel = sel, val = val
            )))
        }
        "scroll" => {
            let dy = if params.direction.as_deref() == Some("up") {
                "-600"
            } else {
                "600"
            };
            Ok(Some(format!(
                "(() => {{ window.scrollBy({{top:{dy},behavior:'smooth'}}); return JSON.stringify({{ok:true}}); }})()",
                dy = dy
            )))
        }
        "read" => {
            let sel = json!(params.selector.as_deref().unwrap_or("")).to_string();
            Ok(Some(format!(
                "(() => {{ const el = document.querySelector({sel}); if (!el) return JSON.stringify({{ok:false,error:'元素未找到'}}); if (el.tagName==='INPUT' && el.type==='password') return JSON.stringify({{ok:false,error:'拒绝读取密码框'}}); return JSON.stringify({{ok:true,text:(el.innerText||el.textContent||'').slice(0,2000)}}); }})()",
                sel = sel
            )))
        }
        "submit" => {
            let sel = json!(params.selector.as_deref().unwrap_or("")).to_string();
            Ok(Some(format!(
                "(() => {{ const el = document.querySelector({sel}); if (!el) return JSON.stringify({{ok:false,error:'元素未找到'}}); if (typeof el.requestSubmit==='function') {{ el.requestSubmit(); }} else if (el.tagName==='FORM') {{ el.submit(); }} else {{ const f=el.closest('form'); if(f) f.requestSubmit(); else return JSON.stringify({{ok:false,error:'找不到表单'}}); }} return JSON.stringify({{ok:true}}); }})()",
                sel = sel
            )))
        }
        // 以下为后端动作（不生成 JS）
        "wait" | "navigate" | "back" | "snapshot" => Ok(None),
        _ => Err(format!("动作 '{}' 未实现", action)),
    }
}

/// 解析页面 JS 返回的 JSON 结果（{ok, text?, error?}）
pub fn parse_js_result(raw: &str) -> Result<String, String> {
    let v: Value = serde_json::from_str(raw).map_err(|_| format!("页面脚本返回异常: {}", raw))?;
    if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) {
        Ok(v.get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string())
    } else {
        Err(v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("动作执行失败")
            .to_string())
    }
}

/// 动作描述（前端确认卡片展示用）
pub fn describe_action(params: &AgentActionParams) -> String {
    match params.action.as_str() {
        "click" => format!("点击元素「{}」", params.selector.as_deref().unwrap_or("?")),
        "fill" => format!(
            "在「{}」填写「{}」",
            params.selector.as_deref().unwrap_or("?"),
            params.value.as_deref().unwrap_or("")
        ),
        "scroll" => format!(
            "页面{}滚动",
            if params.direction.as_deref() == Some("up") {
                "向上"
            } else {
                "向下"
            }
        ),
        "read" => format!(
            "读取元素「{}」内容",
            params.selector.as_deref().unwrap_or("?")
        ),
        "wait" => format!("等待 {} 毫秒", params.ms.unwrap_or(0)),
        "navigate" => format!("跳转到 {}", params.url.as_deref().unwrap_or("?")),
        "back" => "后退一页".to_string(),
        "snapshot" => "读取当前页面快照".to_string(),
        "submit" => format!("提交表单「{}」", params.selector.as_deref().unwrap_or("?")),
        other => format!("动作 {}", other),
    }
}

#[cfg(test)]
#[path = "agent/tests.rs"]
mod tests;
