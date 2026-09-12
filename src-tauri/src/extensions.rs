// 扩展管理（WebKitGTK UserContentManager 用户脚本/样式注入）
// WebKitGTK 不支持真正的 Chrome 扩展 API，MVP 方案：
// 扩展 = 目录（含 manifest.json + content.js/content.css），
// 注入到所有 http/https 页面（allow_list 限定，不污染 tauri://localhost 的 Vue UI）
use crate::db::extensions as ext_db;
use crate::db::Db;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

#[cfg(target_os = "linux")]
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime,
    UserStyleLevel, UserStyleSheet, WebViewExt,
};

/// 返回给前端的数据（含注入内容摘要，便于显示）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtensionInfo {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub version: Option<String>,
    pub description: Option<String>,
    pub js_count: usize,
    pub css_count: usize,
    pub created_at: String,
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content_scripts: Vec<ContentScript>,
}

#[derive(Deserialize)]
struct ContentScript {
    #[serde(default)]
    js: Vec<String>,
    #[serde(default)]
    css: Vec<String>,
}

/// 读取扩展目录内容：返回 (name, version, description, 合并后的 js, 合并后的 css)
fn read_extension_dir(path: &str) -> Result<(String, Option<String>, Option<String>, Option<String>, Option<String>), String> {
    // path 可以是目录（含 manifest.json）或 manifest.json 文件本身
    let manifest_path = if path.ends_with("manifest.json") {
        path.to_string()
    } else {
        format!("{}/manifest.json", path)
    };
    let manifest_str = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 manifest.json 失败: {}", e))?;
    let manifest: Manifest = serde_json::from_str(&manifest_str)
        .map_err(|e| format!("解析 manifest.json 失败: {}", e))?;

    // 扩展目录 = manifest.json 所在目录
    let base_dir = match manifest_path.rfind('/') {
        Some(idx) => manifest_path[..idx].to_string(),
        None => ".".to_string(),
    };

    let mut js_sources: Vec<String> = Vec::new();
    let mut css_sources: Vec<String> = Vec::new();
    for cs in &manifest.content_scripts {
        for js in &cs.js {
            let content = std::fs::read_to_string(format!("{}/{}", base_dir, js))
                .map_err(|e| format!("读取脚本 {} 失败: {}", js, e))?;
            js_sources.push(content);
        }
        for css in &cs.css {
            let content = std::fs::read_to_string(format!("{}/{}", base_dir, css))
                .map_err(|e| format!("读取样式 {} 失败: {}", css, e))?;
            css_sources.push(content);
        }
    }

    let js = if js_sources.is_empty() {
        None
    } else {
        Some(js_sources.join("\n;\n"))
    };
    let css = if css_sources.is_empty() {
        None
    } else {
        Some(css_sources.join("\n"))
    };

    Ok((manifest.name, manifest.version, manifest.description, js, css))
}

/// 在主线程把所有 child webview 的 UserContentManager 清空并注入扩展
/// UserContentManager 是 webview 级（非 WebContext 级），必须逐个 webview 注入
fn inject_extensions(app: &AppHandle, items: Vec<(Option<String>, Option<String>)>) {
    // 注意：主窗口是 Window 类型（tabs.rs 用 get_window("main")），
    // get_webview_window("main") 会返回 None 导致注入被跳过
    let Some(window) = app.get_window("main") else {
        println!("[extensions] 主窗口不可用，跳过注入");
        return;
    };
    let webviews = window.webviews();
    println!("[extensions] 注入到 {} 个 webview", webviews.len());
    let _ = app.run_on_main_thread(move || {
        // 只注入 child 标签页（已跳过主 webview），child 都是 http/https 外部网页，
        // allow_list 留空（全部注入）避免 WebKitGTK 对 "http://*" 通配符格式不识别导致不注入
        let allow_list: &[&str] = &[];
        for wv in &webviews {
            // 跳过主 webview（label="main"，是 Vue UI），只注入 child 标签页（label="tab-*"）
            // remove_all_scripts 会误删 Tauri 主 webview 的 UserScript（Vue UI 桥接），导致界面空白
            if wv.label() == "main" {
                continue;
            }
            let items_clone = items.clone();
            wv.with_webview(move |pwv| {
                let child_gtk = pwv.inner();
                let Some(manager) = child_gtk.user_content_manager() else {
                    return;
                };
                manager.remove_all_scripts();
                manager.remove_all_style_sheets();
                for (js, css) in &items_clone {
                    if let Some(js) = js {
                        let script = UserScript::new(
                            js,
                            UserContentInjectedFrames::AllFrames,
                            UserScriptInjectionTime::End,
                            allow_list,
                            &[],
                        );
                        manager.add_script(&script);
                    }
                    if let Some(css) = css {
                        let sheet = UserStyleSheet::new(
                            css,
                            UserContentInjectedFrames::AllFrames,
                            UserStyleLevel::User,
                            allow_list,
                            &[],
                        );
                        manager.add_style_sheet(&sheet);
                    }
                }
            });
        }
        println!("[extensions] 注入完成: {} 个扩展", items.len());
    });
}

