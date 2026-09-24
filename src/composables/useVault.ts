import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from '../stores/toast'

/** 密码库主密码（R2-1d 从 DataSection 抽出） */
export function useVault() {
  interface VaultStatus {
    protected: boolean
    unlocked: boolean
    idle_lock_minutes: number
  }
  const vaultStatus = ref<VaultStatus | null>(null)
  const vaultPassword = ref('')
  const vaultBusy = ref(false)
  /** 空闲自动锁定分钟数（0 = 关闭，上限 1440） */
  const vaultIdleMinutes = ref<number>(15)
  async function loadVaultStatus() {
    try {
      vaultStatus.value = await invoke<VaultStatus>('vault_status')
      vaultIdleMinutes.value = vaultStatus.value.idle_lock_minutes
    } catch (e) {
      console.error('[settings] 读取密码库状态失败:', e)
    }
  }
  async function saveVaultIdleLock() {
    const mins = Number(vaultIdleMinutes.value)
    if (!Number.isFinite(mins) || mins < 0 || mins > 1440) {
      useToastStore().error('空闲锁定需为 0–1440 分钟（0 = 关闭）')
      return
    }
    try {
      await invoke('set_vault_idle_lock', { minutes: Math.floor(mins) })
      await loadVaultStatus()
      useToastStore().success(mins === 0 ? '空闲自动锁定已关闭' : `空闲 ${mins} 分钟后自动锁定`)
    } catch (e) {
      console.error('[settings] 保存空闲锁定设置失败:', e)
      useToastStore().error('保存失败：' + e)
    }
  }
  async function setVaultPassword() {
    if (vaultPassword.value.length < 8) {
      useToastStore().error('主密码至少 8 位')
      return
    }
    vaultBusy.value = true
    try {
      await invoke('set_vault_password', { password: vaultPassword.value })
      vaultPassword.value = ''
      await loadVaultStatus()
      useToastStore().success('主密码已设置，密码库进入保护状态')
    } catch (e) {
      console.error('[settings] 设置主密码失败:', e)
      useToastStore().error('设置失败：' + e)
    } finally {
      vaultBusy.value = false
    }
  }
  async function unlockVault() {
    vaultBusy.value = true
    try {
      await invoke('unlock_vault', { password: vaultPassword.value })
      vaultPassword.value = ''
      await loadVaultStatus()
      useToastStore().success('密码库已解锁')
    } catch (e) {
      console.error('[settings] 解锁失败:', e)
      useToastStore().error('解锁失败：' + e)
    } finally {
      vaultBusy.value = false
    }
  }
  async function lockVault() {
    try {
      await invoke('lock_vault')
      vaultPassword.value = ''
      await loadVaultStatus()
      useToastStore().success('密码库已锁定')
    } catch (e) {
      console.error('[settings] 锁定失败:', e)
    }
  }
  async function removeVaultPassword() {
    vaultBusy.value = true
    try {
      await invoke('remove_vault_password', { password: vaultPassword.value })
      vaultPassword.value = ''
      await loadVaultStatus()
      useToastStore().success('主密码已移除（回到密钥文件模式）')
    } catch (e) {
      console.error('[settings] 移除主密码失败:', e)
      useToastStore().error('移除失败：' + e)
    } finally {
      vaultBusy.value = false
    }
  }

  /** 上云 E2E 加密同步（第 45 章 · 服务器零知识） */

  return { vaultStatus, vaultPassword, vaultBusy, vaultIdleMinutes, loadVaultStatus, saveVaultIdleLock, setVaultPassword, unlockVault, lockVault, removeVaultPassword }
}
