// 设置弹窗 IA 注册表（2026-09 重组）
// 纯数据 + 纯函数，供 SettingsDialog.vue 与 settings-navigation.test.ts 共用：
// - SECTION_GROUPS：8 栏 4 分组侧栏导航
// - LEGACY_SECTION_ALIASES：旧 10 栏 id → 新栏（深链全兼容）
// - SECTION_ANCHORS：每栏合法的卡片 data-anchor（防搜索注册表漂移）
// - SETTING_SEARCH_INDEX：设置搜索静态注册表（label + 别名 + 目标栏/页签/锚点）

export type SectionId =
  | 'general'
  | 'appearance'
  | 'privacy'
  | 'data'
  | 'ai'
  | 'extensions'
  | 'advanced'
  | 'about'

export type AiTabId = 'model' | 'mcp' | 'labs'

export interface NavItem {
  id: SectionId
  icon: string
  label: string
}
export interface NavGroup {
  label: string
  items: NavItem[]
}

export const SECTION_GROUPS: NavGroup[] = [
  {
    label: '基础',
    items: [
      { id: 'general', icon: 'house', label: '常规' },
      { id: 'appearance', icon: 'palette', label: '外观' },
    ],
  },
  {
    label: '安全与主权',
    items: [
      { id: 'privacy', icon: 'shield', label: '隐私与安全' },
      { id: 'data', icon: 'lock', label: '数据与同步' },
    ],
  },
  {
    label: '智能与平台',
    items: [
      { id: 'ai', icon: 'sparkles', label: 'AI 助手' },
      { id: 'extensions', icon: 'puzzle', label: '扩展与自动化' },
    ],
  },
  {
    label: '系统',
    items: [
      { id: 'advanced', icon: 'gear', label: '高级' },
      { id: 'about', icon: 'info', label: '关于' },
    ],
  },
]

export const SECTION_IDS: SectionId[] = SECTION_GROUPS.flatMap((g) => g.items.map((i) => i.id))

/** 各栏卡片锚点（须与各 SFC 模板里的 data-anchor 一致；AI 栏锚点=子页签 id） */
export const SECTION_ANCHORS: Record<SectionId, string[]> = {
  general: [],
  appearance: ['newtab'],
  privacy: ['shield', 'tracking', 'identity', 'history-cleanup'],
  data: [],
  ai: ['model', 'mcp', 'labs'],
  extensions: ['extensions', 'userscripts', 'automation'],
  advanced: ['api', 'compat', 'selfcheck'],
  about: [],
}

/** 旧栏目标识 → 新目标（mcp 落 AI 栏 MCP 页签；api/selfcheck 落高级栏对应锚点） */
export interface LegacyTarget {
  section: SectionId
  aiTab?: AiTabId
  anchor?: string
}
export const LEGACY_SECTION_ALIASES: Record<string, LegacyTarget> = {
  general: { section: 'general' },
  appearance: { section: 'appearance' },
  privacy: { section: 'privacy' },
  data: { section: 'data' },
  ai: { section: 'ai', aiTab: 'model' },
  mcp: { section: 'ai', aiTab: 'mcp' },
  extensions: { section: 'extensions' },
  api: { section: 'advanced', anchor: 'api' },
  selfcheck: { section: 'advanced', anchor: 'selfcheck' },
  advanced: { section: 'advanced' },
  about: { section: 'about' },
}

export interface SearchEntry {
  /** 结果主显示名 */
  label: string
  /** 命中关键词（含 label 本身与别名，统一小写匹配） */
  keywords: string[]
  section: SectionId
  /** AI 栏子页签 */
  aiTab?: AiTabId
  /** 栏内卡片锚点（data-anchor；AI 栏同页签 id） */
  anchor?: string
}

/**
 * 设置搜索注册表：与 SECTION_GROUPS/SFC data-anchor 同文件维护。
 * 防漂移由 settings-navigation.test.ts 校验（section 合法、anchor 存在于 SFC 源码）。
 */
