// McpSection 组件测试（F-86 · 前端测试补课）
//
// 手法：mock invokeOk（mcp_* 五命令）与 invoke；toast 走真实 store。
// 组件 onMounted 即拉取（无 show prop），挂载后 flushPromises 再断言。
//
// 跑法：npx vitest run src/components/settings/McpSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, DOMWrapper, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import McpSection from './McpSection.vue'
import { useToastStore } from '../../stores/toast'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('../../utils/invoke', () => ({
  invokeOk: vi.fn(),
  errText: (e: unknown) => (e instanceof Error ? e.message : String(e)),
}))

import { invokeOk } from '../../utils/invoke'
const invokeOkMock = vi.mocked(invokeOk)

const SERVERS = [
  { id: 1, name: 'filesystem', command: 'npx', args: ['-y', '@mcp/fs', '/tmp'], enabled: true, allowedTools: [], toolCount: 11 },
  { id: 2, name: 'calendar', command: 'node', args: ['cal.js'], enabled: false, allowedTools: ['read'], toolCount: 3 },
]
const TOOLS = [
  { name: 'read_file', serverName: 'filesystem', description: '读文件', inputSchema: {} },
  { name: 'write_file', serverName: 'filesystem', description: '', inputSchema: {} },
]
const AUDIT = [
  { id: 9, serverName: 'filesystem', toolName: 'read_file', arguments: '{"path":"/tmp/a"}', resultSummary: 'ok', success: true, createdAt: '2026-10-18 09:00' },
  { id: 10, serverName: 'calendar', toolName: 'boom', arguments: '{}', resultSummary: '', success: false, createdAt: '2026-10-18 09:01' },
]

function q(sel: string): DOMWrapper<Element> {
  const el = document.querySelector(sel)
  expect(el, `document 上找不到 ${sel}`).toBeTruthy()
  return new DOMWrapper(el!)
}
function qa(sel: string): DOMWrapper<Element>[] {
  return Array.from(document.querySelectorAll(sel)).map((el) => new DOMWrapper(el))
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeOkMock.mockReset()
  invokeOkMock.mockImplementation((cmd: string) => {
    if (cmd === 'mcp_list_servers') return Promise.resolve(SERVERS)
    if (cmd === 'mcp_list_tools') return Promise.resolve(TOOLS)
    if (cmd === 'mcp_get_audit_log') return Promise.resolve(AUDIT)
    return Promise.resolve(undefined)
  })
})

const MOUNTS: VueWrapper[] = []

function mountSection() {
  // attachTo 必须加：不接 document 的话 q()/qa() 查不到（脱离文档树）
  const w = mount(McpSection, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}

// attachTo 的实例必须卸载，否则跨用例残留：document 里堆多份同名节点，
// qa() 会查到旧实例的行/按钮（曾因此查到 4 行 server、点到旧 select）
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
})

describe('McpSection · 加载与渲染', () => {
  it('挂载即拉三命令（servers/tools/audit limit 30）', async () => {
    mountSection()
    await flushPromises()
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_list_servers')
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_list_tools')
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_get_audit_log', { limit: 30 })
  })

  it('server 行渲染：启停按钮态/名称/命令+参数/工具数/删除', async () => {
    mountSection()
    await flushPromises()
    expect(qa('.mcp-server-row')).toHaveLength(2)
    expect(q('.mcp-server-name').text()).toBe('filesystem')
    expect(q('.mcp-server-command').text()).toContain('npx -y @mcp/fs /tmp')
    expect(q('.mcp-server-count').text()).toContain('11 个工具')
    const toggles = qa('.mcp-server-row .mini-btn:not(.danger)')
    expect(toggles[0]!.text()).toContain('启用')
    expect(toggles[1]!.text()).toContain('停用')
  })

  it('工具列表与审计渲染：无描述回退、失败行 fail class、空 summary 回退', async () => {
    mountSection()
    await flushPromises()
    expect(q('.mcp-tool-name').text()).toBe('read_file')
    expect(qa('.mcp-tool-desc')[1]!.text()).toBe('(无描述)')
    const auditRows = qa('.mcp-audit-row')
    expect(auditRows[0]!.classes()).not.toContain('fail')
    expect(auditRows[1]!.classes()).toContain('fail')
    expect(auditRows[1]!.find('.mcp-audit-result').text()).toBe('(失败)')
    expect(qa('.mcp-audit-row')[0]!.find('.mcp-audit-result').text()).toBe('ok')
  })

  it('加载失败：错误显示在 ext-error', async () => {
    invokeOkMock.mockRejectedValue(new Error('mcp 后端未起'))
    mountSection()
    await flushPromises()
    expect(q('.ext-error').text()).toBe('mcp 后端未起')
  })
})