/// 读取所有启用扩展的注入内容
async fn collect_enabled_items(db: &Db) -> Result<Vec<(Option<String>, Option<String>)>, String> {
    let exts = ext_db::list(db).await.map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for ext in exts {
        if !ext.enabled {
            continue;
        }
        match read_extension_dir(&ext.path) {
            Ok((_, _, _, js, css)) => items.push((js, css)),
            Err(e) => println!("[extensions] 读取扩展 {} 失败: {}", ext.name, e),
        }
    }
    Ok(items)
}

/// 加载扩展（目录或 manifest.json 路径）
#[tauri::command]
pub async fn load_extension(
    db: State<'_, Db>,
    app: AppHandle,
    path: String,
) -> Result<ExtensionInfo, String> {
    println!("[extensions] load_extension: path={}", path);
    let (name, version, description, js, css) = read_extension_dir(&path)?;
    let id = ext_db::create(&db, &name, &path).await.map_err(|e| e.to_string())?;
    println!(
        "[extensions] 已加载扩展: id={} name={} js={} css={}",
        id,
        name,
        js.as_ref().map(|s| s.len()).unwrap_or(0),
        css.as_ref().map(|s| s.len()).unwrap_or(0)
    );
    // 注入（重新注入全部启用扩展，保持一致性）
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(ExtensionInfo {
        id,
        name,
        path,
        enabled: true,
        version,
        description,
        js_count: js.as_ref().map(|_| 1).unwrap_or(0),
        css_count: css.as_ref().map(|_| 1).unwrap_or(0),
        created_at: String::new(),
    })
}

/// 扩展列表
#[tauri::command]
pub async fn list_extensions(db: State<'_, Db>) -> Result<Vec<ExtensionInfo>, String> {
    let exts = ext_db::list(&db).await.map_err(|e| e.to_string())?;
    println!("[extensions] list_extensions: {} 条", exts.len());
    let mut infos = Vec::new();
    for ext in exts {
        let (version, description, js_count, css_count) =
            match read_extension_dir(&ext.path) {
                Ok((_, v, d, js, css)) => (
                    v,
                    d,
                    js.as_ref().map(|_| 1).unwrap_or(0),
                    css.as_ref().map(|_| 1).unwrap_or(0),
                ),
                Err(e) => {
                    println!("[extensions] 读取扩展 {} 失败: {}", ext.name, e);
                    (None, None, 0, 0)
                }
            };
        infos.push(ExtensionInfo {
            id: ext.id,
            name: ext.name,
            path: ext.path,
            enabled: ext.enabled,
            version,
            description,
            js_count,
            css_count,
            created_at: ext.created_at,
        });
    }
    Ok(infos)
}

/// 启用/禁用扩展（重新注入）
#[tauri::command]
pub async fn set_extension_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    println!("[extensions] set_extension_enabled: id={} enabled={}", id, enabled);
    ext_db::set_enabled(&db, id, enabled).await.map_err(|e| e.to_string())?;
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(())
}

/// 删除扩展（重新注入）
#[tauri::command]
pub async fn delete_extension(
    db: State<'_, Db>,
    app: AppHandle,
    id: i64,
) -> Result<(), String> {
    println!("[extensions] delete_extension: id={}", id);
    ext_db::delete(&db, id).await.map_err(|e| e.to_string())?;
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(())
}

/// 启动时注入所有启用扩展（在 setup 阶段调用，db 已就绪）
pub fn inject_on_startup(app: &AppHandle, db: &Db) {
    let items = match tauri::async_runtime::block_on(collect_enabled_items(db)) {
        Ok(items) => items,
        Err(e) => {
            println!("[extensions] 启动注入失败: {}", e);
            return;
        }
    };
    if !items.is_empty() {
        println!("[extensions] 启动注入 {} 个启用扩展", items.len());
        inject_extensions(app, items);
    }
}