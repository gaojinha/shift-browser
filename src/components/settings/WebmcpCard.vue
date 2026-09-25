<script setup lang="ts">
// 设置弹窗 · AI 助手分区「试验特性」卡（IA 重组：原 ApiSection 的 WebMCP polyfill）
import { onMounted, ref } from 'vue'
import { errText, invokeOk } from '../../utils/invoke'
import { useToastStore } from '../../stores/toast'

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
      <label class="switch">
        <input
          type="checkbox"
          :checked="webmcpPolyfill"
          @change="onToggleWebmcpPolyfill"
        />
        <span class="slider"></span>
      </label>
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
.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; flex-wrap: wrap; }.field label { font-size: 13px; color: var(--text-secondary); flex-shrink: 0; }.switch { position: relative; display: inline-block; width: 36px; height: 20px; flex-shrink: 0; }.switch input { opacity: 0; width: 0; height: 0; }.slider { position: absolute; cursor: pointer; inset: 0; background: var(--bg-active); border-radius: 10px; transition: 0.2s; }.slider::before { content: ''; position: absolute; width: 14px; height: 14px; left: 3px; top: 3px; background: var(--text-muted); border-radius: 50%; transition: 0.2s; }.switch input:checked + .slider { background: var(--accent); }.switch input:checked + .slider::before { transform: translateX(16px); background: #fff; }.exp-badge { display: inline-block; margin-left: 6px; padding: 1px 7px; font-size: 10px; font-weight: 600; color: #ffb454; background: rgba(255, 180, 84, 0.14); border: 1px solid rgba(255, 180, 84, 0.35); border-radius: 999px; vertical-align: 1px; }.webmcp-hint { width: 100%; }.kw-hint { font-size: 11px; color: var(--text-muted); margin-bottom: 8px; line-height: 1.4; }.kw-hint code { color: var(--accent); font-family: var(--font-mono, monospace); font-size: 11px; }
</style>
