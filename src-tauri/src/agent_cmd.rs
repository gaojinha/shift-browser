//! Agent 执行层命令（阶段 1.4）
//!
//! 每步人工确认流程：
//! 1. AI 工具循环请求 `browser_action` → `request_agent_action` 生成 request_id，
//!    emit `agent-action-request` 事件给前端（带动作描述），注册 oneshot 等待
//! 2. 前端弹确认卡片 → 用户点"允许/拒绝" → 调 `agent_confirm_action(request_id, approved)`
//! 3. 确认后执行动作（仅当前活动标签），返回结果给工具循环

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

/// 待确认的动作：request_id -> oneshot 应答通道（用户允许/拒绝）
static PENDING: OnceLock<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>> =
    OnceLock::new();

fn pending() -> &'static Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 请求动作（AI 工具循环调用）：发确认事件给前端，等待用户确认（60s 超时）。
/// 返回 Ok(true) 已允许 / Ok(false) 用户拒绝 / Err 超时或系统错误
pub async fn request_agent_action(
    app: &AppHandle,
    params: &crate::engine::agent::AgentActionParams,
) -> Result<bool, String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let description = crate::engine::agent::describe_action(params);
    let (tx, rx) = tokio::sync::oneshot::channel();

    pending().lock().unwrap().insert(request_id.clone(), tx);

    // 通知前端弹确认卡片
    let _ = app.emit(
        "agent-action-request",
        serde_json::json!({
            "requestId": request_id,
            "description": description,
        }),
    );
    log::info!("[agent] 请求动作确认: {} ({})", description, request_id);

    // 等待确认（60s 超时）
    match tokio::time::timeout(std::time::Duration::from_secs(60), rx).await {
        Ok(Ok(approved)) => {
            log::info!("[agent] 动作确认结果: approved={}", approved);
            Ok(approved)
        }
        Ok(Err(_)) => Err("动作确认通道已关闭".to_string()),
        Err(_) => {
            pending().lock().unwrap().remove(&request_id);
            Err("动作确认超时（60 秒未响应）".to_string())
        }
    }
}

/// 前端确认回调：用户点"允许/拒绝"后调用
#[tauri::command]
pub fn agent_confirm_action(request_id: String, approved: bool) -> Result<(), String> {
    let mut map = pending().lock().unwrap();
    let tx = map
        .remove(&request_id)
        .ok_or_else(|| "确认请求不存在或已过期".to_string())?;
    let _ = tx.send(approved);
    log::info!("[agent] 用户确认动作: request={} approved={}", request_id, approved);
    Ok(())
}

/// 请求多步动作（AI 工具循环调用）：发确认事件给前端，等待用户一次性确认整个计划。
pub async fn request_multi_step_action(
    app: &AppHandle,
    plan: &crate::engine::agent::MultiStepPlan,
) -> Result<bool, String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let description = crate::engine::agent::describe_multi_step(plan);
    let (tx, rx) = tokio::sync::oneshot::channel();

    pending().lock().unwrap().insert(request_id.clone(), tx);

    let _ = app.emit(
        "agent-multi-step-request",
        serde_json::json!({
            "requestId": request_id,
            "description": description,
            "steps": plan.steps.len(),
        }),
    );
    log::info!("[agent] 请求多步动作确认: {} ({} 步, {})", description, plan.steps.len(), request_id);

    match tokio::time::timeout(std::time::Duration::from_secs(120), rx).await {
        Ok(Ok(approved)) => {
            log::info!("[agent] 多步动作确认结果: approved={}", approved);
            Ok(approved)
        }
        Ok(Err(_)) => Err("多步确认通道已关闭".to_string()),
        Err(_) => {
            pending().lock().unwrap().remove(&request_id);
            Err("多步确认超时（120 秒未响应）".to_string())
        }
    }
}

/// 前端多步确认回调：用户点"允许全部/拒绝"后调用
#[tauri::command]
pub fn agent_confirm_multi_step(request_id: String, approved: bool) -> Result<(), String> {
    let mut map = pending().lock().unwrap();
    let tx = map
        .remove(&request_id)
        .ok_or_else(|| "多步确认请求不存在或已过期".to_string())?;
    let _ = tx.send(approved);
    log::info!("[agent] 用户确认多步动作: request={} approved={}", request_id, approved);
    Ok(())
}

