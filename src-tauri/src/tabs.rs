// 标签页管理器
// 管理标签元数据 + 嵌入式 webview 生命周期
// 与 Electron 版 electron/tabManager.ts 对应（v4.5.0）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl};

#[cfg(target_os = "linux")]
use gtk::prelude::*;

// UI 布局常量（与前端 CSS 保持一致）
pub const TAB_BAR_HEIGHT: f64 = 40.0;
pub const ADDRESS_BAR_HEIGHT: f64 = 42.0;
pub const BOOKMARKS_BAR_HEIGHT: f64 = 30.0;
pub const STATUS_BAR_HEIGHT: f64 = 22.0;

/// 内容区顶部偏移 = 标签栏 + 地址栏 + 书签栏
pub const CONTENT_TOP_OFFSET: f64 =
  TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT + BOOKMARKS_BAR_HEIGHT;

/// 标签页元数据（运行时状态，不持久化；会话保存走 sessions 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
  pub id: String,
  pub url: String,
  pub title: String,
  pub favicon: String,
  pub is_active: bool,
  pub is_pinned: bool,
}

/// 标签管理器（Tauri State）
pub struct TabManager {
  tabs: Mutex<HashMap<String, Tab>>,
  order: Mutex<Vec<String>>,
  active_id: Mutex<Option<String>>,
}

impl TabManager {
  pub fn new() -> Self {
    Self {
      tabs: Mutex::new(HashMap::new()),
      order: Mutex::new(Vec::new()),
      active_id: Mutex::new(None),
    }
  }

  /// 生成简单 UUID（避免额外依赖）
  /// 注意：{:08x} 强制补零，保证 s 恰好 32 字符，避免切片越界
  fn uuid() -> String {
    let s = format!(
      "{:08x}{:08x}{:08x}{:08x}",
      rand_u32(),
      rand_u32(),
      rand_u32(),
      rand_u32()
    );
    format!(
      "{}-{}-4{}-{}-{}",
      &s[0..8],
      &s[8..12],
      &s[12..15],
      &s[15..19],
      &s[19..31]
    )
  }

  fn all_tabs(&self) -> Vec<Tab> {
    let tabs = self.tabs.lock().unwrap();
    let order = self.order.lock().unwrap();
    order
      .iter()
      .filter_map(|id| tabs.get(id).cloned())
      .collect()
  }

  /// 根据窗口尺寸计算内容区域大小（逻辑像素）
  fn content_size(window: &tauri::Window) -> (f64, f64) {
    let inner = window.inner_size().unwrap_or(tauri::PhysicalSize::new(1280, 800));
    let scale = window.scale_factor().unwrap_or(1.0);
    let w = inner.width as f64 / scale;
    let h = inner.height as f64 / scale;
    (w, (h - CONTENT_TOP_OFFSET - STATUS_BAR_HEIGHT).max(100.0))
  }

  /// 创建嵌入式 webview
  fn create_webview(
    app: &AppHandle,
    id: &str,
    url: &str,
  ) -> Result<(), String> {
    let window = app
      .get_window("main")
      .ok_or_else(|| "主窗口不存在".to_string())?;

    let parsed_url = url
      .parse::<tauri::Url>()
      .map_err(|e| format!("URL 解析失败: {}", e))?;

    let (w, h) = Self::content_size(&window);
    println!("[tabs] create_webview: inner={:?} content=({:.0},{:.0})", window.inner_size(), w, h);

    // 页面加载完成时同步标题（用 host 作标题，MVP 阶段）
    let app_handle = app.clone();
    let tab_id = id.to_string();
    let child = window
      .add_child(
        WebviewBuilder::new(format!("tab-{}", id), WebviewUrl::External(parsed_url))
          .on_page_load(move |_webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
              let url = payload.url();
              let host = url.host_str().unwrap_or("").to_string();
              let title = if host.is_empty() {
                url.to_string()
              } else {
                host
              };

              println!("[tabs] page_load_finished: tab={} title={}", tab_id, title);

              if let Some(state) = app_handle.try_state::<TabManager>() {
                let mut tabs = state.tabs.lock().unwrap();
                if let Some(tab) = tabs.get_mut(&tab_id) {
                  tab.url = url.to_string();
                  tab.title = title;
                }
              }
            }
          }),
        LogicalPosition::new(0.0, CONTENT_TOP_OFFSET),
        LogicalSize::new(w, h),
      )
      .map_err(|e| format!("创建 webview 失败: {}", e))?;

    println!("[tabs] child_webview: pos={:?} size={:?}", child.position().unwrap(), child.size().unwrap());

    // GTK 的 vbox 会忽略 add_child 的位置参数并平分空间（webview 落到窗口下半部分）。
    // 解法：把主 webview 和所有 child webview reparent 到 GtkFixed 容器，实现绝对定位叠层。
    #[cfg(target_os = "linux")]
    {
      Self::fix_gtk_layout(&child, w, h)?;
    }
    Ok(())
  }

