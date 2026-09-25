//! Bing 每日壁纸拉取与缓存（R1 拆分 · 2026-09-25）
//!
//! 主进程拉取规避 CORS；generate 关键路径绝不打网络，壁纸只走只读缓存，
//! 后台任务 refresh_wallpapers_background 负责拉取写缓存。

use tauri::AppHandle;

/// 壁纸缓存有效期（秒）：Bing 每日更新，6 小时缓存足够，避免每次生成都打网络
const WALLPAPER_CACHE_TTL: u64 = 6 * 3600;

/// 拉取 Bing 每日壁纸（原版 bg-images IPC 迁移）。
/// 主进程拉取规避 CORS，失败返回空 Vec（保持纯色渐变背景）。
async fn fetch_bing_wallpapers() -> Vec<String> {
  let api = "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt=zh-CN";
  let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(6))
    .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36")
    .build()
    .unwrap_or_default();
  match client.get(api).send().await {
    Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
      Ok(data) => {
        let imgs = data
          .get("images")
          .and_then(|v| v.as_array())
          .cloned()
          .unwrap_or_default();
        let urls: Vec<String> = imgs
          .iter()
          .filter_map(|i| {
            let u = i.get("url")?.as_str()?;
            Some(format!("https://www.bing.com{}", u))
          })
          .collect();
        log::info!("[newtab] Bing 壁纸拉取成功 {} 张", urls.len());
        urls
      }
      Err(e) => {
        log::warn!("[newtab] Bing 壁纸 JSON 解析失败: {}", e);
        Vec::new()
      }
    },
    Ok(resp) => {
      log::warn!("[newtab] Bing 壁纸 HTTP {}", resp.status());
      Vec::new()
    }
    Err(e) => {
      log::warn!("[newtab] Bing 壁纸拉取失败: {}", e);
      Vec::new()
    }
  }
}

/// 只读壁纸缓存（不拉网络）：generate 关键路径绝不能打网络——
/// 解锁后前端 createTab 的 refresh_newtab 会等 generate 的 GEN_LOCK，
/// 若 generate 里拉 Bing 壁纸（超时 6 秒）启动页就卡住几秒。
/// 壁纸由独立后台任务 refresh_wallpapers_background 拉取写缓存。
pub(super) fn load_wallpapers_from_cache(cache: &std::path::Path) -> Vec<String> {
  if let Ok(s) = std::fs::read_to_string(cache) {
    if let Ok(v) = serde_json::from_str::<Vec<String>>(&s) {
      if !v.is_empty() {
        log::debug!("[newtab] 壁纸走缓存 {} 张", v.len());
        return v;
      }
    }
  }
  Vec::new()
}

/// 后台刷新壁纸缓存：拉取成功后写盘，失败静默（下次再试）。
/// 在 setup 锁屏前 spawn，锁屏期间完成网络拉取，不占解锁时间。
pub(crate) async fn refresh_wallpapers_background(app: &AppHandle) {
  let Some(path) = super::newtab_path(app) else {
    return;
  };
  let cache = path
    .parent()
    .map(|p| p.join("wallpapers.json"))
    .unwrap_or_else(|| std::path::PathBuf::from("wallpapers.json"));
  // 缓存新鲜就不拉（避免每次启动都打网络）
  if let Ok(meta) = std::fs::metadata(&cache) {
    if let Ok(modified) = meta.modified() {
      if modified
        .elapsed()
        .map(|d| d.as_secs() < WALLPAPER_CACHE_TTL)
        .unwrap_or(false)
      {
        log::debug!("[newtab] 壁纸缓存新鲜，跳过后台刷新");
        return;
      }
    }
  }
  let fresh = fetch_bing_wallpapers().await;
  if fresh.is_empty() {
    log::warn!("[newtab] 壁纸拉取失败（保留旧缓存或纯色背景）");
    return;
  }
  if let Err(e) = std::fs::write(&cache, serde_json::to_string(&fresh).unwrap_or_default()) {
    log::warn!("[newtab] 壁纸缓存写入失败: {}", e);
  } else {
    log::info!("[newtab] 壁纸缓存已刷新 {} 张", fresh.len());
  }
}
