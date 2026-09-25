<script setup lang="ts">
// 设置弹窗 · 高级分区「兼容性」卡（IA 重组：原 AppearanceSection 的 DMA-BUF 开关）
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()
</script>

<template>
  <div class="card" data-anchor="compat">
    <div class="card-title"><AppIcon name="gear" :size="14" /> 兼容性</div>
    <div class="field">
      <label>禁用 DMA-BUF 渲染</label>
      <span class="field-desc">
        修复视频播放闪烁（Intel HD 630 实测：禁用后不闪且更省资源）。<b>改后需重启生效</b>；若遇崩溃可关闭回退。
      </span>
      <label class="switch">
        <input
          type="checkbox"
          :checked="settingsStore.get('render_disable_dmabuf') !== 'false'"
          @change="onSet('render_disable_dmabuf', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
        />
        <span class="slider"></span>
      </label>
    </div>
  </div>
</template>

<style scoped>
.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; }.field-desc { flex: 1; font-size: 12px; color: var(--text-secondary); }.field label { font-size: 13px; color: var(--text-secondary); flex-shrink: 0; }.switch { position: relative; display: inline-block; width: 36px; height: 20px; flex-shrink: 0; }.switch input { opacity: 0; width: 0; height: 0; }.slider { position: absolute; cursor: pointer; inset: 0; background: var(--bg-active); border-radius: 10px; transition: 0.2s; }.slider::before { content: ''; position: absolute; width: 14px; height: 14px; left: 3px; top: 3px; background: var(--text-muted); border-radius: 50%; transition: 0.2s; }.switch input:checked + .slider { background: var(--accent); }.switch input:checked + .slider::before { transform: translateX(16px); background: #fff; }
</style>