/// Linux/GTK 布局修复：把 child webview reparent 到 GtkFixed 容器，定位到内容区
  /// 结构：窗口 vbox → [GtkFixed] → 主 webview(0,0 全屏) + child webview(0,82 内容区)
  /// with_webview 闭包在主线程执行；gtk 对象在闭包内创建/查找，不跨线程捕获
  #[cfg(target_os = "linux")]
  fn fix_gtk_layout(
    child: &tauri::Webview,
    w: f64,
    h: f64,
  ) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let child = child.clone();

    child
      .with_webview(move |pwv| {
        let child_gtk = pwv.inner();
        // 从 parent 拿 vbox（不调 window 方法，避免主线程死锁）
        if let Some(vbox) = child_gtk
          .parent()
          .and_then(|p| p.dynamic_cast::<gtk::Box>().ok())
        {
          let fixed = find_or_create_fixed(&vbox);
          // gtk-rs 0.18 无 reparent：先从 vbox 移除再放入 Fixed
          vbox.remove(&child_gtk);
          fixed.put(&child_gtk, 0, CONTENT_TOP_OFFSET as i32);
          child_gtk.set_size_request(w as i32, h as i32);
          child_gtk.show();
          println!(
            "[tabs] child_webview reparent 到 Fixed: pos=(0,{}) size=({:.0},{:.0})",
            CONTENT_TOP_OFFSET, w, h
          );
        } else {
          println!("[tabs] child_webview 的 parent 不是 GtkBox，跳过 reparent");
        }
        let _ = tx.send(());
      })
      .map_err(|e| format!("child webview 句柄获取失败: {}", e))?;

    rx.recv().map_err(|e| format!("主线程布局失败: {}", e))?;
    Ok(())
  }

  /// 切换 webview 显隐
  fn set_webview_visible(app: &AppHandle, id: &str, visible: bool) -> Result<(), String> {
    let window = app
      .get_window("main")
      .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    if let Some(webview) = window.get_webview(&label) {
      if visible {
        let _ = webview.show();
        let _ = webview.set_focus();
      } else {
        let _ = webview.hide();
      }
    }
    Ok(())
  }

  /// 关闭并销毁 webview
  fn destroy_webview(app: &AppHandle, id: &str) {
    let window = match app.get_window("main") {
      Some(w) => w,
      None => return,
    };
    let label = format!("tab-{}", id);
    if let Some(webview) = window.get_webview(&label) {
      let _ = webview.close();
    }
  }
}

/// 应用启动时初始化 GTK 布局（setup 阶段调用，主线程）
/// 创建 GtkFixed 容器并 reparent 主 webview，为 child webview 绝对定位做准备
#[cfg(target_os = "linux")]
pub fn setup_gtk_layout(app: &tauri::App) -> Result<(), String> {
  if let Some(main_wv) = app.get_webview("main") {
    main_wv
      .with_webview(move |pwv| {
        let main_gtk = pwv.inner();
        // 从 parent 拿 vbox（不调 window 方法，避免主线程死锁）
        if let Some(vbox) = main_gtk
          .parent()
          .and_then(|p| p.dynamic_cast::<gtk::Box>().ok())
        {
          let fixed = find_or_create_fixed(&vbox);
          // gtk-rs 0.18 无 reparent：先从 vbox 移除再放入 Fixed
          vbox.remove(&main_gtk);
          fixed.put(&main_gtk, 0, 0);
          main_gtk.set_size_request(1280, 800);
          main_gtk.show();
          println!("[tabs] setup: 主 webview reparent 到 Fixed (1280x800)");
        } else {
          println!("[tabs] setup: 主 webview 的 parent 不是 GtkBox，跳过 reparent");
        }
      })
      .map_err(|e| format!("主 webview 句柄获取失败: {}", e))?;
  }
  Ok(())
}

