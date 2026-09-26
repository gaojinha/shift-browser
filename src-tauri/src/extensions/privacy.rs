//! 隐私防护（P1-3 补完 · 2026-09-22）
//!
//! 隐私统计 / 自检 / 加噪白名单判定。原在 extensions.rs。

use super::*;
use serde::Serialize;
use std::sync::atomic::Ordering;

#[derive(Debug, Serialize, Clone)]
pub struct PrivacyStats {
    pub cleaned_params: u64,
    pub privacy_injections: u64,
    pub adblock_injections: u64,
}

/// 读取隐私防护统计
#[tauri::command]
pub fn get_privacy_stats() -> PrivacyStats {
    PrivacyStats {
        cleaned_params: CLEANED_PARAMS.load(Ordering::Relaxed),
        privacy_injections: PRIVACY_INJECTIONS.load(Ordering::Relaxed),
        adblock_injections: ADBLOCK_INJECTIONS.load(Ordering::Relaxed),
    }
}

/// 重置隐私防护统计
#[tauri::command]
pub fn reset_privacy_stats() {
    CLEANED_PARAMS.store(0, Ordering::Relaxed);
    PRIVACY_INJECTIONS.store(0, Ordering::Relaxed);
    ADBLOCK_INJECTIONS.store(0, Ordering::Relaxed);
    log::info!("[privacy] 统计已重置");
}

// ---- 隐私防护自测（阶段 2「信任需要证明」）----

/// 单项自测结果
#[derive(Debug, Serialize, Clone)]
pub struct PrivacyCheck {
    /// 检查项标识（inject/gpc/canvas/...）
    pub id: String,
    /// 中文名
    pub label: String,
    /// 结果：pass / fail / skip
    pub status: String,
    /// 说明
    pub detail: String,
}

/// 隐私防护自测报告
#[derive(Debug, Serialize, Clone)]
pub struct PrivacySelfTestReport {
    /// 当前档位：off / standard / strict
    pub shield_level: String,
    /// 反指纹模式：noise（加噪）/ uniform（统一化）
    pub fp_mode: String,
    /// 活动标签域名
    pub hostname: String,
    /// 该域名是否在加噪白名单里
    pub whitelisted: bool,
    /// 检查项总数
    pub total: usize,
    /// 通过数
    pub passed: usize,
    /// 逐项结果
    pub checks: Vec<PrivacyCheck>,
}

fn mk_check(id: &str, label: &str, status: &str, detail: &str) -> PrivacyCheck {
    PrivacyCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        detail: detail.to_string(),
    }
}

/// 域名是否命中加噪白名单（与 PRIVACY_JS 内 NOISE_ON 判定同规则）
pub fn is_whitelisted(hostname: &str, whitelist: &[String]) -> bool {
    let h = hostname.trim().to_lowercase();
    if h.is_empty() {
        return false;
    }
    whitelist.iter().any(|w| {
        let d = w
            .to_lowercase()
            .trim_start_matches("*.")
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if d.is_empty() {
            return false;
        }
        h == d || h.ends_with(&format!(".{}", d))
    })
}

