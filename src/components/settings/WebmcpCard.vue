<script setup lang="ts">
// 设置弹窗 · AI 助手分区「试验特性」卡（IA 重组：原 ApiSection 的 WebMCP polyfill）
import { onMounted, ref } from 'vue'
import { errText, invokeOk } from '../../utils/invoke'
import { useToastStore } from '../../stores/toast'
import SettingSwitch from './ui/SettingSwitch.vue'
import './ui/settings-shared.css'

const webmcpPolyfill = ref(false)
async function loadWebmcpPolyfill() {
  try {
    webmcpPolyfill.value = await invokeOk<boolean>('get_webmcp_polyfill')
  } catch (e) {
    console.error('[settings] 读取 WebMCP polyfill 开关失败:', e)
  }
}
async function onToggleWebmcpPolyfill() {
  const next = !webmcpPolyfill.value
  webmcpPolyfill.value = next
  try {
    await invokeOk('set_webmcp_polyfill', { enabled: next })
    useToastStore().success(next ? 'WebMCP polyfill 已开启（刷新页面后生效）' : 'WebMCP polyfill 已关闭')
  } catch (e) {
    webmcpPolyfill.value = !next
    console.error('[settings] WebMCP polyfill 切换失败:', e)
    useToastStore().error('WebMCP polyfill 切换失败：' + errText(e))
  }
}

onMounted(loadWebmcpPolyfill)
</script>

<template>
  <div class="card" data-anchor="webmcp">
    <div class="card-title">
      <AppIcon name="sparkles" :size="14" /> 试验特性
    </div>
    <div class="field" :class="{ 'experiment-tag': webmcpPolyfill }">
      <label>
        WebMCP polyfill <span class="exp-badge" v-if="webmcpPolyfill">试验中</span>
      </label>
      <SettingSwitch
        :checked="webmcpPolyfill"
        label="WebMCP polyfill"
        @change="() => onToggleWebmcpPolyfill()"
      />
      <div class="kw-hint webmcp-hint">
        试验特性（阶段 1.5）· 默认关。开启后向页面注入
        <code>navigator.modelContext.registerTool</code> polyfill，捕获网站声明的 WebMCP
        工具，让 AI 助手能感知「网站为智能体暴露了哪些工具」。仅捕获声明、不伪造执行
        能力，试验完不留痕，可随时关闭。
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>
