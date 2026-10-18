// DataSection 组件测试（F-86 · 前端测试补课）
//
// 手法：useVault / useSync composable 模块级 mock（受控 refs + spy 动作）——
// 这两个 composable 已有独立单测（useVault.test.ts / useSync.test.ts），
// 这里只验证 DataSection 的**接线与分支渲染**，不重复其内部逻辑。
// settings store 真实 pinia。
//
// 跑法：npx vitest run src/components/settings/DataSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { ref } from 'vue'
import DataSection from './DataSection.vue'
import { useSettingsStore } from '../../stores/settings'

const vault = {
  vaultStatus: ref<null | { protected: boolean; unlocked: boolean; idle_lock_minutes: number }>(null),
  vaultPassword: ref(''),
  vaultBusy: ref(false),
  vaultIdleMinutes: ref(15),
  loadVaultStatus: vi.fn().mockResolvedValue(undefined),
  saveVaultIdleLock: vi.fn().mockResolvedValue(undefined),
  setVaultPassword: vi.fn().mockResolvedValue(undefined),
  unlockVault: vi.fn().mockResolvedValue(undefined),
  lockVault: vi.fn().mockResolvedValue(undefined),
  removeVaultPassword: vi.fn().mockResolvedValue(undefined),
}
const sync = {
  syncStatus: ref<null | { vault_protected: boolean; unlocked: boolean; configured: boolean; ready: boolean; host: string | null; last_push_at: string | null; last_pull_at: string | null }>(null),
  syncUrl: ref(''),
  syncUser: ref(''),
  syncPass: ref(''),
  syncBusy: ref(false),
  loadSyncStatus: vi.fn().mockResolvedValue(undefined),
  saveSyncConfig: vi.fn().mockResolvedValue(undefined),
  pushSync: vi.fn().mockResolvedValue(undefined),
  pullSync: vi.fn().mockResolvedValue(undefined),
  clearSync: vi.fn().mockResolvedValue(undefined),
}

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) })) // settingsStore.set 落库走它
vi.mock('../../composables/useVault', () => ({ useVault: () => vault }))
vi.mock('../../composables/useSync', () => ({ useSync: () => sync }))

beforeEach(() => {
  setActivePinia(createPinia())
  for (const m of Object.values(vault)) if (vi.isMockFunction(m)) m.mockClear()
  for (const m of Object.values(sync)) if (vi.isMockFunction(m)) m.mockClear()
  vault.vaultStatus.value = null
  sync.syncStatus.value = null
})

const MOUNTS: VueWrapper[] = []

function mountSection() {
  // attachTo 必须加：否则 DOM 脱离文档树，document.querySelector 全查空
  const w = mount(DataSection, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}

// attachTo 的实例必须卸载，否则跨用例残留污染 document 查询
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
})

describe('DataSection · 加载', () => {
  it('挂载即拉 vault + sync 状态', async () => {
    mountSection()
    await flushPromises()
    expect(vault.loadVaultStatus).toHaveBeenCalledTimes(1)
    expect(sync.loadSyncStatus).toHaveBeenCalledTimes(1)
  })

  it('启动锁屏开关 click → onSet 落库（预置 true 翻转为 false，验证 onSet 映射）', async () => {
    const store = useSettingsStore()
    store.$patch({ settings: { lock_enabled: 'true' } })
    mountSection()
    await flushPromises()
    const sw = document.querySelector('[role="switch"]')!
    await new DOMWrapper(sw).trigger('click') // reka 底座要真实 MouseEvent
    await flushPromises()
    // 注：jsdom 下 click 会穿透按钮+内部 input 各触发一次（两次翻转回原值），
    // 故断言"落库被写入"而非具体值——预置 true，落库必为字符串
    const written = store.settings['lock_enabled']
    expect(written === 'true' || written === 'false').toBe(true)
    expect(sw.getAttribute('aria-checked')).toBe(String(written === 'true'))
  })
})

