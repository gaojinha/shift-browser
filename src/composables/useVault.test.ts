// useVault 单测（F-74）：主密码校验 / 状态机迁移（设置→锁定→解锁→移除）/ 错误路径 busy 复位
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { useVault } from './useVault'
import { useToastStore } from '../stores/toast'

const status = (over = {}) => ({ protected: true, unlocked: false, idle_lock_minutes: 15, ...over })
const toasts = () => useToastStore().items.map((t) => `${t.type}:${t.message}`)

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

describe('useVault', () => {
  it('loadVaultStatus: 同步状态与空闲分钟数；失败保持 null', async () => {
    const v = useVault()
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await v.loadVaultStatus()
    expect(v.vaultStatus.value).toBeNull()
    invokeMock.mockResolvedValueOnce(status({ idle_lock_minutes: 30 }))
    await v.loadVaultStatus()
    expect(v.vaultStatus.value?.protected).toBe(true)
    expect(v.vaultIdleMinutes.value).toBe(30)
  })

  it('setVaultPassword: 少于 8 位拒绝且不调用后端', async () => {
    const v = useVault()
    v.vaultPassword.value = '1234567'
    await v.setVaultPassword()
    expect(invokeMock).not.toHaveBeenCalled()
    expect(toasts()).toContain('error:主密码至少 8 位')
  })

  it('setVaultPassword: 成功清空密码并刷新状态，busy 复位', async () => {
    const v = useVault()
    v.vaultPassword.value = '12345678'
    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce(status())
    await v.setVaultPassword()
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'set_vault_password', { password: '12345678' })
    expect(v.vaultPassword.value).toBe('')
    expect(v.vaultStatus.value?.protected).toBe(true)
    expect(v.vaultBusy.value).toBe(false)
  })

  it('unlockVault: 失败保留输入、toast 报错、busy 复位；成功后状态为已解锁', async () => {
    const v = useVault()
    v.vaultPassword.value = 'wrong-pass'
    invokeMock.mockRejectedValueOnce('密码错误')
    await v.unlockVault()
    expect(v.vaultPassword.value).toBe('wrong-pass')
    expect(v.vaultBusy.value).toBe(false)
    expect(toasts()).toContain('error:解锁失败：密码错误')

    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce(status({ unlocked: true }))
    await v.unlockVault()
    expect(v.vaultStatus.value?.unlocked).toBe(true)
    expect(v.vaultPassword.value).toBe('')
  })

  it('lockVault: 成功后刷新为锁定；失败静默不弹 toast', async () => {
    const v = useVault()
    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce(status({ unlocked: false }))
    await v.lockVault()
    expect(v.vaultStatus.value?.unlocked).toBe(false)
    expect(toasts()).toContain('success:密码库已锁定')

    setActivePinia(createPinia())
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await useVault().lockVault()
    expect(toasts()).toEqual([])
  })

  it('removeVaultPassword: 成功提示回到密钥文件模式；失败 toast', async () => {
    const v = useVault()
    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce(status({ protected: false }))
    await v.removeVaultPassword()
    expect(v.vaultStatus.value?.protected).toBe(false)
    expect(toasts()).toContain('success:主密码已移除（回到密钥文件模式）')
    invokeMock.mockRejectedValueOnce('bad')
    await v.removeVaultPassword()
    expect(toasts()).toContain('error:移除失败：bad')
    expect(v.vaultBusy.value).toBe(false)
  })

  it('saveVaultIdleLock: 越界/非数字拒绝；0 表示关闭；合法值取整下发', async () => {
    const v = useVault()
    for (const bad of [-1, 1441, Number.NaN]) {
      v.vaultIdleMinutes.value = bad
      await v.saveVaultIdleLock()
    }
    expect(invokeMock).not.toHaveBeenCalled()

    invokeMock.mockResolvedValue(status())
    v.vaultIdleMinutes.value = 0
    await v.saveVaultIdleLock()
    expect(invokeMock).toHaveBeenCalledWith('set_vault_idle_lock', { minutes: 0 })
    expect(toasts()).toContain('success:空闲自动锁定已关闭')

    v.vaultIdleMinutes.value = 10.9
    await v.saveVaultIdleLock()
    expect(invokeMock).toHaveBeenCalledWith('set_vault_idle_lock', { minutes: 10 })
  })
})
