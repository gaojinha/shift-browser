// UiDialog + AppToast 组件测试（F-88 · 前端测试补课）
//
// 手法：@vue/test-utils 真实挂载（照 UiSwitch.test.ts 范式）。
// UiDialog 走 reka-ui DialogPortal——DOM 挂在 document.body，不在 wrapper 里，
// 断言一律查 document，且 mount 后要等一拍 nextTick 才能查到。
// AppToast 读真实 toast store（pinia），每例新建 pinia 防跨例残留；自动消失用 fake timers。
//
// 跑法：npx vitest run src/components/ui/UiDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import UiDialog from './UiDialog.vue'
import AppToast from '../AppToast.vue'
import { useToastStore } from '../../stores/toast'

afterEach(() => {
  document.body.innerHTML = ''
})

async function mountDialog(open = true, extra: Record<string, unknown> = {}) {
  const wrapper = mount(UiDialog, {
    props: {
      title: '测试弹窗',
      open,
      'onUpdate:open': (v: boolean) => wrapper.setProps({ open: v }),
      ...extra,
    },
    slots: { default: '<p class="body-content">正文</p>' },
    attachTo: document.body,
  })
  await wrapper.vm.$nextTick()
  await wrapper.vm.$nextTick()
  return wrapper
}

describe('UiDialog', () => {
  it('open=true 渲染 portal：标题、关闭按钮、默认插槽', async () => {
    await mountDialog(true)
    expect(document.querySelector('.ui-dialog-title')?.textContent).toBe('测试弹窗')
    expect(document.querySelector('.close-btn')).toBeTruthy()
    expect(document.querySelector('.body-content')?.textContent).toBe('正文')
  })

  it('open=false 不渲染任何 portal 内容', async () => {
    await mountDialog(false)
    expect(document.querySelector('.ui-dialog')).toBeNull()
    expect(document.querySelector('.ui-dialog-overlay')).toBeNull()
  })

  it('点关闭按钮发出 update:open=false（写回后 DOM 撤下）', async () => {
    const wrapper = await mountDialog(true)
    const btn = document.querySelector<HTMLButtonElement>('.close-btn')!
    btn.click()
    await wrapper.vm.$nextTick()
    expect(wrapper.emitted('update:open')).toEqual([[false]])
    await wrapper.setProps({ open: false })
    await wrapper.vm.$nextTick()
    expect(document.querySelector('.ui-dialog')).toBeNull()
  })

  it('Esc 关闭（reka DialogRoot 行为）', async () => {
    const wrapper = await mountDialog(true)
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await wrapper.vm.$nextTick()
    expect(wrapper.emitted('update:open')).toEqual([[false]])
  })

  it('description 有值才渲染说明行', async () => {
    const w = await mountDialog(true, { description: '这是说明' })
    expect(document.querySelector('.ui-dialog-desc')?.textContent).toBe('这是说明')
    await w.setProps({ description: '' })
    await w.vm.$nextTick()
    expect(document.querySelector('.ui-dialog-desc')).toBeNull()
  })

  it('actions / footer 插槽有内容才渲染对应区块', async () => {
    await mount(UiDialog, {
      props: { title: '带脚注', open: true },
      slots: {
        actions: '<button class="act">动作</button>',
        footer: '<button class="foot">确认</button>',
        default: '<p>x</p>',
      },
      attachTo: document.body,
    })
    await new Promise((r) => setTimeout(r))
    expect(document.querySelector('.ui-dialog-actions')).toBeTruthy()
    expect(document.querySelector('.ui-dialog-footer')).toBeTruthy()

    // 无 footer/actions 的弹窗不渲染空区块
    await mountDialog(true)
    const dialogs = document.querySelectorAll('.ui-dialog')
    const last = dialogs[dialogs.length - 1] as HTMLElement
    expect(last.querySelector('.ui-dialog-footer')).toBeNull()
    expect(last.querySelector('.ui-dialog-actions')).toBeNull()
  })

  it('width / layer 透传为内联样式', async () => {
    await mountDialog(true, { width: 'min(560px, 90vw)', layer: '9999' })
    const panel = document.querySelector<HTMLElement>('.ui-dialog')!
    expect(panel.style.width).toBe('min(560px, 90vw)')
    expect(panel.style.zIndex).toBe('9999')
  })

  it('autoFocus=false 阻止打开时抢焦（onOpenAutoFocus preventDefault）', async () => {
    await mountDialog(true, { autoFocus: false })
    expect(document.querySelector('.ui-dialog')).toBeTruthy()
  })
})

describe('AppToast', () => {
  function mountToast() {
    return mount(AppToast, { attachTo: document.body })
  }

  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('空 store 不渲染任何 toast 条目', () => {
    const w = mountToast()
    expect(w.find('.toast-item').exists()).toBe(false)
  })

  it('show 默认 info：图标 ℹ️、消息渲染', () => {
    const store = useToastStore()
    store.show('提示一条')
    const w = mountToast()
    expect(w.find('.toast-item.info').exists()).toBe(true)
    expect(w.find('.toast-msg').text()).toBe('提示一条')
    expect(w.find('.toast-icon').text()).toBe('ℹ️')
  })

  it('success / error 类型分色：✅ / ⚠️', () => {
    const store = useToastStore()
    store.show('成功', 'success')
    store.error('失败')
    const w = mountToast()
    expect(w.find('.toast-item.success .toast-icon').text()).toBe('✅')
    expect(w.find('.toast-item.error .toast-icon').text()).toBe('⚠️')
  })

  it('aria-live：有 error 时 assertive，否则 polite', async () => {
    const store = useToastStore()
    const w = mountToast()
    store.show('普通', 'info')
    await w.vm.$nextTick()
    expect(w.find('[role="status"]').attributes('aria-live')).toBe('polite')
    store.error('出错')
    await w.vm.$nextTick()
    expect(w.find('[role="status"]').attributes('aria-live')).toBe('assertive')
  })

  it('同文同型重复不堆积（连点不刷屏）', () => {
    const store = useToastStore()
    store.error('重复')
    store.error('重复')
    const w = mountToast()
    expect(w.findAll('.toast-item')).toHaveLength(1)
  })

  it('上限 3 条：超了挤掉最早的', () => {
    const store = useToastStore()
    store.show('一')
    store.show('二')
    store.show('三')
    store.show('四', 'success')
    const w = mountToast()
    const msgs = w.findAll('.toast-msg').map((n) => n.text())
    expect(msgs).toEqual(['二', '三', '四'])
  })

  it('自动消失：info 3s / error 4s / success 2.5s（fake timers）', async () => {
    vi.useFakeTimers()
    try {
      const store = useToastStore()
      const w = mountToast()
      store.show('普通', 'info')
      store.error('错误')
      store.success('成功')
      await w.vm.$nextTick()
      expect(w.findAll('.toast-item')).toHaveLength(3)
      vi.advanceTimersByTime(2500)
      await w.vm.$nextTick()
      expect(w.findAll('.toast-msg').map((n) => n.text())).toEqual(['普通', '错误'])
      vi.advanceTimersByTime(500)
      await w.vm.$nextTick()
      expect(w.findAll('.toast-msg').map((n) => n.text())).toEqual(['错误'])
      vi.advanceTimersByTime(1000)
      await w.vm.$nextTick()
      expect(w.findAll('.toast-item')).toHaveLength(0)
    } finally {
      vi.useRealTimers()
    }
  })
})
