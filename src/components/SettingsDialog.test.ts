// SettingsDialog 焦点归位验证（K-17 · 016 任务 / 021 升级）
//
// 背景：Reka UiDialog 在 present=true→false 时调 triggerElement?.focus()，
// 但 App.vue 中触发按钮在 StatusBar（与 DialogRoot 不同树），triggerElement
// 始终为 null。021 在 SettingsDialog.vue 手动补焦点链：
// 打开时捕获 document.activeElement，关闭时 nextTick 恢复。
//
// 本测试验证：打开前聚焦触发按钮 → 打开 → 关闭 → 焦点真归位到触发按钮。
// 关闭路径覆盖：prop toggle / × 按钮点击。
//
// 跑法：npx vitest run src/components/SettingsDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { nextTick, ref, defineComponent, h } from 'vue'
import { i18n } from '../i18n'
import SettingsDialog from './SettingsDialog.vue'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: vi.fn(() => Promise.resolve()),
}))

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'default_browser_status') {
      return Promise.resolve({ is_default: false, current: 'firefox.desktop' })
    }
    if (cmd === 'app_about_cmd') {
      return Promise.resolve({ appName: 'ShiftBrowser', version: '5.8.3' })
    }
    if (cmd === 'selfcheck_cmd') {
      return Promise.resolve({ healthy: true, timestamp: 0, versions: [], storage: [], runtime: [], capabilities: [] })
    }
    return Promise.resolve(undefined)
  })
})

afterEach(() => {
  document.body.innerHTML = ''
})

/**
 * 构造测试树：
 * - 原生 <button>（直接 DOM，不经过 Vue render）
 * - 父组件管理 show 状态，close 事件正确更新 show
 */
async function mountWithTrigger() {
  const show = ref(false)

  // 原生按钮（模拟 StatusBar 设置按钮），先于 mount 放入 DOM
  const btn = document.createElement('button')
  btn.id = 'settings-trigger'
  btn.setAttribute('aria-label', '设置')
  btn.textContent = '⚙️'
  document.body.appendChild(btn)

  const Parent = defineComponent({
    setup() {
      return () =>
        h(SettingsDialog, {
          show: show.value,
          'onUpdate:show': (v: boolean) => { show.value = v },
          onClose: () => { show.value = false },
        })
    },
  })

  const wrapper = mount(Parent, {
    global: { plugins: [pinia, i18n], attachTo: document.body },
  })
  await nextTick()

  return { wrapper, btn, show }
}

/** 等待焦点归位稳定（Reka FocusScope setTimeout(fn,0) + 我们的 nextTick） */
async function waitForFocus() {
  await flushPromises()
  await nextTick()
  await nextTick()
  await new Promise((r) => setTimeout(r, 0))
  await nextTick()
}

describe('SettingsDialog：焦点归位验证（K-17 · 021 升级）', () => {
  it('prop 关闭 → 焦点真归位到触发按钮', async () => {
    const { wrapper, btn, show } = await mountWithTrigger()
    btn.focus()
    expect(document.activeElement).toBe(btn)

    // 打开
    show.value = true
    await flushPromises()
    await nextTick()
    await nextTick()

    // 打开后 dialog 内应有内容，焦点在 dialog 内
    const dialogEl = document.querySelector('.ui-dialog') as HTMLElement | null
    expect(dialogEl).toBeTruthy()
    expect(document.activeElement).not.toBe(btn)

    // 关闭（模拟 Esc / prop toggle）
    show.value = false
    await waitForFocus()

    expect(document.querySelector('.ui-dialog')).toBeFalsy()
    expect(document.activeElement).toBe(btn)
    wrapper.unmount()
  })

  it('点 × 关闭按钮 → 焦点归位到触发按钮', async () => {
    const { wrapper, btn, show } = await mountWithTrigger()
    btn.focus()

    // 打开（模拟用户点设置按钮：show 变 true）
    show.value = true
    await flushPromises()
    await nextTick()
    await nextTick()

    const dialogEl = document.querySelector('.ui-dialog') as HTMLElement | null
    expect(dialogEl).toBeTruthy()

    // 点 × 按钮（Reka DialogClose → v-model:open → onClose → show=false）
    const closeBtn = document.querySelector('button[aria-label="关闭"]') as HTMLElement | null
    expect(closeBtn).toBeTruthy()
    closeBtn?.click()
    await waitForFocus()

    expect(document.querySelector('.ui-dialog')).toBeFalsy()
    expect(document.activeElement).toBe(btn)
    wrapper.unmount()
  })
})
