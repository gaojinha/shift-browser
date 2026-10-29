//! 工具栏 WebUI：本地 HTML 文件生成
//!
//! 工具栏本身就是一个网页，跑在 CEF 的独立 BrowserView 里——跟内容页同一套
//! 渲染/输入栈。里面用 `console.log("SHIFT_ACT:...")` 把动作传回浏览器进程
//! （MVP 桥，后续换 MessageRouter）。

use super::shell::Panel;
use std::path::Path;

/// 根类面板 class 清单（JSON 数组），来自 Rust 注册表 `shell::Panel`，注入 JS 侧
fn panel_classes_json() -> String {
    let items: Vec<String> = Panel::ALL.iter().map(|p| format!("\"{}\"", p.class())).collect();
    format!("[{}]", items.join(","))
}

/// 生成（覆盖写）工具栏 HTML，返回可直接加载的 `file://` URL
pub fn ensure_toolbar_html(
    cache_dir: &Path,
    bookmarks_json: &str,
    passwords_json: &str,
    downloads_json: &str,
    userscripts_json: &str,
) -> Result<String, String> {
    let dir = cache_dir.join("shell");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建工具栏目录失败: {e}"))?;

    // 书签直接嵌进页面：CEF 侧的推送时机不可靠（glib 定时器在回调链里不触发、
    // console 桥的单向请求也实测没到），生成页面时写死最稳。
    // 密码 JSON 只含 id/站点/用户名；"</" 转义防止数据提前闭合 <script>
    let html = TOOLBAR_HTML
        .replace("__SHIFT_PANEL_CLASSES__", &panel_classes_json())
        .replace("__SHIFT_BOOKMARKS_JSON__", bookmarks_json)
        .replace("__SHIFT_PASSWORDS_JSON__", &passwords_json.replace("</", "<\\/"))
        .replace("__SHIFT_DOWNLOADS_JSON__", &downloads_json.replace("</", "<\\/"))
        .replace("__SHIFT_USERSCRIPTS_JSON__", &userscripts_json.replace("</", "<\\/"));
    let path = dir.join("toolbar.html");
    std::fs::write(&path, html).map_err(|e| format!("写工具栏页失败: {e}"))?;

    // 用绝对路径拼 file:// URL（路径里不能有空格，缓存目录一般没有）
    let url = format!("file://{}", path.display());
    Ok(url)
}

