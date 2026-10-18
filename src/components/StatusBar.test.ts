// StatusBar 组件测试（F-78 前端补课，王二狗接手，0→1）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子 tabs/settings）+
// mock Tauri IPC（invoke）+ mock 事件总线（listen 收集处理器，测试里手动推事件）+
// mock @tauri-apps/plugin-opener（openUrl）。
//
// StatusBar 三段构成：
//   左段：加载态（loadingTabId）+ 站点图标（favicon/内部页无）+ 域名（https 🔒 / http ⚠️）
//   中段：隐私防护战绩（get_privacy_stats，adblock 开关决定颜色 class）
//   右段：网络胶囊（net-status-updated 推送，延迟分档 good/mid/bad）+
//         下载角标（downloads-updated → get_downloads 统计 in_progress）+
//         设置/帮助按钮
//
// 覆盖：
//  1.  加载态：loadingTabId=null → ✓ 就绪；非 null → 🔄 加载中
//  2.  域名与图标：https → 🔒 host；http → ⚠️ host；shift:/file: 不显示域名；favicon 渲染 img
//  3.  隐私战绩：初始「统计中...」；挂载后读取统计展示数字；adblock 关 → privacy-off
//  4.  网络胶囊：初始检测中 + net-mid；推送 → 文案 + 分档 class（good/mid/bad）+ 离线 📵
//  5.  下载角标：in_progress 计数 > 0 显示角标；点击 emit openDownloads
//  6.  中段点击 emit openPrivacy；设置按钮 emit openSettings
//  7.  页面加载失败事件 → 显示 ⚠️ 提示，5 秒后自动消失（假定时器）
//  8.  点网络胶囊 → net_diagnose invoke + toast 展示四层结论
//  9.  帮助按钮 → openUrl 调起外部链接
//
// 跑法：npx vitest run src/components/StatusBar.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import type { Tab } from '../types/tab'

const { invokeMock, listenMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  listenMock: vi.fn(),
}))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}))
vi.mock('@tauri-apps/plugin-opener', () => ({
  openUrl: vi.fn(() => Promise.resolve()),
}))

import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'
import { useToastStore } from '../stores/toast'
import { EVENTS } from '../constants'
import { openUrl } from '@tauri-apps/plugin-opener'
import StatusBar from './StatusBar.vue'

/** 构造合法 Tab（Rust 侧字段 + 前端运行时字段），同 AddressBar.test.ts 口径 */
function makeTab(id: string, over: Partial<Tab> = {}): Tab {
  return {
    id,
    url: `https://${id}.com/`,
    title: id,
    favicon: '',
    is_active: false,
    is_pinned: false,
    is_muted: false,
    hibernated: false,
    private: false,
    groupId: null,
    ...over,
  }
}

const PRIVACY = { cleaned_params: 3, privacy_injections: 1, adblock_injections: 7 }

/** 默认 IPC 实现：隐私统计 + 空下载列表 */
function defaultInvoke(cmd: string) {
  if (cmd === 'get_privacy_stats') return Promise.resolve({ ...PRIVACY })
  if (cmd === 'get_downloads') return Promise.resolve([] as Array<{ state: string }>)
  return Promise.resolve(undefined as unknown)
}

/** 取出组件 onMounted 注册的某事件处理器（按事件名） */
function getListener(event: string) {
  const call = listenMock.mock.calls.find((c) => c[0] === event)
  return call?.[1] as ((e: { payload: unknown }) => void) | undefined
}

let pinia: Pinia
let wrapper: VueWrapper | null = null

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation(defaultInvoke)
  listenMock.mockReset()
  listenMock.mockImplementation(() => Promise.resolve(() => {}))
  vi.mocked(openUrl).mockClear()
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.useRealTimers()
})

interface SeedOpts {
  tabs?: Tab[]
  activeTabId?: string | null
  loadingTabId?: string | null
  settings?: Record<string, string>
}

/** 挂载 StatusBar，等 onMounted 的异步初始化落定 */
async function mountBar(opts: SeedOpts = {}) {
  const {
    tabs = [makeTab('t1', { url: 'https://example.com/page', favicon: 'https://example.com/f.ico' })],
    activeTabId = 't1',
    loadingTabId = null,
    settings = {},
  } = opts

  const tabsStore = useTabsStore(pinia)
  tabsStore.$patch({ tabs, activeTabId, loadingTabId } as never)

  const settingsStore = useSettingsStore(pinia)
  settingsStore.$patch({ settings: { ...settings }, loaded: true } as never)

  wrapper = mount(StatusBar, { global: { plugins: [pinia] } })
  await flushPromises()
  return wrapper
}

