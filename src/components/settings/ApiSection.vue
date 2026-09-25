<script setup lang="ts">
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { useToastStore } from '../../stores/toast'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

// IA 重组（2026-09）：WebMCP polyfill 已移至「AI 助手 → 试验特性」WebmcpCard.vue

// 开放 API：一次性门票打开状态面板 / token 热轮换
const dashboardOpening = ref(false)
const rotatingToken = ref(false)
const rotatedToken = ref('')

async function openDashboard() {
  if (dashboardOpening.value) return
  dashboardOpening.value = true
  try {
    const port = settingsStore.get('api_http_port') || '19701'
    const ticket = await invoke<string>('create_dashboard_ticket')
    await openUrl(`http://127.0.0.1:${port}/?ticket=${encodeURIComponent(ticket)}`)
    useToastStore().success('状态面板已在浏览器打开（门票 5 分钟内一次有效）')
  } catch (e) {
    console.error('[settings] 打开状态面板失败:', e)
    useToastStore().error('打开状态面板失败')
  } finally {
    dashboardOpening.value = false
  }
}

async function rotateApiToken() {
  if (rotatingToken.value) return
  if (
    !window.confirm('轮换后旧 token 立即失效，正在调用 API 的程序需换成新 token。确定轮换？')
  ) {
    return
  }
  rotatingToken.value = true
  try {
    rotatedToken.value = await invoke<string>('rotate_api_token')
    useToastStore().success('API token 已轮换，请立即复制保存')
  } catch (e) {
    console.error('[settings] 轮换 API token 失败:', e)
    useToastStore().error('轮换 API token 失败')
  } finally {
    rotatingToken.value = false
  }
}

async function copyRotatedToken() {
  try {
    await navigator.clipboard.writeText(rotatedToken.value)
    useToastStore().success('新 token 已复制')
  } catch (e) {
    console.error('[settings] 复制 token 失败:', e)
    useToastStore().error('复制失败，请手动选择文本复制')
  }
}

</script>

<template>
  <div class="section-stack">

            <!-- ⑤ 开放 API（高级） -->
            <div class="card" data-anchor="api">
              <div class="card-title"><AppIcon name="plug" :size="14" /> 开放 API（高级）</div>
              <div class="field">
                <label>允许 API 执行 JS（危险）</label>
                <span class="field-desc">
                  开放 API 的 eval 命令；默认关。开启后本机脚本/持 token 者可执行任意 JS
                </span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('api_allow_eval') === 'true'"
                    @change="onSet('api_allow_eval', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>API 只读模式</label>
                <span class="field-desc">
                  开启后拒绝一切写命令（打开页面/填表/点击/导航变更等全部拒绝），只允许读取类命令；适合只做监控/采集的调用方。即时生效
                </span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('api_readonly') === 'true'"
                    @change="onSet('api_readonly', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>允许 API 页面交互（危险）</label>
                <span class="field-desc">
                  允许 click / fill / scroll / dismiss 等页面交互命令；默认关。即使开启，写类动作在只读模式下仍被拒绝。即时生效
                </span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('api_agent_actions') === 'true'"
                    @change="onSet('api_agent_actions', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>开放 API 端口</label>
                <input
                  class="task-num"
                  type="number"
                  min="1024"
                  max="65535"
                  :value="settingsStore.get('api_http_port') || '19701'"
                  @change="onSet('api_http_port', ($event.target as HTMLInputElement).value)"
                />
                <span class="field-desc">默认 19701；改完重启生效</span>
              </div>
              <div class="field">
                <label>开放 API 绑局域网（危险）</label>
                <span class="field-desc">
                  默认只绑 127.0.0.1；开启后局域网可达（务必保管 token）；改完重启生效
                </span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('api_bind_lan') === 'true'"
                    @change="onSet('api_bind_lan', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>API IP 白名单</label>
                <input
                  class="task-num api-allowlist-input"
                  type="text"
                  placeholder="192.168.1.0/24, 10.0.0.5"
                  :value="settingsStore.get('api_allowed_ips')"
                  @change="onSet('api_allowed_ips', ($event.target as HTMLInputElement).value)"
                />
                <span class="field-desc">
                  绑局域网时必填；逗号分隔精确 IP 或 CIDR（如 192.168.1.0/24）；改完重启生效
                </span>
              </div>
              <div class="field">
                <label>允许模型网关（消耗额度）</label>
                <span class="field-desc">
                  POST /v1/chat/completions → 转发到 AI provider，其它程序可用同一模型（耗你的额度）
                </span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('api_model_gateway') === 'true'"
                    @change="onSet('api_model_gateway', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>状态面板与 token</label>
                <div class="api-actions">
                  <button
                    class="selfcheck-btn"
                    :disabled="dashboardOpening"
                    @click="openDashboard"
                  >
                    {{ dashboardOpening ? '打开中…' : '在浏览器打开状态面板' }}
                  </button>
                  <button
                    class="selfcheck-btn"
                    :disabled="rotatingToken"
                    @click="rotateApiToken"
                  >
                    {{ rotatingToken ? '轮换中…' : '轮换 API token' }}
                  </button>
                </div>
                <span class="field-desc">
                  打开面板走一次性门票（5 分钟有效、用后即焚），token 不会出现在网址里；
                  轮换后旧 token 立即失效，使用 API 的程序需更新
                </span>
                <div v-if="rotatedToken" class="api-token-result">
                  <code>{{ rotatedToken }}</code>
                  <button class="selfcheck-btn" @click="copyRotatedToken">复制新 token</button>
                </div>
              </div>
              <!-- IA 重组：WebMCP polyfill 已移至「AI 助手 → 试验特性」 -->
            </div>
  </div>
