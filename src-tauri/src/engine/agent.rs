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
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// 动作白名单（AI 只能请求这些动作）
pub const AGENT_ACTIONS: [&str; 9] = [
    "click", "fill", "scroll", "read", "wait", "navigate", "back", "snapshot", "submit",
];

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

/// /// OpenAI function calling 工具定义（`browser_action`）
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
            let dy = if params.direction.as_deref() == Some("up") { "-600" } else { "600" };
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
        Ok(v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string())
    } else {
        Err(v.get("error").and_then(|e| e.as_str()).unwrap_or("动作执行失败").to_string())
    }
}

/// 动作描述（前端确认卡片展示用）
pub fn describe_action(params: &AgentActionParams) -> String {
    match params.action.as_str() {
        "click" => format!("点击元素「{}」", params.selector.as_deref().unwrap_or("?")),
        "fill" => format!("在「{}」填写「{}」", params.selector.as_deref().unwrap_or("?"), params.value.as_deref().unwrap_or("")),
        "scroll" => format!("页面{}滚动", if params.direction.as_deref() == Some("up") { "向上" } else { "向下" }),
        "read" => format!("读取元素「{}」内容", params.selector.as_deref().unwrap_or("?")),
        "wait" => format!("等待 {} 毫秒", params.ms.unwrap_or(0)),
        "navigate" => format!("跳转到 {}", params.url.as_deref().unwrap_or("?")),
        "back" => "后退一页".to_string(),
        "snapshot" => "读取当前页面快照".to_string(),
        "submit" => format!("提交表单「{}」", params.selector.as_deref().unwrap_or("?")),
        other => format!("动作 {}", other),
    }
}


/// 动作历史（阶段 1.8：回放/重试的可信地基）
///
/// 每次 Agent 动作确认执行后，由命令层调用 `record_action` 落一条快照
/// （动作名 + 参数 + 结果 + 时间戳），AI/用户可回看、回放、重试。
/// 安全：快照只存动作参数与结果文本，**不存页面凭据/密码**（铁律 1.4）。
///
/// 历史是**审计链路**的一部分（对齐倒查/零遥测承诺）：重放/重试
/// 必须重新走 `execute_agent_action`，不提供绕过窗口/标签的捷径。

/// 单条动作历史记录
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActionHistoryItem {
    /// 自增步号（回放定位用）
    pub step: u64,
    /// 动作名（click/fill/...）
    pub action: String,
    pub selector: Option<String>,
    pub value: Option<String>,
    pub direction: Option<String>,
    pub url: Option<String>,
    pub ms: Option<u64>,
    /// 执行结果文本（给 AI 回看）
    pub result: String,
    /// unix 秒时间戳
    pub ts: i64,
    /// 是否成功（结果不含"失败/错误"）
    pub ok: bool,
}

/// 环形历史缓冲（最多 128 条，超限丢最旧）
static ACTION_HISTORY: OnceLock<Mutex<VecDeque<ActionHistoryItem>>> = OnceLock::new();
/// 自增步号
static ACTION_STEP: OnceLock<Mutex<u64>> = OnceLock::new();

fn action_history() -> std::sync::MutexGuard<'static, VecDeque<ActionHistoryItem>> {
    ACTION_HISTORY
        .get_or_init(|| Mutex::new(VecDeque::with_capacity(128)))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn action_step_no() -> std::sync::MutexGuard<'static, u64> {
    ACTION_STEP.get_or_init(|| Mutex::new(0)).lock().unwrap_or_else(|e| e.into_inner())
}

/// 记录一条动作（命令层每次执行后调用；返回本步号）
pub fn record_action(params: &AgentActionParams, result: &str) -> u64 {
    let step = {
        let mut n = action_step_no();
        *n += 1;
        *n
    };
    let item = ActionHistoryItem {
        step,
        action: params.action.clone(),
        selector: params.selector.clone(),
        value: params.value.clone(),
        direction: params.direction.clone(),
        url: params.url.clone(),
        ms: params.ms,
        result: result.to_string(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        ok: !result.starts_with("失败") && !result.contains("错误") && !result.contains("未找到"),
    };
    let mut h = action_history();
    h.push_back(item);
    while h.len() > 128 {
        h.pop_front();
    }
    step
}

/// 读取历史（最新在前，limit 默认 50）
pub fn action_history_list(limit: Option<usize>) -> Vec<ActionHistoryItem> {
    let h = action_history();
    let n = limit.unwrap_or(50).min(h.len());
    h.iter().rev().take(n).cloned().collect()
}

/// 按步号取回参数（回放/重试用；找不到返回 Err）
pub fn rebuild_params(step: u64) -> Result<AgentActionParams, String> {
    let h = action_history();
    let item = h
        .iter()
        .find(|i| i.step == step)
        .ok_or_else(|| format!("没有第 {} 步的历史", step))?;
    Ok(AgentActionParams {
        action: item.action.clone(),
        selector: item.selector.clone(),
        value: item.value.clone(),
        direction: item.direction.clone(),
        url: item.url.clone(),
        ms: item.ms,
    })
}

/// 最近一次失败动作的步号（重试用；无则 Err）
pub fn last_failed_step() -> Result<u64, String> {
    let h = action_history();
    h.iter()
        .rev()
        .find(|i| !i.ok)
        .map(|i| i.step)
        .ok_or_else(|| "没有失败过的动作".to_string())
}

/// 从历史里挑出**所有失败步**并重建参数（整批重试地基）。
/// 先收集步号再重建，避免与 `rebuild_params` 重复加锁（std Mutex 不可重入）。
pub fn failed_action_params() -> Result<Vec<(u64, AgentActionParams)>, String> {
    let steps: Vec<u64> = {
        let h = action_history();
        h.iter().filter(|i| !i.ok).map(|i| i.step).collect()
    };
    if steps.is_empty() {
        return Err("历史里没有失败过的动作".to_string());
    }
    steps
        .into_iter()
        .map(|s| rebuild_params(s).map(|p| (s, p)))
        .collect()
}

/// 把批量重建的失败步参数转成多步计划（供 `request_multi_step_action` 整批确认）。
pub fn batch_retry_plan(pairs: &[(u64, AgentActionParams)]) -> MultiStepPlan {
    let steps = pairs
        .iter()
        .map(|(_, p)| AgentStep {
            step: 0,
            action: p.action.clone(),
            selector: p.selector.clone(),
            value: p.value.clone(),
            direction: p.direction.clone(),
            url: p.url.clone(),
            ms: p.ms,
        })
        .collect();
    MultiStepPlan {
        steps,
        description: format!("整批重试 {} 个失败动作", pairs.len()),
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(parse_js_result(r#"{"ok":true,"text":"你好"}"#).unwrap(), "你好");
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
        assert!(pairs.iter().any(|(s, _)| *s == step), "整批重建应含刚记录的失败步");
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
}