export const SETTING_SEARCH_INDEX: SearchEntry[] = [
  // —— 基础 · 常规 ——
  { label: '默认浏览器', keywords: ['默认浏览器', 'default browser', '设为默认'], section: 'general' },
  { label: '下载保存目录', keywords: ['下载目录', '下载路径', 'download dir', 'download path', 'download_dir'], section: 'general' },
  { label: '主页 URL', keywords: ['主页', '首页', 'homepage'], section: 'general' },
  { label: '搜索引擎', keywords: ['搜索引擎', '搜索', 'search', 'bing', 'google'], section: 'general' },
  { label: '标签休眠时长', keywords: ['标签休眠', '休眠时长', '闲置', 'hibernate', '内存'], section: 'general' },
  { label: '休眠标签数阈值', keywords: ['休眠数量', '标签数量', '休眠阈值', 'tab_hibernate_count'], section: 'general' },
  { label: '垂直标签栏', keywords: ['垂直标签', '竖排标签', 'vertical'], section: 'general' },
  { label: '翻译', keywords: ['翻译', '划词翻译', '整页翻译', 'translate', 'google', 'bing'], section: 'general' },
  // —— 基础 · 外观 ——
  { label: '深色/浅色主题', keywords: ['主题', '深色', '浅色', 'dark', 'light', 'theme'], section: 'appearance' },
  { label: '书签栏', keywords: ['书签栏', '收藏栏', 'bookmarks bar'], section: 'appearance' },
  { label: '主题色', keywords: ['主题色', '强调色', 'theme_color', '取色'], section: 'appearance', anchor: 'newtab' },
  { label: '自定义壁纸', keywords: ['壁纸', 'wallpaper', '背景图', '图片链接'], section: 'appearance', anchor: 'newtab' },
  { label: '动态背景', keywords: ['动态背景', 'bing 壁纸', '每日壁纸', 'show_bg'], section: 'appearance', anchor: 'newtab' },
  { label: '字体大小', keywords: ['字体', '字号', 'font', 'font_size'], section: 'appearance', anchor: 'newtab' },
  { label: '农历', keywords: ['农历', '阴历', 'lunar'], section: 'appearance', anchor: 'newtab' },
  { label: '每日一句', keywords: ['每日一句', '金句', '英文句子', 'quote'], section: 'appearance', anchor: 'newtab' },
  { label: '天气', keywords: ['天气', 'weather', '天气预报'], section: 'appearance', anchor: 'newtab' },
  { label: '天气城市', keywords: ['天气城市', '城市', '定位', 'weather_city'], section: 'appearance', anchor: 'newtab' },
  // —— 安全与主权 · 隐私与安全 ——
  { label: '隐私防护等级', keywords: ['隐私防护', '防护等级', '反指纹', '指纹', 'fingerprint', '标准', '严格'], section: 'privacy', anchor: 'shield' },
  { label: 'GPC 全局隐私控制', keywords: ['gpc', 'sec-gpc', '全局隐私控制', '不要出售'], section: 'privacy', anchor: 'tracking' },
  { label: '广告/追踪拦截', keywords: ['广告拦截', '广告屏蔽', '拦截', '追踪', '关键词拦截', 'adblock'], section: 'privacy', anchor: 'tracking' },
  { label: '网络拦截规则', keywords: ['网络规则', '拦截规则', '自定义请求头', 'net rules', 'hosts'], section: 'privacy', anchor: 'tracking' },
  { label: '外联连接审计', keywords: ['外联', '连接审计', '端口', '进程连接', 'tcp'], section: 'privacy', anchor: 'tracking' },
  { label: 'User-Agent 管理', keywords: ['user-agent', 'user agent', 'ua', '身份伪装', '浏览器标识'], section: 'privacy', anchor: 'identity' },
  { label: '证书中心', keywords: ['证书', 'certificate', '证书例外', 'tls', 'https 证书'], section: 'privacy', anchor: 'identity' },
  { label: '记录浏览历史', keywords: ['记录历史', '浏览历史', '历史记录开关', 'record_history'], section: 'privacy', anchor: 'history-cleanup' },
  { label: '清除浏览数据', keywords: ['清除', '清理', '清除历史', 'cookie', '缓存', 'cache', '清除浏览数据'], section: 'privacy', anchor: 'history-cleanup' },
  // —— 安全与主权 · 数据与同步 ——
  { label: '密码库主密码', keywords: ['主密码', '密码库', 'vault', '密码锁'], section: 'data' },
  { label: 'WebDAV 端到端同步', keywords: ['webdav', '同步', 'e2e', '端到端', 'sync', '备份'], section: 'data' },
  { label: '启动锁屏', keywords: ['锁屏', '自动锁定', 'lock', '空闲锁定'], section: 'data' },
  // —— 智能与平台 · AI 助手 ——
  { label: '推理服务 Provider', keywords: ['provider', '推理服务', 'ai 服务', 'agnes', 'ai_provider'], section: 'ai', aiTab: 'model', anchor: 'model' },
  { label: 'AI 模型', keywords: ['模型', 'model', 'flash', 'pro', 'ai_model'], section: 'ai', aiTab: 'model', anchor: 'model' },
  { label: 'AI API 地址/Key', keywords: ['api key', 'api 地址', 'base url', '密钥', 'agnes_api_key'], section: 'ai', aiTab: 'model', anchor: 'model' },
  { label: 'MCP 工具', keywords: ['mcp', 'mcp server', '工具', 'function call', 'stdio', '模型上下文'], section: 'ai', aiTab: 'mcp', anchor: 'mcp' },
  { label: 'WebMCP polyfill', keywords: ['webmcp', 'polyfill', '试验', '实验特性', 'navigator.modelcontext'], section: 'ai', aiTab: 'labs', anchor: 'labs' },
  // —— 智能与平台 · 扩展与自动化 ——
  { label: '扩展加载', keywords: ['扩展', 'extension', '加载扩展', 'manifest', 'js 注入', 'css 注入'], section: 'extensions', anchor: 'extensions' },
  { label: '用户脚本', keywords: ['用户脚本', '油猴', 'tampermonkey', 'userscript', '脚本管理'], section: 'extensions', anchor: 'userscripts' },
  { label: '定时任务', keywords: ['定时任务', '自动清理', '定时刷新订阅', '定时 agent', 'auto', '自动化'], section: 'extensions', anchor: 'automation' },
  // —— 系统 · 高级 ——
  { label: '开放 API', keywords: ['开放 api', 'api', 'eval', '端口', '局域网', 'ip 白名单', '模型网关', 'token', '状态面板'], section: 'advanced', anchor: 'api' },
  { label: 'API 只读模式', keywords: ['只读', 'readonly', 'api_readonly', '禁写'], section: 'advanced', anchor: 'api' },
  { label: 'API 页面交互闸门', keywords: ['页面交互', 'click', 'fill', 'scroll', 'dismiss', 'agent action', 'api_agent_actions'], section: 'advanced', anchor: 'api' },
  { label: 'DMA-BUF 渲染兼容', keywords: ['dmabuf', 'dma-buf', '兼容', '渲染', '视频闪烁'], section: 'advanced', anchor: 'compat' },
  { label: '系统自检', keywords: ['自检', '体检', 'selfcheck', '健康检查', '诊断'], section: 'advanced', anchor: 'selfcheck' },
  // —— 系统 · 关于 ——
  { label: '关于 Shift Browser', keywords: ['关于', '版本', '仓库', '数据目录', 'about', '隐私承诺'], section: 'about' },
]

