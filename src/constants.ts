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
