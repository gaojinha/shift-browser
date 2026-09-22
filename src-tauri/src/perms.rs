//! WebKit 权限请求处理（安全 #2 · 2026-09-22）
//!
//! 此前全项目**未注册** `permission-request`，摄像头/麦克风/地理位置/通知的放行
//! 完全交给 WebKitGTK 默认（不可控、不可审计）。本模块：
//! - **默认挂起**（不静默放行）：先查白名单，命中则放行；
//! - 未命中则挂起请求 + emit `permission-request` 事件，由前端弹提示；
//! - 用户决定后 `respond_permission` 放行/拒绝，可"记住"→ 落库持久化。
//!
//! 注意：PermissionRequest 是 GObject（非 Send），只能在主线程持有 → thread_local。

use serde::Serialize;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use gtk::glib;
use webkit2gtk::{PermissionRequest, PermissionRequestExt, WebView, WebViewExt};

static PERM_SEQ: AtomicU64 = AtomicU64::new(1);
/// 白名单缓存：key = "kind|host"（启动时从 settings 载入，respond 时更新）
static PERM_ALLOW: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

thread_local! {
    /// 挂起中的权限请求（id → request），等前端回答
    static PENDING: RefCell<HashMap<u64, PermissionRequest>> = RefCell::new(HashMap::new());
}

fn allow_set() -> &'static Mutex<HashSet<String>> {
    PERM_ALLOW.get_or_init(|| Mutex::new(HashSet::new()))
}

fn key(kind: &str, host: &str) -> String {
    format!("{}|{}", kind, host)
}

pub fn is_allowed(kind: &str, host: &str) -> bool {
    allow_set()
        .lock()
        .map(|s| s.contains(&key(kind, host)))
        .unwrap_or(false)
}

fn set_allowed(kind: &str, host: &str, allow: bool) {
    if let Ok(mut s) = allow_set().lock() {
        if allow {
            s.insert(key(kind, host));
        } else {
            s.remove(&key(kind, host));
        }
    }
}

/// 权限类型中文名
pub fn kind_zh(kind: &str) -> &'static str {
    match kind {
        "media" => "摄像头/麦克风",
        "geolocation" => "地理位置",
        "notification" => "通知",
        "pointer-lock" => "鼠标指针锁定",
        "device-info" => "设备信息",
        _ => "其他权限",
    }
}

fn kind_of(req: &PermissionRequest) -> &'static str {
    // 接口类型不支持 Cast，直接走 GTypeInstance 拿实例的具体 GType 名
    use glib::translate::ToGlibPtr;
    let tname = unsafe {
        let inst: *mut webkit2gtk::ffi::WebKitPermissionRequest = req.to_glib_none().0;
        if inst.is_null() {
            String::new()
        } else {
            let ti = inst as *mut glib::gobject_ffi::GTypeInstance;
            let cls = (*ti).g_class;
            if cls.is_null() {
                String::new()
            } else {
                let name = glib::gobject_ffi::g_type_name((*cls).g_type);
                if name.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(name).to_string_lossy().into_owned()
                }
            }
        }
    };
    if tname.contains("UserMedia") {
        "media"
    } else if tname.contains("Geolocation") {
        "geolocation"
    } else if tname.contains("Notification") {
        "notification"
    } else if tname.contains("PointerLock") {
        "pointer-lock"
    } else if tname.contains("DeviceInfo") {
        "device-info"
    } else {
        "other"
    }
}

/// 在 child webview 上注册权限请求处理（tabs.rs 建 webview 时调用）
pub fn attach(app: tauri::AppHandle, tab_id: String, wv: &WebView) {
    wv.connect_permission_request(move |view, req| {
        let host = WebViewExt::uri(view)
            .and_then(|u| url::Url::parse(&u).ok())
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_default();
        let kind = kind_of(req);
        if is_allowed(kind, &host) {
            req.allow();
            log::info!("[perm] 白名单放行 {} {}", kind, host);
            return true;
        }
        // 挂起 + 通知前端（不静默放行）
        let id = PERM_SEQ.fetch_add(1, Ordering::Relaxed);
        PENDING.with(|m| {
            m.borrow_mut().insert(id, req.clone());
        });
        use tauri::Emitter;
        let _ = app.emit(
            "permission-request",
            serde_json::json!({
                "id": id,
                "tabId": tab_id,
                "host": host,
                "kind": kind,
                "kindZh": kind_zh(kind),
            }),
        );
        log::info!("[perm] 挂起 #{} {} {}（等用户决定）", id, kind, host);
        true
    });
}

