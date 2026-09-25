// 本文件由 src-tauri/build.rs 自动生成，请勿手改！
// 单一来源：src-tauri/src/constants.rs

export const LAYOUT = {
  tabBar: 38,
  addressBar: 36,
  bookmarksBar: 26,
  statusBar: 22,
  aiPanelWidth: 380,
  verticalTabBarWidth: 220,
  splitLeftRatio: 0.25,
  contentTopOffset: 100,
  contentTopOffsetNoBookmarks: 74,
} as const

export const SEARCH_ENGINES = [
  { id: 'bing', label: '必应', url: 'https://www.bing.com/search?q=' },
  { id: 'baidu', label: '百度', url: 'https://www.baidu.com/s?wd=' },
  { id: 'google', label: '谷歌', url: 'https://www.google.com/search?q=' },
] as const

export const SEARCH_URLS: Record<string, string> = Object.fromEntries(
  SEARCH_ENGINES.map((e) => [e.id, e.url]),
)

export const UI = {
  groupColors: ['#4a9eff', '#f06292', '#ffb74d', '#66bb6a', '#ab47bc', '#26c6da', '#ec407a', '#7e57c2', '#8d6e63', '#9ccc65'],
  spaceColors: ['#4a9eff', '#43a047', '#e53935', '#fb8c00', '#8e24aa', '#00acc1', '#f06292', '#5e35b1'],
  groupPalette: ['var(--accent)', '#e53935', '#43a047', '#fb8c00', '#8e24aa', '#00acc1', '#6d4c41', '#546e7a'],
  defaultFavicon: 'data:image/svg+xml;utf8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20viewBox%3D%220%200%2016%2016%22%3E%3Ccircle%20cx%3D%228%22%20cy%3D%228%22%20r%3D%226.5%22%20fill%3D%22var(--accent)%22%2F%3E%3Cellipse%20cx%3D%228%22%20cy%3D%228%22%20rx%3D%223%22%20ry%3D%226.5%22%20fill%3D%22none%22%20stroke%3D%22%23fff%22%20stroke-width%3D%220.8%22%2F%3E%3Cpath%20d%3D%22M1.5%208h13%22%20stroke%3D%22%23fff%22%20stroke-width%3D%220.8%22%2F%3E%3C%2Fsvg%3E',
} as const

export const SETTINGS_SCHEMA = {
  homepage: { default: 'newtab', kind: 'str', sensitive: false, desc: '启动页（\'newtab\' 或 URL）' },
  search_engine: { default: 'bing', kind: 'str', sensitive: false, desc: '搜索引擎（bing/baidu/google）' },
  theme: { default: 'dark', kind: 'str', sensitive: false, desc: '主题（dark/light）' },
  theme_color: { default: '', kind: 'str', sensitive: false, desc: '主题色（空=默认）' },
  wallpaper: { default: '', kind: 'str', sensitive: false, desc: '启动页壁纸（空=Bing 每日图）' },
  show_bookmarks_bar: { default: 'true', kind: 'bool', sensitive: false, desc: '显示书签栏' },
  vertical_tabs: { default: 'false', kind: 'bool', sensitive: false, desc: '垂直标签栏' },
  record_history: { default: 'false', kind: 'bool', sensitive: false, desc: '记录浏览历史（隐私优先默认关）' },
  font_size: { default: '16', kind: 'int', sensitive: false, desc: '启动页字号' },
  show_bg: { default: 'true', kind: 'bool', sensitive: false, desc: '启动页显示壁纸' },
  show_lunar: { default: 'true', kind: 'bool', sensitive: false, desc: '启动页显示农历' },
  show_quote: { default: 'true', kind: 'bool', sensitive: false, desc: '启动页显示每日一句' },
  show_weather: { default: 'true', kind: 'bool', sensitive: false, desc: '启动页显示天气' },
  weather_city: { default: '', kind: 'str', sensitive: false, desc: '天气城市（空=自动定位）' },
  lock_enabled: { default: 'true', kind: 'bool', sensitive: false, desc: '启动锁屏' },
  ai_model: { default: 'agnes-2.0-flash', kind: 'str', sensitive: false, desc: 'AI 模型' },
  ai_base_url: { default: 'https://apihub.agnes-ai.com/v1', kind: 'str', sensitive: false, desc: 'AI API 地址' },
  ai_api_key: { default: '', kind: 'str', sensitive: false, desc: 'AI API Key（留空用环境变量；掩码存储）' },
  adblock_enabled: { default: 'true', kind: 'bool', sensitive: false, desc: '广告拦截' },
  adblock_keywords: { default: '', kind: 'str', sensitive: false, desc: '自定义拦截关键词（换行分隔）' },
  privacy_shield: { default: 'true', kind: 'bool', sensitive: false, desc: '隐私防护（旧布尔开关，兼容）' },
  privacy_shield_level: { default: 'standard', kind: 'str', sensitive: false, desc: '隐私防护档位（off/standard/strict）' },
  privacy_whitelist: { default: '', kind: 'str', sensitive: false, desc: '隐私防护白名单域名' },
  gpc_header: { default: 'true', kind: 'bool', sensitive: false, desc: '发送 GPC 请求头（Sec-GPC: 1）' },
  net_block_rules: { default: '', kind: 'str', sensitive: false, desc: '网络层拦截规则（每行一个 URL 子串）' },
  net_headers: { default: '', kind: 'str', sensitive: false, desc: '自定义请求头（每行 Name: Value）' },
  tab_hibernate_minutes: { default: '30', kind: 'int', sensitive: false, desc: '空闲标签休眠超时（分钟，0=关）' },
  tab_hibernate_count: { default: '0', kind: 'int', sensitive: false, desc: '空闲标签休眠数量阈值（0=关）' },
  translate_engine: { default: 'google', kind: 'str', sensitive: false, desc: '翻译源（google/bing）' },
  translate_target: { default: 'zh-CN', kind: 'str', sensitive: false, desc: '翻译目标语言' },
  translate_timeout: { default: '15', kind: 'int', sensitive: false, desc: '翻译超时（秒，5-120）' },
  auto_clear_history_days: { default: '0', kind: 'int', sensitive: false, desc: '自动清理历史（天，0=关）' },
  auto_refresh_feeds_minutes: { default: '0', kind: 'int', sensitive: false, desc: '自动刷新资讯（分钟，0=关）' },
  auto_agent_minutes: { default: '0', kind: 'int', sensitive: false, desc: '定时 Agent（分钟，0=关）' },
  auto_agent_prompt: { default: '', kind: 'str', sensitive: false, desc: '定时 Agent 提示词' },
  api_allow_eval: { default: 'false', kind: 'bool', sensitive: false, desc: '允许执行 JS（危险，默认关）' },
  api_http_port: { default: '19701', kind: 'int', sensitive: false, desc: '开放 API HTTP 端口' },
  api_bind_lan: { default: 'false', kind: 'bool', sensitive: false, desc: 'API 绑局域网（0.0.0.0）' },
  api_model_gateway: { default: 'false', kind: 'bool', sensitive: false, desc: '模型网关' },
  api_readonly: { default: 'false', kind: 'bool', sensitive: false, desc: 'API 只读模式（禁写命令）' },
  api_agent_actions: { default: 'false', kind: 'bool', sensitive: false, desc: '允许 API 页面交互（click/fill/scroll/dismiss 等，默认关）' },
  api_allowed_ips: { default: '', kind: 'str', sensitive: false, desc: 'API IP 白名单（逗号分隔 IP/CIDR，如 192.168.1.0/24）' },
} as const

