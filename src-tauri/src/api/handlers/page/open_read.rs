//! 「打开并读取」一体化 handler（R1 三次拆分 · 2026-09-25）：
//! 开标签 → 等加载就绪（含 SPA 内容稳定判定）→ 重定向 SSRF 复核 → 读取。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

use super::read::read_page;
use super::util::probe_page;

/// 移动版 UA（M 类响应式读取）：`open_and_read` 传 `ua:"mobile"` 时启用，
/// 便于读取移动版更简洁的页面（闲鱼/淘宝等）。版本与默认 UA 对齐。
const MOBILE_UA: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Mobile Safari/537.36";

/// open_and_read 的内网 SSRF 守卫（入口 + 最终 URL 复核共用）。
///
/// 返回 `Ok(())` 放行；`Err(响应)` 为拒绝（非 http/https、或内网地址且未进白名单）。
async fn guard_url(app: &AppHandle, url: &str) -> Result<(), Value> {
    let Ok(parsed) = tauri::Url::parse(url) else {
        return Err(AppError::validation("URL 解析失败").error_json());
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::permission(format!(
            "open_and_read 只接受 http/https（收到 {}）",
            parsed.scheme()
        ))
        .error_json());
    }
    let Some(host) = parsed.host_str().map(|h| h.to_string()) else {
        return Err(AppError::validation("URL 缺少主机名").error_json());
    };
    // 内网 / 私有地址：查库取白名单（同时刷新进程内缓存，供 GTK 导航守卫同步读取）
    let allowlist: String = {
        let db = app.state::<crate::db::Db>();
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'api_allowed_ips'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    crate::net_guard::set_allowlist(&allowlist);
    if !crate::net_guard::host_permitted(&host) {
        return Err(AppError::permission(format!(
            "内网 / 私有地址默认拒绝：{host}（命中规则：私有地址字符串判定；如确需读取，请在 api_allowed_ips 白名单中显式放行，例如 192.168.1.0/24）"
        ))
        .error_json());
    }
    // DNS 解析复核（与 reader/fetch.rs 同口径）：域名恒放行是字符串检查的盲区，
    // 双答 DNS（公网+内网混答）或解析后翻转会绕过。解析全部结果逐一校验，
    // 任一落内网或解析失败直接拒绝（fail-closed）。
    // 注意：WebKit 加载时还会自行解析（浏览器导航无法像 reqwest/curl 那样 pin 死），
    // 入口 + 最终 URL 双次复核把窗口压到最小，残余风险见 net_guard.rs 模块注释。
    if host.parse::<std::net::IpAddr>().is_err() {
        let port = parsed.port_or_known_default().unwrap_or(80);
        // DNS 超时（2026-09-29）：lookup_host 无内置超时，包 DNS_LOOKUP_SECS，
        // 超时按解析失败拒绝（fail-closed）
        let addrs: Vec<std::net::SocketAddr> = tokio::time::timeout(
            std::time::Duration::from_secs(crate::contracts::timeout::DNS_LOOKUP_SECS),
            tokio::net::lookup_host((host.as_str(), port)),
        )
        .await
        .map_err(|_| {
            AppError::permission(format!(
                "域名解析超时（{}s），已拒绝：{host}",
                crate::contracts::timeout::DNS_LOOKUP_SECS
            ))
            .error_json()
        })?
        .map_err(|_| AppError::permission(format!("域名解析失败，已拒绝：{host}")).error_json())?
        .collect();
        if let Err(e) = crate::ai::check_resolved_ips(&host, addrs.iter().map(|s| s.ip()), port) {
            log::warn!("[open_and_read] SSRF 拦截域名解析: {}", e);
            return Err(AppError::permission(e).error_json());
        }
    }
    Ok(())
}

/// 读取前可达验证（DNS rebinding 纵深，2026-09-29）：
/// 入口解析、WebKit 加载、最终复核是三次独立 DNS 查询，TTL=0 攻击可让复核看公网、
// 加载走内网。此处把 final_url 域名 pin 到本次解析出的公网 IP 并发 HEAD 验证——
/// 只有该公网 IP 当前真实可达才放行读取。任何 HTTP 响应（2xx/3xx/4xx/5xx）都算可达，
/// 只有建连/解析/超时失败才拦（正常公网站点无误报）。
/// IP 字面量无 rebinding 面（pin 返回 None），直接放行（私有的入口 guard 已拦）。
async fn verify_reachable(url: &str) -> Result<(), Value> {
    let (resolve_key, pinned) = crate::ai::resolve_pin_for_https(url)
        .await
        .map_err(|e| AppError::permission(format!("可达验证解析失败，已拒绝：{e}")).error_json())?;
    let Some(addrs) = pinned else {
        return Ok(());
    };
    let builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::HTTP_READ_TIMEOUT_SECS,
        ))
        .connect_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::CONNECT_TIMEOUT_SECS,
        ))
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(&resolve_key, &addrs);
    let client = builder
        .build()
        .map_err(|e| AppError::net(format!("可达验证客户端创建失败: {}", e)).error_json())?;
    match client.head(url).send().await {
        Ok(_) => Ok(()),
        Err(e) => Err(AppError::permission(format!(
            "可达验证失败（pin 的公网 IP 不可达，可能 DNS 已翻转）：{e}"
        ))
        .error_json()),
    }
}

