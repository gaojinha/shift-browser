// LINES-EXEMPT: Agent 执行层命令（阶段 1.4），拆分排期 R4（2026-10-14 红线卡标注）
//! Agent 执行层命令（阶段 1.4）
//!
//! 每步人工确认流程：
//! 1. AI 工具循环请求 `browser_action` → `request_agent_action` 生成 request_id，
//!    emit `agent-action-request` 事件给前端（带动作描述），注册 oneshot 等待
//! 2. 前端弹确认卡片 → 用户点"允许/拒绝" → 调 `agent_confirm_action(request_id, approved)`
//! 3. 确认后执行动作（仅当前活动标签），返回结果给工具循环

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AppError, CommandError, CommandResult};

/// 待确认的动作：request_id -> oneshot 应答通道（用户允许/拒绝）
static PENDING: OnceLock<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>> =
    OnceLock::new();

fn pending() -> &'static Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 请求动作（AI 工具循环调用）：发确认事件给前端，等待用户确认（60s 超时）。
/// 返回 Ok(true) 已允许 / Ok(false) 用户拒绝 / Err 超时或系统错误
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub async fn request_agent_action(
    app: &AppHandle,
    params: &crate::engine::agent::AgentActionParams,
) -> CommandResult<bool> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let description = crate::engine::agent::describe_action(params);
    let (tx, rx) = tokio::sync::oneshot::channel();

    pending().lock().unwrap().insert(request_id.clone(), tx);

    // 通知前端弹确认卡片
    let _ = app.emit(
        crate::contracts::events::AGENT_ACTION_REQUEST,
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
        Ok(Err(_)) => Err(AppError::internal("动作确认通道已关闭").into()),
        Err(_) => {
            pending().lock().unwrap().remove(&request_id);
            Err(AppError::timeout("动作确认超时（60 秒未响应）").into())
        }
    }
}

/// 前端确认回调：用户点"允许/拒绝"后调用
#[tauri::command]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub fn agent_confirm_action(request_id: String, approved: bool) -> CommandResult<()> {
    let mut map = pending().lock().unwrap();
    let tx = map
        .remove(&request_id)
        .ok_or_else(|| AppError::validation("确认请求不存在或已过期"))?;
    let _ = tx.send(approved);
    log::info!(
        "[agent] 用户确认动作: request={} approved={}",
        request_id,
        approved
    );
    Ok(())
}

/// 请求多步动作（AI 工具循环调用）：发确认事件给前端，等待用户一次性确认整个计划。
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub async fn request_multi_step_action(
    app: &AppHandle,
    plan: &crate::engine::agent::MultiStepPlan,
) -> CommandResult<bool> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let description = crate::engine::agent::describe_multi_step(plan);
    let (tx, rx) = tokio::sync::oneshot::channel();

    pending().lock().unwrap().insert(request_id.clone(), tx);

    let _ = app.emit(
        crate::contracts::events::AGENT_MULTI_STEP_REQUEST,
        serde_json::json!({
            "requestId": request_id,
            "description": description,
            "steps": plan.steps.len(),
        }),
    );
    log::info!(
        "[agent] 请求多步动作确认: {} ({} 步, {})",
        description,
        plan.steps.len(),
        request_id
    );

    match tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::AGENT_STEP_SECS),
        rx,
    )
    .await
    {
        Ok(Ok(approved)) => {
            log::info!("[agent] 多步动作确认结果: approved={}", approved);
            Ok(approved)
        }
        Ok(Err(_)) => Err(AppError::internal("多步确认通道已关闭").into()),
        Err(_) => {
            pending().lock().unwrap().remove(&request_id);
            Err(AppError::timeout("多步确认超时（120 秒未响应）").into())
        }
    }
}

