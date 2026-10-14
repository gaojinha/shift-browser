//! 扩展管理（P1-3 补完 · 2026-09-22）
//!
//! 扩展目录解析（manifest.json + content.js/css）+ 加载/启停/删除命令。原在 extensions.rs。

use super::*;
use ts_rs::TS;

/// 返回给前端的数据（含注入内容摘要，便于显示）
#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct ExtensionInfo {
    #[ts(type = "number")]
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

pub fn read_extension_dir(path: &str) -> Result<ExtensionDirResult, String> {
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
    // 安全：manifest 里的脚本/样式路径必须解析后仍落在扩展目录内，
    // 防止恶意/被篡改的 manifest 声明 "../../../.ssh/id_rsa" 之类路径越界读任意文件。
    //
    // 两道防线：
    //  1) 词法：拒绝含 ".."（ParentDir）组件的路径；Path::starts_with 是纯词法比较、
    //     不解析 ".."，若只靠 starts_with，声明 "../../../home/gao/.ssh/id_rsa" 可绕过越界读取。
    //  2) 物理（symlink 兜底）：文件真实存在时做 canonicalize，解析符号链接后
    //     再校验「真实路径」仍落在「真实 base」内——防止扩展包在目录里埋一个
    //     symlink 指向 /home/gao/.ssh，再让 manifest 引用该链接名间接越界。
    //     文件不存在（如首次注入尚未生成）时退回到已通过词法校验的相对路径，
    //     因为相对路径 + 无 ".." + 以 base 开头，词法上已确定无法越界。
    let base = std::path::Path::new(&base_dir);
    // 真实 base：解析 base 自身可能的 symlink（防御扩展目录本身是链接指向外部的场景）
    let base_real = base.canonicalize().unwrap_or_else(|_| base.to_owned());
    for cs in &manifest.content_scripts {
        for js in &cs.js {
            let p = base.join(js);
            // 拒绝含 ".." 组件的路径
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!("脚本路径含 ..（拒绝）: {}", js));
            }
            // 词法越界
            if !p.starts_with(base) {
                return Err(format!("脚本路径越界（拒绝）: {}", js));
            }
            // symlink 兜底：解析真实路径后再校验一次
            if let Ok(p_real) = p.canonicalize() {
                if !p_real.starts_with(&base_real) {
                    return Err(format!("脚本路径经符号链接越界（拒绝）: {}", js));
                }
            }
            let content =
                std::fs::read_to_string(&p).map_err(|e| format!("读取脚本 {} 失败: {}", js, e))?;
            js_sources.push(content);
        }
        for css in &cs.css {
            let p = base.join(css);
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!("样式路径含 ..（拒绝）: {}", css));
            }
            if !p.starts_with(base) {
                return Err(format!("样式路径越界（拒绝）: {}", css));
            }
            // symlink 兜底（与 js 分支保持一致）：解析真实路径后校验仍落在真实 base 内
            if let Ok(p_real) = p.canonicalize() {
                if !p_real.starts_with(&base_real) {
                    return Err(format!("样式路径经符号链接越界（拒绝）: {}", css));
                }
            }
            let content =
                std::fs::read_to_string(&p).map_err(|e| format!("读取样式 {} 失败: {}", css, e))?;
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

    Ok((
        manifest.name,
        manifest.version,
        manifest.description,
        js,
        css,
        js_sources.len(),
        css_sources.len(),
    ))
}

/// 加载扩展（目录或 manifest.json 路径）
#[tauri::command]
pub async fn load_extension(
    db: State<'_, Db>,
    app: AppHandle,
    path: String,
) -> Result<ExtensionInfo, String> {
    log::info!("[extensions] load_extension: path={}", path);
    let (name, version, description, js, css, js_count, css_count) = read_extension_dir(&path)?;
    let id = ext_db::create(&db, &name, &path)
        .await
        .map_err(|e| e.to_string())?;
    log::info!(
        "[extensions] 已加载扩展: id={} name={} js={} css={}",
        id,
        name,
        js.as_ref().map(|s| s.len()).unwrap_or(0),
        css.as_ref().map(|s| s.len()).unwrap_or(0)
    );
    // 注入（重新注入全部启用扩展，保持一致性）
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(
        &app,
        items,
        webmcp_on,
        super::compat_enabled(&db).await,
        None,
    );
    Ok(ExtensionInfo {
        id,
        name,
        path,
        enabled: true,
        version,
        description,
        js_count,
        css_count,
        created_at: String::new(),
    })
}

/// 扩展列表
#[tauri::command]
pub async fn list_extensions(db: State<'_, Db>) -> Result<Vec<ExtensionInfo>, String> {
    let exts = ext_db::list(&db).await.map_err(|e| e.to_string())?;
    log::info!("[extensions] list_extensions: {} 条", exts.len());
    let mut infos = Vec::new();
    for ext in exts {
        let (version, description, js_count, css_count) = match read_extension_dir(&ext.path) {
            Ok((_, v, d, _, _, jc, cc)) => (v, d, jc, cc),
            Err(e) => {
                log::warn!("[extensions] 读取扩展 {} 失败: {}", ext.name, e);
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
    log::info!(
        "[extensions] set_extension_enabled: id={} enabled={}",
        id,
        enabled
    );
    ext_db::set_enabled(&db, id, enabled)
        .await
        .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(
        &app,
        items,
        webmcp_on,
        super::compat_enabled(&db).await,
        None,
    );
    Ok(())
}

/// 删除扩展（重新注入）
#[tauri::command]
pub async fn delete_extension(db: State<'_, Db>, app: AppHandle, id: i64) -> Result<(), String> {
    log::info!("[extensions] delete_extension: id={}", id);
    ext_db::delete(&db, id).await.map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(
        &app,
        items,
        webmcp_on,
        super::compat_enabled(&db).await,
        None,
    );
    Ok(())
}
