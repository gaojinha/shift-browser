import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from '../stores/toast'

/** 上云 E2E 加密同步（R2-1d 从 DataSection 抽出） */
export function useSync() {
  interface SyncStatus {
    vault_protected: boolean
    unlocked: boolean
    configured: boolean
    ready: boolean
    host: string | null
    last_push_at: string | null
    last_pull_at: string | null
  }
  const syncStatus = ref<SyncStatus | null>(null)
  const syncUrl = ref('')
  const syncUser = ref('')
  const syncPass = ref('')
  const syncBusy = ref(false)
  async function loadSyncStatus() {
    try {
      syncStatus.value = await invoke<SyncStatus>('sync_status')
      if (syncStatus.value.host && !syncUrl.value) {
        syncUrl.value = 'https://' + syncStatus.value.host
      }
    } catch (e) {
      console.error('[settings] 读取同步状态失败:', e)
    }
  }
  async function saveSyncConfig() {
    if (!syncUrl.value.trim() || !syncUser.value.trim()) {
      useToastStore().error('请填写 WebDAV 地址与用户名')
      return
    }
    syncBusy.value = true
    try {
      await invoke('sync_configure', {
        url: syncUrl.value,
        username: syncUser.value,
        password: syncPass.value,
      })
      syncPass.value = ''
      await loadSyncStatus()
      useToastStore().success('WebDAV 已配置（凭据已用主密码加密落盘）')
    } catch (e) {
      console.error('[settings] 配置同步失败:', e)
      useToastStore().error('配置失败：' + e)
    } finally {
      syncBusy.value = false
    }
  }
  async function pushSync() {
    syncBusy.value = true
    try {
      const msg = await invoke<string>('sync_push')
      await loadSyncStatus()
      useToastStore().success(msg)
    } catch (e) {
      console.error('[settings] 推送失败:', e)
      useToastStore().error('推送失败：' + e)
    } finally {
      syncBusy.value = false
    }
  }
  async function pullSync() {
    syncBusy.value = true
    try {
      const msg = await invoke<string>('sync_pull')
      await loadSyncStatus()
      useToastStore().success(msg)
    } catch (e) {
      console.error('[settings] 拉取失败:', e)
      useToastStore().error('拉取失败：' + e)
    } finally {
      syncBusy.value = false
    }
  }
  async function clearSync() {
    try {
      await invoke('sync_clear')
      await loadSyncStatus()
      useToastStore().success('已清除本地同步配置（云端包保留）')
    } catch (e) {
      console.error('[settings] 清除同步配置失败:', e)
    }
  }

  return { syncStatus, syncUrl, syncUser, syncPass, syncBusy, loadSyncStatus, saveSyncConfig, pushSync, pullSync, clearSync }
}