/// 前端多步确认回调：用户点"允许全部/拒绝"后调用
#[tauri::command]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub fn agent_confirm_multi_step(request_id: String, approved: bool) -> CommandResult<()> {
    let mut map = pending().lock().unwrap();
    let tx = map
        .remove(&request_id)
        .ok_or_else(|| AppError::validation("多步确认请求不存在或已过期"))?;
    let _ = tx.send(approved);
    log::info!(
        "[agent] 用户确认多步动作: request={} approved={}",
        request_id,
        approved
    );
    Ok(())
}

/// 执行多步动作（确认后调用）。按序执行每步，仅当前活动标签。
/// 返回动作结果文本（每步结果拼接）。
pub async fn execute_multi_step_action(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    state: &crate::tabs::TabManager,
    plan: &crate::engine::agent::MultiStepPlan,
) -> CommandResult<String> {
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
/// 记录一条动作执行（审计/回放/重试的历史地基）。
// Tauri 命令参数与前端 IPC 一一对应，不宜为减参数引入包装结构体
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn agent_action_record(
    _app: AppHandle,
    action: String,
    selector: Option<String>,
    value: Option<String>,
    direction: Option<String>,
    url: Option<String>,
    ms: Option<u64>,
    result: String,
) -> CommandResult<u64> {
    let params = crate::engine::agent::AgentActionParams {
        action,
        selector,
        value,
        direction,
        url,
        ms,
    };
    Ok(crate::engine::agent::record_action(&params, &result))
}

/// 读取动作历史（最新在前）。
#[tauri::command]
pub async fn agent_action_history(
    _app: AppHandle,
    limit: Option<usize>,
) -> Vec<crate::engine::agent::ActionHistoryItem> {
    crate::engine::agent::action_history_list(limit)
}

/// 回放指定步号 / 重试最后失败动作（都要新的 selector 可覆盖）。
/// 统一走 execute_agent_action（铁律：重放也必须过白名单 + 当前标签）。
#[tauri::command]
pub async fn agent_action_replay_or_retry(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, crate::tabs::TabManager>,
    step: Option<u64>,
    retry_last: Option<bool>,
) -> CommandResult<String> {
    let step_no = if retry_last.unwrap_or(false) {
        crate::engine::agent::last_failed_step()?
    } else {
        step.ok_or_else(|| AppError::validation("需要 step 或 retry_last=true"))?
    };
    let params = crate::engine::agent::rebuild_params(step_no)?;
    let action = params.action.clone();
    // 重新走执行链（参数已按历史重建；保持每步结果返回）
    let r = execute_agent_action(&app, &window, &state, &params).await;
    match &r {
        Ok(txt) => crate::engine::agent::record_action(&params, txt),
        Err(e) => crate::engine::agent::record_action(&params, &format!("失败: {}", e)),
    };
    let verb = if retry_last.unwrap_or(false) {
        "重试"
    } else {
        "回放"
    };
    match r {
        Ok(txt) => Ok(format!("{}第 {} 步（{}）→ {}", verb, step_no, action, txt)),
        // 保留原始错误码（超时/白名单等分类不因回放包装丢失），仅附加上下文
        Err(e) => Err(CommandError {
            code: e.code,
            message: format!("{}第 {} 步（{}）失败: {}", verb, step_no, action, e.message),
        }),
    }
}

/// 整批重试：把历史里所有失败步重建参数 → 走多步确认链 → 按序执行。
/// 铁律①：整体一次人工确认（复用 request_multi_step_action），确认后由
/// execute_multi_step_action 逐条走 execute_agent_action（白名单 + 仅当前标签）。
#[tauri::command]
pub async fn agent_actions_batch_retry(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, crate::tabs::TabManager>,
) -> CommandResult<String> {
    let pairs = crate::engine::agent::failed_action_params()?;
    let plan = crate::engine::agent::batch_retry_plan(&pairs);
    let approved = request_multi_step_action(&app, &plan).await?;
    if !approved {
        return Ok("整批重试已被用户拒绝".to_string());
    }
    execute_multi_step_action(&app, &window, &state, &plan).await
}

/// 校验 Agent 导航 URL 是否符合安全护栏（U-2 规约：纯函数，供 execute_agent_action 与单测共用）。
///
/// 规约（对标 Brave 诀窍 5 第 3 条）：
/// - 仅允许 `https` scheme（非 HTTPS/http 均拒绝，防明文窃听/篡改；仅在非 strict 下保留测试，默认拒绝非 HTTPS）；
/// - 严禁 `about:` / `chrome:` / `javascript:` / `data:` / `file:` / `view-source:` 等内部特权页；
/// - 严禁扩展商店域名（如 `chrome.google.com/webstore`, `chromewebstore.google.com`, `addons.mozilla.org`）；
/// - 拒绝内网地址（与既有 net_guard 同口径）。
pub fn validate_agent_nav_url(url_str: &str) -> CommandResult<url::Url> {
    let parsed = url::Url::parse(url_str)
        .map_err(|e| AppError::validation(format!("网址不合法: {e}")))?;

    // 1. 严格 scheme 校验：仅允许 https（严禁 about/chrome/file/javascript 等）
    match parsed.scheme() {
        "https" => {}
        "about" | "chrome" | "javascript" | "data" | "file" | "view-source" => {
            return Err(AppError::permission(format!(
                "Agent 禁入内部/敏感页面 scheme: {}",
                parsed.scheme()
            ))
            .into());
        }
        _ => {
            return Err(AppError::permission(format!(
                "Agent 仅允许访问 HTTPS 安全页面，拒绝 scheme: {}",
                parsed.scheme()
            ))
            .into());
        }
    }

    let host = parsed.host_str().unwrap_or("").to_lowercase();
    if host.is_empty() {
        return Err(AppError::validation("网址缺少主机名").into());
    }

    // 2. 严禁扩展商店
    if host.contains("chromewebstore.google.com")
        || (host.ends_with("chrome.google.com") && parsed.path().starts_with("/webstore"))
        || host.ends_with("addons.mozilla.org")
    {
        return Err(AppError::permission(format!(
            "Agent 严禁访问浏览器扩展商店: {}",
            host
        ))
        .into());
    }

    // 3. 内网守卫（net_guard host_permitted 校验）
    if !crate::net_guard::host_permitted(&host) {
        return Err(AppError::permission(format!("Agent 不允许导航到内网地址：{host}")).into());
    }

    Ok(parsed)
}

pub async fn execute_agent_action(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    state: &crate::tabs::TabManager,
    params: &crate::engine::agent::AgentActionParams,
) -> CommandResult<String> {
    let action = params.action.as_str();

    // 后端动作（不依赖页面 JS）
    match action {
        "wait" => {
            tokio::time::sleep(std::time::Duration::from_millis(
                params.ms.unwrap_or(500).min(10000),
            ))
            .await;
            return Ok(format!("已等待 {} 毫秒", params.ms.unwrap_or(500)));
        }
        "navigate" => {
            let url = params
                .url
                .clone()
                .ok_or_else(|| AppError::validation("缺少 url"))?;
            let id = state
                .runtime_snapshot()
                .1
                .ok_or_else(|| AppError::validation("没有活动标签"))?;
            let label = format!("tab-{}", id);
            let wv = window
                .get_webview(&label)
                .ok_or_else(|| AppError::internal("活动标签的 webview 不存在"))?;

            // Agent 导航守卫（U-2 规约：纯函数护栏校验 + 异步 DNS 解析 SSRF 校验）
            let parsed = validate_agent_nav_url(&url)?;
            let host = parsed.host_str().unwrap_or("").to_string();

            if host.parse::<std::net::IpAddr>().is_err() && !host.is_empty() {
                let port = parsed.port_or_known_default().unwrap_or(443);
                let sock: Vec<std::net::SocketAddr> =
                    tokio::net::lookup_host((host.as_str(), port))
                        .await
                        .map_err(|_| AppError::net(format!("域名解析失败，已拒绝：{host}")))?
                        .collect();
                // SSRF 双答防护：拒绝类错误统一 E_PERM（同 error.rs 表：SSRF→E_PERM）
                crate::ai::check_resolved_ips(&host, sock.iter().map(|s| s.ip()), port)
                    .map_err(AppError::permission)?;
            }
            wv.navigate(parsed)
                .map_err(|e| AppError::net(format!("导航失败: {e}")))?;
            // 存量同步（与 navigate_tab/切标签兑现同口径）：AI 驱使跳转后存的 url
            // 若还是旧的，标签栏与地址栏就两岔；加载完成信号会再对一次，这里先对齐。
            state.mutate(|s| {
                if let Some(t) = s.tabs.get_mut(&id) {
                    t.url = url.clone();
                    t.title = url.clone();
                }
            });
            let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
            return Ok(format!("已跳转到 {}", url));
        }
        "back" => {
            let id = state
                .runtime_snapshot()
                .1
                .ok_or_else(|| AppError::validation("没有活动标签"))?;
            let label = format!("tab-{}", id);
            let wv = window
                .get_webview(&label)
                .ok_or_else(|| AppError::internal("活动标签的 webview 不存在"))?;
            wv.eval("history.back()")
                .map_err(|e| AppError::internal(format!("后退失败: {e}")))?;
            return Ok("已后退一页".to_string());
        }
        "snapshot" => {
            // content_snapshot 的三处 Err 均为服务端状态异常（未初始化/webview 缺失）
            let snap = crate::content_cmd::content_snapshot(app.clone())
                .await
                .map_err(AppError::from)?;
            // 用 serde_json 序列化快照给 AI（限制长度）
            let text = serde_json::to_string(&snap).unwrap_or_default();
            return Ok(text.chars().take(3000).collect());
        }
        _ => {}
    }

    // 页面 JS 动作（click/fill/scroll/read/submit）
    let js = crate::engine::agent::build_action_js(params)?
        .ok_or_else(|| AppError::internal("动作未生成脚本"))?;
    let id = state
        .runtime_snapshot()
        .1
        .ok_or_else(|| AppError::validation("没有活动标签"))?;
    let label = format!("tab-{}", id);
    let wv = window
        .get_webview(&label)
        .ok_or_else(|| AppError::internal("活动标签的 webview 不存在"))?;

    // 只发起 JS 执行（with_webview 闭包内不阻塞），结果异步回来
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &js, move |res| {
            let _ = tx.send(res);
        });
    })
    .map_err(|e| AppError::internal(format!("动作执行失败: {e}")))?;

    let raw = rx
        .recv_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::WEBVIEW_IPC_SECS,
        ))
        .map_err(|e| AppError::timeout(format!("动作执行超时: {e}")))?
        .map_err(|e| AppError::internal(format!("页面脚本执行失败: {e}")))?;
    crate::engine::agent::parse_js_result(&raw)
}

