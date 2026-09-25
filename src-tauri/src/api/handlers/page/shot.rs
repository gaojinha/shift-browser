//! 标签截图 handler 与截图目录治理（R1 三次拆分 · 2026-09-25）。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

pub(crate) async fn screenshot(app: &AppHandle, req: &Value) -> Value {
    // 截图指定标签（缺省 = 当前活动标签；webkit snapshot → PNG），返回落盘路径
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let all = state.all_tabs();
    let target: Option<&crate::tabs::Tab> = req
        .get("id")
        .and_then(|v| v.as_str())
        .map(|id| all.iter().find(|t| t.id == id))
        .unwrap_or_else(|| all.iter().find(|t| t.is_active));
    let Some(target) = target else {
        return AppError::validation("指定标签不存在（先 list_tabs 取 id；缺省为活动标签）")
            .error_json();
    };
    let label = format!("tab-{}", target.id);
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到标签 webview").error_json();
    };
    let out_path = {
        let db = app.state::<crate::db::Db>();
        let dir = db
            .vault_path
            .parent()
            .map(|p| p.join("screenshots"))
            .unwrap_or_default();
        let _ = std::fs::create_dir_all(&dir);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        dir.join(format!("shot-{}.png", ts))
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let out2 = out_path.clone();
    let dispatched = wv.with_webview(move |pwv| {
        use webkit2gtk::WebViewExt;
        let gtk_wv = pwv.inner().clone();
        // GTK 对象非 Send → 在 GTK 主上下文里跑（spawn_local），结果经 oneshot 回传
        gtk::glib::MainContext::default().spawn_local(async move {
            let res = match gtk_wv
                .snapshot_future(
                    webkit2gtk::SnapshotRegion::Visible,
                    webkit2gtk::SnapshotOptions::NONE,
                )
                .await
            {
                Ok(surface) => std::fs::File::create(&out2)
                    .map_err(|e| format!("创建 PNG 失败: {}", e))
                    .and_then(|mut f| {
                        // write_to_png 收的是 Write 流（不是路径）
                        surface
                            .write_to_png(&mut f)
                            .map_err(|e| format!("写 PNG 失败: {}", e))
                    })
                    .map(|_| out2.display().to_string()),
                Err(e) => Err(format!("截图失败: {}", e)),
            };
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("截图调度失败").error_json();
    }
    match rx.await {
        Ok(Ok(path)) => {
            // 截图治理（O2）：目录只保留最近 N 张，防止 AI 大量截图堆爆磁盘
            if let Some(dir) = out_path.parent() {
                prune_screenshots(dir, SCREENSHOT_KEEP);
            }
            json!({"ok": true, "data": {"path": path}})
        }
        Ok(Err(e)) => AppError::internal(e).error_json(),
        Err(_) => AppError::internal("截图结果丢失").error_json(),
    }
}

/// 截图保留上限（O2）：超过则按时间淘汰最旧。
const SCREENSHOT_KEEP: usize = 100;

/// 截图文件筛选：只认 `shot-<定长毫秒时间戳>.png`（字典序 = 时间序）。
fn is_screenshot_name(name: &str) -> bool {
    name.starts_with("shot-") && name.ends_with(".png")
}

/// 截图治理（O2）：目录内只保留最近 `keep` 张 `shot-*.png`，多余按文件名（= 时间戳）淘汰最旧。
/// 失败静默（治理是尽力而为，不影响截图主流程）。
fn prune_screenshots(dir: &std::path::Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut shots: Vec<std::path::PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(is_screenshot_name)
                .unwrap_or(false)
        })
        .collect();
    if shots.len() <= keep {
        return;
    }
    shots.sort();
    let remove_n = shots.len() - keep;
    for p in shots.into_iter().take(remove_n) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 截图治理（O2）：超出 keep 张时按时间淘汰最旧，非 shot-*.png 不受影响。
    #[test]
    fn prune_screenshots_keeps_newest() {
        let dir = std::env::temp_dir().join(format!("shift-shot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 造 5 张截图（时间戳递增）+ 1 个无关文件
        for ts in [1000u128, 2000, 3000, 4000, 5000] {
            std::fs::write(dir.join(format!("shot-{ts}.png")), b"x").unwrap();
        }
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        // 只留最近 3 张
        prune_screenshots(&dir, 3);
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["keep.txt", "shot-3000.png", "shot-4000.png", "shot-5000.png"],
            "应淘汰最旧两张、保留无关文件"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_screenshot_name_filters_non_shots() {
        assert!(is_screenshot_name("shot-1727000000000.png"));
        assert!(!is_screenshot_name("keep.txt"));
        assert!(!is_screenshot_name("shot-1.jpg"));
        assert!(!is_screenshot_name("readme.md"));
    }
}