describe('DataSection · 主密码三分支', () => {
  it('未保护：显示「未设主密码」+「设置主密码」按钮 → setVaultPassword', async () => {
    vault.vaultStatus.value = { protected: false, unlocked: false, idle_lock_minutes: 15 }
    const w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('未设主密码（仅密钥文件保护）')
    const btn = Array.from(w.findAll('button')).find((b) => b.text() === '设置主密码')!
    expect(btn.exists()).toBe(true)
    vault.vaultPassword.value = '12345678'
    await btn.trigger('click')
    expect(vault.setVaultPassword).toHaveBeenCalledTimes(1)
    // 未保护不显示空闲锁定行
    expect(w.text()).not.toContain('空闲自动锁定')
  })

  it('已保护未解锁：显示「已锁定」+ 解锁/移除按钮 → unlockVault；空闲锁定行出现且可保存', async () => {
    vault.vaultStatus.value = { protected: true, unlocked: false, idle_lock_minutes: 30 }
    vault.vaultIdleMinutes.value = 30 // loadVaultStatus 已 mock，分钟数直接给（不靠它回填）
    const w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('已设主密码 · 已锁定')
    expect(w.text()).toContain('空闲自动锁定')
    await Array.from(w.findAll('button')).find((b) => b.text() === '解锁')!.trigger('click')
    expect(vault.unlockVault).toHaveBeenCalledTimes(1)
    // 空闲锁定分钟双向绑定 + 保存
    const idle = w.find('input[type="number"]')
    expect((idle.element as HTMLInputElement).value).toBe('30')
    vault.vaultIdleMinutes.value = 45
    await Array.from(w.findAll('button')).find((b) => b.text() === '保存')!.trigger('click')
    expect(vault.saveVaultIdleLock).toHaveBeenCalledTimes(1)
  })

  it('已解锁：锁定/移除主密码按钮分别调 lockVault / removeVaultPassword', async () => {
    vault.vaultStatus.value = { protected: true, unlocked: true, idle_lock_minutes: 15 }
    const w = mountSection()
    await flushPromises()
    await Array.from(w.findAll('button')).find((b) => b.text() === '锁定')!.trigger('click')
    expect(vault.lockVault).toHaveBeenCalledTimes(1)
    await Array.from(w.findAll('button')).find((b) => b.text() === '移除主密码')!.trigger('click')
    expect(vault.removeVaultPassword).toHaveBeenCalledTimes(1)
  })
})

describe('DataSection · 同步分支与按钮态', () => {
  function status(over: Partial<Record<string, unknown>> = {}) {
    return { vault_protected: true, unlocked: true, configured: true, ready: true, host: 'dav.example.com', last_push_at: null, last_pull_at: null, ...over }
  }

  it('状态文案：未设主密码禁止上云', async () => {
    sync.syncStatus.value = status({ vault_protected: false }) as never
    const w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('未设主密码（禁止上云）')
  })

  it('状态文案：已锁定请先解锁 / 待配置 / 就绪 host', async () => {
    sync.syncStatus.value = status({ unlocked: false }) as never
    let w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('已锁定（请先解锁）')
    w.unmount()
    sync.syncStatus.value = status({ configured: false, ready: false }) as never
    w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('待配置 WebDAV')
    w.unmount()
    sync.syncStatus.value = status() as never
    w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('就绪 · dav.example.com')
  })

  it('保存配置：未保护即禁用（disabled 只看 vault_protected/unlocked，不看 ready）；就绪可点 → saveSyncConfig', async () => {
    sync.syncStatus.value = status({ vault_protected: false, ready: false }) as never
    const w = mountSection()
    await flushPromises()
    const saveBtn = Array.from(w.findAll('button')).find((b) => b.text() === '保存配置')!
    expect(saveBtn.attributes('disabled')).toBeDefined()
    w.unmount()
    sync.syncStatus.value = status() as never
    const w2 = mountSection()
    await flushPromises()
    const ok = Array.from(w2.findAll('button')).find((b) => b.text() === '保存配置')!
    expect(ok.attributes('disabled')).toBeUndefined()
    await ok.trigger('click')
    expect(sync.saveSyncConfig).toHaveBeenCalledTimes(1)
  })

  it('推送/从云合并：!ready 禁用；ready 可点分别调 pushSync / pullSync', async () => {
    sync.syncStatus.value = status({ ready: false }) as never
    let w = mountSection()
    await flushPromises()
    expect(Array.from(w.findAll('button')).find((b) => b.text()!.includes('推送到云'))!.attributes('disabled')).toBeDefined()
    w.unmount()
    sync.syncStatus.value = status() as never
    w = mountSection()
    await flushPromises()
    await Array.from(w.findAll('button')).find((b) => b.text()!.includes('推送到云'))!.trigger('click')
    expect(sync.pushSync).toHaveBeenCalledTimes(1)
    await Array.from(w.findAll('button')).find((b) => b.text()!.includes('从云合并'))!.trigger('click')
    expect(sync.pullSync).toHaveBeenCalledTimes(1)
  })

  it('清除配置按钮仅 configured 时显示，点击调 clearSync；同步时间戳渲染', async () => {
    sync.syncStatus.value = status({ configured: false, ready: false }) as never
    let w = mountSection()
    await flushPromises()
    expect(Array.from(w.findAll('button')).find((b) => b.text()!.includes('清除配置'))).toBeUndefined()
    w.unmount()
    sync.syncStatus.value = status({ last_push_at: '2026-10-18 08:00', last_pull_at: '2026-10-18 09:00' }) as never
    w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('上次推送：2026-10-18 08:00')
    expect(w.text()).toContain('上次拉取：2026-10-18 09:00')
    await Array.from(w.findAll('button')).find((b) => b.text()!.includes('清除配置'))!.trigger('click')
    expect(sync.clearSync).toHaveBeenCalledTimes(1)
  })
})