/// 手动执行动作（前端"测试"用，绕过 AI 确认——仅供调试，生产由 AI 循环走确认流程）
/// 注意：此命令不经过确认，仅限设置页调试入口使用。
#[allow(clippy::too_many_arguments)]
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
) -> CommandResult<String> {
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
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    execute_agent_action(&app, &window, &state, &params).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 防漂移：确认不存在/过期 request_id 必须是 E_VALIDATION
    /// （前端据此提示"请求已过期"，而不是当成系统故障）
    #[test]
    fn confirm_unknown_request_is_validation() {
        let err = agent_confirm_action("no-such-request".to_string(), true).unwrap_err();
        assert_eq!(err.code, crate::error::E_VALIDATION);
        let err = agent_confirm_multi_step("no-such-request".to_string(), false).unwrap_err();
        assert_eq!(err.code, crate::error::E_VALIDATION);
    }

    /// U-1 验证：单步动作人工确认管道正确接收用户允许与拒绝
    #[tokio::test]
    async fn single_action_confirm_approved_and_rejected() {
        // 允许通道测试
        let req_id = "test-req-allow".to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        pending().lock().unwrap().insert(req_id.clone(), tx);

        let res = agent_confirm_action(req_id, true);
        assert!(res.is_ok());
        let approved = rx.await.expect("channel failed");
        assert!(approved, "用户允许应传达 true");

        // 拒绝通道测试（拒绝必须中止）
        let req_id_deny = "test-req-deny".to_string();
        let (tx2, rx2) = tokio::sync::oneshot::channel();
        pending().lock().unwrap().insert(req_id_deny.clone(), tx2);

        let res2 = agent_confirm_action(req_id_deny, false);
        assert!(res2.is_ok());
        let approved2 = rx2.await.expect("channel failed");
        assert!(!approved2, "用户拒绝应传达 false 并导致动作中止");
    }

    /// U-1 验证：多步计划人工确认管道正确接收用户确认与拒绝
    #[tokio::test]
    async fn multi_step_confirm_approved_and_rejected() {
        let req_id = "test-multi-req-allow".to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        pending().lock().unwrap().insert(req_id.clone(), tx);

        let res = agent_confirm_multi_step(req_id, true);
        assert!(res.is_ok());
        assert!(rx.await.expect("channel failed"), "多步计划允许应为 true");

        let req_id_deny = "test-multi-req-deny".to_string();
        let (tx2, rx2) = tokio::sync::oneshot::channel();
        pending().lock().unwrap().insert(req_id_deny.clone(), tx2);

        let res2 = agent_confirm_multi_step(req_id_deny, false);
        assert!(res2.is_ok());
        assert!(!rx2.await.expect("channel failed"), "多步计划拒绝应为 false");
    }

    /// U-1 验证：白名单外动作直接被白名单拒绝（E_PERM），不能执行
    #[test]
    fn agent_action_params_whitelist_enforced() {
        let p = crate::engine::agent::AgentActionParams {
            action: "malicious_script_eval".to_string(),
            selector: None,
            value: None,
            direction: None,
            url: None,
            ms: None,
        };
        let res = crate::engine::agent::build_action_js(&p);
        assert!(res.is_err(), "非白名单动作必须被拒绝");
        assert_eq!(res.unwrap_err().code, crate::error::E_PERM);
    }

    /// U-2 验证：Agent 浏览访问限制守卫
    #[test]
    fn test_validate_agent_nav_url_guards() {
        // 1. 合法 HTTPS 允许
        assert!(validate_agent_nav_url("https://example.com/test").is_ok());

        // 2. 拒绝非 HTTPS (如 http)
        let http_err = validate_agent_nav_url("http://example.com/test").unwrap_err();
        assert_eq!(http_err.code, crate::error::E_PERM);

        // 3. 拒绝 about: / chrome: / javascript: / file: 等内部特权页
        for internal in &[
            "about:blank",
            "chrome://settings",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,<h1>hi</h1>",
        ] {
            let err = validate_agent_nav_url(internal).unwrap_err();
            assert_eq!(err.code, crate::error::E_PERM, "必须拒绝 {}", internal);
        }

        // 4. 拒绝扩展商店
        for store in &[
            "https://chromewebstore.google.com/detail/123",
            "https://chrome.google.com/webstore/category/extensions",
            "https://addons.mozilla.org/zh-CN/firefox/",
        ] {
            let err = validate_agent_nav_url(store).unwrap_err();
            assert_eq!(err.code, crate::error::E_PERM, "必须拒绝扩展商店 {}", store);
        }

        // 5. 拒绝内网（localhost / 127.0.0.1）
        for lan in &["https://localhost/admin", "https://127.0.0.1:8080/"] {
            let err = validate_agent_nav_url(lan).unwrap_err();
            assert_eq!(err.code, crate::error::E_PERM, "必须拒绝内网地址 {}", lan);
        }
    }
}