/// 执行多步动作（确认后调用）。按序执行每步，仅当前活动标签。
/// 返回动作结果文本（每步结果拼接）。
pub async fn execute_multi_step_action(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    state: &crate::tabs::TabManager,
    plan: &crate::engine::agent::MultiStepPlan,
) -> Result<String, String> {
    let mut results = Vec::new();
    for step in &plan.steps {
        let params = crate::engine::agent::AgentActionParams {
            action: step.action.clone(),
            selector: step.selector.clone(),
            value: step.value.clone(),
            direction: step.direction.clone(),
            url: step.url.clone(),
            ms: step.ms,
        };
        let desc = crate::engine::agent::describe_action(&params);
        log::info!("[agent] 多步执行第 {} 步: {}", step.step, desc);
        match execute_agent_action(app, window, state, &params).await {
            Ok(text) => results.push(format!("第 {} 步 {}：{}", step.step, desc, text)),
            Err(e) => results.push(format!("第 {} 步 {}：失败 ({})", step.step, desc, e)),
        }
    }
    Ok(results.join("\n"))
}

/// /// 执行动作（确认后调用）。仅当前活动标签。
/// 返回动作结果文本（供 AI 继续分析）。
pub async fn execute_agent_action(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    state: &crate::tabs::TabManager,
    params: &crate::engine::agent::AgentActionParams,
) -> Result<String, String> {
    let action = params.action.as_str();

    // 后端动作（不依赖页面 JS）
    match action {
        "wait" => {
            tokio::time::sleep(std::time::Duration::from_millis(params.ms.unwrap_or(500).min(10000)))
                .await;
            return Ok(format!("已等待 {} 毫秒", params.ms.unwrap_or(500)));
        }
        "navigate" => {
            let url = params.url.clone().ok_or_else(|| "缺少 url".to_string())?;
            let id = state
                .runtime_snapshot()
                .1
                .ok_or_else(|| "没有活动标签".to_string())?;
            let label = format!("tab-{}", id);
            let wv = window
                .get_webview(&label)
                .ok_or_else(|| "活动标签的 webview 不存在".to_string())?;
            let parsed = url::Url::parse(&url).map_err(|e| format!("网址不合法: {}", e))?;
            wv.navigate(parsed)
                .map_err(|e| format!("导航失败: {}", e))?;
            return Ok(format!("已跳转到 {}", url));
        }
        "back" => {
            let id = state
                .runtime_snapshot()
                .1
                .ok_or_else(|| "没有活动标签".to_string())?;
            let label = format!("tab-{}", id);
            let wv = window
                .get_webview(&label)
                .ok_or_else(|| "活动标签的 webview 不存在".to_string())?;
            wv.eval("history.back()")
                .map_err(|e| format!("后退失败: {}", e))?;
            return Ok("已后退一页".to_string());
        }
        "snapshot" => {
            let snap = crate::content_cmd::content_snapshot(app.clone()).await?;
            // 用 serde_json 序列化快照给 AI（限制长度）
            let text = serde_json::to_string(&snap).unwrap_or_default();
            return Ok(text.chars().take(3000).collect());
        }
        _ => {}
    }

    // 页面 JS 动作（click/fill/scroll/read/submit）
    let js = crate::engine::agent::build_action_js(params)?
        .ok_or_else(|| "动作未生成脚本".to_string())?;
    let id = state
        .runtime_snapshot()
        .1
        .ok_or_else(|| "没有活动标签".to_string())?;
    let label = format!("tab-{}", id);
    let wv = window
        .get_webview(&label)
        .ok_or_else(|| "活动标签的 webview 不存在".to_string())?;

    // 只发起 JS 执行（with_webview 闭包内不阻塞），结果异步回来
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &js, move |res| {
            let _ = tx.send(res);
        });
    })
    .map_err(|e| format!("动作执行失败: {}", e))?;

    let raw = rx
        .recv_timeout(std::time::Duration::from_secs(6))
        .map_err(|e| format!("动作执行超时: {}", e))?
        .map_err(|e| format!("页面脚本执行失败: {}", e))?;
    crate::engine::agent::parse_js_result(&raw)
}

/// 手动执行动作（前端"测试"用，绕过 AI 确认——仅供调试，生产由 AI 循环走确认流程）
/// 注意：此命令不经过确认，仅限设置页调试入口使用。
#[tauri::command]
pub async fn agent_debug_execute(
    app: AppHandle,
    state: tauri::State<'_, crate::tabs::TabManager>,
    action: String,
    selector: Option<String>,
    value: Option<String>,
    direction: Option<String>,
    url: Option<String>,
    ms: Option<u64>,
) -> Result<String, String> {
    let params = crate::engine::agent::AgentActionParams {
        action,
        selector,
        value,
        direction,
        url,
        ms,
    };
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    execute_agent_action(&app, &window, &state, &params).await
}