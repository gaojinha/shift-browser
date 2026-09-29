<script setup lang="ts">
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { useToastStore } from '../../stores/toast'
import SettingSwitch from './ui/SettingSwitch.vue'
import './ui/settings-shared.css'

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
                <SettingSwitch
                  :checked="settingsStore.get('api_allow_eval') === 'true'"
                  label="允许 API 执行 JS（危险）"
                  @change="(v) => onSet('api_allow_eval', v ? 'true' : 'false')"
                />
              </div>
              <div class="field">
                <label>API 只读模式</label>
                <span class="field-desc">
                  开启后拒绝一切写命令（打开页面/填表/点击/导航变更等全部拒绝），只允许读取类命令；适合只做监控/采集的调用方。即时生效
                </span>
                <SettingSwitch
                  :checked="settingsStore.get('api_readonly') === 'true'"
                  label="API 只读模式"
                  @change="(v) => onSet('api_readonly', v ? 'true' : 'false')"
                />
              </div>
              <div class="field">
                <label>允许 API 页面交互（危险）</label>
                <span class="field-desc">
                  允许 click / fill / scroll / dismiss 等页面交互命令；默认关。即使开启，写类动作在只读模式下仍被拒绝。即时生效
                </span>
                <SettingSwitch
                  :checked="settingsStore.get('api_agent_actions') === 'true'"
                  label="允许 API 页面交互（危险）"
                  @change="(v) => onSet('api_agent_actions', v ? 'true' : 'false')"
                />
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
                <SettingSwitch
                  :checked="settingsStore.get('api_bind_lan') === 'true'"
                  label="开放 API 绑局域网（危险）"
                  @change="(v) => onSet('api_bind_lan', v ? 'true' : 'false')"
                />
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
                <SettingSwitch
                  :checked="settingsStore.get('api_model_gateway') === 'true'"
                  label="允许模型网关（消耗额度）"
                  @change="(v) => onSet('api_model_gateway', v ? 'true' : 'false')"
                />
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
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>
