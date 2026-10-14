// 快捷键弹窗守卫单测：锁定与 onGlobalKeydown 的对应关系。
// keydown 侧在弹窗打开时拦截的动作必须全在表里，其余一律不在——
// 任一增删都要两边同步改，否则双焦点行为分叉（焦点统一实施）。
import { describe, it, expect } from 'vitest'
import { DIALOG_GUARDED_ACTIONS, isDialogGuardedAction } from './shortcut-guard'

describe('shortcut-guard', () => {
  it('弹窗打开时拦截的八个动作全在表里', () => {
    for (const a of [
      'translate-panel',
      'sessions',
      'new-tab',
      'close-tab',
      'reload',
      'history',
      'downloads',
      'bookmarks',
    ]) {
      expect(isDialogGuardedAction(a)).toBe(true)
    }
    expect(DIALOG_GUARDED_ACTIONS.size).toBe(8)
  })

  it('两边都不拦的动作不在表里（devtools/寻址/命令栏/锁库/切签）', () => {
    for (const a of [
      'devtools',
      'focus-address',
      'command-palette',
      'lock-vault',
      'next-tab',
      'prev-tab',
    ]) {
      expect(isDialogGuardedAction(a)).toBe(false)
    }
  })
})
