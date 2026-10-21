// LINES-EXEMPT: WebKit 权限请求矩阵穷举，集中一处才能核对每种权限都有显式处置
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
/// 白名单规则：(权限类型, 主机)。用元组而非 "kind|host" 拼串——
/// 后者要在 `list_permission_rules` 里再 split 拆回来，属重复解释。
type PermRule = (String, String);
/// 白名单缓存（启动时从 settings 载入，respond 时更新）
static PERM_ALLOW: OnceLock<Mutex<HashSet<PermRule>>> = OnceLock::new();

thread_local! {
    /// 挂起中的权限请求（id → request），等前端回答
    static PENDING: RefCell<HashMap<u64, PermissionRequest>> = RefCell::new(HashMap::new());
}

fn allow_set() -> &'static Mutex<HashSet<PermRule>> {
    PERM_ALLOW.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn is_allowed(kind: &str, host: &str) -> bool {
    allow_set()
        .lock()
        .map(|s| s.contains(&(kind.to_string(), host.to_string())))
        .unwrap_or(false)
}

fn set_allowed(kind: &str, host: &str, allow: bool) {
    if let Ok(mut s) = allow_set().lock() {
        if allow {
            s.insert((kind.to_string(), host.to_string()));
        } else {
            s.remove(&(kind.to_string(), host.to_string()));
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
                    std::ffi::CStr::from_ptr(name)
                        .to_string_lossy()
                        .into_owned()
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
            crate::contracts::events::PERMISSION_REQUEST,
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
                log::info!(
                    "[perm] 用户{} {} {}",
                    if allow { "放行" } else { "拒绝" },
                    k,
                    h
                );
            }
        });
    })
    .map_err(|e| e.to_string())?;

    if remember {
        apply_rule(&db.pool, &kind, &host, allow)
            .await
            .map_err(|e| {
                log::error!("[perm] 记忆落库失败：{} {} = {} — {}", kind, host, allow, e);
                e
            })?;
        log::info!("[perm] 记忆生效：{} {} = {}", kind, host, allow);
    }
    Ok(())
}