/// 由页面自测 JSON + 后端信息组装报告（纯函数，便于单测）。
pub fn build_privacy_selftest(
    level: &str,
    hostname: &str,
    whitelisted: bool,
    page_json: &str,
) -> PrivacySelfTestReport {
    let v: serde_json::Value =
        serde_json::from_str(page_json).unwrap_or(serde_json::Value::Null);
    let installed = v["installed"].as_bool().unwrap_or(false);
    let feat = &v["features"];
    let has = |k: &str| feat[k].as_bool().unwrap_or(false);
    // 非网页标签（如 tauri:// 新标签页）不会注入防护脚本，此时统一标 skip
    // 而非 fail——避免"新标签页自测一片红"的误导。
    let non_web = {
        let p = v["protocol"].as_str().unwrap_or("");
        !p.is_empty() && !p.starts_with("http")
    };
    let off = level == "off" || non_web;
    let skip_reason = if non_web {
        "当前标签不是网页（请先打开一个网页再自测）"
    } else {
        "隐私防护未开启"
    };

    let mut checks: Vec<PrivacyCheck> = Vec::new();

    // 1) 脚本注入
    if off {
        checks.push(mk_check("inject", "防护脚本注入", "skip", skip_reason));
    } else if installed {
        checks.push(mk_check("inject", "防护脚本注入", "pass", "防护脚本已在页面生效"));
    } else {
        checks.push(mk_check(
            "inject",
            "防护脚本注入",
            "fail",
            "未检测到防护脚本（页面可能未注入或已跳转）",
        ));
    }

    // 2) GPC 信号
    // 白名单 = 完全兼容模式：privacy.js 在装任何防护前整体 return（含 GPC），
    // 故白名单帧 GPC 缺失是预期行为，标 skip 而非 fail。
    if off {
        checks.push(mk_check("gpc", "GPC 隐私信号", "skip", skip_reason));
    } else if whitelisted {
        checks.push(mk_check(
            "gpc",
            "GPC 隐私信号",
            "skip",
            "当前站点在白名单（兼容模式），整帧保持浏览器原生",
        ));
    } else if has("gpc") {
        checks.push(mk_check(
            "gpc",
            "GPC 隐私信号",
            "pass",
            "navigator.globalPrivacyControl = true",
        ));
    } else {
        checks.push(mk_check("gpc", "GPC 隐私信号", "fail", "GPC 信号未生效"));
    }

    // 3-5) 加噪类（白名单站点跳过）
    for (id, label) in [
        ("canvas", "Canvas 指纹加噪"),
        ("webgl", "WebGL 参数归一化"),
        ("audio", "AudioContext 加噪"),
    ] {
        if off {
            checks.push(mk_check(id, label, "skip", skip_reason));
        } else if whitelisted {
            checks.push(mk_check(
                id,
                label,
                "skip",
                "当前站点在白名单（兼容模式），整帧保持浏览器原生",
            ));
        } else if has(id) {
            checks.push(mk_check(id, label, "pass", "已启用"));
        } else {
            checks.push(mk_check(id, label, "fail", "未生效"));
        }
    }

    // 6-9) 常开归一化类（白名单兼容模式下同样整体不装，标 skip）
    for (id, label) in [
        ("hardware", "硬件信息归一化"),
        ("webrtc", "WebRTC 内网 IP 防泄露"),
        ("webgpu", "WebGPU 指纹防护"),
        ("misc", "杂项指纹归一化"),
    ] {
        if off {
            checks.push(mk_check(id, label, "skip", skip_reason));
        } else if whitelisted {
            checks.push(mk_check(
                id,
                label,
                "skip",
                "当前站点在白名单（兼容模式），整帧保持浏览器原生",
            ));
        } else if has(id) {
            checks.push(mk_check(id, label, "pass", "已启用"));
        } else {
            checks.push(mk_check(id, label, "fail", "未生效"));
        }
    }

    let total = checks.len();
    let passed = checks.iter().filter(|c| c.status == "pass").count();
    PrivacySelfTestReport {
        shield_level: level.to_string(),
        fp_mode: feat["fp_mode"].as_str().unwrap_or("noise").to_string(),
        hostname: hostname.to_string(),
        whitelisted,
        total,
        passed,
        checks,
    }
}

/// 隐私防护自测：在活动标签页核对各项防护是否真的生效（阶段 2「信任需要证明」）。
#[tauri::command]
pub async fn privacy_selftest(
    app: AppHandle,
    state: State<'_, crate::tabs::TabManager>,
) -> Result<PrivacySelfTestReport, String> {
    let db = app.state::<Db>();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM settings WHERE key IN \
         ('privacy_shield_level','privacy_shield','privacy_whitelist')",
    )
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let kv: std::collections::HashMap<String, String> = rows.into_iter().collect();
    let level = match kv.get("privacy_shield_level") {
        Some(l) => l.clone(),
        None => match kv.get("privacy_shield").map(|s| s.as_str()) {
            Some("false") => "off".to_string(),
            _ => "standard".to_string(),
        },
    };
    let whitelist: Vec<String> = kv
        .get("privacy_whitelist")
        .and_then(|v| serde_json::from_str(v).ok())
        .unwrap_or_default();

    let id = state
        .runtime_snapshot()
        .1
        .ok_or_else(|| "没有活动标签".to_string())?;
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let wv = window
        .get_webview(&label)
        .ok_or_else(|| "活动标签的 webview 不存在".to_string())?;

    // 发起在 with_webview 内、等待在闭包外——避免主线程死锁
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), PRIVACY_SELFTEST_JS, move |res| {
            let _ = tx.send(res);
        });
    })
    .map_err(|e| format!("自测脚本执行失败: {}", e))?;
    let raw = rx
        .recv_timeout(std::time::Duration::from_secs(6))
        .map_err(|_| "自测超时（页面无响应）".to_string())?
        .map_err(|e| format!("自测脚本执行失败: {}", e))?;

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    let hostname = v["hostname"].as_str().unwrap_or("").to_string();
    let whitelisted = is_whitelisted(&hostname, &whitelist);

    Ok(build_privacy_selftest(&level, &hostname, whitelisted, &raw))
}