/// 从 vbox 查找已创建的 GtkFixed 容器，不存在则创建并挂到 vbox（填满窗口）
#[cfg(target_os = "linux")]
fn find_or_create_fixed(vbox: &gtk::Box) -> gtk::Fixed {
  if let Some(f) = vbox
    .children()
    .iter()
    .find(|c| c.type_().name() == "GtkFixed")
    .and_then(|c| c.dynamic_cast_ref::<gtk::Fixed>().map(|f| f.clone()))
  {
    return f;
  }
  let f = gtk::Fixed::new();
  vbox.pack_start(&f, true, true, 0);
  f.show();
  f
}

/// 随机数（简单 xorshift，够用即可）
fn rand_u32() -> u32 {
  use std::time::{SystemTime, UNIX_EPOCH};
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.subsec_nanos())
    .unwrap_or(0);
  let mut x = nanos.wrapping_mul(0x9E3779B9).wrapping_add(0x85EBCA6B);
  x ^= x >> 13;
  x = x.wrapping_mul(0xC2B2AE35);
  x ^ (x >> 16)
}

/// 创建新标签页
#[tauri::command]
pub async fn create_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  url: String,
) -> Result<Tab, String> {
  println!("[tabs] create_tab: url={}", url);
  let id = TabManager::uuid();
  let title = url.clone();

  TabManager::create_webview(&app, &id, &url)?;
  println!("[tabs] create_tab: webview 创建成功 id={}", id);

  let tab = Tab {
    id: id.clone(),
    url,
    title,
    favicon: String::new(),
    is_active: true,
    is_pinned: false,
  };

  // 更新状态：新标签激活，旧标签取消激活
  {
    let mut tabs = state.tabs.lock().unwrap();
    let mut order = state.order.lock().unwrap();
    let mut active = state.active_id.lock().unwrap();

    for t in tabs.values_mut() {
      t.is_active = false;
    }
    tabs.insert(id.clone(), tab.clone());
    order.push(id.clone());
    *active = Some(id.clone());
  }

  Ok(tab)
}

/// 关闭标签页（切换到相邻标签）
#[tauri::command]
pub async fn close_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  id: String,
) -> Result<Option<Tab>, String> {
  println!("[tabs] close_tab: id={}", id);
  let mut next_id: Option<String> = None;

  {
    let mut tabs = state.tabs.lock().unwrap();
    let mut order = state.order.lock().unwrap();
    let mut active = state.active_id.lock().unwrap();

    let was_active = *active == Some(id.clone());
    let removed = tabs.remove(&id);
    if let Some(pos) = order.iter().position(|x| x == &id) {
      order.remove(pos);
    }

    if was_active {
      // 优先切到右边的标签，没有则切左边
      if let Some(pos) = order.iter().position(|x| x == &id) {
        let _ = pos;
      }
      next_id = order.last().cloned();
      if let Some(nid) = &next_id {
        if let Some(t) = tabs.get_mut(nid) {
          t.is_active = true;
        }
      }
      *active = next_id.clone();
    }

    if removed.is_none() {
      return Ok(None);
    }
  }

  TabManager::destroy_webview(&app, &id);

  // 激活下一个标签的 webview
  if let Some(nid) = &next_id {
    let _ = TabManager::set_webview_visible(&app, nid, true);
  }

  let tabs = state.tabs.lock().unwrap();
  Ok(next_id.and_then(|nid| tabs.get(&nid).cloned()))
}