const TOOLBAR_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  * { box-sizing: border-box; }
  html, body {
    margin: 0; padding: 0; height: 122px; overflow: hidden;
    /* Chrome 深色模式配色（2026-10-26 按 Chrome 实测对齐） */
    background: #35363a; color: #e8eaed;
    font-family: "Segoe UI", "Noto Sans CJK SC", "Microsoft YaHei", system-ui, sans-serif;
    font-size: 13px;
    user-select: none;
  }
  .bar {
    display: flex; align-items: center; gap: 2px;
    height: 48px; padding: 0 8px;
    background: #35363a;
  }
  button {
    width: 28px; height: 28px; border: 0; border-radius: 50%;
    background: transparent; color: #e8eaed; cursor: default;
    display: flex; align-items: center; justify-content: center;
    padding: 0; flex: 0 0 auto;
  }
  button:hover { background: #3c4043; }
  button:active { background: #4a4d51; }
  button:disabled { color: #5f6368; }
  button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 2;
    stroke-linecap: round; stroke-linejoin: round; }
  /* 标签栏：Chrome 深色 40px（标签在上、工具栏在下，顺序与 Chrome 一致） */
  .tabbar {
    display: flex; align-items: flex-end; gap: 0;
    height: 40px; padding: 4px 8px 0;
    background: #202124;
    overflow-x: auto; overflow-y: hidden;
  }
  .tabbar::-webkit-scrollbar { height: 0; }
  .tabs {
    display: flex; align-items: flex-end; flex: 1; min-width: 0;
    height: 100%; overflow-x: auto; overflow-y: hidden;
  }
  .tabs::-webkit-scrollbar { height: 0; }
  /* 窗口控制按钮（Chrome 画在标题栏右侧，Linux 版是方角 hover 块） */
  .win-ctrl { display: flex; align-items: flex-end; flex: 0 0 auto; height: 40px; }
  .win-ctrl button {
    width: 44px; height: 32px; border-radius: 0; color: #e8eaed;
    font-size: 13px; line-height: 1;
  }
  .win-ctrl button:hover { background: #3c4043; }
  .win-ctrl button.close:hover { background: #e81123; color: #fff; }
  .tab {
    display: flex; align-items: center; gap: 8px;
    min-width: 200px; max-width: 240px; height: 36px;
    padding: 0 6px 0 12px;
    background: transparent; color: #9aa0a6; cursor: default;
    font-size: 12px; white-space: nowrap; flex: 0 1 auto;
    position: relative;
    border-radius: 10px 10px 0 0;
  }
  .tab:hover { background: #2a2b2e; color: #e8eaed; }
  .tab.active { background: #35363a; color: #e8eaed; }
  /* 固定标签（F-CEF-30）：只露图标、隐藏标题与关闭按钮 */
  .tab.pinned { min-width: 40px; max-width: 40px; padding: 0; justify-content: center; gap: 0; }
  .tab.dragging { opacity: 0.4; }
  .tab.drop-target { background: #3c4043; }
  .tab.pinned .t-title, .tab.pinned .t-close { display: none; }
  .tab .t-pin, .tab .t-mute { font-size: 11px; line-height: 1; }
  .tab .t-pin { display: none; }
  .tab.pinned .t-pin { display: inline; }
  .tab .t-mute { display: none; }
  .tab.muted .t-mute { display: inline; }
  /* 标签之间的分隔线（Chrome 非激活标签右侧一道细线，激活/相邻时隐藏） */
  .tab + .tab::before {
    content: ''; position: absolute; left: -1px; top: 10px; bottom: 10px;
    width: 1px; background: #3c4043;
  }
  .tab.active::before, .tab.active + .tab::before { display: none; }
  .tab:hover::before, .tab:hover + .tab::before { display: none; }
  .tab .t-title {
    flex: 1; overflow: hidden; text-overflow: ellipsis;
  }
  .tab .t-close {
    width: 16px; height: 16px; border-radius: 50%; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    font-size: 12px; line-height: 1; color: #9aa0a6;
  }
  .tab .t-close:hover { background: #3c4043; color: #e8eaed; }
  .tab-new {
    width: 28px; height: 28px; border-radius: 50%; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    color: #e8eaed; font-size: 18px; line-height: 1; margin: 0 0 4px 4px;
  }
  /* Chrome 标签栏左侧：标签页下拉 + 标签分组 */
  .tab-search {
    width: 28px; height: 28px; border-radius: 50%; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    color: #9aa0a6; margin: 0 2px 4px 0; cursor: default;
  }
  .tab-search:hover { background: #3c4043; color: #e8eaed; }
  .tab-search svg, .tab-group svg {
    width: 16px; height: 16px; fill: none; stroke: currentColor;
    stroke-width: 2; stroke-linecap: round; stroke-linejoin: round;
  }
  .tab-new:hover { background: #3c4043; }
  /* 文件夹项与下拉 */
  .bm.folder .bm-ico { background: #4a5a72; }
  #bmfolders {
    position: fixed; z-index: 180; display: none;
    min-width: 180px; max-width: 320px; max-height: 300px; overflow-y: auto;
    scrollbar-width: thin; scrollbar-color: #5f6368 transparent;
    padding: 4px; background: #292a2d; border: 1px solid #3c4043;
    border-radius: 8px; box-shadow: 0 8px 24px rgba(0,0,0,.5);
    font-size: 12px; color: #e8eaed;
  }
  #bmfolders.open { display: block; }
  #bmfolders::-webkit-scrollbar { width: 10px; }
  #bmfolders::-webkit-scrollbar-thumb {
    background: #5f6368; border-radius: 5px; border: 2px solid #292a2d;
  }
  #bmfolders::-webkit-scrollbar-track { background: transparent; }
  #bmfolders .fd-title { padding: 6px 10px; color: #9aa0a6; font-size: 11px; }
  #bmfolders .fd-item { padding: 6px 10px; border-radius: 6px; cursor: default;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  #bmfolders .fd-item:hover { background: #3c4043; }
  #bmfolders .fd-empty { padding: 8px 10px; color: #9aa0a6; }
  .bm-edit {
    width: 110px; height: 20px; padding: 0 6px;
    background: #202124; color: #e8eaed; font-size: 12px;
    border: 1px solid #8ab4f8; border-radius: 4px; outline: 0;
  }
  /* 书签右键菜单 */
  #bmmenu {
    position: fixed; z-index: 200; display: none;
    min-width: 130px; padding: 4px;
    background: #292a2d; border: 1px solid #3c4043; border-radius: 8px;
    box-shadow: 0 6px 20px rgba(0,0,0,.5); font-size: 12px; color: #e8eaed;
  }
  #bmmenu.open { display: block; }
  #bmmenu div { padding: 7px 10px; border-radius: 6px; cursor: default; }
  #bmmenu div:hover { background: #3c4043; }
  #bmmenu div.danger:hover { background: #e81123; color: #fff; }
  #bmmenu .menu-edit { padding: 2px; }
  #bmmenu .menu-edit input { width: 150px; }
  /* 加载中：刷新按钮转圈（Rust 侧 data-loading 控制） */
  @keyframes spin { to { transform: rotate(360deg); } }
  html[data-loading="1"] #reload svg { animation: spin .9s linear infinite; }
  html[data-loading="1"] #reload { color: #8ab4f8; }
  /* 书签栏：Chrome 34px，图标 + 名称，横向滚动 */
  .bookmarks {
    display: flex; align-items: center; gap: 2px;
    height: 34px; padding: 0 8px;
    background: #35363a;
    overflow-x: auto; overflow-y: hidden;
  }
  .bookmarks::-webkit-scrollbar { height: 0; }
  /* 书签栏收起态（F-CEF-09，Ctrl+Shift+B）：在 html 上挂 bookmarks-hidden，
     同时把页面高度从 122 收到 88（少书签栏 34px），否则底部留白。 */
  html.bookmarks-hidden, html.bookmarks-hidden body { height: 88px; }
  html.bookmarks-hidden .bookmarks { display: none; }
  .bm {
    display: flex; align-items: center; gap: 6px;
    height: 26px; padding: 0 8px; border-radius: 6px;
    color: #e8eaed; font-size: 12px; white-space: nowrap;
    flex: 0 0 auto; cursor: default;
  }
  .bm:hover { background: #3c4043; }
  .bm .bm-ico {
    width: 16px; height: 16px; border-radius: 4px; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    background: #4a4d51; font-size: 10px; color: #e8eaed;
  }
  .bm .bm-name {
    max-width: 140px; overflow: hidden; text-overflow: ellipsis;
  }
  /* 密码面板（F-CEF-12）：展开时 Rust 把工具栏视图加高 PW_PANEL_EXTRA，html 跟随视图高度 */
  html.pw-open, html.pw-open body { height: 100vh; }
  #pwpanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10;
  }
  html.pw-open #pwpanel { display: flex; }
  html:not(.bookmarks-hidden) #pwpanel { top: 122px; }
  #pwsearch {
    height: 28px; border: 1px solid #3c4043; border-radius: 14px; outline: 0;
    background: #202124; color: #e8eaed; padding: 0 12px; font-size: 12px; user-select: text;
  }
  #pwlist { flex: 1; overflow-y: auto; }
  .pw-row {
    display: flex; align-items: center; gap: 10px; height: 28px; padding: 0 6px;
    border-radius: 6px; font-size: 12px;
  }
  .pw-row:hover { background: #3c4043; }
  .pw-row .pw-dom { flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pw-row .pw-usr { flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #9aa0a6; }
  .pw-row .pw-mask { flex: 0 0 48px; color: #9aa0a6; letter-spacing: 1px; }
  .pw-row .pw-btn {
    flex: 0 0 auto; padding: 0 8px; height: 22px; border-radius: 11px; line-height: 22px;
    color: #8ab4f8; font-size: 12px;
  }
  .pw-row .pw-btn:hover { background: #4a4d51; }
  .pw-row .pw-btn.danger { color: #f28b82; }
  .pw-empty { color: #9aa0a6; padding: 12px 6px; font-size: 12px; }
  /* 下载面板（F-CEF-15）：与密码面板同一套撑高机制，两个面板互斥 */
  html.dl-open, html.dl-open body { height: 100vh; }
  #dlpanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10;
  }
  html.dl-open #dlpanel { display: flex; }
  html:not(.bookmarks-hidden) #dlpanel { top: 122px; }
  #dllist { flex: 1; overflow-y: auto; }
  .dl-head { display: flex; align-items: center; gap: 8px; }
  /* 命令面板（F-CEF-29） */
  #palettepanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 12;
  }
  html.palette-open #palettepanel { display: flex; }
  html:not(.bookmarks-hidden) #palettepanel { top: 122px; }
  #paletteinput {
    height: 32px; border-radius: 6px; border: 1px solid #5f6368; background: #202124;
    color: #e8eaed; padding: 0 10px; font-size: 13px; outline: none;
  }
  #paletteinput:focus { border-color: #8ab4f8; }
  #palettelist { flex: 1; overflow-y: auto; }
  .pl-group { color: #9aa0a6; font-size: 11px; margin: 6px 4px 2px; }
  .pl-item {
    display: flex; align-items: center; gap: 10px; height: 30px; padding: 0 8px;
    border-radius: 4px; color: #e8eaed; font-size: 12px; cursor: pointer;
  }
  .pl-item.sel { background: #3c4043; }
  .pl-item .pl-title { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pl-item .pl-hint { color: #9aa0a6; font-size: 11px; max-width: 45%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dl-head .dl-title { flex: 1; color: #e8eaed; font-size: 12px; }
  .dl-row {
    display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 6px;
    border-radius: 6px; font-size: 12px;
  }
  .dl-row:hover { background: #3c4043; }
  .dl-row .dl-name {
    flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .dl-row .dl-state { flex: 0 0 92px; color: #9aa0a6; }
  .dl-row .dl-bar {
    flex: 1; height: 4px; background: #3c4043; border-radius: 2px; overflow: hidden;
  }
  .dl-row .dl-bar > i { display: block; height: 100%; width: 0; background: #8ab4f8; }
  .dl-row .dl-btn {
    flex: 0 0 auto; padding: 0 8px; height: 22px; border-radius: 11px; line-height: 22px;
    color: #8ab4f8; font-size: 12px; cursor: pointer;
  }
  .dl-row .dl-btn:hover { background: #4a4d51; }
  .dl-row .dl-btn.danger { color: #f28b82; }
  .dl-empty { color: #9aa0a6; padding: 12px 6px; font-size: 12px; }
  /* 历史面板（F-CEF-19）：与密码/下载/菜单同一套撑高机制 */
  html.hist-open, html.hist-open body { height: 100vh; }
  #histpanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10;
  }
  html.hist-open #histpanel { display: flex; }
  html:not(.bookmarks-hidden) #histpanel { top: 122px; }
  .hist-head { display: flex; align-items: center; gap: 8px; }
  #histsearch {
    flex: 1; height: 28px; border: 1px solid #3c4043; border-radius: 14px; outline: 0;
    background: #202124; color: #e8eaed; padding: 0 12px; font-size: 12px; user-select: text;
  }
  .hist-clear, .hist-close {
    flex: 0 0 auto; padding: 0 10px; height: 24px; line-height: 24px; border-radius: 12px;
    color: #8ab4f8; font-size: 12px; cursor: default;
  }
  .hist-clear:hover, .hist-close:hover { background: #4a4d51; }
  .hist-clear.danger { color: #f28b82; }
  #histlist { flex: 1; overflow-y: auto; }
  .hist-row {
    display: flex; align-items: center; gap: 10px; height: 28px; padding: 0 6px;
    border-radius: 6px; font-size: 12px;
  }
  .hist-row:hover { background: #3c4043; }
  .hist-row .h-title {
    flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .hist-row .h-url {
    flex: 3; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: #9aa0a6;
  }
  .hist-row .h-time { flex: 0 0 130px; color: #5f6368; }
  .hist-row .h-del {
    flex: 0 0 auto; padding: 0 8px; height: 22px; line-height: 22px;
    border-radius: 11px; color: #f28b82;
  }
  .hist-row .h-del:hover { background: #4a4d51; }
  .hist-empty { color: #9aa0a6; padding: 12px 6px; font-size: 12px; }
  /* 书签管理面板（F-CEF-20） */
  html.bmmanage-open, html.bmmanage-open body { height: 100vh; }
  #bmmanage {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10; font-size: 12px;
  }
  html.bmmanage-open #bmmanage { display: flex; }
  html:not(.bookmarks-hidden) #bmmanage { top: 122px; }
  .bmm-head { display: flex; align-items: center; gap: 8px; }
  #bmmsearch {
    flex: 1; height: 28px; border: 1px solid #3c4043; border-radius: 14px; outline: 0;
    background: #202124; color: #e8eaed; padding: 0 12px; font-size: 12px; user-select: text;
  }
  .bmm-btn {
    flex: 0 0 auto; color: #8ab4f8; padding: 0 10px; height: 24px;
    line-height: 24px; border-radius: 12px; cursor: default;
  }
  .bmm-btn:hover { background: #4a4d51; }
  .bmm-body { flex: 1; display: flex; gap: 10px; min-height: 0; }
  #bmmtree { flex: 0 0 190px; overflow-y: auto; border-right: 1px solid #3c4043; padding-right: 6px; }
  #bmmlist { flex: 1; overflow-y: auto; }
  .bmm-folder {
    display: flex; align-items: center; gap: 6px; height: 28px; padding: 0 8px;
    border-radius: 6px; color: #e8eaed; cursor: default;
  }
  .bmm-folder:hover, .bmm-folder.active { background: #3c4043; }
  .bmm-folder.active { color: #8ab4f8; }
  .bmm-folder-del {
    flex: 0 0 auto; color: #f28b82; padding: 0 6px; font-size: 12px;
  }
  .bmm-folder-del:hover { background: #4a4d51; border-radius: 4px; }
  .bmm-item {
    display: flex; align-items: center; gap: 10px; height: 30px; padding: 0 6px;
    border-radius: 6px;
  }
  .bmm-item:hover { background: #3c4043; }
  .bmm-item .m-title {
    flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .bmm-item .m-url {
    flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: #9aa0a6;
  }
  .bmm-item .m-op {
    flex: 0 0 auto; color: #8ab4f8; padding: 0 8px; height: 22px;
    line-height: 22px; border-radius: 11px;
  }
  .bmm-item .m-op:hover { background: #4a4d51; }
  .bmm-item .m-op.danger { color: #f28b82; }
  .bmm-empty { color: #9aa0a6; padding: 12px 6px; }
  .bmm-item .m-edit {
    flex: 1; height: 24px; background: #202124; border: 1px solid #8ab4f8;
    border-radius: 4px; color: #e8eaed; padding: 0 8px; font-size: 12px;
  }
  .bmm-item .m-move {
    height: 24px; background: #202124; border: 1px solid #3c4043; border-radius: 4px;
    color: #e8eaed; font-size: 12px; max-width: 180px;
  }
  /* 分组/工作区面板（F-CEF-27） */
  html.groups-open, html.groups-open body { height: 100vh; }
  #groupspanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10; font-size: 12px;
  }
  html.groups-open #groupspanel { display: flex; }
  html:not(.bookmarks-hidden) #groupspanel { top: 122px; }
  #groupslist { flex: 1; overflow-y: auto; }
  .g-row { display: flex; align-items: center; gap: 10px; height: 30px; padding: 0 6px; border-radius: 6px; }
  .g-row:hover { background: #3c4043; }
  .g-row .g-kind { flex: 0 0 60px; color: #9aa0a6; }
  .g-row .g-name { flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .g-row .g-del { flex: 0 0 auto; color: #f28b82; padding: 0 8px; }
  .g-empty { color: #9aa0a6; padding: 12px 6px; }
  /* 会话面板（F-CEF-26） */
  html.sess-open, html.sess-open body { height: 100vh; }
  #sesspanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10; font-size: 12px;
  }
  html.sess-open #sesspanel { display: flex; }
  html:not(.bookmarks-hidden) #sesspanel { top: 122px; }
  .sess-head { display: flex; align-items: center; }
  .sess-title { flex: 1; color: #e8eaed; }
  #sesslist { flex: 1; overflow-y: auto; }
  .sess-row { display: flex; align-items: center; gap: 10px; height: 30px; padding: 0 6px; border-radius: 6px; }
  .sess-row:hover { background: #3c4043; }
  .sess-row .s-name { flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sess-row .s-time { flex: 1; color: #9aa0a6; }
  .sess-row .s-count { flex: 0 0 70px; color: #9aa0a6; }
  .sess-row .s-del { flex: 0 0 auto; color: #f28b82; padding: 0 8px; }
  .sess-row .s-restore { flex: 0 0 auto; color: #8ab4f8; padding: 0 8px; }
  .sess-empty { color: #9aa0a6; padding: 12px 6px; }
  /* 证书面板（F-CEF-21） */
  html.cert-open, html.cert-open body { height: 100vh; }
  #certpanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10; font-size: 12px;
  }
  html.cert-open #certpanel { display: flex; }
  html:not(.bookmarks-hidden) #certpanel { top: 122px; }
  .cert-head { display: flex; align-items: center; gap: 8px; }
  #certsearch {
    flex: 1; height: 28px; border: 1px solid #3c4043; border-radius: 14px; outline: 0;
    background: #202124; color: #e8eaed; padding: 0 12px; font-size: 12px; user-select: text;
  }
  #certlist { flex: 1; overflow-y: auto; }
  .cert-row {
    display: flex; align-items: center; gap: 10px; height: 30px; padding: 0 6px;
    border-radius: 6px;
  }
  .cert-row:hover { background: #3c4043; }
  .cert-row .c-host { flex: 0 0 200px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cert-row .c-subject { flex: 1; color: #9aa0a6; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cert-row .c-days { flex: 0 0 90px; color: #81c995; }
  .cert-row .c-days.warn { color: #fdd663; }
  .cert-row .c-days.danger { color: #f28b82; }
  .cert-row .c-op { flex: 0 0 auto; color: #f28b82; padding: 0 8px; height: 22px; line-height: 22px; border-radius: 11px; }
  .cert-row .c-op:hover { background: #4a4d51; }
  .cert-empty { color: #9aa0a6; padding: 12px 6px; }
  /* 用户脚本面板（F-CEF-23） */
  html.us-open, html.us-open body { height: 100vh; }
  #uspanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10; font-size: 12px;
  }
  html.us-open #uspanel { display: flex; }
  html:not(.bookmarks-hidden) #uspanel { top: 122px; }
  .us-head { display: flex; align-items: center; gap: 8px; }
  #ussearch {
    flex: 1; height: 28px; border: 1px solid #3c4043; border-radius: 14px; outline: 0;
    background: #202124; color: #e8eaed; padding: 0 12px; font-size: 12px; user-select: text;
  }
  #uslist { flex: 1; overflow-y: auto; }
  .us-row {
    display: flex; align-items: center; gap: 8px; height: 32px; padding: 0 6px;
    border-radius: 6px;
  }
  .us-row:hover { background: #3c4043; }
  .us-row .us-name { flex: 0 0 180px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 500; }
  .us-row .us-patterns { flex: 1; color: #9aa0a6; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .us-row .us-enabled { flex: 0 0 auto; display: flex; align-items: center; }
  .us-row .us-del { flex: 0 0 auto; color: #f28b82; padding: 0 8px; height: 22px; line-height: 22px; border-radius: 11px; }
  .us-row .us-del:hover { background: #4a4d51; }
  .us-empty { color: #9aa0a6; padding: 12px 6px; }
  /* 站点信息（F-CEF-19）：点地址栏锁图标弹出 */
  html.site-open, html.site-open body { height: 100vh; }
  #sitepanel {
    display: none; position: absolute; left: 8px; top: 88px; width: 320px;
    background: #292a2d; border: 1px solid #3c4043; border-radius: 8px;
    padding: 10px 12px; z-index: 20; box-shadow: 0 8px 24px rgba(0,0,0,.5);
    font-size: 12px;
  }
  html.site-open #sitepanel { display: block; }
  html:not(.bookmarks-hidden) #sitepanel { top: 122px; }
  .site-host { font-size: 14px; color: #e8eaed; margin-bottom: 4px; }
  .site-state { color: #81c995; margin-bottom: 4px; }
  .site-state.insecure { color: #f28b82; }
  .site-url { color: #9aa0a6; word-break: break-all; margin-bottom: 8px; }
  .site-close { color: #8ab4f8; cursor: default; }
  /* 主菜单（F-CEF-16）：右上角下拉，样式对齐 Chrome 深色菜单 */
  html.menu-open, html.menu-open body { height: 100vh; }
  #menupanel {
    display: none; position: absolute; right: 8px; top: 88px; width: 240px;
    background: #292a2d; border: 1px solid #3c4043; border-radius: 8px;
    padding: 6px; z-index: 20; box-shadow: 0 8px 24px rgba(0,0,0,.5);
  }
  /* F-CEF-40：主菜单改走 GTK 原生弹出菜单，页面内不再显示 */
  html.menu-open #menupanel { display: none !important; }
  html:not(.bookmarks-hidden) #menupanel { top: 122px; }
  .menu-item {
    display: flex; align-items: center; gap: 10px; height: 30px; padding: 0 10px;
    border-radius: 4px; font-size: 12px; color: #e8eaed; cursor: default;
  }
  .menu-item:hover { background: #3c4043; }
  .menu-item.disabled { color: #5f6368; pointer-events: none; }
  .menu-item .menu-key { margin-left: auto; color: #9aa0a6; font-size: 11px; }
  .menu-sep { height: 1px; background: #3c4043; margin: 6px 0; }
  .addr-wrap {
    flex: 1; display: flex; align-items: center; height: 34px;
    background: #202124; border: 1px solid transparent; border-radius: 17px;
    padding: 0 10px; min-width: 0; margin: 0 6px; position: relative;
  }
  .addr-wrap:hover { background: #292a2d; }
  .addr-wrap:focus-within { background: #202124; border-color: #8ab4f8; }
  .lockbtn {
    flex: 0 0 auto; display: flex; align-items: center;
    border: 0; background: transparent; padding: 0; margin: 0;
  }
  .addr-wrap svg { width: 16px; height: 16px; stroke: #9aa0a6; fill: none;
    stroke-width: 2; stroke-linecap: round; margin-right: 8px; flex: 0 0 auto; }
  #addr { flex: 1; height: 100%; border: 0; outline: 0; background: transparent;
    color: #e8eaed; font-size: 13px; min-width: 0; }
  #addr::placeholder { color: #9aa0a6; }
  #addr {
    flex: 1; min-width: 0; border: 0; outline: 0; background: transparent;
    color: #e6e8eb; font-size: 13px;
  }
  #addr::placeholder { color: #6b7280; }
  /* 星标 + 地址栏下拉建议（F-CEF-17） */
  #starbtn {
    flex: 0 0 auto; width: 24px; height: 24px; border: 0; background: transparent;
    color: #9aa0a6; font-size: 16px; line-height: 24px; padding: 0; cursor: default;
  }
  #starbtn.on { color: #fdd663; }
  #omni {
    display: none; position: absolute; left: 0; right: 0; top: 38px;
    background: #292a2d; border: 1px solid #3c4043; border-radius: 8px;
    max-height: 300px; overflow-y: auto; z-index: 30; padding: 4px;
    box-shadow: 0 8px 24px rgba(0,0,0,.5);
  }
  #omni.open { display: block; }
  .omni-item {
    display: flex; align-items: center; gap: 8px; padding: 6px 10px;
    border-radius: 4px; font-size: 12px; cursor: default;
  }
  .omni-item.active, .omni-item:hover { background: #3c4043; }
  .omni-item .omni-type { flex: 0 0 auto; color: #9aa0a6; }
  .omni-item .omni-title {
    flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .omni-item .omni-url {
    max-width: 45%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    color: #9aa0a6;
  }
  .lock { width: 14px; height: 14px; margin-right: 8px; color: #8b93a1; flex: none; }
  /* Chrome 地址栏右侧只留星标/扩展/菜单，密码收进菜单 */
  #pwbtn { display: none; }
  /* 缩放控件（F-CEF-18）：− / 百分比 / +，放地址栏右侧 */
  .zoomctl {
    display: none; align-items: center; flex: 0 0 auto;
    height: 28px; background: #202124; border-radius: 14px; padding: 0 2px; margin: 0 2px;
  }
  .zoomctl button {
    width: 24px; height: 24px; border-radius: 50%; font-size: 14px; line-height: 24px;
  }
  #zoomval {
    min-width: 44px; text-align: center; font-size: 12px; color: #9aa0a6;
    cursor: default; user-select: none;
  }
  #zoomval:hover { color: #e8eaed; }
  /* 页内查找（F-CEF-18）：地址栏下方浮层，覆盖书签栏位置。
     工具栏是独立视图，浮层会被固定视口裁掉，开栏时 Rust 撑高 FIND_BAR_EXTRA。 */
  #findbar {
    display: none; position: absolute; left: 0; right: 0; top: 88px;
    height: 44px; z-index: 40;
    align-items: center; gap: 6px; padding: 0 10px 0 12px;
    background: #292a2d; border-top: 1px solid #3c4043;
  }
  html.find-open #findbar { display: flex; }
  #findinput {
    flex: 1; min-width: 0; height: 28px;
    background: #202124; border: 1px solid #3c4043; border-radius: 14px;
    outline: 0; color: #e8eaed; font-size: 12px; padding: 0 12px; user-select: text;
  }
  #findinput:focus { border-color: #8ab4f8; }
  #findcount {
    flex: 0 0 auto; min-width: 40px; text-align: center;
    color: #9aa0a6; font-size: 12px;
  }
</style>
</head>
<body>
  <div class="tabbar" id="tabbar">
    <button class="tab-search" id="tabsearch" title="搜索标签页">
      <svg viewBox="0 0 24 24"><path d="M6 9l6 6 6-6"/></svg>
    </button>
    <div class="tabs" id="tabs">
      <div class="tab active" data-idx="0">
        <span class="t-title">新标签页</span>
        <span class="t-close" title="关闭标签">×</span>
      </div>
      <div class="tab-new" id="tabnew" title="新建标签">+</div>
    </div>
    <div class="win-ctrl">
      <button title="最小化" data-act="winmin">─</button>
      <button title="最大化" data-act="winmax">□</button>
      <button class="close" title="关闭" data-act="winclose">✕</button>
    </div>
  </div>
  <div class="bar">
    <button title="后退" onclick="act('back')">
      <svg viewBox="0 0 24 24"><path d="M15 18l-6-6 6-6"/></svg>
    </button>
    <button title="前进" onclick="act('forward')">
      <svg viewBox="0 0 24 24"><path d="M9 18l6-6-6-6"/></svg>
    </button>
    <button title="刷新" onclick="act('reload')">
      <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 3v6h-6"/></svg>
    </button>

    <div class="addr-wrap">
      <button class="lockbtn" id="lockbtn" title="站点信息">
        <svg class="lock" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="4" y="10" width="16" height="10" rx="2"/>
          <path d="M8 10V7a4 4 0 0 1 8 0v3"/>
        </svg>
      </button>
      <input id="addr" type="text" spellcheck="false" placeholder="搜索或输入网址">
      <button class="star" id="starbtn" title="收藏此页">☆</button>
      <div id="omni"></div>
    </div>
    <div class="zoomctl" title="缩放（Ctrl++ / Ctrl+- / Ctrl+0）">
      <button title="缩小（Ctrl+-）" onclick="act('zoomout')">−</button>
      <span id="zoomval" title="点击复位 100%（Ctrl+0）" onclick="act('zoomreset')">100%</span>
      <button title="放大（Ctrl++）" onclick="act('zoomin')">+</button>
    </div>
    <button title="密码" id="pwbtn">
      <svg viewBox="0 0 24 24"><circle cx="8" cy="15" r="4"/><path d="M11 12l9-9"/><path d="M16 7l3 3"/></svg>
    </button>
    <button title="下载" id="dlbtn">
      <svg viewBox="0 0 24 24"><path d="M12 3v12"/><path d="M7 10l5 5 5-5"/><path d="M4 21h16"/></svg>
    </button>
    <button title="菜单" id="menubtn">
      <svg viewBox="0 0 24 24"><circle cx="12" cy="5" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="12" cy="19" r="1.6"/></svg>
    </button>
  </div>
  <div class="bookmarks" id="bookmarks"></div>
  <div id="findbar">
    <input id="findinput" type="text" spellcheck="false" placeholder="在此页面上查找">
    <span id="findcount">0/0</span>
    <button id="findprev" title="上一个（Shift+Enter）">↑</button>
    <button id="findnext" title="下一个（Enter）">↓</button>
    <button id="findclose" title="关闭（Esc）">✕</button>
  </div>
  <div id="bmfolders"></div>
  <div id="bmmenu"></div>
  <div id="pwpanel">
    <input id="pwsearch" type="text" spellcheck="false" placeholder="搜索站点或用户名">
    <div id="pwlist"></div>
  </div>
  <div id="dlpanel">
    <div class="dl-head"><span class="dl-title">下载记录</span><span id="dlclear" class="bmm-btn">清空已结束</span><span id="dlclose" class="bmm-btn">关闭</span></div>
    <div id="dllist"></div>
  </div>
  <div id="palettepanel">
    <input id="paletteinput" type="text" spellcheck="false" placeholder="搜索标签、历史、书签，或输入命令（↑↓ 选择，回车执行，Esc 关闭）">
    <div id="palettelist"></div>
  </div>
  <div id="menupanel">
    <div class="menu-item" data-act="tabnew">新标签页<span class="menu-key">Ctrl+T</span></div>
    <div class="menu-item" data-panel="palette">命令面板<span class="menu-key">Ctrl+K</span></div>
    <div class="menu-sep"></div>
    <div class="menu-item" data-panel="hist">历史记录<span class="menu-key">Ctrl+H</span></div>
    <div class="menu-item" data-panel="dl">下载内容<span class="menu-key">Ctrl+J</span></div>
    <div class="menu-item" data-act="bmtoggle">书签栏<span class="menu-key">Ctrl+Shift+B</span></div>
    <div class="menu-item" data-panel="bmmanage">书签管理器<span class="menu-key">Ctrl+Shift+O</span></div>
    <div class="menu-item" data-panel="pw">密码<span class="menu-key"></span></div>
    <div class="menu-sep"></div>
    <div class="menu-item disabled">打印<span class="menu-key">待补</span></div>
    <div class="menu-item" data-act="findopen">查找<span class="menu-key">Ctrl+F</span></div>
    <div class="menu-sep"></div>
    <div class="menu-item" data-panel="groups">分组/工作区<span class="menu-key"></span></div>
    <div class="menu-item" data-panel="sess">会话<span class="menu-key"></span></div>
    <div class="menu-item" data-panel="cert">证书<span class="menu-key"></span></div>
    <div class="menu-item" data-panel="us">用户脚本<span class="menu-key"></span></div>
    <div class="menu-item" data-panel="settings">设置<span class="menu-key"></span></div>
    <div class="menu-item" data-act="diag">诊断信息<span class="menu-key">写日志目录</span></div>
    <div class="menu-item disabled">关于 Shift<span class="menu-key">待补</span></div>
    <div class="menu-sep"></div>
    <div class="menu-item" data-act="winclose">退出<span class="menu-key"></span></div>
  </div>
  <div id="histpanel">
    <div class="hist-head">
      <input id="histsearch" type="text" spellcheck="false" placeholder="搜索历史记录">
      <span id="histclear" class="hist-clear">清空</span>
      <span id="histclose" class="hist-close">关闭</span>
    </div>
    <div id="histlist"></div>
  </div>
  <div id="sitepanel">
    <div class="site-host" id="sitehost"></div>
    <div class="site-state" id="sitestate"></div>
    <div class="site-url" id="siteurl"></div>
    <span id="siteclose" class="site-close">关闭</span>
  </div>
  <div id="bmmanage">
    <div class="bmm-head">
      <input id="bmmsearch" type="text" spellcheck="false" placeholder="搜索书签">
      <span id="bmmnew" class="bmm-btn">新建文件夹</span>
      <span id="bmmclose" class="bmm-btn">关闭</span>
    </div>
    <div class="bmm-body">
      <div id="bmmtree"></div>
      <div id="bmmlist"></div>
    </div>
  </div>
  <div id="groupspanel">
    <div class="sess-head"><span class="sess-title">分组 / 工作区</span><span id="groupsclose" class="bmm-btn">关闭</span></div>
    <div id="groupslist"></div>
  </div>
  <div id="sesspanel">
    <div class="sess-head"><span class="sess-title">已保存的会话</span><span id="sessclose" class="bmm-btn">关闭</span></div>
    <div id="sesslist"></div>
  </div>
  <div id="certpanel">
    <div class="cert-head">
      <input id="certsearch" type="text" spellcheck="false" placeholder="搜索 host / 主体 / 签发者">
      <span id="certclose" class="bmm-btn">关闭</span>
    </div>
    <div id="certlist"></div>
  </div>
  <div id="uspanel">
    <div class="us-head">
      <input id="ussearch" type="text" spellcheck="false" placeholder="搜索脚本名称 / 匹配模式">
      <span id="usclose" class="bmm-btn">关闭</span>
    </div>
    <div id="uslist"></div>
  </div>
  <script id="bm-data" type="application/json">__SHIFT_BOOKMARKS_JSON__</script>
  <script id="pw-data" type="application/json">__SHIFT_PASSWORDS_JSON__</script>
  <script id="dl-data" type="application/json">__SHIFT_DOWNLOADS_JSON__</script>
  <script id="us-data" type="application/json">__SHIFT_USERSCRIPTS_JSON__</script>


<script>
  const addr = document.getElementById('addr');

  // 8 个根类面板的 class 清单由 Rust 注册表（shell::Panel）注入；开任一面板 = 清掉其余 7 个
  const PANEL_CLASSES = __SHIFT_PANEL_CLASSES__;
  function removePanelClasses(except) {
    document.documentElement.classList.remove(...PANEL_CLASSES.filter((c) => c !== except));
  }

  function act(cmd) {
    console.log('SHIFT_ACT:' + cmd);
  }

  // 地址栏下拉建议 + 星标（F-CEF-17）
  const omniEl = document.getElementById('omni');
  const starBtn = document.getElementById('starbtn');
  let omniList = [];
  let omniIdx = -1;
  let omniTimer = null;

  function closeOmni() {
    omniEl.classList.remove('open');
    omniEl.innerHTML = '';
    omniList = [];
    omniIdx = -1;
  }

  function renderOmni(list) {
    omniList = Array.isArray(list) ? list : [];
    omniIdx = -1;
    omniEl.innerHTML = '';
    if (omniList.length === 0) { closeOmni(); return; }
    omniList.forEach((it, i) => {
      const row = document.createElement('div');
      row.className = 'omni-item';
      row.dataset.i = String(i);
      const type = document.createElement('span');
      type.className = 'omni-type';
      type.textContent = it.type === 'bookmark' ? '★' : '🕘';
      const title = document.createElement('span');
      title.className = 'omni-title';
      title.textContent = it.title || it.url || '';
      const url = document.createElement('span');
      url.className = 'omni-url';
      url.textContent = it.url || '';
      row.append(type, title, url);
      omniEl.appendChild(row);
    });
    omniEl.classList.add('open');
  }

  window.__shiftOmni = renderOmni;
  window.__shiftStar = function (on) {
    starBtn.classList.toggle('on', !!on);
    starBtn.textContent = on ? '★' : '☆';
    starBtn.title = on ? '已收藏（点击取消）' : '收藏此页';
  };

  function openOmniItem(it) {
    if (!it) return;
    act('nav|' + it.url);
    addr.value = it.url;
    closeOmni();
    addr.blur();
  }

  addr.addEventListener('input', () => {
    clearTimeout(omniTimer);
    const q = addr.value.trim();
    if (!q) { closeOmni(); return; }
    omniTimer = setTimeout(() => act('omni|' + q), 150);
  });

  addr.addEventListener('keydown', (e) => {
    const omniOpen = omniEl.classList.contains('open');
    if (omniOpen && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault();
      const n = omniList.length;
      if (n === 0) return;
      omniIdx = e.key === 'ArrowDown' ? (omniIdx + 1) % n : (omniIdx - 1 + n) % n;
      Array.from(omniEl.children).forEach((el, i) => el.classList.toggle('active', i === omniIdx));
      return;
    }
    if (e.key === 'Enter') {
      if (omniOpen && omniIdx >= 0) {
        e.preventDefault();
        openOmniItem(omniList[omniIdx]);
        return;
      }
      const v = addr.value.trim();
      if (v) act('nav|' + v);
      addr.blur();
    } else if (e.key === 'Escape') {
      if (omniOpen) { closeOmni(); return; }
      addr.blur();
    }
  });

  omniEl.addEventListener('mousedown', (e) => {
    const row = e.target.closest('.omni-item');
    if (!row) return;
    openOmniItem(omniList[Number(row.dataset.i)]);
  });

  addr.addEventListener('blur', () => setTimeout(closeOmni, 150));
  starBtn.addEventListener('click', () => act('startoggle'));


  addr.addEventListener('focus', () => addr.select());

  // ---- 地址栏 / 加载态（Rust 侧 execute_java_script 调用）----
  window.__shiftUrl = function (url) {
    if (document.activeElement === addr) return; // 用户正在输入时不打断
    addr.value = url || '';
  };
  window.__shiftLoading = function (on) {
    document.documentElement.dataset.loading = on ? '1' : '';
  };

  // ---- 缩放显示（F-CEF-18）：Rust 侧推活动标签的档位百分比 ----
  const zoomval = document.getElementById('zoomval');
  window.__shiftZoom = function (pct) {
    zoomval.textContent = pct + '%';
    // Chrome 只在非 100% 时显示缩放控件
    const z = document.querySelector('.zoomctl');
    if (z) z.style.display = (pct === '100%') ? 'none' : 'flex';
  };

  // ---- 标签栏（M2 独立开窗）----
  // Rust 侧通过 execute_java_script 调用 window.__shiftTabs(data) 刷新标签栏；
  // 点击/关闭/新建通过 console.log SHIFT_ACT 回传，与地址栏同一套桥。
  const tabbar = document.getElementById('tabbar');
  const tabs = document.getElementById('tabs');
  // Chrome 标签栏左侧：标签页下拉（分组按钮在书签栏，renderBookmarks 里动态绑定）
  document.getElementById('tabsearch').addEventListener('click', () => act('tablist'));

  let tabData = [];
  let dragFrom = null;
  window.__shiftTabs = function (tabs_, active) {
    if (!Array.isArray(tabs_)) return;
    tabData = tabs_;
    tabs.innerHTML = '';
    tabs_.forEach((t, i) => {
      const el = document.createElement('div');
      el.className = 'tab' + (i === active ? ' active' : '')
        + (t.pinned ? ' pinned' : '') + (t.muted ? ' muted' : '');
      el.dataset.idx = String(i);
      const pin = document.createElement('span');
      pin.className = 't-pin';
      pin.textContent = '📌';
      const mute = document.createElement('span');
      mute.className = 't-mute';
      mute.textContent = '🔇';
      const title = document.createElement('span');
      title.className = 't-title';
      title.textContent = t.title || '新标签页';
      const close = document.createElement('span');
      close.className = 't-close';
      close.title = '关闭标签';
      close.textContent = '×';
      el.appendChild(pin);
      el.appendChild(mute);
      el.appendChild(title);
      el.appendChild(close);
      el.draggable = true;
      el.addEventListener('dragstart', (e) => {
        dragFrom = i;
        el.classList.add('dragging');
        if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
      });
      el.addEventListener('dragend', () => {
        el.classList.remove('dragging');
        document.querySelectorAll('#tabbar .tab.drop-target').forEach((x) => x.classList.remove('drop-target'));
      });
      el.addEventListener('dragover', (e) => {
        e.preventDefault();
        if (dragFrom !== null && dragFrom !== i) el.classList.add('drop-target');
      });
      el.addEventListener('dragleave', () => el.classList.remove('drop-target'));
      el.addEventListener('drop', (e) => {
        e.preventDefault();
        el.classList.remove('drop-target');
        if (dragFrom !== null && dragFrom !== i) act('tabmove|' + dragFrom + ',' + i);
        dragFrom = null;
      });
      el.addEventListener('contextmenu', (e) => {
        e.preventDefault();
        e.stopPropagation();
        // F-CEF-40：标签右键菜单交给 GTK 原生弹出，不再困在 122px 工具栏里。
        act('tabmenu|' + i);
      });
      tabs.appendChild(el);
    });
    const add = document.createElement('div');
    add.className = 'tab-new';
    add.id = 'tabnew';
    add.title = '新建标签';
    add.textContent = '+';
    tabs.appendChild(add);
  };

  tabbar.addEventListener('click', (e) => {
    const closeBtn = e.target.closest('.t-close');
    if (closeBtn) {
      const idx = closeBtn.closest('.tab').dataset.idx;
      act('tabclose|' + idx);
      e.stopPropagation();
      return;
    }
    if (e.target.closest('.tab-new')) {
      act('tabnew');
      return;
    }
    const tab = e.target.closest('.tab');
    if (tab) act('tab|' + tab.dataset.idx);
  });

  // 书签栏：Rust 侧读到数据库后整表刷新；未读到前保留 HTML 里的静态兜底
  window.__shiftBookmarks = function (data) {
    // 新格式 {folders,bookmarks}；旧格式（纯数组）也能吃
    if (Array.isArray(data)) { renderBookmarks({ folders: [], bookmarks: data }); return; }
    renderBookmarks(data);
  };

  window.__shiftBookmarksVisible = function (visible) {
    document.documentElement.classList.toggle('bookmarks-hidden', !visible);
  };

  // 书签点击 → 当前标签导航
  document.getElementById('bookmarks').addEventListener('click', (e) => {
    const bm = e.target.closest('.bm');
    if (bm && bm.dataset.url) act('nav|' + bm.dataset.url);
  });

  // 窗口控制按钮
  document.querySelector('.win-ctrl').addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-act]');
    if (!btn) return;
    act(btn.dataset.act);
  });

  // 拖动窗口：按住标签栏空白处拖动（走 _NET_WM_MOVERESIZE，由 WM 接管）
  tabbar.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    if (e.target.closest('.tab') || e.target.closest('button')) return;
    act('windrag');
  });

  // 中键关闭标签（浏览器通用习惯）
  tabbar.addEventListener('auxclick', (e) => {
    if (e.button !== 1) return;
    const tab = e.target.closest('.tab');
    if (!tab) return;
    e.preventDefault();
    act('tabclose|' + tab.dataset.idx);
  });

  // 标签栏空白处双击 = 新建标签（与主界面一致）
  tabbar.addEventListener('dblclick', (e) => {
    if (e.target.closest('.tab')) return;
    act('tabnew');
  });

  // 内联改标题（CEF 的 Views 模式没有 window.prompt，只能自己画输入框）
  function editBookmarkTitle(item) {
    const nameEl = item.querySelector('.bm-name');
    if (!nameEl || item.querySelector('.bm-edit')) return;
    const old = nameEl.textContent;
    const input = document.createElement('input');
    input.className = 'bm-edit';
    input.value = old;
    input.spellcheck = false;
    nameEl.replaceWith(input);
    input.focus();
    input.select();
    let done = false;
    const finish = (save) => {
      if (done) return;
      done = true;
      const v = input.value.trim();
      const span = document.createElement('span');
      span.className = 'bm-name';
      span.textContent = save && v ? v : old;
      input.replaceWith(span);
      if (save && v && v !== old) {
        act('bmedit|' + JSON.stringify({ id: item.dataset.id, title: v }));
      }
    };
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); finish(true); }
      else if (e.key === 'Escape') { e.preventDefault(); finish(false); }
    });
    input.addEventListener('blur', () => finish(false));
    input.addEventListener('click', (e) => e.stopPropagation());
  }

  // ---- 书签右键菜单（F-CEF-13）----
  const bmmenu = document.getElementById('bmmenu');
  let bmMenuTarget = null;
  let bmMenuPos = { x: 0, y: 0 };

  function hideBmMenu() {
    bmmenu.classList.remove('open');
    bmMenuTarget = null;
  }

  // 菜单里就地输入文件夹名（CEF 没有 window.prompt，只能自己画）
  function newFolderPrompt() {
    bmmenu.innerHTML = '';
    const box = document.createElement('div');
    box.className = 'menu-edit';
    const input = document.createElement('input');
    input.className = 'bm-edit';
    input.placeholder = '文件夹名称';
    input.spellcheck = false;
    box.appendChild(input);
    bmmenu.appendChild(box);
    input.focus();
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        const v = input.value.trim();
        hideBmMenu();
        if (v) act('bmnewfolder|' + v);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        hideBmMenu();
      }
    });
    input.addEventListener('click', (e) => e.stopPropagation());
  }

  document.getElementById('bookmarks').addEventListener('contextmenu', (e) => {
    e.preventDefault();
    const item = e.target.closest('.bm');
    bmMenuTarget = item;
    bmMenuPos = { x: e.clientX, y: e.clientY };
    let rows;
    if (!item) {
      rows = [
        ['新建书签（当前页）', () => act('bmadd')],
        ['新建文件夹', () => newFolderPrompt()],
      ];
    } else if (item.classList.contains('folder')) {
      const fid = Number(item.dataset.folder);
      rows = [
        ['打开文件夹', () => showFolderMenu({ id: fid, name: item.querySelector('.bm-name').textContent }, item)],
        ['在此新建子文件夹', () => newSubFolderPrompt(fid)],
      ];
    } else {
      rows = [
        ['打开', () => act('nav|' + item.dataset.url)],
        ['编辑标题', () => editBookmarkTitle(item)],
        ['移到文件夹', () => showMoveMenu(item.dataset.id, bmMenuPos)],
        ['删除', () => act('bmdel|' + item.dataset.id), 'danger'],
      ];
    }
    bmmenu.innerHTML = '';
    rows.forEach(([label, fn, cls]) => {
      const d = document.createElement('div');
      d.textContent = label;
      if (cls) d.className = cls;
      d.addEventListener('click', () => { hideBmMenu(); fn(); });
      bmmenu.appendChild(d);
    });
    bmmenu.style.left = e.clientX + 'px';
    bmmenu.style.top = e.clientY + 'px';
    bmmenu.classList.add('open');
  });

  document.addEventListener('click', (e) => {
    if (!e.target.closest('#bmmenu')) hideBmMenu();
  });

  // 「移到文件夹」选择面板（复用 #bmfolders 面板，位置沿用右键点）
  function showMoveMenu(bmId, pos) {
    const panel = document.getElementById('bmfolders');
    panel.innerHTML = '';
    const head = document.createElement('div');
    head.className = 'fd-title';
    head.textContent = '移到文件夹';
    panel.appendChild(head);
    const mk = (label, folderId) => {
      const d = document.createElement('div');
      d.className = 'fd-item';
      d.textContent = label;
      d.addEventListener('click', () => {
        hideFolderMenu();
        act('bmmove|' + JSON.stringify({ id: Number(bmId), folderId }));
      });
      panel.appendChild(d);
    };
    mk('— 书签栏（不分组）', null);
    // 按父子关系展开，缩进表示层级
    const kids = new Map();
    (bmData.folders || []).forEach((f) => {
      const k = f.parentId === null || f.parentId === undefined ? 'root' : f.parentId;
      if (!kids.has(k)) kids.set(k, []);
      kids.get(k).push(f);
    });
    const walk = (key, depth) => {
      (kids.get(key) || []).forEach((f) => {
        mk('　'.repeat(depth) + f.name, f.id);
        walk(f.id, depth + 1);
      });
    };
    walk('root', 0);
    panel.style.left = Math.min(pos.x, window.innerWidth - 340) + 'px';
    panel.style.top = Math.min(pos.y + 6, window.innerHeight - 380) + 'px';
    panel.classList.add('open');
    act('bmfoldopen|' + (Math.min(panel.scrollHeight, 300) + 20));
  }

  // 在指定文件夹下新建子文件夹
  function newSubFolderPrompt(parentId) {
    bmmenu.innerHTML = '';
    const box = document.createElement('div');
    box.className = 'menu-edit';
    const input = document.createElement('input');
    input.className = 'bm-edit';
    input.placeholder = '子文件夹名称';
    input.spellcheck = false;
    box.appendChild(input);
    bmmenu.appendChild(box);
    input.focus();
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        const v = input.value.trim();
        hideBmMenu();
        if (v) act('bmnewfolder|' + JSON.stringify({ name: v, parentId }));
      } else if (e.key === 'Escape') {
        e.preventDefault();
        hideBmMenu();
      }
    });
    input.addEventListener('click', (e) => e.stopPropagation());
  }

  // 书签栏渲染：未分组书签平铺 + 顶层文件夹（点击展开下拉）
  // 数据格式 { folders: [{id,name,parentId}], bookmarks: [{id,title,url,folderId}] }
  const bmfolders = document.getElementById('bmfolders');
  let bmData = { folders: [], bookmarks: [] };

  function hideFolderMenu() {
    if (!bmfolders.classList.contains('open')) return; // 避免每次点页面都往 Rust 发消息
    bmfolders.classList.remove('open');
    act('bmfoldclose');
  }

  function bmItemHtml(b) {
    const el = document.createElement('div');
    el.className = 'bm';
    el.dataset.url = b.url || '';
    el.dataset.id = String(b.id || '');
    const ico = document.createElement('span');
    ico.className = 'bm-ico';
    const title = b.title || b.url || '书签';
    ico.textContent = title.trim().charAt(0).toUpperCase();
    const name = document.createElement('span');
    name.className = 'bm-name';
    name.textContent = title;
    el.appendChild(ico);
    el.appendChild(name);
    return el;
  }

  function renderBookmarks(data) {
    const bar = document.getElementById('bookmarks');
    if (!bar || !data || !Array.isArray(data.bookmarks)) return;
    bmData = data;
    bar.innerHTML = '';
    const topFolders = (data.folders || []).filter((f) => f.parentId === null || f.parentId === undefined);
    // 所有层级的文件夹 id：子文件夹里的书签也只在下拉里出现，不平铺
    const allFolderIds = new Set((data.folders || []).map((f) => f.id));
    data.bookmarks
      .filter((b) => b.folderId === null || b.folderId === undefined || !allFolderIds.has(b.folderId))
      .forEach((b) => bar.appendChild(bmItemHtml(b)));
    if (document.documentElement.classList.contains('bmmanage-open')) { renderBmmTree(); renderBmmList(); }
    // 顶层文件夹
    topFolders.forEach((f) => {
      const el = document.createElement('div');
      el.className = 'bm folder';
      el.dataset.folder = String(f.id);
      const ico = document.createElement('span');
      ico.className = 'bm-ico';
      ico.textContent = '📁';
      const name = document.createElement('span');
      name.className = 'bm-name';
      name.textContent = f.name;
      el.appendChild(ico);
      el.appendChild(name);
      el.addEventListener('click', () => showFolderMenu(f, el));
      bar.appendChild(el);
    });
  }

  // F-CEF-40：文件夹下拉改 GTK 原生菜单，浮在内容上，不再加高工具栏。
  // 把点击项的左下角坐标一起传给宿主，宿主按窗口位置精确落点。
  function showFolderMenu(folder, anchorEl) {
    const rc = anchorEl ? anchorEl.getBoundingClientRect() : { left: 0, bottom: 0 };
    act('bmfoldermenu|' + folder.id + ',' + Math.round(rc.left) + ',' + Math.round(rc.bottom + 4));
  }

  document.addEventListener('click', (e) => {
    if (!e.target.closest('#bmfolders') && !e.target.closest('.bm.folder')) hideFolderMenu();
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      hideFolderMenu();
      hideBmMenu();
    }
  });

  // 启动：解析内嵌数据并渲染
  (function bootBookmarks() {
    const el = document.getElementById('bm-data');
    if (!el) return;
    let data = { folders: [], bookmarks: [] };
    try { data = JSON.parse(el.textContent || '{}'); } catch (e) { data = { folders: [], bookmarks: [] }; }
    if (Array.isArray(data)) data = { folders: [], bookmarks: data };
    renderBookmarks(data);
  })();

  // ---- 密码面板（F-CEF-12）----
  // 数据只有 id/站点/用户名；明文永不进入本页——复制/删除全在 Rust 侧完成。
  let pwList = [];
  try { pwList = JSON.parse(document.getElementById('pw-data').textContent || '[]'); } catch (e) { pwList = []; }
  if (!Array.isArray(pwList)) pwList = [];
  const pwListEl = document.getElementById('pwlist');
  const pwSearch = document.getElementById('pwsearch');

  function renderPasswords() {
    const q = pwSearch.value.trim().toLowerCase();
    pwListEl.innerHTML = '';
    const hit = pwList.filter((p) =>
      !q || (p.domain || '').toLowerCase().includes(q) || (p.username || '').toLowerCase().includes(q));
    if (hit.length === 0) {
      const e = document.createElement('div');
      e.className = 'pw-empty';
      e.textContent = pwList.length === 0 ? '密码库为空' : '没有匹配项';
      pwListEl.appendChild(e);
      return;
    }
    hit.forEach((p) => {
      const row = document.createElement('div');
      row.className = 'pw-row';
      row.dataset.id = String(p.id);
      const dom = document.createElement('span');
      dom.className = 'pw-dom';
      dom.textContent = p.domain || '';
      const usr = document.createElement('span');
      usr.className = 'pw-usr';
      usr.textContent = p.username || '';
      const mask = document.createElement('span');
      mask.className = 'pw-mask';
      mask.textContent = '••••';
      const cp = document.createElement('span');
      cp.className = 'pw-btn';
      cp.dataset.op = 'copy';
      cp.textContent = '复制';
      const del = document.createElement('span');
      del.className = 'pw-btn danger';
      del.dataset.op = 'del';
      del.textContent = '删除';
      row.append(dom, usr, mask, cp, del);
      pwListEl.appendChild(row);
    });
  }

  function setPwPanel(open) {
    document.documentElement.classList.toggle('pw-open', open);
    if (open) removePanelClasses('pw-open');
    act('pwpanel|' + (open ? '1' : '0'));
    if (open) { pwSearch.value = ''; renderPasswords(); pwSearch.focus(); }
  }
  document.getElementById('pwbtn').addEventListener('click', () => {
    setPwPanel(!document.documentElement.classList.contains('pw-open'));
  });
  pwSearch.addEventListener('input', renderPasswords);
  pwSearch.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') setPwPanel(false);
  });
  pwListEl.addEventListener('click', (e) => {
    const btn = e.target.closest('.pw-btn');
    const row = e.target.closest('.pw-row');
    if (!btn || !row) return;
    const id = row.dataset.id;
    if (btn.dataset.op === 'copy') {
      act('pwcopy|' + id);
      btn.textContent = '已复制';
      setTimeout(() => { btn.textContent = '复制'; }, 1500);
    } else if (btn.dataset.op === 'del') {
      // 二次确认：第一次点变「确认删除」，3 秒内再点才真删
      if (btn.dataset.armed === '1') {
        act('pwdel|' + id);
        pwList = pwList.filter((p) => String(p.id) !== id);
        renderPasswords();
      } else {
        btn.dataset.armed = '1';
        btn.textContent = '确认删除';
        setTimeout(() => { btn.dataset.armed = ''; btn.textContent = '删除'; }, 3000);
      }
    }
  });

  // ---- 下载面板（F-CEF-15）----
  // 数据只含 id/文件名/状态/字节数；打开目录/重下/删记录都回 Rust 执行，
  // 页面拿不到真实路径（减少一个可被页面脚本滥用的面）。
  let dlList = [];
  try { dlList = JSON.parse(document.getElementById('dl-data').textContent || '[]'); } catch (e) { dlList = []; }
  if (!Array.isArray(dlList)) dlList = [];
  const dlListEl = document.getElementById('dllist');

  function dlPct(d) {
    if (!d || !d.total || d.total <= 0) return 0;
    return Math.min(100, Math.round((d.received || 0) * 100 / d.total));
  }

  function dlStateText(d) {
    switch (d.state) {
      case 'in_progress': return '下载中 ' + dlPct(d) + '%';
      case 'completed': return '已完成';
      case 'failed': return '失败';
      case 'cancelled': return '已取消';
      case 'interrupted': return '已中断';
      default: return d.state || '';
    }
  }

  function renderDownloads() {
    dlListEl.innerHTML = '';
    if (dlList.length === 0) {
      const e = document.createElement('div');
      e.className = 'dl-empty';
      e.textContent = '还没有下载记录';
      dlListEl.appendChild(e);
      return;
    }
    dlList.forEach((d) => {
      const row = document.createElement('div');
      row.className = 'dl-row';
      row.dataset.id = String(d.id);
      const name = document.createElement('span');
      name.className = 'dl-name';
      name.textContent = d.filename || '(未命名)';
      const st = document.createElement('span');
      st.className = 'dl-state';
      st.textContent = dlStateText(d);
      const bar = document.createElement('div');
      bar.className = 'dl-bar';
      const fill = document.createElement('i');
      fill.style.width = dlPct(d) + '%';
      bar.appendChild(fill);
      const open = document.createElement('span');
      open.className = 'dl-btn';
      open.dataset.op = 'open';
      open.textContent = '打开目录';
      const retry = document.createElement('span');
      retry.className = 'dl-btn';
      retry.dataset.op = 'retry';
      retry.textContent = '重下';
      const del = document.createElement('span');
      del.className = 'dl-btn danger';
      del.dataset.op = 'del';
      del.textContent = '删除记录';
      if (d.state === 'in_progress') retry.style.display = 'none';
      row.append(name, st, bar, open, retry, del);
      dlListEl.appendChild(row);
    });
  }

  // ---- 命令面板（F-CEF-29）----
  let plData = { tabs: [], history: [], bookmarks: [], commands: [] };
  let plSel = 0;
  function plEsc(v) {
    return String(v == null ? '' : v).replace(/[&<>"']/g, c => (
      { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]
    ));
  }
  function plFlat() {
    const out = [];
    for (const g of ['tabs', 'history', 'bookmarks', 'commands']) {
      for (const it of (plData[g] || [])) out.push({ it, g });
    }
    return out;
  }
  function plRender() {
    const list = document.getElementById('palettelist');
    if (!list) return;
    const groups = { tabs: '标签页', history: '历史记录', bookmarks: '书签', commands: '命令' };
    let html = '', idx = 0;
    for (const g of Object.keys(groups)) {
      const items = plData[g] || [];
      if (!items.length) continue;
      html += '<div class="pl-group">' + groups[g] + '</div>';
      for (const it of items) {
        const title = it.title || it.label || it.url || '';
        const hint = it.hint || (it.url || '');
        html += '<div class="pl-item' + (idx === plSel ? ' sel' : '') + '" data-i="' + idx + '">'
          + '<span class="pl-title">' + plEsc(title) + '</span>'
          + '<span class="pl-hint">' + plEsc(String(hint).slice(0, 36)) + '</span></div>';
        idx++;
      }
    }
    list.innerHTML = html || '<div class="pl-group">无匹配结果</div>';
    const sel = list.querySelector('.pl-item.sel');
    if (sel && sel.scrollIntoView) sel.scrollIntoView({ block: 'nearest' });
  }
  window.__shiftPalette = function (d) { plData = d || {}; plSel = 0; plRender(); };
  function plExec(it, g) {
    let arg;
    if (g === 'commands') arg = 'cmd:' + it.id;
    else if (g === 'tabs') arg = 'tab:' + it.id;
    else arg = 'url:' + (it.url || '');
    act('paletteexec|' + arg);
    setPalettePanel(false);
  }
  function setPalettePanel(open) {
    document.documentElement.classList.toggle('palette-open', open);
    if (open) {
      removePanelClasses('palette-open');
      act('palette|1');
      const inp = document.getElementById('paletteinput');
      if (inp) { inp.value = ''; setTimeout(() => inp.focus(), 0); }
      act('palettequery|');
    } else {
      act('palette|0');
    }
  }
  document.getElementById('paletteinput')?.addEventListener('input', (e) => {
    act('palettequery|' + e.target.value);
  });
  document.getElementById('paletteinput')?.addEventListener('keydown', (e) => {
    const flat = plFlat();
    if (e.key === 'ArrowDown') { plSel = Math.min(plSel + 1, flat.length - 1); plRender(); e.preventDefault(); }
    else if (e.key === 'ArrowUp') { plSel = Math.max(plSel - 1, 0); plRender(); e.preventDefault(); }
    else if (e.key === 'Enter') { const f = flat[plSel]; if (f) plExec(f.it, f.g); e.preventDefault(); }
    else if (e.key === 'Escape') { setPalettePanel(false); e.preventDefault(); }
  });
  document.getElementById('palettelist')?.addEventListener('click', (e) => {
    const el = e.target.closest('.pl-item');
    if (!el) return;
    const flat = plFlat();
    const f = flat[+el.dataset.i];
    if (f) plExec(f.it, f.g);
  });

  function setDlPanel(open) {
    document.documentElement.classList.toggle('dl-open', open);
    if (open) removePanelClasses('dl-open');
    act('dlpanel|' + (open ? '1' : '0'));
    if (open) renderDownloads();
  }
  document.getElementById('dlclear')?.addEventListener('click', () => {
    act('dlclear');
    dlList = [];
    renderDownloads();
  });
  document.getElementById('dlclose')?.addEventListener('click', () => setDlPanel(false));
  document.getElementById('dlbtn').addEventListener('click', () => {
    setDlPanel(!document.documentElement.classList.contains('dl-open'));
  });
  dlListEl.addEventListener('click', (e) => {
    const btn = e.target.closest('.dl-btn');
    const row = e.target.closest('.dl-row');
    if (!btn || !row) return;
    const id = row.dataset.id;
    if (btn.dataset.op === 'open') {
      act('dlopen|' + id);
    } else if (btn.dataset.op === 'retry') {
      act('dlretry|' + id);
    } else if (btn.dataset.op === 'del') {
      if (btn.dataset.armed === '1') {
        act('dldel|' + id);
        dlList = dlList.filter((d) => String(d.id) !== id);
        renderDownloads();
      } else {
        btn.dataset.armed = '1';
        btn.textContent = '确认删除';
        setTimeout(() => { btn.dataset.armed = ''; btn.textContent = '删除记录'; }, 3000);
      }
    }
  });
  // Rust 推送最新列表（下载登记/进度/终态时调用）
  window.__shiftDownloads = (list) => {
    if (!Array.isArray(list)) return;
    dlList = list;
    if (document.documentElement.classList.contains('dl-open')) renderDownloads();
  };

  // ---- 历史面板（F-CEF-19）----
  // 数据来自 Rust（history 表），页面不自己数 DOM；点击行导航，删除/清空二次确认。
  let histList = [];
  const histPanelEl = document.getElementById('histpanel');
  const histSearch = document.getElementById('histsearch');
  const histListEl = document.getElementById('histlist');
  let histTimer = null;

  function renderHistory(list) {
    histList = Array.isArray(list) ? list : [];
    histListEl.innerHTML = '';
    if (histList.length === 0) {
      const e = document.createElement('div');
      e.className = 'hist-empty';
      e.textContent = '没有历史记录';
      histListEl.appendChild(e);
      return;
    }
    histList.forEach((h) => {
      const row = document.createElement('div');
      row.className = 'hist-row';
      row.dataset.id = String(h.id);
      const t = document.createElement('span');
      t.className = 'h-title';
      t.textContent = h.title || h.url || '';
      const u = document.createElement('span');
      u.className = 'h-url';
      u.textContent = h.url || '';
      const tm = document.createElement('span');
      tm.className = 'h-time';
      tm.textContent = (h.last_visit_at || '').slice(0, 16);
      const del = document.createElement('span');
      del.className = 'h-del';
      del.textContent = '删除';
      row.append(t, u, tm, del);
      histListEl.appendChild(row);
    });
  }
  window.__shiftHistory = renderHistory;

  function setHistPanel(open) {
    document.documentElement.classList.toggle('hist-open', open);
    if (open) removePanelClasses('hist-open');
    act('histpanel|' + (open ? '1' : '0'));
    if (open) {
      histSearch.value = '';
      renderHistory([]);
      histSearch.focus();
    }
  }
  document.getElementById('histclose').addEventListener('click', () => setHistPanel(false));
  histSearch.addEventListener('input', () => {
    clearTimeout(histTimer);
    histTimer = setTimeout(() => act('histsearch|' + histSearch.value.trim()), 150);
  });
  histListEl.addEventListener('click', (e) => {
    const del = e.target.closest('.h-del');
    const row = e.target.closest('.hist-row');
    if (!row) return;
    const id = row.dataset.id;
    if (del) {
      if (del.dataset.armed === '1') {
        act('histdel|' + id);
        histList = histList.filter((h) => String(h.id) !== id);
        renderHistory(histList);
      } else {
        del.dataset.armed = '1';
        del.textContent = '确认';
        setTimeout(() => { del.dataset.armed = ''; del.textContent = '删除'; }, 3000);
      }
      return;
    }
    const h = histList.find((x) => String(x.id) === id);
    if (h) {
      act('nav|' + h.url);
      setHistPanel(false);
    }
  });
  const histClearBtn = document.getElementById('histclear');
  histClearBtn.addEventListener('click', () => {
    if (histClearBtn.dataset.armed === '1') {
      act('histclear');
      histList = [];
      renderHistory([]);
      histClearBtn.dataset.armed = '';
      histClearBtn.textContent = '清空';
      histClearBtn.classList.remove('danger');
    } else {
      histClearBtn.dataset.armed = '1';
      histClearBtn.textContent = '确认清空';
      histClearBtn.classList.add('danger');
      setTimeout(() => {
        histClearBtn.dataset.armed = '';
        histClearBtn.textContent = '清空';
        histClearBtn.classList.remove('danger');
      }, 3000);
    }
  });

  // ---- 书签管理面板（F-CEF-20）----
  let bmmFilter = 'all';
  function bmmFoldersOf(parent) {
    return (bmData.folders || []).filter((f) => (f.parentId === undefined ? null : f.parentId) === parent);
  }
  function renderBmmTree() {
    const tree = document.getElementById('bmmtree');
    if (!tree) return;
    tree.innerHTML = '';
    const mk = (label, key, depth, folderId) => {
      const d = document.createElement('div');
      d.className = 'bmm-folder' + (bmmFilter === key ? ' active' : '');
      d.style.paddingLeft = 8 + depth * 14 + 'px';
      const name = document.createElement('span');
      name.textContent = label;
      name.style.flex = '1';
      name.style.overflow = 'hidden';
      name.style.textOverflow = 'ellipsis';
      name.style.whiteSpace = 'nowrap';
      d.appendChild(name);
      if (folderId != null) {
        const del = document.createElement('span');
        del.className = 'bmm-folder-del';
        del.textContent = '×';
        del.title = '删除文件夹（会删掉夹内书签）';
        del.addEventListener('click', (e) => {
          e.stopPropagation();
          if (del.dataset.armed === '1') {
            act('bmdeletefolder|' + folderId);
          } else {
            del.dataset.armed = '1';
            del.textContent = '确认';
            setTimeout(() => { del.dataset.armed = ''; del.textContent = '×'; }, 3000);
          }
        });
        d.appendChild(del);
      }
      d.addEventListener('click', () => { bmmFilter = key; renderBmmTree(); renderBmmList(); });
      tree.appendChild(d);
    };
    mk('全部书签', 'all', 0);
    mk('未分类', 'root', 0);
    const walk = (parent, depth) => {
      bmmFoldersOf(parent).forEach((f) => {
        mk('📁 ' + f.name, String(f.id), depth, f.id);
        walk(f.id, depth + 1);
      });
    };
    walk(null, 0);
  }
  function renderBmmList() {
    const list = document.getElementById('bmmlist');
    if (!list) return;
    const q = (document.getElementById('bmmsearch')?.value || '').trim().toLowerCase();
    let rows = (bmData.bookmarks || []).slice();
    if (bmmFilter === 'root') rows = rows.filter((b) => b.folderId === null || b.folderId === undefined);
    else if (bmmFilter !== 'all') rows = rows.filter((b) => String(b.folderId) === bmmFilter);
    if (q) {
      rows = rows.filter(
        (b) => (b.title || '').toLowerCase().includes(q) || (b.url || '').toLowerCase().includes(q)
      );
    }
    list.innerHTML = '';
    if (rows.length === 0) {
      const e = document.createElement('div');
      e.className = 'bmm-empty';
      e.textContent = '没有书签';
      list.appendChild(e);
      return;
    }
    rows.forEach((b) => {
      const row = document.createElement('div');
      row.className = 'bmm-item';
      row.dataset.id = String(b.id);
      const t = document.createElement('span');
      t.className = 'm-title';
      t.textContent = b.title || b.url || '';
      const u = document.createElement('span');
      u.className = 'm-url';
      u.textContent = b.url || '';
      const open = document.createElement('span');
      open.className = 'm-op';
      open.dataset.op = 'open';
      open.textContent = '打开';
      const edit = document.createElement('span');
      edit.className = 'm-op';
      edit.dataset.op = 'edit';
      edit.textContent = '编辑';
      const move = document.createElement('span');
      move.className = 'm-op';
      move.dataset.op = 'move';
      move.textContent = '移动';
      const del = document.createElement('span');
      del.className = 'm-op danger';
      del.dataset.op = 'del';
      del.textContent = '删除';
      row.append(t, u, open, edit, move, del);
      list.appendChild(row);
    });
  }
  function setBmManage(open) {
    document.documentElement.classList.toggle('bmmanage-open', open);
    if (open) removePanelClasses('bmmanage-open');
    act('bmmanage|' + (open ? '1' : '0'));
    if (open) { bmmFilter = 'all'; document.getElementById('bmmsearch').value = ''; renderBmmTree(); renderBmmList(); }
  }
  document.getElementById('bmmclose')?.addEventListener('click', () => setBmManage(false));
  document.getElementById('bmmnew')?.addEventListener('click', () => {
    const head = document.querySelector('.bmm-head');
    if (!head || head.querySelector('.m-newfolder')) return;
    const input = document.createElement('input');
    input.className = 'm-newfolder';
    input.placeholder = '文件夹名称，回车创建';
    input.style.cssText = 'flex:1;height:26px;background:#202124;border:1px solid #8ab4f8;border-radius:6px;color:#e8eaed;padding:0 8px;font-size:12px;';
    head.insertBefore(input, document.getElementById('bmmnew'));
    input.focus();
    input.addEventListener('keydown', (ev) => {
      if (ev.key === 'Enter') {
        const name = input.value.trim();
        if (name) act('bmnewfolder|' + name);
        input.remove();
      } else if (ev.key === 'Escape') {
        input.remove();
      }
    });
  });
  document.getElementById('bmmsearch')?.addEventListener('input', renderBmmList);
  document.getElementById('bmmlist')?.addEventListener('click', (e) => {
    const op = e.target.closest('.m-op');
    const row = e.target.closest('.bmm-item');
    if (!op || !row) return;
    const id = row.dataset.id;
    if (op.dataset.op === 'open') {
      const b = (bmData.bookmarks || []).find((x) => String(x.id) === id);
      if (b) { act('nav|' + b.url); setBmManage(false); }
    } else if (op.dataset.op === 'edit') {
      const b = (bmData.bookmarks || []).find((x) => String(x.id) === id);
      if (!b || row.querySelector('.m-edit')) return;
      const input = document.createElement('input');
      input.className = 'm-edit';
      input.value = b.title || '';
      const titleEl = row.querySelector('.m-title');
      titleEl.replaceWith(input);
      input.focus();
      input.select();
      const commit = () => { act('bmedit|' + JSON.stringify({ id: id, title: input.value.trim() })); };
      input.addEventListener('keydown', (ev) => {
        if (ev.key === 'Enter') { commit(); }
        else if (ev.key === 'Escape') { renderBmmList(); }
      });
      input.addEventListener('blur', () => setTimeout(commit, 150));
    } else if (op.dataset.op === 'move') {
      if (row.querySelector('.m-move')) return;
      const sel = document.createElement('select');
      sel.className = 'm-move';
      const add = (label, value, depth) => {
        const o = document.createElement('option');
        o.value = value;
        o.textContent = '\u00a0'.repeat(depth * 2) + label;
        sel.appendChild(o);
      };
      add('未分类（根目录）', '');
      const walk = (parent, depth) => {
        bmmFoldersOf(parent).forEach((f) => {
          add(f.name, String(f.id), depth + 1);
          walk(f.id, depth + 1);
        });
      };
      walk(null, 0);
      const cur = (bmData.bookmarks || []).find((x) => String(x.id) === id);
      sel.value = cur && cur.folderId != null ? String(cur.folderId) : '';
      const opEl = op;
      opEl.replaceWith(sel);
      sel.focus();
      sel.addEventListener('change', () => {
        act('bmmove|' + JSON.stringify({ id: id, folderId: sel.value === '' ? null : Number(sel.value) }));
      });
      sel.addEventListener('blur', () => setTimeout(renderBmmList, 200));
    } else if (op.dataset.op === 'del') {
      if (op.dataset.armed === '1') { act('bmdel|' + id); }
      else { op.dataset.armed = '1'; op.textContent = '确认'; setTimeout(() => { op.dataset.armed = ''; op.textContent = '删除'; }, 3000); }
    }
  });

  // ---- 分组/工作区面板（F-CEF-27）----
  let groupsData = { groups: [], workspaces: [] };
  function renderGroups(data) {
    window.__shiftGroupMeta = (data && data.groups) || [];
    groupsData = data && typeof data === 'object' ? data : { groups: [], workspaces: [] };
    const box = document.getElementById('groupslist');
    if (!box) return;
    box.innerHTML = '';
    const rows = [];
    (groupsData.groups || []).forEach((g) => rows.push({ kind: '分组', id: g.id, name: g.name, del: 'groupdel' }));
    (groupsData.workspaces || []).forEach((w) => rows.push({ kind: '工作区', id: w.id, name: w.name, del: 'spacedel' }));
    if (rows.length === 0) {
      const e = document.createElement('div');
      e.className = 'g-empty';
      e.textContent = '还没有分组或工作区';
      box.appendChild(e);
      return;
    }
    rows.forEach((r) => {
      const row = document.createElement('div');
      row.className = 'g-row';
      const kind = document.createElement('span');
      kind.className = 'g-kind';
      kind.textContent = r.kind;
      const name = document.createElement('span');
      name.className = 'g-name';
      name.textContent = r.name || ('#' + r.id);
      const del = document.createElement('span');
      del.className = 'g-del';
      del.textContent = '删除';
      del.addEventListener('click', () => {
        if (del.dataset.armed === '1') {
          act(r.del + '|' + r.id);
        } else {
          del.dataset.armed = '1';
          del.textContent = '确认';
          setTimeout(() => { del.dataset.armed = ''; del.textContent = '删除'; }, 3000);
        }
      });
      row.append(kind, name, del);
      box.appendChild(row);
    });
  }
  window.__shiftGroups = renderGroups;
  function setGroupsPanel(open) {
    document.documentElement.classList.toggle('groups-open', open);
    if (open) removePanelClasses('groups-open');
    act('groupspanel|' + (open ? '1' : '0'));
    if (open) renderGroups({ groups: [], workspaces: [] });
  }
  document.getElementById('groupsclose')?.addEventListener('click', () => setGroupsPanel(false));

  // ---- 会话面板（F-CEF-26）----
  let sessList = [];
  function renderSessions(list) {
    sessList = Array.isArray(list) ? list : [];
    const box = document.getElementById('sesslist');
    if (!box) return;
    box.innerHTML = '';
    if (sessList.length === 0) {
      const e = document.createElement('div');
      e.className = 'sess-empty';
      e.textContent = '还没有保存的会话';
      box.appendChild(e);
      return;
    }
    sessList.forEach((s) => {
      const row = document.createElement('div');
      row.className = 'sess-row';
      const name = document.createElement('span');
      name.className = 's-name';
      name.textContent = s.name || ('会话 ' + s.id);
      const time = document.createElement('span');
      time.className = 's-time';
      time.textContent = (s.created_at || '').slice(0, 16);
      const count = document.createElement('span');
      count.className = 's-count';
      count.textContent = (s.tab_count != null ? s.tab_count : '?') + ' 个标签';
      const restore = document.createElement('span');
      restore.className = 's-restore';
      restore.textContent = '恢复';
      restore.addEventListener('click', () => {
        act('sessrestore|' + s.id);
        setSessPanel(false);
      });
      const del = document.createElement('span');
      del.className = 's-del';
      del.textContent = '删除';
      del.addEventListener('click', () => {
        if (del.dataset.armed === '1') {
          act('sessdel|' + s.id);
          sessList = sessList.filter((x) => x.id !== s.id);
          renderSessions(sessList);
        } else {
          del.dataset.armed = '1';
          del.textContent = '确认';
          setTimeout(() => { del.dataset.armed = ''; del.textContent = '删除'; }, 3000);
        }
      });
      row.append(name, time, count, restore, del);
      box.appendChild(row);
    });
  }
  window.__shiftSessions = renderSessions;
  function setSessPanel(open) {
    document.documentElement.classList.toggle('sess-open', open);
    if (open) removePanelClasses('sess-open');
    act('sesspanel|' + (open ? '1' : '0'));
    if (open) renderSessions([]);
  }
  document.getElementById('sessclose')?.addEventListener('click', () => setSessPanel(false));

  // ---- 证书面板（F-CEF-21）----
  let certList = [];
  function renderCerts(list) {
    certList = Array.isArray(list) ? list : [];
    const box = document.getElementById('certlist');
    if (!box) return;
    const q = (document.getElementById('certsearch')?.value || '').trim().toLowerCase();
    const rows = certList.filter((c) => !q ||
      (c.host || '').toLowerCase().includes(q) ||
      (c.subject || '').toLowerCase().includes(q) ||
      (c.issuer || '').toLowerCase().includes(q));
    box.innerHTML = '';
    if (rows.length === 0) {
      const e = document.createElement('div');
      e.className = 'cert-empty';
      e.textContent = certList.length === 0 ? '还没有证书记录' : '没有匹配项';
      box.appendChild(e);
      return;
    }
    rows.forEach((c) => {
      const row = document.createElement('div');
      row.className = 'cert-row';
      const host = document.createElement('span');
      host.className = 'c-host';
      host.textContent = c.host + ':' + c.port;
      const subj = document.createElement('span');
      subj.className = 'c-subject';
      subj.textContent = c.subject || c.issuer || '';
      const days = document.createElement('span');
      days.className = 'c-days';
      const d = c.days_left;
      days.textContent = d == null ? '—' : (d + ' 天');
      if (d != null && d <= 7) days.classList.add('danger');
      else if (d != null && d <= 30) days.classList.add('warn');
      const del = document.createElement('span');
      del.className = 'c-op';
      del.textContent = '删除';
      del.dataset.host = c.host;
      del.dataset.port = String(c.port);
      del.addEventListener('click', () => {
        if (del.dataset.armed === '1') {
          act('certdel|' + c.host + ':' + c.port);
          certList = certList.filter((x) => !(x.host === c.host && x.port === c.port));
          renderCerts(certList);
        } else {
          del.dataset.armed = '1';
          del.textContent = '确认';
          setTimeout(() => { del.dataset.armed = ''; del.textContent = '删除'; }, 3000);
        }
      });
      row.append(host, subj, days, del);
      box.appendChild(row);
    });
  }
  window.__shiftCerts = renderCerts;
  function setCertPanel(open) {
    document.documentElement.classList.toggle('cert-open', open);
    if (open) removePanelClasses('cert-open');
    act('certpanel|' + (open ? '1' : '0'));
    if (open) { document.getElementById('certsearch').value = ''; renderCerts([]); }
  }
  document.getElementById('certclose')?.addEventListener('click', () => setCertPanel(false));
  document.getElementById('certsearch')?.addEventListener('input', () => renderCerts(certList));

  // ---- 用户脚本面板（F-CEF-23）----
  let usList = [];
  try { usList = JSON.parse(document.getElementById('us-data')?.textContent || '[]'); } catch (e) { usList = []; }
  function renderUs(list) {
    usList = Array.isArray(list) ? list : [];
    const box = document.getElementById('uslist');
    if (!box) return;
    const q = (document.getElementById('ussearch')?.value || '').trim().toLowerCase();
    const rows = usList.filter((u) => !q ||
      (u.name || '').toLowerCase().includes(q) ||
      (u.url_patterns || '').toLowerCase().includes(q));
    box.innerHTML = '';
    if (rows.length === 0) {
      const e = document.createElement('div');
      e.className = 'us-empty';
      e.textContent = usList.length === 0 ? '还没有用户脚本' : '没有匹配项';
      box.appendChild(e);
      return;
    }
    rows.forEach((u) => {
      const row = document.createElement('div');
      row.className = 'us-row';
      const name = document.createElement('span');
      name.className = 'us-name';
      name.textContent = u.name;
      name.title = u.name;
      const pat = document.createElement('span');
      pat.className = 'us-patterns';
      pat.textContent = (u.url_patterns || '').split('\n').filter(Boolean).join('  ');
      pat.title = u.url_patterns || '';
      const en = document.createElement('input');
      en.type = 'checkbox';
      en.className = 'us-enabled';
      en.checked = !!u.enabled;
      en.addEventListener('change', () => act('ustoggle|' + u.id));
      const del = document.createElement('span');
      del.className = 'us-del';
      del.textContent = '删除';
      del.addEventListener('click', () => {
        if (del.dataset.armed === '1') {
          del.remove();
          act('usdel|' + u.id);
        } else {
          del.dataset.armed = '1';
          del.textContent = '确认';
          setTimeout(() => { del.dataset.armed = ''; del.textContent = '删除'; }, 3000);
        }
      });
      row.append(name, pat, en, del);
      box.appendChild(row);
    });
  }
  window.__shiftUserscripts = renderUs;
  function setUsPanel(open) {
    document.documentElement.classList.toggle('us-open', open);
    if (open) removePanelClasses('us-open');
    act('uspanel|' + (open ? '1' : '0'));
    if (open) { document.getElementById('ussearch').value = ''; renderUs([]); }
  }
  document.getElementById('usclose')?.addEventListener('click', () => setUsPanel(false));
  document.getElementById('ussearch')?.addEventListener('input', () => renderUs(usList));


  // ---- 站点信息（F-CEF-19）----
  const lockBtn = document.getElementById('lockbtn');
  const sitePanel = document.getElementById('sitepanel');
  const siteHost = document.getElementById('sitehost');
  const siteState = document.getElementById('sitestate');
  const siteUrl = document.getElementById('siteurl');

  function setSitePanel(open) {
    document.documentElement.classList.toggle('site-open', open);
    if (open) removePanelClasses('site-open');
    act('sitepanel|' + (open ? '1' : '0'));
  }
  window.__shiftSiteInfo = function (data) {
    if (!data) return;
    siteHost.textContent = data.host || data.url || '本地页面';
    siteState.textContent = data.secure
      ? '连接安全（HTTPS）'
      : (data.scheme === 'http' ? '连接不安全（HTTP）' : '本地页面');
    siteState.classList.toggle('insecure', !!data.scheme && !data.secure);
    siteUrl.textContent = data.url || '';
  };
  lockBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    setSitePanel(true);
    act('siteinfo');
  });
  document.getElementById('siteclose').addEventListener('click', () => setSitePanel(false));
  document.addEventListener('click', (e) => {
    if (!e.target.closest('#sitepanel') && !e.target.closest('#lockbtn')) setSitePanel(false);
  });

  // ---- 主菜单（F-CEF-16）----
  // F-CEF-40：菜单改 GTK 原生弹出，由 Rust 管理开合；
  // 页面只发「弹一下」，不保留 menu-open 状态（否则隔次点击会被 toggle 成关闭）。
  function setMenuPanel(open) {
    document.documentElement.classList.remove('menu-open');
    if (open) act('menupanel|1');
  }
  document.getElementById('menubtn').addEventListener('click', (e) => {
    e.stopPropagation();
    act('menupanel|1');
  });
  document.getElementById('menupanel').addEventListener('click', (e) => {
    const item = e.target.closest('.menu-item');
    if (!item || item.classList.contains('disabled')) return;
    setMenuPanel(false);
    if (item.dataset.panel === 'bmmanage') { setBmManage(true); return; }
    if (item.dataset.panel === 'hist') { setHistPanel(true); return; }
    if (item.dataset.panel === 'groups') { setGroupsPanel(true); return; }
    if (item.dataset.panel === 'sess') { setSessPanel(true); return; }
    if (item.dataset.panel === 'cert') { setCertPanel(true); return; }
    if (item.dataset.panel === 'us') { setUsPanel(true); return; }
    if (item.dataset.panel === 'dl') { setDlPanel(true); return; }
    if (item.dataset.panel === 'pw') { setPwPanel(true); return; }
    if (item.dataset.panel === 'palette') { setPalettePanel(true); return; }
    act(item.dataset.act);
  });
  document.addEventListener('click', (e) => {
    if (!e.target.closest('#menupanel') && !e.target.closest('#menubtn')) setMenuPanel(false);
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') setMenuPanel(false);
  });

  // ---- 页内查找（F-CEF-18）----
  // 计数与当前条号来自 CEF FindHandler 真回调（Rust 侧推 window.__shiftFind），
  // 本页不自己数 DOM。动作走 console 桥：find|<词> / findnext / findprev / findclose。
  const findbar = document.getElementById('findbar');
  const findinput = document.getElementById('findinput');
  const findcount = document.getElementById('findcount');
  let findTimer = null;

  function renderFindCount(count, active) {
    // CEF 的 active_match_ordinal 是 0-based（第一个匹配 = 0），界面按 Chrome 显示 1-based
    findcount.textContent = count <= 0 ? '0/0' : Math.min(active + 1, count) + '/' + count;
  }
  window.__shiftFind = renderFindCount;

  window.__shiftFindOpen = function () {
    removePanelClasses('find-open');
    document.documentElement.classList.add('find-open');
    closeOmni();
    findinput.focus();
    findinput.select();
  };
  window.__shiftFindClose = function () {
    if (!document.documentElement.classList.contains('find-open')) return;
    document.documentElement.classList.remove('find-open');
    renderFindCount(0, 0);
  };

  // 输入防抖发起查找（find_next=0 = 新查找）；清空输入只重置计数，不发空查找
  findinput.addEventListener('input', () => {
    clearTimeout(findTimer);
    const q = findinput.value;
    if (!q.trim()) { renderFindCount(0, 0); return; }
    findTimer = setTimeout(() => act('find|' + q), 150);
  });
  findinput.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (!findinput.value) return;
      // Enter = 下一个，Shift+Enter = 上一个（继续查找，find_next=1）
      act(e.shiftKey ? 'findprev' : 'findnext');
      findinput.select();
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      act('findclose');
    }
  });
  document.getElementById('findnext').addEventListener('click', () => act('findnext'));
  document.getElementById('findprev').addEventListener('click', () => act('findprev'));
  document.getElementById('findclose').addEventListener('click', () => act('findclose'));
  findbar.addEventListener('mousedown', (e) => {
    // 点查找栏空白处不给焦点跳走（按钮/输入框除外）
    if (e.target === findbar) e.preventDefault();
  });

  // 防止工具栏页面拖动/选中
  document.addEventListener('dragstart', (e) => e.preventDefault());
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::super::shell::Panel;
    use super::{ensure_toolbar_html, panel_classes_json, TOOLBAR_HTML};
    use std::path::Path;

    #[test]
    fn panel_registry_has_twelve_unique_classes_and_matches_css() {
        assert_eq!(Panel::ALL.len(), 12);
        let json = panel_classes_json();
        for p in Panel::ALL {
            assert_eq!(json.matches(&format!("\"{}\"", p.class())).count(), 1, "{:?}", p);
            let css = format!("html.{}", p.class());
            assert!(TOOLBAR_HTML.contains(&css), "CSS 缺根类 {css}");
        }
    }

    #[test]
    fn every_js_setter_uses_registry_not_hand_lists() {
        // F-CEF-40：Menu 改 GTK 原生、Settings 改独立标签页，两者不再走 JS setter；
        // 其余 11 个面板仍必须走同一份 removePanelClasses 注册表。
        assert_eq!(TOOLBAR_HTML.matches("removePanelClasses('").count(), 11);
        for line in TOOLBAR_HTML.lines().filter(|l| l.contains("classList.remove(")) {
            assert!(line.matches("-open").count() <= 1, "残留手写互斥清单: {line}");
        }
    }

    #[test]
    fn injected_html_has_no_placeholder_and_lists_all_classes() {
        let dir = std::env::temp_dir().join(format!("cef-toolbar-pc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let url = ensure_toolbar_html(&dir, "[]", "[]", "[]", "[]").unwrap();
        let body = std::fs::read_to_string(url.strip_prefix("file://").unwrap()).unwrap();
        assert!(!body.contains("__SHIFT_PANEL_CLASSES__"));
        let want = format!("const PANEL_CLASSES = {};", panel_classes_json());
        assert!(body.contains(&want));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_toolbar_html_writes_file_and_content_bridge() {
        let dir = std::env::temp_dir().join(format!("cef-toolbar-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let out = ensure_toolbar_html(
            &dir,
            r#"[{"title":"测试书签","url":"https://a.com"}]"#,
            r#"[{"id":1,"domain":"a.com</script>","username":"u"}]"#,
            r#"[{"id":7,"filename":"a.zip","state":"completed","total":10,"received":10}]"#,
            "[]",
        )
        .unwrap();
        assert!(out.starts_with("file://"), "须返回 file:// URL，实得 {out}");
        assert!(out.ends_with("toolbar.html"));
        // URL 是 file:// 形态，落盘路径剥掉前缀再验
        let disk_path = Path::new(out.strip_prefix("file://").expect("URL 前缀"));
        assert!(disk_path.exists(), "工具栏 HTML 未落盘: {out}");
        let body = std::fs::read_to_string(disk_path).unwrap();
        assert!(
            body.contains("SHIFT_ACT"),
            "工具栏 HTML 必须带 console.log 动作桥"
        );
        assert!(body.contains("addr"), "工具栏 HTML 必须含地址栏");
        assert!(body.contains("pwpanel"), "工具栏 HTML 必须含密码面板");
        assert!(body.contains("dlpanel"), "工具栏 HTML 必须含下载面板");
        assert!(body.contains("menupanel"), "工具栏 HTML 必须含主菜单");
        assert!(body.contains("uspanel"), "工具栏 HTML 必须含用户脚本面板（F-CEF-23）");
        assert!(body.contains("findbar"), "工具栏 HTML 必须含页内查找栏（F-CEF-18）");
        assert!(body.contains("zoomval"), "工具栏 HTML 必须含缩放百分比显示（F-CEF-18）");
        assert!(
            body.contains(r#""filename":"a.zip""#),
            "下载记录 JSON 必须嵌进页面"
        );
        assert!(
            body.contains(r#""domain":"a.com<\/script>""#),
            "密码 JSON 里的 </ 必须转义，防提前闭合 script"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