/// 读 settings.perm_allow → `{kind: [host, …]}`；缺失或损坏一律视为空表
async fn read_allow_map(pool: &sqlx::SqlitePool) -> HashMap<String, Vec<String>> {
    let cur: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'perm_allow'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    cur.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 写回 settings.perm_allow（upsert：无行则插入，不依赖调用方先建行）
///
/// 失败返回 `Err` 上抛——内存放行表与 DB 失同步会导致下次启动权限状态不一致
/// （F-79 D1：安全相关，不允许静默吞错）。
async fn write_allow_map(
    pool: &sqlx::SqlitePool,
    map: &HashMap<String, Vec<String>>,
) -> Result<(), sqlx::Error> {
    let json = serde_json::to_string(map).unwrap_or_else(|_| "{}".into());
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ('perm_allow', ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(&json)
    .execute(pool)
    .await
    .map(|_| ())
}

/// 纯函数：把一条规则并进白名单表。授权去重；撤销后若某 kind 空列表，
/// 整条剔除，避免库里留下 `{"media": []}` 这种残壳。
fn merge_rule(map: &mut HashMap<String, Vec<String>>, kind: &str, host: &str, allow: bool) {
    let list = map.entry(kind.to_string()).or_default();
    if allow {
        if !list.iter().any(|x| x == host) {
            list.push(host.to_string());
        }
    } else {
        list.retain(|x| x != host);
    }
    if list.is_empty() {
        map.remove(kind);
    }
}

/// 记住/撤销一条授权：同步更新内存缓存并落库（唯一写入口）
///
/// 落库失败返回 `Err`（内存缓存已更新——本会话生效，但持久化未成，
/// 下次启动会不一致，故如实上报而非吞掉）。
async fn apply_rule(
    pool: &sqlx::SqlitePool,
    kind: &str,
    host: &str,
    allow: bool,
) -> Result<(), String> {
    set_allowed(kind, host, allow);
    let mut map = read_allow_map(pool).await;
    merge_rule(&mut map, kind, host, allow);
    write_allow_map(pool, &map)
        .await
        .map_err(|e| format!("权限白名单落库失败: {}", e))
}

/// 启动时从 settings 载入白名单到内存缓存
pub async fn load_allowlist(pool: &sqlx::SqlitePool) {
    let map = read_allow_map(pool).await;
    if let Ok(mut set) = allow_set().lock() {
        set.clear();
        for (kind, hosts) in &map {
            for h in hosts {
                set.insert((kind.clone(), h.clone()));
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
        .map(|(kind, host)| PermissionRule {
            kind_zh: kind_zh(kind).to_string(),
            kind: kind.clone(),
            host: host.clone(),
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
    apply_rule(&db.pool, &kind, &host, false)
        .await
        .map_err(|e| {
            log::error!("[perm] 删除规则落库失败：{} {} — {}", kind, host, e);
            e
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// 建一个跑完整迁移的真实库（含 settings 表）
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    #[test]
    fn kind_zh_maps_known_kinds_and_falls_back() {
        assert_eq!(kind_zh("media"), "摄像头/麦克风");
        assert_eq!(kind_zh("geolocation"), "地理位置");
        assert_eq!(kind_zh("notification"), "通知");
        assert_eq!(kind_zh("pointer-lock"), "鼠标指针锁定");
        assert_eq!(kind_zh("device-info"), "设备信息");
        // 未知类型不 panic，落到统一兜底文案
        assert_eq!(kind_zh("someth-new"), "其他权限");
        assert_eq!(kind_zh(""), "其他权限");
    }

    #[test]
    fn merge_rule_dedupes_adds_and_prunes_empty_kind() {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();

        merge_rule(&mut map, "media", "a.example", true);
        merge_rule(&mut map, "media", "a.example", true); // 重复授权不重复记
        merge_rule(&mut map, "media", "b.example", true);
        assert_eq!(
            map.get("media").unwrap(),
            &vec!["a.example".to_string(), "b.example".to_string()]
        );

        // 撤掉一条：仍留 kind，只去掉该 host
        merge_rule(&mut map, "media", "a.example", false);
        assert_eq!(map.get("media").unwrap(), &vec!["b.example".to_string()]);

        // 撤掉最后一条：整个 kind 剔除，不留空壳
        merge_rule(&mut map, "media", "b.example", false);
        assert!(!map.contains_key("media"));
    }

    #[test]
    fn merge_rule_cancelling_unknown_host_is_noop() {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        merge_rule(&mut map, "geolocation", "never.example", false);
        assert!(map.is_empty(), "撤销从未授权的主机不该凭空建 kind");
    }

    #[tokio::test]
    async fn apply_rule_persists_and_round_trips_through_db() {
        let (_dir, db) = temp_db().await;
        apply_rule(&db.pool, "media", "meet.example", true).await.unwrap();
        apply_rule(&db.pool, "geolocation", "maps.example", true).await.unwrap();

        let map = read_allow_map(&db.pool).await;
        assert_eq!(map.get("media").unwrap(), &vec!["meet.example".to_string()]);
        assert_eq!(
            map.get("geolocation").unwrap(),
            &vec!["maps.example".to_string()]
        );
    }

    #[tokio::test]
    async fn apply_rule_twice_is_idempotent() {
        let (_dir, db) = temp_db().await;
        apply_rule(&db.pool, "notification", "dup.example", true).await.unwrap();
        apply_rule(&db.pool, "notification", "dup.example", true).await.unwrap();

        let map = read_allow_map(&db.pool).await;
        assert_eq!(
            map.get("notification").unwrap(),
            &vec!["dup.example".to_string()],
            "重复记住同一条规则不该在库里堆重复项"
        );
    }

    #[tokio::test]
    async fn revoking_last_host_removes_kind_from_db() {
        let (_dir, db) = temp_db().await;
        apply_rule(&db.pool, "media", "meet.example", true).await.unwrap();
        apply_rule(&db.pool, "media", "meet.example", false).await.unwrap();

        let map = read_allow_map(&db.pool).await;
        assert!(
            !map.contains_key("media"),
            "撤权后不应在库里留下 {{media: []}} 残壳"
        );
    }

    #[tokio::test]
    async fn read_allow_map_tolerates_missing_and_corrupt_value() {
        let (_dir, db) = temp_db().await;
        // 缺键 → 空表，不 panic
        assert!(read_allow_map(&db.pool).await.is_empty());

        // 损坏 JSON → 同样视为空表，不 panic、不误报
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES ('perm_allow', ?, datetime('now'))",
        )
        .bind("{ 这不是 JSON")
        .execute(&db.pool)
        .await
        .unwrap();
        assert!(read_allow_map(&db.pool).await.is_empty());
    }

    #[tokio::test]
    async fn load_allowlist_populates_cache_from_db() {
        let (_dir, db) = temp_db().await;
        // 直接落库（绕过内存缓存），再模拟启动加载
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        merge_rule(&mut map, "notification", "load-test.example", true);
        write_allow_map(&db.pool, &map).await.unwrap();

        load_allowlist(&db.pool).await;
        assert!(is_allowed("notification", "load-test.example"));
        // 未登记的主机不得命中（默认拒绝）
        assert!(!is_allowed("notification", "other.example"));
        assert!(!is_allowed("media", "load-test.example"));
    }
}
