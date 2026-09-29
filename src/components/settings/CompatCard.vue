<script setup lang="ts">
// 设置弹窗 · 高级分区「兼容性」卡（IA 重组：原 AppearanceSection 的 DMA-BUF 开关）
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import './ui/settings-shared.css'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()
</script>

<template>
  <div class="card" data-anchor="compat">
    <div class="card-title"><AppIcon name="gear" :size="14" /> 兼容性</div>
    <div class="field">
      <label>开发者工具（F12）</label>
      <span class="field-desc">
        打开网页调试面板。开后<b>新开标签直接生效</b>，已开的标签关掉重开即可，免重启浏览器。
      </span>
      <label class="switch">
        <input
          type="checkbox"
          :checked="settingsStore.get('enable_devtools') === 'true'"
          @change="onSet('enable_devtools', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
        />
        <span class="slider"></span>
      </label>
    </div>
    <div class="field">
      <label>Chrome 指纹补齐</label>
      <span class="field-desc">
        补上 WebKit 缺的 <b>userAgentData / deviceMemory / scheduler</b>（值照桌面 Chrome），验证墙类站点更易通过。关后回原生，改后已开标签即时生效。
      </span>
      <label class="switch">
        <input
          type="checkbox"
          :checked="settingsStore.get('compat_chrome_fp') !== 'false'"
          @change="onSet('compat_chrome_fp', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
        />
        <span class="slider"></span>
      </label>
    </div>
    <div class="field">
      <label>禁用 DMA-BUF 渲染</label>
      <span class="field-desc">
        可修复部分机型的视频闪烁，<b>但禁用后打开微博等重合成页面会导致浏览器崩溃</b>（2026-09-26 实测），默认关闭。<b>改后需重启生效</b>。
      </span>
      <label class="switch">
        <input
          type="checkbox"
          :checked="settingsStore.get('render_disable_dmabuf') === 'true'"
          @change="onSet('render_disable_dmabuf', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
        />
        <span class="slider"></span>
      </label>
    </div>
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>