</template>

<style scoped>
.settings-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 14px 16px;
}
.settings-scroll::-webkit-scrollbar {
  width: 6px;
}
.settings-scroll::-webkit-scrollbar-thumb {
  background: var(--bg-active);
  border-radius: 3px;
}
.card {
  margin-bottom: 12px;
}
.selfcheck-btn {
  float: right;
  padding: 3px 10px;
  font-size: 12px;
  color: var(--accent);
  background: transparent;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  cursor: pointer;
}
.selfcheck-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.api-allowlist-input {
  width: 100%;
  box-sizing: border-box;
}
.api-actions {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  margin: 6px 0 2px;
}
.api-token-result .selfcheck-btn {
  float: none;
  flex-shrink: 0;
  padding: 5px 12px;
}
.api-token-result {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
}
.api-token-result code {
  flex: 1;
  min-width: 0;
  padding: 6px 8px;
  font-size: 12px;
  background: var(--bg-secondary, #f5f5f5);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  user-select: all;
}
.card-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 14px;
  color: var(--text-primary);
  font-weight: 600;
  margin-bottom: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-subtle);
}
.card-title .app-icon {
  color: var(--accent);
}
.field {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
.field-desc {
  flex: 1;
  font-size: 12px;
  color: var(--text-secondary);
}
.field label {
  font-size: 13px;
  color: var(--text-secondary);
  flex-shrink: 0;
}
.field select {
  flex: 1;
  max-width: 300px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 7px 11px;
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s;
  /* WebKitGTK 下 select/input 默认用系统白色渲染，强制自定义样式 */
  -webkit-appearance: none;
  appearance: none;
}
.field select {
  padding-right: 26px;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'><path fill='%23c6d0df' d='M2.5 4.5L6 8l3.5-3.5'/></svg>");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 12px;
}
.field select:focus {
  border-color: var(--accent);
}
.switch {
  position: relative;
  display: inline-block;
  width: 36px;
  height: 20px;
  flex-shrink: 0;
}
.switch input {
  opacity: 0;
  width: 0;
  height: 0;
}
.slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background: var(--bg-active);
  border-radius: 10px;
  transition: 0.2s;
}
.switch input:checked + .slider {
  background: var(--accent);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
  background: #fff;
}
</style>