/// 前端回答权限请求
#[tauri::command]
pub async fn respond_permission(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    id: u64,
    allow: bool,
    remember: bool,
    kind: String,
    host: String,
) -> Result<(), String> {
    let k = kind.clone();
    let h = host.clone();
    // GTK 对象只能在主线程操作
    app.run_on_main_thread(move || {
        PENDING.with(|m| {
            if let Some(req) = m.borrow_mut().remove(&id) {
                if allow {
                    req.allow();
                } else {
                    req.deny();
                }
                log::info!("[perm] 用户{} {} {}", if allow { "放行" } else { "拒绝" }, k, h);
            }
        });
    })
    .map_err(|e| e.to_string())?;

    if remember {
        set_allowed(&kind, &host, allow);
        // 持久化：settings.perm_allow = JSON {kind: [host,...]}
        let cur: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'perm_allow'")
                .fetch_optional(&db.pool)
                .await
                .ok()
                .flatten();
        let mut map: HashMap<String, Vec<String>> = cur
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let list = map.entry(kind.clone()).or_default();
        if allow {
            if !list.contains(&host) {
                list.push(host.clone());
            }
        } else {
            list.retain(|x| x != &host);
        }
        let json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".into());
        let _ = sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES ('perm_allow', ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        )
        .bind(&json)
        .execute(&db.pool)
        .await;
        log::info!("[perm] 记忆生效：{} {} = {}", kind, host, allow);
    }
    Ok(())
}

/// 启动时从 settings 载入白名单到内存缓存
pub async fn load_allowlist(pool: &sqlx::SqlitePool) {
    let cur: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'perm_allow'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    let map: HashMap<String, Vec<String>> = cur
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if let Ok(mut set) = allow_set().lock() {
        set.clear();
        for (kind, hosts) in &map {
            for h in hosts {
                set.insert(key(kind, h));
            }
        }
        log::info!("[perm] 白名单已载入：{} 条", set.len());
    }
}

#[derive(Debug, Serialize)]
pub struct PermissionRule {
    pub kind: String,
    pub kind_zh: String,
    pub host: String,
}

/// 列出当前白名单（管理用）
#[tauri::command]
pub async fn list_permission_rules() -> Result<Vec<PermissionRule>, String> {
    let set = allow_set().lock().map_err(|e| e.to_string())?;
    Ok(set
        .iter()
        .map(|s| {
            let mut it = s.splitn(2, '|');
            let kind = it.next().unwrap_or("").to_string();
            let host = it.next().unwrap_or("").to_string();
            PermissionRule {
                kind_zh: kind_zh(&kind).to_string(),
                kind,
                host,
            }
        })
        .collect())
}

/// 删除一条白名单规则
#[tauri::command]
pub async fn remove_permission_rule(
    db: tauri::State<'_, crate::db::Db>,
    kind: String,
    host: String,
) -> Result<(), String> {
    set_allowed(&kind, &host, false);
    let cur: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'perm_allow'")
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten();
    let mut map: HashMap<String, Vec<String>> = cur
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if let Some(list) = map.get_mut(&kind) {
        list.retain(|x| x != &host);
    }
    let json = serde_json::to_string(&map).unwrap_or_else(|_| "{}".into());
    let _ = sqlx::query("UPDATE settings SET value = ?, updated_at = datetime('now') WHERE key = 'perm_allow'")
        .bind(&json)
        .execute(&db.pool)
        .await;
    Ok(())
}
