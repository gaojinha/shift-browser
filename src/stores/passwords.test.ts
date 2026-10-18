// passwords store 单测（F-80，王二狗）
// 契约：列表拉取（不含明文）/ 保存成功即重载 + toast / 保存失败 toast 不抛 /
//       明文读取两条通道 / 删除成功本地过滤 / 删除失败 toast 且列表不动。
// 范式：真实 pinia + mock @tauri-apps/api/core 的 invoke（见 stores/settings.test.ts）。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { usePasswordsStore } from './passwords'
import { useToastStore } from './toast'
import type { PasswordMeta } from '../types/password'

function meta(id: number, over: Partial<PasswordMeta> = {}): PasswordMeta {
  return {
    id,
    domain: `site${id}.com`,
    username: `user${id}`,
    notes: null,
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    ...over,
  }
}

const LIST = [meta(1), meta(2)]

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
})

describe('passwords store：列表拉取', () => {
  it('loadPasswords：调 list_passwords，写入 list 并置 loaded', async () => {
    invokeMock.mockResolvedValueOnce([...LIST])
    const s = usePasswordsStore()
    expect(s.list).toEqual([])
    expect(s.loaded).toBe(false)

    await s.loadPasswords()
    expect(invokeMock).toHaveBeenCalledWith('list_passwords')
    expect(s.list).toEqual(LIST)
    expect(s.loaded).toBe(true)
  })

  it('loadPasswords：失败向上抛，且不置 loaded', async () => {
    invokeMock.mockRejectedValueOnce(new Error('db locked'))
    const s = usePasswordsStore()
    await expect(s.loadPasswords()).rejects.toThrow('db locked')
    expect(s.loaded).toBe(false)
  })
})

describe('passwords store：保存', () => {
  it('savePassword 成功：按 {domain,username,password,notes} 调 save_password，随后重载 + toast', async () => {
    const s = usePasswordsStore()
    const toast = useToastStore()
    const successSpy = vi.spyOn(toast, 'success')
    const loadSpy = vi.spyOn(s, 'loadPasswords').mockResolvedValue(undefined)

    await s.savePassword('example.com', 'alice', 'p@ss', '备注')

    expect(invokeMock).toHaveBeenCalledWith('save_password', {
      domain: 'example.com',
      username: 'alice',
      password: 'p@ss',
      notes: '备注',
    })
    expect(loadSpy).toHaveBeenCalledTimes(1)
    expect(successSpy).toHaveBeenCalledWith('密码已保存')
  })

  it('savePassword 失败：toast 报错、不抛出、不重载', async () => {
    invokeMock.mockRejectedValueOnce(new Error('write fail'))
    const s = usePasswordsStore()
    const toast = useToastStore()
    const errSpy = vi.spyOn(toast, 'error')
    const loadSpy = vi.spyOn(s, 'loadPasswords').mockResolvedValue(undefined)

    await expect(s.savePassword('x.com', null, 'pw')).resolves.toBeUndefined()
    expect(errSpy).toHaveBeenCalledWith('保存密码失败')
    expect(loadSpy).not.toHaveBeenCalled()
  })
})

describe('passwords store：明文读取', () => {
  it('getPassword：调 get_password（按域名）并原样返回', async () => {
    const entry = { ...meta(7), password: 'secret' }
    invokeMock.mockResolvedValueOnce(entry)
    const s = usePasswordsStore()
    const r = await s.getPassword('site7.com')
    expect(invokeMock).toHaveBeenCalledWith('get_password', { domain: 'site7.com' })
    expect(r).toEqual(entry)
  })

  it('getPasswordById：调 get_password_by_id（按 id）', async () => {
    invokeMock.mockResolvedValueOnce(null)
    const s = usePasswordsStore()
    const r = await s.getPasswordById(7)
    expect(invokeMock).toHaveBeenCalledWith('get_password_by_id', { id: 7 })
    expect(r).toBeNull()
  })
})

describe('passwords store：删除', () => {
  it('deletePassword 成功：调 delete_password 并从本地 list 过滤', async () => {
    const s = usePasswordsStore()
    s.list = [...LIST]
    await s.deletePassword(1)
    expect(invokeMock).toHaveBeenCalledWith('delete_password', { id: 1 })
    expect(s.list.map((p) => p.id)).toEqual([2])
  })

  it('deletePassword 失败：toast 报错、列表不动', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = usePasswordsStore()
    s.list = [...LIST]
    const toast = useToastStore()
    const errSpy = vi.spyOn(toast, 'error')

    await expect(s.deletePassword(1)).resolves.toBeUndefined()
    expect(errSpy).toHaveBeenCalledWith('删除密码失败')
    expect(s.list).toEqual(LIST)
  })
})
