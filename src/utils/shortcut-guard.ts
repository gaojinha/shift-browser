// 快捷键弹窗守卫（与 App.vue onGlobalKeydown 同口径，单一来源）
// 背景：前端 keydown 在弹窗打开时拦截建签/关签等动作，但 GTK 层（页面持焦时）
// 转发直接调 handleGlobalAction，不过守卫——AI 开着时点一下页面再按 Ctrl+T，
// 两边行为分叉。两条路径共用本表，保证双焦点行为一致。
// 非表中动作（devtools/focus-address/command-palette/lock-vault/next-tab/prev-tab/tab-search）
// 两边都不拦，改这里必须同步改 onGlobalKeydown（单测锁死对应关系）。
export const DIALOG_GUARDED_ACTIONS: ReadonlySet<string> = new Set([
  'translate-panel',
  'sessions',
  'new-tab',
  'close-tab',
  'reload',
  'history',
  'downloads',
  'bookmarks',
])

/** 该动作是否受弹窗守卫约束（弹窗打开时拒绝执行） */
export function isDialogGuardedAction(action: string): boolean {
  return DIALOG_GUARDED_ACTIONS.has(action)
}