describe('StatusBar：左段加载状态', () => {
  it('loadingTabId=null → ✓ 就绪；非 null → 🔄 加载中', async () => {
    const w = await mountBar({ loadingTabId: null })
    expect(w.find('.status-icon').text()).toBe('✓')
    expect(w.find('.status-text').text()).toBe('就绪')

    const tabsStore = useTabsStore(pinia)
    tabsStore.$patch({ loadingTabId: 't1' } as never)
    await flushPromises()
    expect(w.find('.status-icon').text()).toBe('🔄')
    expect(w.find('.status-text').text()).toBe('加载中')
  })
})

describe('StatusBar：域名与站点图标', () => {
  it('https → 🔒 主机名；http → ⚠️ 主机名', async () => {
    const w = await mountBar({
      tabs: [makeTab('t1', { url: 'https://sec.example.com/a/b' })],
      activeTabId: 't1',
    })
    expect(w.find('.status-domain').text()).toBe('🔒 sec.example.com')

    const tabsStore = useTabsStore(pinia)
    tabsStore.$patch({
      tabs: [makeTab('t1', { url: 'http://plain.example.com/' })],
      activeTabId: 't1',
    } as never)
    await flushPromises()
    expect(w.find('.status-domain').text()).toBe('⚠️ plain.example.com')
  })

  it('内部页面（shift:/file:）不显示域名', async () => {
    const w = await mountBar({
      tabs: [makeTab('t1', { url: 'shift://settings' })],
      activeTabId: 't1',
    })
    expect(w.find('.status-domain').exists()).toBe(false)

    const tabsStore = useTabsStore(pinia)
    tabsStore.$patch({
      tabs: [makeTab('t1', { url: 'file:///tmp/x.html' })],
      activeTabId: 't1',
    } as never)
    await flushPromises()
    expect(w.find('.status-domain').exists()).toBe(false)
  })

  it('有 favicon 时渲染站点图标 img，无 favicon 时不渲染', async () => {
    const w = await mountBar({
      tabs: [makeTab('t1', { url: 'https://a.com/', favicon: 'https://a.com/f.ico' })],
      activeTabId: 't1',
    })
    const img = w.find('img.status-favicon')
    expect(img.exists()).toBe(true)
    expect(img.attributes('src')).toBe('https://a.com/f.ico')

    const tabsStore = useTabsStore(pinia)
    tabsStore.$patch({
      tabs: [makeTab('t1', { url: 'https://a.com/', favicon: '' })],
      activeTabId: 't1',
    } as never)
    await flushPromises()
    expect(w.find('img.status-favicon').exists()).toBe(false)
  })
})

describe('StatusBar：中段隐私战绩', () => {
  it('挂载后读取统计并展示拦截/清参数字', async () => {
    const w = await mountBar()
    expect(invokeMock).toHaveBeenCalledWith('get_privacy_stats')
    const badge = w.find('.privacy-badge')
    expect(badge.text()).toBe('🛡 已拦 7 · 清参 3')
    expect(badge.classes()).toContain('privacy-on')
    expect(badge.attributes('title')).toContain('广告拦截 7 次')
  })

  it('adblock_enabled=false → privacy-off class', async () => {
    const w = await mountBar({ settings: { adblock_enabled: 'false' } })
    expect(w.find('.privacy-badge').classes()).toContain('privacy-off')
  })

  it('点击中段 → emit openPrivacy', async () => {
    const w = await mountBar()
    await w.find('.privacy-badge').trigger('click')
    expect(w.emitted('openPrivacy')).toBeTruthy()
  })
})

