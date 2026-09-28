<script setup lang="ts">
// 设置弹窗 · 隐私防护分区（R2-1d 拆分：原 SettingsDialog.vue 内联节抽出）
// 浏览防护（档位/反指纹/白名单/统计/自测）+ 网络与追踪（GPC/广告拦截/拦截规则/请求头/GPC 检测/外联审计）
import { computed, onMounted, ref } from 'vue'
import { invokeOk } from '../../utils/invoke'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { usePrivacyAudit } from '../../composables/usePrivacyAudit'
import { useToastStore } from '../../stores/toast'
import PrivacyDataCard from './PrivacyDataCard.vue'
import './ui/settings-shared.css'

const emit = defineEmits<{ (e: 'openCerts'): void }>()

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

const privacyLevel = computed(() => {
  const l = settingsStore.get('privacy_shield_level')
  if (l === 'off' || l === 'standard' || l === 'strict') return l
  // 兼容旧布尔开关
  return settingsStore.get('privacy_shield') === 'false' ? 'off' : 'standard'
})

async function onChangePrivacyLevel(e: Event) {
  const level = (e.target as HTMLSelectElement).value
  await settingsStore.set('privacy_shield_level', level)
  try {
    await invokeOk('set_privacy_shield_level', { level })
    const msg =
      level === 'off' ? '隐私防护已关闭' : level === 'strict' ? '已切换到严格档' : '已切换到标准档'
    useToastStore().success(msg)
  } catch (err) {
    console.error('[settings] 隐私防护档位同步失败:', err)
    useToastStore().error('隐私防护设置失败')
  }
}

/** 反指纹模式（阶段 2 反指纹 2.0）：noise 加噪（默认）/ uniform 统一化 */
const fpMode = computed(() => {
  const m = settingsStore.get('privacy_fp_mode')
  return m === 'uniform' ? 'uniform' : 'noise'
})
async function onChangeFpMode(e: Event) {
  const mode = (e.target as HTMLSelectElement).value
  await settingsStore.set('privacy_fp_mode', mode)
  try {
    await invokeOk('set_privacy_fp_mode', { mode })
    useToastStore().success(
      mode === 'uniform' ? '已切换到统一化（Tor/Mullvad 派）' : '已切换到加噪（推荐）'
    )
  } catch (err) {
    console.error('[settings] 反指纹模式同步失败:', err)
    useToastStore().error('反指纹模式设置失败')
  }
}

/** 兼容白名单：这些站点完全不装指纹防护（风控/Canvas 敏感站点），域名按行分隔 */
const whitelistText = ref('')
async function loadWhitelist() {
  try {
    const list = await invokeOk<string[]>('get_privacy_whitelist')
    whitelistText.value = list.join('\n')
  } catch (e) {
    console.error('[settings] 读取加噪白名单失败:', e)
  }
}
async function saveWhitelist() {
  const domains = whitelistText.value
    .split(/[\n,，\s]+/)
    .map((s) => s.trim())
    .filter(Boolean)
  try {
    await invokeOk('set_privacy_whitelist', { domains })
    const list = await invokeOk<string[]>('get_privacy_whitelist')
    whitelistText.value = list.join('\n')
    useToastStore().success(`白名单已保存（${list.length} 个站点）`)
  } catch (e) {
    console.error('[settings] 保存加噪白名单失败:', e)
    useToastStore().error('白名单保存失败')
  }
}

/** 隐私防护统计（本次运行期间的内存计数，重启归零） */
const { privacyStats, loadPrivacyStats, resetPrivacyStats, privacySelfTest, privacySelfTestRunning, runPrivacySelfTest, gpcSite, gpcChecking, checkGpcSite, egressReport, egressRunning, runEgressAudit } = usePrivacyAudit()
async function onToggleAdblock() {
  const current = settingsStore.get('adblock_enabled') !== 'false'
  const next = (!current).toString()
  await settingsStore.set('adblock_enabled', next)
  try {
    await invokeOk('set_adblock_enabled', { enabled: next === 'true' })
  } catch (e) {
    console.error('[settings] 广告拦截同步失败:', e)
    useToastStore().error('广告拦截切换失败')
  }
}