export const DEFAULT_SETTINGS: Record<string, string> = Object.fromEntries(
  Object.entries(SETTINGS_SCHEMA).map(([k, v]) => [k, v.default]),
)

export const EVENTS = {
  APP_UNLOCKED: 'app-unlocked',
  AUTO_AGENT: 'auto-agent',
  DOWNLOADS_UPDATED: 'downloads-updated',
  GLOBAL_SHORTCUT: 'global-shortcut',
  OPEN_AI_FROM_NEWTAB: 'open-ai-from-newtab',
  OPEN_SETTINGS_FROM_NEWTAB: 'open-settings-from-newtab',
  OPEN_URL_FROM_NEWTAB: 'open-url-from-newtab',
  TABS_UPDATED: 'tabs-updated',
  TRANSLATE_PROGRESS: 'translate-progress',
  TRAY_OPEN_DOWNLOADS: 'tray-open-downloads',
  TRAY_OPEN_PASSWORDS: 'tray-open-passwords',
  VAULT_AUTO_LOCKED: 'vault-auto-locked',
  AGENT_ACTION_REQUEST: 'agent-action-request',
  AGENT_MULTI_STEP_REQUEST: 'agent-multi-step-request',
  NET_STATUS_UPDATED: 'net-status-updated',
  PAGE_LOAD_FAILED: 'page-load-failed',
  PERMISSION_REQUEST: 'permission-request',
  TLS_ERROR: 'tls-error',
  WEB_PROCESS_CRASHED: 'web-process-crashed',
} as const

export const URLS = {
  NEWTAB: 'shift://newtab',
  WORLD_NAV: 'shift://worldnav',
  NEWTAB_MARKER: '/newtab/',
  READER_MARKER: '/reader/',
  WORLD_NAV_FILE: 'world-nav.html',
} as const

export const TIMEOUT = {
  WEBVIEW_IPC_SECS: 6,
  WEBVIEW_READY_SECS: 2,
  MCP_REQUEST_SECS: 30,
  API_COMMAND_SECS: 120,
  AGENT_STEP_SECS: 120,
  AI_TOOL_ROUND_SECS: 60,
  FEED_FETCH_SECS: 15,
  FEED_LOOP_SECS: 1800,
  SYNC_REQUEST_SECS: 20,
  TRANSLATE_SECS: 120,
  DB_BUSY_SECS: 10,
  SINGLE_INSTANCE_WAIT_MS: 300,
  DEFAULT_BROWSER_WAIT_MS: 2000,
  TAB_POLL_MS: 8000,
} as const

// 组合键字符串，形如 'ctrl+shift+t'（与前端 matchShortcut 约定一致）
export const SHORTCUTS = {
  DEVTOOLS: 'f12',
  DEVTOOLS_ALT: 'ctrl+shift+i',
  NEXT_TAB: 'ctrl+tab',
  PREV_TAB: 'ctrl+shift+tab',
  NEW_TAB: 'ctrl+t',
  TRANSLATE_PANEL: 'ctrl+shift+t',
  CLOSE_TAB: 'ctrl+w',
  RELOAD: 'ctrl+r',
  FOCUS_ADDRESS: 'ctrl+l',
  HISTORY: 'ctrl+h',
  COMMAND_PALETTE: 'ctrl+k',
  SESSIONS: 'ctrl+shift+s',
  LOCK_VAULT: 'ctrl+shift+l',
} as const

export const SKILLS = [
  { id: 'summarize_page', name: '总结当前页', icon: '📄', desc: '提炼页面要点，长文 30 秒看懂' },
  { id: 'translate_page', name: '翻译当前页', icon: '🌐', desc: '整页翻译为中文' },
  { id: 'explain_page', name: '解释当前页', icon: '💡', desc: '用大白话讲解专业内容' },
  { id: 'page_qa', name: '页面问答', icon: '💬', desc: '基于当前页内容提问' },
] as const
