// AboutSection 组件测试（F-87 · 前端测试补课）
//
// 手法：mock invoke（app_about_cmd）与 plugin-opener；toast 走真实 store。
// 只读信息卡：挂载加载 + 渲染回退 + 两按钮接线。
//
// 跑法：npx vitest run src/components/settings/AboutSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import AboutSection from './AboutSection.vue'
import { useToastStore } from '../../stores/toast'
import { openUrl } from '@tauri-apps/plugin-opener'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }))

import { invoke } from '@tauri-apps/api/core'
const invokeMock = vi.mocked(invoke)
const openUrlMock = vi.mocked(openUrl)

const MOUNTS: VueWrapper[] = []
function mountSection() {
  const w = mount(AboutSection, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
})

beforeEach(() => {
  setActivePinia(createPinia()) // 测试体里 useToastStore() 在组件外，必须有活跃 pinia
  invokeMock.mockReset()
  invokeMock.mockResolvedValue({
    appName: 'Shift Browser',
    version: '1.2.3',
    webkitVersion: '2.46.0',
    osArch: 'x86_64 linux',
    dataDir: '/home/gao/.local/share/shift-browser',
    repoUrl: 'https://example.com/shift-browser',
    profile: 'release',
    iconDataUrl: '',
  })
  openUrlMock.mockClear()
})

describe('AboutSection · 信息渲染', () => {
  it('挂载即 app_about_cmd；渲染版本/徽章/环境项', async () => {
    const w = mountSection()
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('app_about_cmd')
    expect(w.find('.about-version').text()).toContain('v1.2.3')
    expect(w.find('.about-badge').text()).toBe('正式版')
    expect(w.text()).toContain('WebKitGTK 2.46.0')
    expect(w.text()).toContain('x86_64 linux')
  })

  it('加载前回退不崩：名字 Shift Browser / 版本 —；debug profile → 调试版徽章', async () => {
    invokeMock.mockReturnValue(new Promise(() => {})) // 永不 resolve
    let w = mountSection()
    expect(w.find('.about-version').text()).toContain('—')
    w.unmount()
    invokeMock.mockResolvedValue({ appName: 'X', version: '9', webkitVersion: '', osArch: '', dataDir: '', repoUrl: '', profile: 'debug', iconDataUrl: '' })
    w = mountSection()
    await flushPromises()
    expect(w.find('.about-badge').text()).toBe('调试版')
  })

  it('加载失败静默（console.error），渲染回退值不崩', async () => {
    invokeMock.mockRejectedValue(new Error('x'))
    const w = mountSection()
    await flushPromises()
    expect(w.find('.about-name').text()).toBe('Shift Browser')
  })
})

describe('AboutSection · 按钮接线', () => {
  it('打开仓库 → openUrl(repoUrl)；打开数据目录 → openUrl(dataDir)', async () => {
    const w = mountSection()
    await flushPromises()
    const btns = w.findAll('.mini-btn')
    await btns[0]!.trigger('click') // 打开仓库
    await flushPromises()
    expect(openUrlMock).toHaveBeenCalledWith('https://example.com/shift-browser')
    await btns[1]!.trigger('click') // 打开文件夹
    await flushPromises()
    expect(openUrlMock).toHaveBeenCalledWith('/home/gao/.local/share/shift-browser')
  })

  it('repoUrl/dataDir 为空时不调 openUrl', async () => {
    invokeMock.mockResolvedValue({ appName: 'X', version: '9', webkitVersion: '', osArch: '', dataDir: '', repoUrl: '', profile: 'release', iconDataUrl: '' })
    const w = mountSection()
    await flushPromises()
    const btns = w.findAll('.mini-btn')
    await btns[0]!.trigger('click')
    await btns[1]!.trigger('click')
    await flushPromises()
    expect(openUrlMock).not.toHaveBeenCalled()
  })

  it('openUrl 失败 → toast「打开项目主页失败」', async () => {
    openUrlMock.mockRejectedValue(new Error('x'))
    const w = mountSection()
    await flushPromises()
    await w.findAll('.mini-btn')[0]!.trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '打开项目主页失败')).toBe(true)
  })
})