describe('StatusBar：右段网络胶囊', () => {
  it('初始无数据：检测中 + net-mid + ping --', async () => {
    const w = await mountBar()
    const net = w.find('.net-status')
    expect(net.classes()).toContain('net-mid')
    expect(net.text()).toContain('📶')
    expect(net.text()).toContain('--')
    expect(net.text()).toContain('📶 检测中...')
  })

  it('推送在线低延迟 → net-good，展示速度与延迟', async () => {
    const w = await mountBar()
    getListener(EVENTS.NET_STATUS_UPDATED)!({
      payload: { down_kb: 512, up_kb: 128, ping_ms: 50, online: true },
    })
    await flushPromises()
    const net = w.find('.net-status')
    expect(net.classes()).toContain('net-good')
    expect(net.text()).toContain('50ms')
    expect(net.text()).toContain('⬇512K ⬆128K')
    expect(net.attributes('title')).toContain('⬇下行 512K/s')
  })

  it('延迟分档：150ms → net-mid；300ms → net-bad', async () => {
    const w = await mountBar()
    getListener(EVENTS.NET_STATUS_UPDATED)!({
      payload: { down_kb: 0, up_kb: 0, ping_ms: 150, online: true },
    })
    await flushPromises()
    expect(w.find('.net-status').classes()).toContain('net-mid')

    getListener(EVENTS.NET_STATUS_UPDATED)!({
      payload: { down_kb: 0, up_kb: 0, ping_ms: 300, online: true },
    })
    await flushPromises()
    expect(w.find('.net-status').classes()).toContain('net-bad')
  })

  it('离线上报 → net-bad + 📵 图标 + 标题带（离线）', async () => {
    const w = await mountBar()
    getListener(EVENTS.NET_STATUS_UPDATED)!({
      payload: { down_kb: 0, up_kb: 0, ping_ms: -1, online: false },
    })
    await flushPromises()
    const net = w.find('.net-status')
    expect(net.classes()).toContain('net-bad')
    expect(net.text()).toContain('📵')
    expect(net.attributes('title')).toContain('（离线）')
  })

  it('点网络胶囊 → net_diagnose invoke + toast 展示四层结论', async () => {
    const w = await mountBar()
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'net_diagnose') {
        return { iface_ok: true, gateway: '192.168.1.1', gateway_ok: true, dns_ok: true, remote_ms: 42 }
      }
      return defaultInvoke(cmd)
    })
    const toast = useToastStore(pinia)
    const showSpy = vi.spyOn(toast, 'show')

    await w.find('.net-status').trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('net_diagnose')
    expect(showSpy).toHaveBeenCalledTimes(1)
    const msg = showSpy.mock.calls[0][0] as string
    expect(msg).toContain('断网诊断')
    expect(msg).toContain('网卡：正常')
    expect(msg).toContain('网关（192.168.1.1）：通')
    expect(msg).toContain('域名解析：正常')
    expect(msg).toContain('出网：42ms')
  })

  it('诊断失败 → toast 报错', async () => {
    const w = await mountBar()
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'net_diagnose') throw new Error('boom')
      return defaultInvoke(cmd)
    })
    const toast = useToastStore(pinia)
    const showSpy = vi.spyOn(toast, 'show')

    await w.find('.net-status').trigger('click')
    await flushPromises()

    expect(showSpy).toHaveBeenCalledTimes(1)
    expect(showSpy.mock.calls[0][1]).toBe('error')
    expect(showSpy.mock.calls[0][0]).toContain('诊断失败')
  })
})

describe('StatusBar：右段下载与按钮', () => {
  it('in_progress 下载计数 > 0 → 显示角标；点击 emit openDownloads', async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_downloads') {
        return [
          { state: 'in_progress' },
          { state: 'in_progress' },
          { state: 'completed' },
        ]
      }
      return defaultInvoke(cmd)
    })
    const w = await mountBar()
    const dlBtn = w.find('.dl-btn')
    expect(dlBtn.find('.dl-badge').text()).toBe('2')
    expect(dlBtn.attributes('title')).toContain('下载管理（2 进行中）')

    await dlBtn.trigger('click')
    expect(w.emitted('openDownloads')).toBeTruthy()
  })

  it('无进行中下载 → 不显示角标', async () => {
    const w = await mountBar()
    expect(w.find('.dl-btn .dl-badge').exists()).toBe(false)
  })

  it('downloads-updated 事件 → 重新拉取并刷新角标', async () => {
    const w = await mountBar()
    expect(w.find('.dl-badge').exists()).toBe(false)

    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_downloads') return [{ state: 'in_progress' }]
      return defaultInvoke(cmd)
    })
    getListener(EVENTS.DOWNLOADS_UPDATED)!({ payload: undefined })
    await flushPromises()
    expect(w.find('.dl-badge').text()).toBe('1')
  })

  it('设置按钮 → emit openSettings', async () => {
    const w = await mountBar()
    const btns = w.findAll('.status-btn')
    const settingsBtn = btns.find((b) => b.attributes('title') === '设置')!
    await settingsBtn.trigger('click')
    expect(w.emitted('openSettings')).toBeTruthy()
  })

  it('帮助按钮 → openUrl 调起外部链接', async () => {
    const w = await mountBar()
    const helpBtn = w.findAll('.status-btn').find((b) => b.attributes('title') === '帮助')!
    await helpBtn.trigger('click')
    expect(openUrl).toHaveBeenCalledWith('https://gitcode.com/gaojinha/shiftbrowser-tauri')
  })
})

describe('StatusBar：页面加载失败提示', () => {
  it('page-load-failed 事件 → 显示提示，5 秒后自动消失', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval'] })
    const w = await mountBar()
    expect(w.find('.status-load-failed').exists()).toBe(false)

    getListener(EVENTS.PAGE_LOAD_FAILED)!({ payload: { tabId: 't1', uri: 'https://bad.example' } })
    await flushPromises()
    const alert = w.find('.status-load-failed')
    expect(alert.exists()).toBe(true)
    expect(alert.text()).toContain('页面加载失败')

    vi.advanceTimersByTime(5000)
    await flushPromises()
    expect(w.find('.status-load-failed').exists()).toBe(false)
  })
})
