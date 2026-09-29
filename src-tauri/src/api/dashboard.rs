// 状态面板 + 模型网关（R2-1c 拆分：从 api.rs 抽出）
//
// 状态面板登录/主页 HTML、API token 轮换与一次性门票签发命令、
// OpenAI 兼容模型网关转发。依赖 api.rs 的 auth 辅助（super:: 访问）。

use serde_json::json;
use tauri::{AppHandle, Manager};

use super::auth::{atomic_write_token, generate_token, ApiTokenShared, DashboardTickets};
use crate::error::AppError;

/// 状态面板登录页（无脚本、严格 CSP；长期 token 只接受 POST 表单）
pub(crate) fn dashboard_login_html(error: Option<&str>, retry_after: u64) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let banner = if retry_after > 0 {
        format!("<p class=err>认证失败过多，请 {retry_after} 秒后再试</p>")
    } else {
        String::new()
    };
    let error_html = error
        .map(|e| format!("<p class=err>{}</p>", esc(e)))
        .unwrap_or_default();
    r##"<!doctype html><meta charset=utf-8><title>ShiftBrowser API 登录</title>
<style>body{font:14px/1.6 system-ui,sans-serif;max-width:560px;margin:64px auto;padding:0 16px;color:#222}
h1{font-size:20px} .card{border:1px solid #ddd;border-radius:10px;padding:18px 20px;margin:12px 0}
input{width:100%;box-sizing:border-box;padding:8px 10px;margin:10px 0;font:13px monospace;border:1px solid #ccc;border-radius:6px}
button{padding:8px 18px;border:0;border-radius:6px;background:#2f6fed;color:#fff;font-size:14px;cursor:pointer}
.hint{color:#888;font-size:12px;line-height:1.7} .err{color:#c0392b} code{background:#f2f2f2;padding:1px 5px;border-radius:4px}</style>
<h1>ShiftBrowser API</h1>
<div class=card>
<form method="post" action="/">
  <label>粘贴 API token（或一次性门票）</label>
  <input name="token" autocomplete="off" autofocus required placeholder="api-token 文件内容 / 应用内签发的门票">
  <button type="submit">打开状态面板</button>
</form>
{BANNER}{ERROR}
<p class=hint>长期 token 不再支持放在 URL 查询串（<code>?token=</code> 会落入浏览器历史、Referer 与代理日志）。
两种安全方式：① 在此表单粘贴（POST body，不进 URL）；② 在应用「设置 → 开放 API」点「打开状态面板」，
自动签发 5 分钟有效、用后即焚的一次性门票。</p>
</div>"##
        .replace("{BANNER}", &banner)
        .replace("{ERROR}", &error_html)
}

/// 状态面板 HTML（自托管套件的"管理页"；凭据见 spawn_http 中三种安全方式）
pub(crate) async fn dashboard_html(app: &AppHandle) -> String {
    let (n, active) = match app.try_state::<crate::tabs::TabManager>() {
        Some(s) => {
            let all = s.all_tabs();
            let a = all
                .iter()
                .find(|t| t.is_active)
                .map(|t| (t.title.clone(), t.url.clone()));
            (all.len(), a)
        }
        None => (0, None),
    };
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let active_html = match active {
        Some((t, u)) => format!(
            "<div><b>{}</b><br><span class=u>{}</span></div>",
            esc(&t),
            esc(&u)
        ),
        None => "<div>（无活动标签）</div>".to_string(),
    };
    let cmds = "status · list_tabs · active_tab · history · bookmarks · search_history · \
                list_downloads · list_sessions · list_groups · list_spaces · list_userscripts · \
                get_setting · list_settings · open_url · close_tab · switch_tab · reload_tab · \
                navigate · search · new_tab · screenshot · pin_tab · mute_tab · \
                minimize_window · maximize_window · read_page · eval（默认关）· tools · tool_call";
    format!(
        r#"<!doctype html><meta charset=utf-8><title>ShiftBrowser 状态</title>
<style>body{{font:14px/1.6 system-ui,sans-serif;max-width:760px;margin:32px auto;padding:0 16px;color:#222}}
h1{{font-size:20px}} .u{{color:#888;font-size:12px}} code{{background:#f2f2f2;padding:1px 5px;border-radius:4px}}
.card{{border:1px solid #ddd;border-radius:10px;padding:14px 16px;margin:12px 0}}</style>
<h1>ShiftBrowser 状态面板</h1>
<div class=card><b>版本</b>：{ver}　<b>标签数</b>：{n}</div>
<div class=card><b>当前活动标签</b>{active}</div>
<div class=card><b>开放 API 命令</b><br><code>{cmds}</code></div>
<div class=card><b>用法</b>：<code>POST /api</code> + <code>Authorization: Bearer &lt;token&gt;</code>；详见 <code>docs/local-api.md</code></div>
"#,
        ver = env!("CARGO_PKG_VERSION"),
        n = n,
        active = active_html,
        cmds = cmds,
    )
}

/// 轮换 API token（HANDOFF P2-3）：生成新 token 原子落盘并热替换运行期状态，
/// 旧 token 在下一请求立即失效（HTTP 服务无需重启）。返回新 token，仅本次展示。
#[tauri::command]
pub(crate) async fn rotate_api_token(
    app: AppHandle,
    tokens: tauri::State<'_, ApiTokenShared>,
) -> Result<String, String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or_else(|| "数据库未就绪".to_string())?;
    let path = db.vault_path.with_file_name("api-token");
    let new = generate_token();
    atomic_write_token(&path, &new).map_err(|e| format!("写入 api-token 失败: {e}"))?;
    *tokens.0.write().await = new.clone();
    log::info!("[api] API token 已轮换（旧 token 即时失效）");
    Ok(new)
}

/// 签发状态面板一次性门票：32hex 随机，5 分钟有效、兑换一次即焚。
/// 前端拿到后拼 `http://127.0.0.1:<port>/?ticket=<t>` 用外部浏览器打开。
#[tauri::command]
pub(crate) fn create_dashboard_ticket(
    tickets: tauri::State<'_, DashboardTickets>,
) -> Result<String, String> {
    Ok(tickets.issue())
}

/// 模型网关：把 OpenAI 兼容的 chat/completions 请求转发到设置里的 provider。
/// 默认关（设置 `api_model_gateway` 需为 'true'）；复用设置里的 base_url / key / model。
pub(crate) async fn model_gateway(app: &AppHandle, body: &str) -> serde_json::Value {
    let db = app.state::<crate::db::Db>();
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT key, value FROM settings WHERE key IN \
         ('api_model_gateway','ai_base_url','ai_api_key','ai_model')",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap_or_default();
    let m: std::collections::HashMap<String, String> = rows.into_iter().collect();
    if m.get("api_model_gateway").map(|v| v.as_str()) != Some("true") {
        return AppError::permission("模型网关已禁用：需在设置里显式开启「允许模型网关」")
            .gateway_error_json();
    }
    let base = m
        .get("ai_base_url")
        .cloned()
        .unwrap_or_else(|| "https://apihub.agnes-ai.com/v1".to_string());
    let key = m
        .get("ai_api_key")
        .cloned()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("AGNES_API_KEY").ok())
        .unwrap_or_default();
    if key.is_empty() {
        return AppError::auth("未配置 AI API Key（设置里填或设 AGNES_API_KEY）")
            .gateway_error_json();
    }
    let model = m
        .get("ai_model")
        .cloned()
        .unwrap_or_else(|| "agnes-2.0-flash".to_string());

    let mut payload: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            return AppError::validation(format!("请求 JSON 非法: {}", e)).gateway_error_json()
        }
    };
    if payload.get("model").is_none() {
        if let Some(o) = payload.as_object_mut() {
            o.insert("model".to_string(), json!(model));
        }
    }
    // SSRF 防护（P0-5，2026-09-24）：网关转发携带 API Key，上游地址必须与
    // AI 聊天同一校验口径——https-only、拒绝 userinfo、拒绝本机/内网字面地址
    // （含 IPv4 混淆与 IPv4-mapped IPv6 绕过）。否则持 token 的调用方可把
    // 网关变成携带密钥打内网的跳板。
    let base = base.trim();
    if let Err(e) = crate::ai::validate_base_url(base) {
        log::warn!("[api] 模型网关上游地址被安全策略拒绝: {} ({})", e, crate::redact_url(base));
        return AppError::permission(format!("上游地址未通过安全校验: {e}")).gateway_error_json();
    }
    // P0-6：域名解析后 pin 死公网地址，消除"校验时解析一次、发送时再解析一次"
    // 的 DNS-rebinding TOCTOU（双答 DNS 在内网混答时直接拒绝）。
    let (resolve_key, pinned) = match crate::ai::resolve_pin_for_https(base).await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[api] 模型网关注解/pin 上游地址失败: {} ({})", e, crate::redact_url(base));
            return AppError::permission(format!("上游地址未通过安全校验: {e}"))
                .gateway_error_json();
        }
    };
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        // 建连上限（单边地址族卡住时快速让路给另一族，见 contracts::timeout::CONNECT_TIMEOUT_SECS）
        .connect_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::CONNECT_TIMEOUT_SECS,
        ))
        // SSRF（P0-5 同口径）：禁自动重定向——重定向目标不经过 validate_base_url /
        // resolve_pin_for_https 校验，跟随会把 API Key 与请求体带到未校验地址。
        // 与 feed.rs 一致（Policy::none），上游 chat/completions 返回 3xx 属异常，按错误处理。
        .redirect(reqwest::redirect::Policy::none());
    if let Some(addrs) = &pinned {
        builder = builder.resolve_to_addrs(&resolve_key, addrs);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => {
            return AppError::net(format!("创建 HTTP 客户端失败: {}", e)).gateway_error_json()
        }
    };
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    match client
        .post(&url)
        .bearer_auth(&key)
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => v,
            Err(e) => AppError::net(format!("解析上游响应失败: {}", e)).gateway_error_json(),
        },
        Err(e) => AppError::net(format!("上游请求失败: {}", e)).gateway_error_json(),
    }
}
