//! 标签与窗口命令 handler：查询/打开/关闭/切换/刷新/导航/搜索/固定/静音/窗口控制

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::error::AppError;

/// 取标签管理器（缺失统一 E_INTERNAL）
fn manager(app: &AppHandle) -> Result<State<'_, crate::tabs::TabManager>, Value> {
    app.try_state()
        .ok_or_else(|| AppError::internal("标签管理器不可用").error_json())
}

/// 取去空白后的非空字符串参数（空 → E_VALIDATION）
fn required_str(req: &Value, field: &str, err_msg: &str) -> Result<String, Value> {
    let v = req
        .get(field)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if v.is_empty() {
        Err(AppError::validation(err_msg).error_json())
    } else {
        Ok(v)
    }
}

pub(super) async fn list_tabs(app: &AppHandle, _req: &Value) -> Value {
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    json!({"ok": true, "data": state.all_tabs()})
}

pub(super) async fn active_tab(app: &AppHandle, _req: &Value) -> Value {
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let active = state.all_tabs().into_iter().find(|t| t.is_active);
    json!({"ok": true, "data": active})
}

pub(super) async fn open_url(app: &AppHandle, req: &Value) -> Value {
    let url = match required_str(req, "url", "缺少 url") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    match crate::tabs::commands::create_tab(app.clone(), state, url, None, None, None).await {
        Ok(t) => json!({"ok": true, "data": t}),
        Err(e) => AppError::from(e).error_json(),
    }
}

pub(super) async fn new_tab(app: &AppHandle, _req: &Value) -> Value {
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    // 注意：Rust 侧 create_tab 需要绝对 URL，"newtab" 会解析失败 → 用 shift://newtab
    match crate::tabs::commands::create_tab(
        app.clone(),
        state,
        crate::contracts::urls::NEWTAB.to_string(),
        None,
        None,
        None,
    )
    .await
    {
        Ok(t) => json!({"ok": true, "data": t}),
        Err(e) => AppError::from(e).error_json(),
    }
}

/// close/switch/reload 共用：取 id + manager，执行具体动作
async fn with_id(
    app: &AppHandle,
    req: &Value,
    action: impl for<'a> FnOnce(
        &'a AppHandle,
        State<'a, crate::tabs::TabManager>,
        String,
    )
        -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = crate::error::CommandResult<Option<crate::tabs::Tab>>>
                    + Send
                    + 'a,
            >,
        >,
) -> Value {
    let id = match required_str(req, "id", "缺少 id") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    match action(app, state, id).await {
        Ok(t) => json!({"ok": true, "data": t}),
        Err(e) => AppError::from(e).error_json(),
    }
}

pub(super) async fn close_tab(app: &AppHandle, req: &Value) -> Value {
    with_id(app, req, |a, s, id| {
        Box::pin(crate::tabs::commands::close_tab(a.clone(), s, id))
    })
    .await
}

/// 关闭所有 AI 标签（J 类治理）：只关 `origin=="ai"` 的标签，**绝不碰用户标签**。
/// AI 读完后回收自己开的标签，避免堆积污染用户标签栏。
pub(super) async fn close_ai(app: &AppHandle, _req: &Value) -> Value {
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let ids = state.ai_tab_ids();
    let mut closed: Vec<String> = Vec::new();
    for id in ids {
        if let Ok(st) = manager(app) {
            if crate::tabs::commands::close_tab(app.clone(), st, id.clone())
                .await
                .is_ok()
            {
                closed.push(id);
            }
        }
    }
    json!({"ok": true, "data": {"closed": closed, "count": closed.len()}})
}

pub(super) async fn switch_tab(app: &AppHandle, req: &Value) -> Value {
    with_id(app, req, |a, s, id| {
        Box::pin(crate::tabs::commands::switch_tab(a.clone(), s, id))
    })
    .await
}

pub(super) async fn reload_tab(app: &AppHandle, req: &Value) -> Value {
    with_id(app, req, |a, s, id| {
        Box::pin(crate::tabs::commands::reload_tab(a.clone(), s, id))
    })
    .await
}

pub(super) async fn navigate(app: &AppHandle, req: &Value) -> Value {
    let id = req
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let url = req
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if id.is_empty() || url.is_empty() {
        return AppError::validation("缺少 id 或 url").error_json();
    }
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    match crate::tabs::commands::navigate_tab(app.clone(), state, id, url).await {
        Ok(t) => json!({"ok": true, "data": t}),
        Err(e) => AppError::from(e).error_json(),
    }
}

pub(super) async fn search(app: &AppHandle, req: &Value) -> Value {
    let q = match required_str(req, "query", "缺少 query") {
        Ok(v) => v,
        Err(resp) => return resp,
    };
    // 读 search_engine 设置，拼搜索 URL（与 newtab/AddressBar 同映射）
    let engine: String = {
        let db = app.state::<crate::db::Db>();
        sqlx::query_as::<_, (String,)>(
            "SELECT value FROM settings WHERE key = 'search_engine'",
        )
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .map(|(v,)| v)
        .unwrap_or_else(|| crate::constants::search::DEFAULT_ENGINE.to_string())
    };
    // 搜索引擎 URL 模板来自 constants.rs（唯一来源，未知 id 自动回退默认引擎）
    let template = crate::constants::search::url_template(&engine);
    let url = format!("{}{}", template, crate::api::url_encode(&q));
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    match crate::tabs::commands::create_tab(app.clone(), state, url, None, None, None).await {
        Ok(t) => json!({"ok": true, "data": t}),
        Err(e) => AppError::from(e).error_json(),
    }
}

pub(super) async fn pin_tab(app: &AppHandle, req: &Value) -> Value {
    let id = req.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    match crate::tabs::commands::toggle_pin(state, id).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::from(e).error_json(),
    }
}

pub(super) async fn mute_tab(app: &AppHandle, req: &Value) -> Value {
    let id = req.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let Ok(state) = manager(app) else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    match crate::tabs::commands::toggle_mute(app.clone(), state, id).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::from(e).error_json(),
    }
}

pub(super) async fn minimize_window(app: &AppHandle, _req: &Value) -> Value {
    match crate::tabs::commands::minimize_window(app.clone()).await {
        Ok(_) => json!({"ok": true, "data": null}),
        Err(e) => AppError::from(e).error_json(),
    }
}

pub(super) async fn maximize_window(app: &AppHandle, _req: &Value) -> Value {
    match crate::tabs::commands::toggle_maximize(app.clone()).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::from(e).error_json(),
    }
}