/// 打开 URL 并读取页面（P0 杀招命令 · 方案 §4.1）。
///
/// 一次调用完成「开标签 → 等加载 → 读页面」，返回统一结构（tab_id/final_url/meta/read）。
/// 登记 `Risk::Read`（语义读）→ api_readonly 下放行（P2 核心拍板：能看不能动）。
/// 一期实现：SSRF 内网守卫 + 等就绪 + 读取 + `close` + `reuse`；`ua` / 重定向逐跳校验留待后续切片。
pub(crate) async fn open_and_read(app: &AppHandle, req: &Value) -> Value {
    let url = req
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        return AppError::validation("缺少 url").error_json();
    }
    if let Err(resp) = guard_url(app, &url).await {
        return resp;
    }
    let close_after = req.get("close").and_then(|v| v.as_bool()).unwrap_or(false);
    let reuse = req.get("reuse").and_then(|v| v.as_bool()).unwrap_or(false);

    // 1) 决定目标标签：reuse 命中已有同 URL 的 AI 标签 → 直接复用；否则开新标签
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let reused_id = if reuse {
        state
            .all_tabs()
            .into_iter()
            .find(|t| t.origin == "ai" && t.url == url)
            .map(|t| t.id)
    } else {
        None
    };
    let (tab_id, reused) = match reused_id {
        Some(id) => (id, true),
        None => {
            // AI 标签上限（J 类）：默认 10，超限拒绝（提示先 tabs/close_ai 回收），防标签堆爆
            const AI_TAB_LIMIT: usize = 10;
            if state.ai_tab_count() >= AI_TAB_LIMIT {
                return AppError::permission(format!(
                    "AI 标签已达上限 {AI_TAB_LIMIT}（先调用 tabs/close_ai 回收再试）"
                ))
                .error_json();
            }
            let tab = match crate::tabs::commands::create_tab(
                app.clone(),
                state,
                url.clone(),
                Some(true), // U-3：AI 标签走独立 ephemeral WebContext，Cookie/缓存/存储与主 Profile 零交叉隔离
                None,
                Some(true), // J6：AI 标签后台打开，不抢用户焦点
                None,       // AI 标签读内容不能等，不延迟（probe 紧接着读页）
            )
            .await
            {
                Ok(t) => t,
                Err(e) => return AppError::from(e).error_json(),
            };
            let id = tab.id.clone();
            // 标记 AI 来源（J 类）：供 tabs/close_ai 回收、AI 上限计数、看板区分
            app.state::<crate::tabs::TabManager>().set_origin(&id, "ai");
            (id, false)
        }
    };

    // ua 覆盖（M 类响应式）：mobile 时切移动版 UA 并重载，读移动版（闲鱼/淘宝类更简洁）
    let ua_mode = req.get("ua").and_then(|v| v.as_str()).unwrap_or("desktop");
    let ua_mobile = ua_mode == "mobile";
    if ua_mobile {
        if let Some(wv) = app.get_webview(&format!("tab-{}", tab_id)) {
            let ua = MOBILE_UA.to_string();
            crate::log_if_err!(
                "webview",
                wv.with_webview(move |pwv| {
                    use webkit2gtk::{SettingsExt, WebViewExt};
                    if let Some(settings) = pwv.inner().settings() {
                        settings.set_user_agent(Some(&ua));
                    }
                })
            );
            if let Some(st) = app.try_state::<crate::tabs::TabManager>() {
                crate::log_if_err!(
                    "tabs",
                    crate::tabs::commands::reload_tab(app.clone(), st, tab_id.clone()).await
                );
            }
        }
    }

    // 2) 等加载就绪：复用标签视为已就绪（探测一次）；新开标签轮询 readyState（最多 ~8s）
    let mut ready_state = "unknown".to_string();
    let mut final_url = url.clone();
    if reused {
        if let Some(p) = probe_page(app, &tab_id).await {
            ready_state = p
                .get("ready_state")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();
            if let Some(u) = p.get("url").and_then(|s| s.as_str()) {
                if !u.is_empty() {
                    final_url = u.to_string();
                }
            }
        }
    } else {
        // 等就绪：需满足 ① URL 已是 http/https（排除初始 about:blank）
        // ② readyState === complete ③ **文本长度连续两次稳定**（SPA 的 complete ≠ 渲染完；
        //    gitee/36kr 等客户端渲染站点在 complete 时内容还是空的）。
        // 修复 2026-09-25：批量测试暴露 SPA 读到空/骨架。
        let mut prev_len: i64 = -1;
        let mut stable_hits = 0;
        for _ in 0..60 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if let Some(p) = probe_page(app, &tab_id).await {
                let rs = p
                    .get("ready_state")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                if let Some(u) = p.get("url").and_then(|s| s.as_str()) {
                    if !u.is_empty() {
                        final_url = u.to_string();
                    }
                }
                ready_state = rs.clone();
                let len = p.get("text_len").and_then(|n| n.as_i64()).unwrap_or(-1);
                let url_is_http =
                    final_url.starts_with("http://") || final_url.starts_with("https://");
                if url_is_http && rs == "complete" {
                    if len > 0 && len == prev_len {
                        stable_hits += 1;
                        // 连续两次长度一致 → 认为内容已稳定（约 400ms 无变化）
                        if stable_hits >= 2 {
                            break;
                        }
                    } else {
                        stable_hits = 0;
                    }
                    prev_len = len;
                }
            }
        }
    }

    // 2b) O1 重定向逐跳/最终 URL 复核：加载后的真实 URL 无条件再跑一遍 SSRF 守卫。
    //     公开页跳转到内网（重定向绕过入口校验）→ 读取前拦截并回收，绝不让内网内容外流。
    //     注意：仅当 final_url 是 http/https 才复核——新标签未加载完时为 `about:blank`
    //     （非 http），不应触发拦截（修复 2026-09-25：曾误伤所有读取）。
    //     DNS rebinding（2026-09-29 修复）：不能只复核「URL 文本变化」的情况——攻击者用
    //     双答 DNS 让入口解析到公网、WebKit 加载时解析到内网，此时 final_url 文本与入口
    //     相同但仍实际连接了内网。故无条件复核（每次重新解析 + 校验），把窗口压到最小。
    let final_is_http = final_url.starts_with("http://") || final_url.starts_with("https://");
    if final_is_http {
        if let Err(resp) = guard_url(app, &final_url).await {
            // 复核失败：回收刚开的标签（复用标签不关，避免误关用户标签）
            if !reused {
                if let Some(st) = app.try_state::<crate::tabs::TabManager>() {
                    crate::log_if_err!(
                        "tabs",
                        crate::tabs::commands::close_tab(app.clone(), st, tab_id.clone()).await
                    );
                }
            }
            let msg = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("重定向到内网地址被拦截");
            return AppError::permission(format!("重定向复核拦截（{final_url}）：{msg}"))
                .error_json();
        }
    }

    // 2c) 读取前可达验证（DNS rebinding 纵深）：复核解析与 WebKit 实际加载仍是两次
    //     独立查询，TTL=0 攻击可让复核看公网、加载走内网。此处 pin 公网 IP 发 HEAD 验证。
    //     失败降级为警告放行（2026-09-29 实测）：CDN IP 轮换/边缘节点波动导致偶发
    //     连接失败，百度/B站/微博等 1/3 正常站点被误拦；入口+复核两道 fail-closed
    //     还在，可达验证作为纵深只告警不拦截。标签不回收，直接读取。
    if final_is_http {
        if let Err(resp) = verify_reachable(&final_url).await {
            let msg = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("可达验证失败");
            log::warn!("[open_and_read] 可达验证未通过，放行读取（{final_url}）：{msg}");
        }
    }

    // 3) 读取（复用 read_page，显式指定刚开的 tab_id，避免活动标签漂移读错）
    let mut read_req = json!({
        "mode": req.get("mode").cloned().unwrap_or(json!("text")),
        "selector": req.get("selector").cloned().unwrap_or(json!("")),
        "tab_id": tab_id.clone(),
        "max_chars": req.get("max_chars").cloned().unwrap_or(json!(200000)),
        "max_items": req.get("max_items").cloned().unwrap_or(json!(500)),
    });
    if let Some(obj) = read_req.as_object_mut() {
        obj.insert("cmd".to_string(), json!("read_page"));
    }
    let read_resp = read_page(app, &read_req).await;
    let read_ok = read_resp
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 4) 可选关闭
    if close_after {
        if let Some(st) = app.try_state::<crate::tabs::TabManager>() {
            crate::log_if_err!(
                "tabs",
                crate::tabs::commands::close_tab(app.clone(), st, tab_id.clone()).await
            );
        }
    }

    // final_url：优先用 read_page 采集到的真实 URL（探测时可能还是 about:blank）
    let final_url_resp = {
        let from_read = read_resp
            .get("data")
            .and_then(|d| d.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or("");
        if from_read.starts_with("http") {
            from_read.to_string()
        } else {
            final_url.clone()
        }
    };
    let data = json!({
        "tab_id": tab_id,
        "url": url,
        "final_url": final_url_resp,
        "title": read_resp.get("data").and_then(|d| d.get("title")).cloned().unwrap_or(json!("")),
        "ready_state": ready_state,
        "closed": close_after,
        "reused": reused,
        "ua": if ua_mobile { "mobile" } else { "desktop" },
        "read_ok": read_ok,
        "content_type": "page",
        "read": if read_ok { read_resp.get("data").cloned().unwrap_or(Value::Null) } else { Value::Null },
        "read_error": if read_ok { Value::Null } else { read_resp.get("error").cloned().unwrap_or(Value::Null) },
    });
    json!({"ok": true, "data": data})
}