/// 切换标签页
#[tauri::command]
pub async fn switch_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  id: String,
) -> Result<Option<Tab>, String> {
  println!("[tabs] switch_tab: id={}", id);
  // 先隐藏当前活动 webview
  let old_active = {
    let mut active = state.active_id.lock().unwrap();
    let old = active.clone();
    *active = Some(id.clone());
    old
  };
  if let Some(old) = &old_active {
    if old != &id {
      let _ = TabManager::set_webview_visible(&app, old, false);
    }
  }

  // 更新激活状态
  {
    let mut tabs = state.tabs.lock().unwrap();
    let exists = tabs.contains_key(&id);
    if !exists {
      return Ok(None);
    }
    for t in tabs.values_mut() {
      t.is_active = t.id == id;
    }
  }

  // 显示目标 webview
  let _ = TabManager::set_webview_visible(&app, &id, true);

  let tabs = state.tabs.lock().unwrap();
  Ok(tabs.get(&id).cloned())
}

/// 导航到新 URL
#[tauri::command]
pub async fn navigate_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  id: String,
  url: String,
) -> Result<Option<Tab>, String> {
  println!("[tabs] navigate_tab: id={} url={}", id, url);
  let window = app
    .get_window("main")
    .ok_or_else(|| "主窗口不存在".to_string())?;
  let label = format!("tab-{}", id);
  let webview = window
    .get_webview(&label)
    .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;

  let parsed_url = url
    .parse::<tauri::Url>()
    .map_err(|e| format!("URL 解析失败: {}", e))?;
  webview
    .navigate(parsed_url)
    .map_err(|e| format!("导航失败: {}", e))?;

  let mut tabs = state.tabs.lock().unwrap();
  let mut tab = tabs.get_mut(&id);
  if let Some(ref mut t) = tab {
    t.url = url.clone();
    t.title = url;
  }
  Ok(tab.cloned())
}

/// 获取所有标签页（按顺序）
#[tauri::command]
pub async fn get_tabs(state: tauri::State<'_, TabManager>) -> Result<Vec<Tab>, String> {
  Ok(state.all_tabs())
}

/// 后退（对当前活动标签执行 history.back()）
#[tauri::command]
pub async fn go_back(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
  let id = state.active_id.lock().unwrap().clone();
  let Some(id) = id else {
    return Ok(None);
  };
  println!("[tabs] go_back: tab={}", id);
  let window = app
    .get_window("main")
    .ok_or_else(|| "主窗口不存在".to_string())?;
  let label = format!("tab-{}", id);
  let webview = window
    .get_webview(&label)
    .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
  webview
    .eval("history.back()")
    .map_err(|e| format!("后退失败: {}", e))?;
  let tabs = state.tabs.lock().unwrap();
  Ok(tabs.get(&id).cloned())
}

/// 前进（对当前活动标签执行 history.forward()）
#[tauri::command]
pub async fn go_forward(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
  let id = state.active_id.lock().unwrap().clone();
  let Some(id) = id else {
    return Ok(None);
  };
  println!("[tabs] go_forward: tab={}", id);
  let window = app
    .get_window("main")
    .ok_or_else(|| "主窗口不存在".to_string())?;
  let label = format!("tab-{}", id);
  let webview = window
    .get_webview(&label)
    .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
  webview
    .eval("history.forward()")
    .map_err(|e| format!("前进失败: {}", e))?;
  let tabs = state.tabs.lock().unwrap();
  Ok(tabs.get(&id).cloned())
}

/// 更新标签页元数据（标题/图标等，供前端同步）
#[tauri::command]
pub async fn update_tab(
  state: tauri::State<'_, TabManager>,
  id: String,
  title: Option<String>,
  favicon: Option<String>,
) -> Result<Option<Tab>, String> {
  let mut tabs = state.tabs.lock().unwrap();
  let mut tab = tabs.get_mut(&id);
  if let Some(ref mut t) = tab {
    if let Some(new_title) = title {
      t.title = new_title;
    }
    if let Some(new_favicon) = favicon {
      t.favicon = new_favicon;
    }
  }
  Ok(tab.cloned())
}