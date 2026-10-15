<script setup lang="ts">
// 设置弹窗 · 高级分区 · 一键重置设置
// 只重置设置项（settings 表），绝不碰用户数据（书签/历史/密码/Cookie/登录态）
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../../stores/settings'
import { useToastStore } from '../../stores/toast'
import UiDialog from '../ui/UiDialog.vue'
import SettingCard from './ui/SettingCard.vue'
import './ui/settings-shared.css'

const settingsStore = useSettingsStore()
const confirmOpen = ref(false)
const busy = ref(false)

async function doReset() {
  if (busy.value) return
  busy.value = true
  try {
    const n = await invoke<number>('reset_settings')
    confirmOpen.value = false
    // 重新拉取全部设置（内存态同步）
    await settingsStore.loadSettings()
    useToastStore().success(`设置已重置（${n} 项恢复默认）`)
  } catch (e) {
    console.error('[settings] 重置设置失败:', e)
    useToastStore().error('重置设置失败')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <SettingCard title="重置" icon="refresh" anchor="reset">
    <div class="field">
      <label>一键重置设置</label>
      <span class="field-desc">
        将所有设置项恢复默认值。书签、浏览历史、密码、Cookie 和登录态不受影响。
      </span>
      <button
        class="mini-btn danger"
        :disabled="busy"
        aria-label="一键重置设置"
        @click="confirmOpen = true"
      >
        {{ busy ? '重置中…' : '重置设置' }}
      </button>
    </div>

    <UiDialog
      v-model:open="confirmOpen"
      title="确认重置设置"
      width="min(440px, calc(100vw - 48px))"
    >
      <p class="confirm-body">
        此操作会将所有设置项恢复默认值。<br />
        <strong>不受影响</strong>：书签、浏览历史、密码、Cookie、登录态。
      </p>
      <template #footer>
        <button class="mini-btn" @click="confirmOpen = false">取消</button>
        <button
          class="mini-btn danger"
          :disabled="busy"
          :aria-label="busy ? '重置中' : '确认重置设置'"
          @click="doReset"
        >
          {{ busy ? '重置中…' : '确认重置' }}
        </button>
      </template>
    </UiDialog>
  </SettingCard>
</template>

<style scoped>
.confirm-body {
  font-size: var(--text-base);
  color: var(--text-primary);
  line-height: 1.6;
}
</style>