describe('McpSection · 添加 server', () => {
  it('名称或命令为空 → 行内错误「名称与命令不能为空」不调 add', async () => {
    mountSection()
    await flushPromises()
    await qa('.mcp-add-row input')[0]!.setValue('只有名')
    await q('.mcp-add-row .mini-btn.primary').trigger('click')
    await flushPromises()
    expect(q('.ext-error').text()).toBe('名称与命令不能为空')
    expect(invokeOkMock).not.toHaveBeenCalledWith('mcp_add_server', expect.anything())
  })

  it('参数/白名单非法 JSON → 错误提示，不调 add', async () => {
    mountSection()
    await flushPromises()
    await qa('.mcp-add-row input')[0]!.setValue('fs')
    await qa('.mcp-add-row input')[1]!.setValue('npx')
    await qa('.mcp-add-row input')[2]!.setValue('not-json')
    await q('.mcp-add-row .mini-btn.primary').trigger('click')
    await flushPromises()
    expect(q('.ext-error').text()).toBe('参数/白名单必须是合法 JSON 数组')
    expect(invokeOkMock).not.toHaveBeenCalledWith('mcp_add_server', expect.anything())
  })

  it('成功：mcp_add_server 全参（args/allowedTools 解析 JSON）+ 表单清空 + toast + 重拉', async () => {
    mountSection()
    await flushPromises()
    invokeOkMock.mockClear()
    const inputs = qa('.mcp-add-row input')
    await inputs[0]!.setValue(' fs ')
    await inputs[1]!.setValue(' npx ')
    await inputs[2]!.setValue('["-y","@mcp/fs"]')
    await inputs[3]!.setValue('["read_file"]')
    await q('.mcp-add-row .mini-btn.primary').trigger('click')
    await flushPromises()
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_add_server', {
      name: 'fs',
      command: 'npx',
      args: ['-y', '@mcp/fs'],
      allowedTools: ['read_file'],
    })
    expect((inputs[0]!.element as HTMLInputElement).value).toBe('')
    expect(useToastStore().items.some((t) => t.message === 'MCP server 已添加')).toBe(true)
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_list_servers')
  })

  it('添加失败：errText 进错误区', async () => {
    mountSection()
    await flushPromises()
    invokeOkMock.mockClear()
    // 仅让 add 拒绝一次（Once 用完回落 beforeEach 的列表实现）；
    // 整体 mockRejectedValue/mockResolvedValue 会把重拉列表也变掉，炸渲染出 unhandled
    invokeOkMock.mockImplementationOnce(() => Promise.reject(new Error('端口占用')))
    const inputs = qa('.mcp-add-row input')
    await inputs[0]!.setValue('fs')
    await inputs[1]!.setValue('npx')
    await q('.mcp-add-row .mini-btn.primary').trigger('click')
    await flushPromises()
    expect(q('.ext-error').text()).toBe('端口占用')
  })
})

describe('McpSection · 删除 / 启停 / 手动调用', () => {
  it('删除：mcp_remove_server {id} + toast + 重拉', async () => {
    mountSection()
    await flushPromises()
    invokeOkMock.mockClear()
    await qa('.mcp-server-row .mini-btn.danger')[0]!.trigger('click')
    await flushPromises()
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_remove_server', { id: 1 })
    expect(useToastStore().items.some((t) => t.message === '已删除')).toBe(true)
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_list_servers')
  })

  it('启停：mcp_set_server_enabled enabled 取反 + 重拉', async () => {
    mountSection()
    await flushPromises()
    invokeOkMock.mockClear()
    const toggles = qa('.mcp-server-row .mini-btn:not(.danger)')
    await toggles[1]!.trigger('click') // 第二个 enabled=false → 请求启用
    await flushPromises()
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_set_server_enabled', { id: 2, enabled: true })
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_list_servers')
  })

  it('手动调用：未选工具按钮禁用；调用成功结果渲染', async () => {
    mountSection()
    await flushPromises()
    const callBtn = qa('.mini-btn.primary').pop()!
    expect(callBtn.attributes('disabled')).toBeDefined()
    await q('select').setValue('read_file')
    await qa('.mcp-add-row input').pop()!.setValue('{"path":"/tmp/a"}')
    invokeOkMock.mockClear()
    // 仅 call_tool 返回结果一次，重拉列表仍走 beforeEach 实现（否则 undefined 炸渲染）
    invokeOkMock.mockImplementationOnce(() => Promise.resolve('文件内容 xyz'))
    await callBtn.trigger('click')
    await flushPromises()
    expect(invokeOkMock).toHaveBeenCalledWith('mcp_call_tool', {
      name: 'read_file',
      arguments: { path: '/tmp/a' },
    })
    expect(q('.mcp-result').text()).toBe('文件内容 xyz')
  })

  it('调用参数非法 JSON → 结果区 ❌ 前缀错误，不调命令', async () => {
    mountSection()
    await flushPromises()
    await q('select').setValue('read_file')
    invokeOkMock.mockClear()
    await qa('.mcp-add-row input').pop()!.setValue('bad json')
    await qa('.mini-btn.primary').pop()!.trigger('click')
    await flushPromises()
    expect(q('.mcp-result').text()).toContain('❌')
    expect(invokeOkMock).not.toHaveBeenCalledWith('mcp_call_tool', expect.anything())
  })

  it('调用失败 → 结果区 ❌ errText', async () => {
    mountSection()
    await flushPromises()
    await q('select').setValue('read_file')
    invokeOkMock.mockClear()
    invokeOkMock.mockImplementationOnce(() => Promise.reject(new Error('工具超时')))
    await qa('.mini-btn.primary').pop()!.trigger('click')
    await flushPromises()
    expect(q('.mcp-result').text()).toContain('❌ 工具超时')
  })
})