/** 用户自定义广告关键词 */
const customKeywords = ref<string[]>([])
const newKeyword = ref('')
const kwMsg = ref('')

async function loadKeywords() {
  try {
    customKeywords.value = await invokeOk<string[]>('get_adblock_keywords')
  } catch {
    customKeywords.value = []
  }
}

async function saveKeywords(list: string[]) {
  const prev = customKeywords.value
  customKeywords.value = list
  try {
    await invokeOk('set_adblock_keywords', { keywords: list })
    kwMsg.value = '已保存，刷新页面后生效'
    setTimeout(() => (kwMsg.value = ''), 2500)
  } catch (e) {
    console.error('[settings] 保存关键词失败:', e)
    // 回滚，避免 UI 与数据库不一致
    customKeywords.value = prev
    useToastStore().error('保存关键词失败')
  }
}

async function onAddKeyword() {
  const k = newKeyword.value.trim()
  if (!k) return
  if (customKeywords.value.includes(k)) {
    kwMsg.value = '这个词已经加过了'
    setTimeout(() => (kwMsg.value = ''), 2000)
    return
  }
  newKeyword.value = ''
  await saveKeywords([...customKeywords.value, k])
}

async function onRemoveKeyword(k: string) {
  await saveKeywords(customKeywords.value.filter((x) => x !== k))
}

onMounted(() => {
  loadWhitelist()
  loadPrivacyStats()
  loadKeywords()
})
</script>