export interface ResolvedTarget {
  section: SectionId
  aiTab: AiTabId
  anchor: string
}

/** 解析 initialSection（支持旧 id 与 section#anchor / ai#mcp 形态），非法值兜底常规栏 */
export function resolveTarget(raw?: string): ResolvedTarget {
  if (!raw) return { section: 'general', aiTab: 'model', anchor: '' }
  const [idRaw, hash = ''] = raw.split('#')
  const legacy = LEGACY_SECTION_ALIASES[idRaw]
  const section: SectionId = legacy
    ? legacy.section
    : SECTION_IDS.includes(idRaw as SectionId)
      ? (idRaw as SectionId)
      : 'general'
  // hash 指定 AI 页签时优先；否则用旧映射自带页签
  const aiTab: AiTabId = hash === 'mcp' || hash === 'labs' ? hash : (legacy?.aiTab ?? 'model')
  // hash 是本栏合法锚点则用之；无 hash 时用旧映射自带锚点（api→api、selfcheck→selfcheck）
  const anchor = SECTION_ANCHORS[section].includes(hash)
    ? hash
    : hash
      ? ''
      : (legacy?.anchor ?? '')
  return { section, aiTab, anchor }
}

/** 搜索：按关键词不区分大小写子串匹配，空查询返回空数组 */
export function searchSettings(query: string): SearchEntry[] {
  const q = query.trim().toLowerCase()
  if (!q) return []
  return SETTING_SEARCH_INDEX.filter((e) => e.keywords.some((k) => k.toLowerCase().includes(q))).slice(0, 12)
}