<template>
  <div class="section-stack">
    <!-- ① 浏览防护 -->
    <div class="card" data-anchor="shield">
      <div class="card-title"><AppIcon name="shield" :size="14" /> 浏览防护</div>
      <div class="field">
        <label>隐私防护</label>
        <select class="privacy-select" :value="privacyLevel" @change="onChangePrivacyLevel">
          <option value="standard">标准（推荐）</option>
          <option value="strict">严格</option>
          <option value="off">关闭</option>
        </select>
      </div>
      <div class="privacy-desc">
        <b>标准</b>：Canvas / WebGL / 音频 / WebGPU 反指纹加噪，GPC 隐私信号，WebRTC 内网 IP 防护，
        字体与设备探针收敛，追踪参数清洗，第三方 Cookie 阻断。<br />
        <b>严格</b>：再移除 WebUSB / 蓝牙 / 串口 / HID / Gamepad / 传感器等高危 API，归一化语音音色、
        降级高精度计时（部分网站功能可能受影响）。<br />
        切换后刷新页面即对已开标签生效。
      </div>
      <div class="field">
        <label>反指纹模式</label>
        <select class="privacy-select" :value="fpMode" @change="onChangeFpMode">
          <option value="noise">加噪（默认）</option>
          <option value="uniform">统一化（Tor/Mullvad 派）</option>
        </select>
      </div>
      <div class="privacy-desc">
        <b>加噪</b>：Canvas / WebGL / 音频加「同源稳定、跨源不同」的确定性噪声——网站仍能正常用，
        但你的指纹不再稳定可追踪。<br />
        <b>统一化</b>：返回<strong>固定值</strong>，所有使用者呈现一致指纹（更强匿名，但个别依赖
        画布/音频的应用可能受影响）。<br />
        切换后刷新页面即生效。
      </div>
      <div class="kw-block" v-if="privacyLevel !== 'off'">
        <div class="kw-hint">
          <b>兼容白名单</b>：下列站点<strong>完全不装指纹防护</strong>，整页保持浏览器原生环境
          （过不了 Cloudflare / Turnstile 等安全检查的站点，如 upwork.com，以及在线地图、设计工具、
          视频剪辑等加这里）。每行一个域名，子域名自动包含；第三方验证 iframe 会自动豁免，无需添加。
        </div>
        <textarea
          class="kw-textarea"
          v-model="whitelistText"
          rows="4"
          placeholder="upwork.com&#10;ditu.amap.com&#10;www.figma.com"
          spellcheck="false"
        ></textarea>
        <button class="mini-btn primary" @click="saveWhitelist">保存白名单</button>
      </div>
      <div class="stat-row">
        <span class="stat-text">
          <b>防护统计</b>（本次运行）：清洗追踪参数 <b>{{ privacyStats?.cleaned_params ?? 0 }}</b> 次 ·
          隐私注入 <b>{{ privacyStats?.privacy_injections ?? 0 }}</b> 次 ·
          广告拦截 <b>{{ privacyStats?.adblock_injections ?? 0 }}</b> 次
        </span>
        <button class="mini-btn" @click="resetPrivacyStats">重置</button>
      </div>
      <div class="kw-block" v-if="privacyLevel !== 'off'">
        <div class="kw-hint">
          <b>防护自测</b>：在当前标签页核对各项防护是否<strong>真的生效</strong>（对齐「信任需要证明」）。
          需先打开一个网页。
        </div>
        <div class="btn-row">
          <button
            class="mini-btn primary"
            :disabled="privacySelfTestRunning"
            @click="runPrivacySelfTest"
          >
            {{ privacySelfTestRunning ? '自测中…' : '运行自测' }}
          </button>
        </div>
        <div v-if="privacySelfTest" class="test-summary">
          档位 <b>{{ privacySelfTest.shield_level }}</b> · 反指纹
          <b>{{ privacySelfTest.fp_mode === 'uniform' ? '统一化' : '加噪' }}</b> · 站点
          <b>{{ privacySelfTest.hostname || '(未知)' }}</b> · 通过
          <b>{{ privacySelfTest.passed }}/{{ privacySelfTest.total }}</b>
          <span v-if="privacySelfTest.whitelisted">（白名单站点，兼容模式：防护整体跳过）</span>
        </div>
        <div v-if="privacySelfTest" class="ps-list">
          <div
            v-for="c in privacySelfTest.checks"
            :key="c.id"
            class="ps-item"
            :class="c.status"
          >
            <span class="ps-icon">{{
              c.status === 'pass' ? '✅' : c.status === 'fail' ? '❌' : '➖'
            }}</span>
            <span class="ps-label">{{ c.label }}</span>
            <span class="ps-detail">{{ c.detail }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- ② 网络与追踪 -->
    <div class="card" data-anchor="tracking">
      <div class="card-title"><AppIcon name="globe" :size="14" /> 网络与追踪</div>
      <div class="field">
        <label>GPC 请求头（Sec-GPC: 1）</label>
        <span class="field-desc">W3C Global Privacy Control：网络层告诉站点「别卖我的数据」</span>
        <label class="switch">
          <input
            type="checkbox"
            :checked="settingsStore.get('gpc_header') !== 'false'"
            @change="onSet('gpc_header', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
          />
          <span class="slider"></span>
        </label>
      </div>
      <div class="field">
        <label>广告拦截</label>
        <label class="switch">
          <input
            type="checkbox"
            :checked="settingsStore.get('adblock_enabled') !== 'false'"
            @change="onToggleAdblock"
          />
          <span class="slider"></span>
        </label>
      </div>
      <div class="kw-block" v-if="settingsStore.get('adblock_enabled') !== 'false'">
        <div class="kw-hint">
          内置已含「关闭广告 / 头条报 / 推广」等。下面可手工加词，刷新页面后生效。
        </div>
        <div class="kw-list" v-if="customKeywords.length > 0">
          <span v-for="k in customKeywords" :key="k" class="kw-chip">
            {{ k }}
            <button class="kw-x" title="删除" @click="onRemoveKeyword(k)">✕</button>
          </span>
        </div>
        <div class="kw-add">
          <input
            v-model="newKeyword"
            type="text"
            placeholder="输入关键词，回车添加"
            spellcheck="false"
            @keyup.enter="onAddKeyword"
          />
          <button class="mini-btn primary" @click="onAddKeyword">添加</button>
        </div>
        <div v-if="kwMsg" class="cleared-msg">{{ kwMsg }}</div>
      </div>
      <div class="kw-block">
        <div class="kw-hint">
          <b>网络层拦截规则</b>（阶段3.2）：每行一个 URL 子串，命中即<strong>取消该请求</strong>
          （请求出网前拦，比内容层更彻底）。保存后约 <strong>2 秒自动生效</strong>（热更新）。
        </div>
        <textarea
          class="net-rules"
          :value="settingsStore.get('net_block_rules')"
          placeholder="每行一个 URL 子串，例如：doubleclick.net"
          spellcheck="false"
          rows="4"
          @change="onSet('net_block_rules', ($event.target as HTMLTextAreaElement).value)"
        ></textarea>
      </div>
      <div class="kw-block">
        <div class="kw-hint">
          <b>自定义请求头</b>（阶段3.2）：每行 <code>Name: Value</code>，注入到每个出网请求
          （如 <code>DNT: 1</code>、自定义 UA）。保存后约 <strong>2 秒自动生效</strong>。
          <br />注：<code>Sec-GPC: 1</code> 已默认常开，无需在此重复。
        </div>
        <textarea
          class="net-rules"
          :value="settingsStore.get('net_headers')"
          placeholder="每行 Name: Value，例如：DNT: 1"
          spellcheck="false"
          rows="4"
          @change="onSet('net_headers', ($event.target as HTMLTextAreaElement).value)"
        ></textarea>
      </div>
      <div class="kw-block" v-if="privacyLevel !== 'off'">
        <div class="kw-hint">
          <b>站点 GPC 声明</b>：抓取本站的 <code>/.well-known/gpc.json</code>，看站点是否声明遵守
          GPC（需先打开网页）。
        </div>
        <div class="btn-row">
          <button
            class="mini-btn primary"
            :disabled="gpcChecking"
            @click="checkGpcSite"
          >
            {{ gpcChecking ? '检测中…' : '检测本站声明' }}
          </button>
        </div>
        <div v-if="gpcSite" class="test-summary">
          <b>{{ gpcSite.origin }}</b> · {{ gpcSite.summary }}
          <span v-if="gpcSite.last_update">（更新于 {{ gpcSite.last_update }}）</span>
        </div>
      </div>
      <div class="kw-block" v-if="privacyLevel !== 'off'">
        <div class="kw-hint">
          <b>外联审计</b>：列出本应用及其子进程当前的全部网络出口连接（只读 <code>/proc</code>，
          零出网）——「可验证无外联」。
        </div>
        <div class="btn-row">
          <button
            class="mini-btn primary"
            :disabled="egressRunning"
            @click="runEgressAudit"
          >
            {{ egressRunning ? '审计中…' : '运行审计' }}
          </button>
        </div>
        <div v-if="egressReport" class="test-summary">
          进程 <b>{{ egressReport.processes }}</b> · 连接
          <b>{{ egressReport.connections.length }}</b> · 外网
          <b>{{ egressReport.external }}</b> · 本机回环
          <b>{{ egressReport.loopback }}</b> · 去重端点
          <b>{{ egressReport.distinct_remotes }}</b>
        </div>
        <div v-if="egressReport && egressReport.connections.length > 0" class="ps-list">
          <div
            v-for="(c, i) in egressReport.connections"
            :key="i"
            class="ps-item"
            :class="{ fail: c.remote_ip.indexOf('127.') !== 0 && c.remote_ip !== '::1' }"
          >
            <span class="ps-icon">
              <AppIcon
                :name="c.remote_ip.indexOf('127.') === 0 || c.remote_ip === '::1' ? 'house' : 'globe'"
                :size="12"
              />
            </span>
            <span class="ps-label">{{ c.remote_ip }}:{{ c.remote_port }}</span>
            <span class="ps-detail">{{ c.proto }} · {{ c.state }} · pid {{ c.pid }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- ③ 身份与传输 / ④ 历史与清理（IA 重组自原 DataSection） -->
    <PrivacyDataCard @open-certs="emit('openCerts')" />
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>
