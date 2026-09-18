<script setup lang="ts">
// 设置弹窗（通用 / 外观 / AI 助手 / 隐私 / 扩展）
// 设置项存 settings 表（key-value），保存即生效
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useSettingsStore } from '../stores/settings'
import { useExtensionsStore } from '../stores/extensions'
import { useToastStore } from '../stores/toast'
import type { Extension } from '../types/extension'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const settingsStore = useSettingsStore()
const extensionsStore = useExtensionsStore()

// 侧边导航：当前激活区块
const activeSection = ref('general')
const SECTIONS = [
  { id: 'general', icon: '🏠', label: '通用' },
  { id: 'appearance', icon: '🎨', label: '外观' },
  { id: 'ai', icon: '🤖', label: 'AI 助手' },
  { id: 'extensions', icon: '🧩', label: '扩展' },
  { id: 'privacy', icon: '🛡', label: '隐私' },
]

// AI Key 输入框：不回显掩码（get_all_settings 返回前4后4掩码），
// 留空 = 保持原值；输入新值才保存。hasAiKey 判断是否已配置（掩码非空即已配置）
const aiKeyInput = ref('')
const hasAiKey = () => {
  const v = settingsStore.get('ai_api_key')
  return v !== '' && v !== undefined
}
async function onAiKeyChange() {
  const v = aiKeyInput.value.trim()
  if (v) {
    await settingsStore.set('ai_api_key', v)
  }
  aiKeyInput.value = ''
}

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
// 竞态防护：seq 令牌丢弃过期回调；加载失败也必须在 finally 里恢复/隐藏 webview
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        if (!settingsStore.loaded) {
          settingsStore.loadSettings().catch((e) => {
            console.error('[settings] 加载设置失败:', e)
            useToastStore().error('加载设置失败')
          })
        }
        extensionsStore.loadExtensions().catch((e) => {
          console.error('[settings] 加载扩展失败:', e)
          useToastStore().error('加载扩展失败')
        })
        loadKeywords().catch((e) => {
          console.error('[settings] 加载搜索引擎失败:', e)
          useToastStore().error('加载搜索引擎失败')
        })
        // 加噪白名单（隐私分区用）
        loadWhitelist()
        // 打开时清空 AI Key 输入框：不回显掩码，避免用户无意把掩码写回库
        aiKeyInput.value = ''
      }
    } catch (e) {
      console.error('[settings] 加载失败:', e)
      useToastStore().error('加载设置失败')
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'settings-dialog')
      }
    }
  },
)

const SEARCH_ENGINES = [
  { id: 'bing', label: '必应', url: 'https://www.bing.com/search?q=' },
  { id: 'baidu', label: '百度', url: 'https://www.baidu.com/s?wd=' },
  { id: 'google', label: '谷歌', url: 'https://www.google.com/search?q=' },
]

const clearedMsg = ref('')

// 扩展管理：加载路径输入
const extPath = ref('')
const extError = ref('')
const extLoading = ref(false)

async function onSet(key: string, value: string) {
  await settingsStore.set(key, value)
  // 书签栏显隐切换需同步通知 Rust 侧重算 top_offset
  if (key === 'show_bookmarks_bar') {
    try {
      await invoke('set_bookmarks_bar_visible', { visible: value === 'true' })
    } catch (e) {
      console.error('[settings] 书签栏显隐同步失败:', e)
      useToastStore().error('书签栏切换失败')
    }
  }
  // 启动页相关设置：保存后重新生成新标签页 HTML
  if (key === 'show_bg' || key === 'font_size' || key === 'search_engine') {
    try {
      await invoke('refresh_newtab')
    } catch (e) {
      console.error('[settings] 启动页刷新失败:', e)
    }
  }
}

async function onClearHistory() {
  await settingsStore.clearHistory()
  clearedMsg.value = '历史记录已清除'
  setTimeout(() => (clearedMsg.value = ''), 2000)
}

/** 加载扩展（目录或 manifest.json 路径） */
async function onLoadExtension() {
  const path = extPath.value.trim()
  if (!path) {
    extError.value = '请输入扩展目录或 manifest.json 路径'
    return
  }
  extLoading.value = true
  extError.value = ''
  try {
    await extensionsStore.loadExtension(path)
    extPath.value = ''
  } catch (e) {
    extError.value = String(e)
  } finally {
    extLoading.value = false
  }
}

/** 切换启用状态 */
async function onToggleExtension(e: Extension) {
  try {
    await extensionsStore.setEnabled(e.id, !e.enabled)
  } catch (err) {
    console.error('[settings] 切换扩展状态失败:', err)
    useToastStore().error('切换扩展状态失败')
  }
}

/** 删除扩展 */
async function onDeleteExtension(id: number) {
  try {
    await extensionsStore.deleteExtension(id)
  } catch (err) {
    console.error('[settings] 删除扩展失败:', err)
    useToastStore().error('删除扩展失败')
  }
}

/** 隐私防护档位（off / standard / strict），默认标准档 */
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
    await invoke('set_privacy_shield_level', { level })
    const msg =
      level === 'off' ? '隐私防护已关闭' : level === 'strict' ? '已切换到严格档' : '已切换到标准档'
    useToastStore().success(msg)
  } catch (err) {
    console.error('[settings] 隐私防护档位同步失败:', err)
    useToastStore().error('隐私防护设置失败')
  }
}

/** 加噪白名单：这些站点不做加噪（Canvas 敏感站点可加进来），域名按行分隔 */
const whitelistText = ref('')
async function loadWhitelist() {
  try {
    const list = await invoke<string[]>('get_privacy_whitelist')
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
    await invoke('set_privacy_whitelist', { domains })
    const list = await invoke<string[]>('get_privacy_whitelist')
    whitelistText.value = list.join('\n')
    useToastStore().success(`白名单已保存（${list.length} 个站点）`)
  } catch (e) {
    console.error('[settings] 保存加噪白名单失败:', e)
    useToastStore().error('白名单保存失败')
  }
}

/** 广告拦截开关：settings 表是唯一状态源，set_adblock_enabled 只负责把开关同步给注入器 */
async function onToggleAdblock() {
  const current = settingsStore.get('adblock_enabled') !== 'false'
  const next = (!current).toString()
  await settingsStore.set('adblock_enabled', next)
  try {
    await invoke('set_adblock_enabled', { enabled: next === 'true' })
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
    customKeywords.value = await invoke<string[]>('get_adblock_keywords')
  } catch {
    customKeywords.value = []
  }
}

async function saveKeywords(list: string[]) {
  const prev = customKeywords.value
  customKeywords.value = list
  try {
    await invoke('set_adblock_keywords', { keywords: list })
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
</script>

<template>
  <Transition name="fade">
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
    <div class="dialog settings-dialog">
      <div class="dialog-header">
        <span>⚙ 设置</span>
        <button class="close-btn" aria-label="关闭" @click="emit('close')">✕</button>
      </div>
      <div class="dialog-body settings-body">
        <!-- 左侧导航 -->
        <div class="settings-nav">
          <div
            v-for="s in SECTIONS"
            :key="s.id"
            class="nav-item"
            :class="{ active: activeSection === s.id }"
            @click="activeSection = s.id"
          >
            <span class="nav-icon">{{ s.icon }}</span>
            <span class="nav-label">{{ s.label }}</span>
          </div>
        </div>

        <!-- 右侧内容 -->
        <div class="settings-content">
          <!-- 通用 -->
          <div v-if="activeSection === 'general'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🖥 通用</div>
              <div class="field">
                <label>主页 URL</label>
                <input
                  type="text"
                  :value="settingsStore.get('homepage')"
                  placeholder="newtab 或 https://..."
                  spellcheck="false"
                  @change="onSet('homepage', ($event.target as HTMLInputElement).value.trim())"
                />
              </div>
              <div class="field">
                <label>搜索引擎</label>
                <div class="seg-group">
                  <button
                    v-for="e in SEARCH_ENGINES"
                    :key="e.id"
                    class="seg-btn"
                    :class="{ active: (settingsStore.get('search_engine') || 'bing') === e.id }"
                    @click="onSet('search_engine', e.id)"
                  >
                    {{ e.label }}
                  </button>
                </div>
              </div>
            </div>
          </div>

          <!-- 外观 -->
          <div v-if="activeSection === 'appearance'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🎨 外观</div>
              <div class="field">
                <label>主题</label>
                <div class="theme-picker">
                  <button
                    class="theme-card"
                    :class="{ active: (settingsStore.get('theme') || 'dark') === 'dark' }"
                    @click="onSet('theme', 'dark')"
                  >
                    <span class="theme-swatch dark-swatch"></span>
                    <span>深色</span>
                  </button>
                  <button
                    class="theme-card"
                    :class="{ active: (settingsStore.get('theme') || 'dark') === 'light' }"
                    @click="onSet('theme', 'light')"
                  >
                    <span class="theme-swatch light-swatch"></span>
                    <span>浅色</span>
                  </button>
                </div>
              </div>
              <div class="field">
                <label>显示书签栏</label>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('show_bookmarks_bar') !== 'false'"
                    @change="onSet('show_bookmarks_bar', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="card-title" style="margin-top: 16px">🏠 启动页</div>
              <div class="field">
                <label>动态背景</label>
                <span class="field-desc">Bing 每日高清壁纸 + 缓慢缩放动画</span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('show_bg') !== 'false'"
                    @change="onSet('show_bg', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>字体大小</label>
                <select
                  :value="settingsStore.get('font_size') || '14'"
                  @change="onSet('font_size', ($event.target as HTMLSelectElement).value)"
                >
                  <option value="12">12px（小）</option>
                  <option value="13">13px</option>
                  <option value="14">14px（默认）</option>
                  <option value="15">15px</option>
                  <option value="16">16px（大）</option>
                  <option value="18">18px</option>
                  <option value="20">20px（特大）</option>
                </select>
              </div>
            </div>
          </div>

          <!-- AI 助手 -->
          <div v-if="activeSection === 'ai'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🤖 AI 助手</div>
              <div class="field">
                <label>模型</label>
                <select
                  :value="settingsStore.get('ai_model')"
                  @change="onSet('ai_model', ($event.target as HTMLSelectElement).value)"
                >
                  <option value="agnes-2.0-flash">agnes-2.0-flash（推荐）</option>
                  <option value="agnes-2.5-pro">agnes-2.5-pro</option>
                  <option value="agnes-2.5-flash">agnes-2.5-flash（可能超时）</option>
                </select>
              </div>
              <div class="field">
                <label>API 地址</label>
                <input
                  type="text"
                  :value="settingsStore.get('ai_base_url')"
                  placeholder="https://apihub.agnes-ai.com/v1"
                  spellcheck="false"
                  @change="onSet('ai_base_url', ($event.target as HTMLInputElement).value.trim())"
                />
              </div>
              <div class="field">
                <label>API Key</label>
                <input
                  type="password"
                  v-model="aiKeyInput"
                  :placeholder="hasAiKey() ? '已配置（不回显），留空保持不变' : '留空使用环境变量 AGNES_API_KEY'"
                  spellcheck="false"
                  @change="onAiKeyChange"
                />
              </div>
            </div>
          </div>

          <!-- 扩展 -->
          <div v-if="activeSection === 'extensions'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🧩 扩展</div>
              <div class="ext-load-row">
                <input
                  v-model="extPath"
                  type="text"
                  placeholder="扩展目录或 manifest.json 路径"
                  spellcheck="false"
                  @keyup.enter="onLoadExtension"
                />
                <button class="mini-btn primary" :disabled="extLoading" @click="onLoadExtension">
                  {{ extLoading ? '加载中…' : '加载' }}
                </button>
              </div>
              <div v-if="extError" class="ext-error">{{ extError }}</div>
              <div class="ext-list" v-if="extensionsStore.list.length > 0">
                <div v-for="e in extensionsStore.list" :key="e.id" class="ext-item">
                  <div class="ext-info">
                    <span class="ext-name">{{ e.name }}</span>
                    <span class="ext-desc">
                      {{ e.version ? 'v' + e.version : '' }}
                      {{ e.description ? '· ' + e.description : '' }}
                    </span>
                  </div>
                  <div class="ext-actions">
                    <span v-if="e.js_count" class="ext-badge">JS</span>
                    <span v-if="e.css_count" class="ext-badge">CSS</span>
                    <label class="switch">
                      <input type="checkbox" :checked="e.enabled" @change="onToggleExtension(e)" />
                      <span class="slider"></span>
                    </label>
                    <button class="mini-btn danger" @click="onDeleteExtension(e.id)">删除</button>
                  </div>
                </div>
              </div>
              <div v-else class="ext-empty">还没有扩展，输入路径加载一个试试</div>
            </div>
          </div>

          <!-- 隐私 -->
          <div v-if="activeSection === 'privacy'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🛡 隐私</div>
              <div class="field">
                <label>隐私防护</label>
                <select class="privacy-select" :value="privacyLevel" @change="onChangePrivacyLevel">
                  <option value="standard">标准（推荐）</option>
                  <option value="strict">严格</option>
                  <option value="off">关闭</option>
                </select>
              </div>
              <div class="kw-hint">
                <b>标准</b>：Canvas / WebGL / 音频 / WebGPU 反指纹加噪，GPC 隐私信号，WebRTC 内网 IP 防护，
                字体与设备探针收敛，追踪参数清洗，第三方 Cookie 阻断。<br />
                <b>严格</b>：在标准之上再移除 WebUSB / 蓝牙 / 串口 / HID / Gamepad / 传感器等高危 API，
                并归一化语音音色、降级高精度计时（部分网站功能可能受影响）。<br />
                切换后刷新页面即对已开标签生效。
              </div>
              <div class="kw-block" v-if="privacyLevel !== 'off'">
                <div class="kw-hint">
                  <b>加噪白名单</b>：下列站点<strong>不做加噪</strong>（Canvas 敏感的在线地图、
                  设计工具、视频剪辑等可加进来）。每行一个域名，子域名自动包含。
                </div>
                <textarea
                  class="kw-textarea"
                  v-model="whitelistText"
                  rows="4"
                  placeholder="ditu.amap.com&#10;www.figma.com"
                  spellcheck="false"
                ></textarea>
                <button class="mini-btn primary" @click="saveWhitelist">保存白名单</button>
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
              <div class="field">
                <label>记录浏览历史</label>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('record_history') !== 'false'"
                    @change="onSet('record_history', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>启动锁屏（每次启动输入账号密码解锁）</label>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('lock_enabled') === 'true'"
                    @change="onSet('lock_enabled', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field danger-field">
                <label>历史记录</label>
                <div class="history-actions">
                  <button class="danger-btn" @click="onClearHistory">清除历史数据</button>
                  <span v-if="clearedMsg" class="cleared-msg">{{ clearedMsg }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
.settings-dialog {
  width: min(620px, calc(100vw - 32px));
  height: min(600px, calc(100vh - 120px));
  display: flex;
  flex-direction: column;
}
.settings-body {
  display: flex;
  gap: 0;
  padding: 0;
  overflow: hidden;
  flex: 1;
  min-height: 0;
}

/* 左侧导航 */
.settings-nav {
  width: 132px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-subtle);
  padding: 10px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: var(--bg-secondary);
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 7px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition:
    background 0.15s,
    color 0.15s;
  user-select: none;
}
.nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.nav-item.active {
  background: rgba(74, 158, 255, 0.15);
  color: var(--accent);
  font-weight: 600;
}
.nav-icon {
  font-size: 15px;
  width: 18px;
  text-align: center;
  flex-shrink: 0;
}

/* 右侧内容 */
.settings-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
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

/* 卡片 */
.card {
  margin-bottom: 12px;
}
.card-title {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 600;
  margin-bottom: 10px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border-subtle);
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
  color: var(--text-muted);
}
.field label {
  font-size: 13px;
  color: var(--text-secondary);
  flex-shrink: 0;
}
.field input,
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
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'><path fill='%23999' d='M2.5 4.5L6 8l3.5-3.5'/></svg>");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 12px;
}
.field input:focus,
.field select:focus {
  border-color: var(--accent);
}

/* 搜索引擎分段按钮 */
.seg-group {
  display: flex;
  gap: 6px;
  flex: 1;
  max-width: 300px;
}
.seg-btn {
  flex: 1;
  padding: 6px 0;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-secondary);
  border-radius: 7px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}
.seg-btn:hover {
  background: var(--bg-hover);
}
.seg-btn.active {
  background: rgba(74, 158, 255, 0.15);
  border-color: var(--accent);
  color: var(--accent);
  font-weight: 600;
}

/* 主题选择卡片 */
.theme-picker {
  display: flex;
  gap: 8px;
  flex: 1;
  max-width: 300px;
}
.theme-card {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 10px 0 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-secondary);
  border-radius: 8px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.theme-card:hover {
  background: var(--bg-hover);
}
.theme-card.active {
  border-color: var(--accent);
  color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent);
}
.theme-swatch {
  width: 44px;
  height: 26px;
  border-radius: 5px;
  border: 1px solid var(--border-color);
}
.dark-swatch {
  background: linear-gradient(135deg, #16213e 50%, #1e2a45 50%);
}
.light-swatch {
  background: linear-gradient(135deg, #f5f6f8 50%, #e8eaee 50%);
}

/* 开关 */
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
.slider::before {
  content: '';
  position: absolute;
  width: 14px;
  height: 14px;
  left: 3px;
  top: 3px;
  background: var(--text-muted);
  border-radius: 50%;
  transition: 0.2s;
}
.switch input:checked + .slider {
  background: rgba(74, 158, 255, 0.45);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
  background: var(--accent);
}

/* 危险操作 */
.history-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.danger-field {
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px dashed var(--border-subtle);
}
.danger-btn {
  background: rgba(229, 57, 53, 0.12);
  color: #ff6b6b;
  border: 1px solid rgba(229, 57, 53, 0.35);
  border-radius: 7px;
  padding: 6px 14px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.danger-btn:hover {
  background: rgba(229, 57, 53, 0.22);
}
.cleared-msg {
  font-size: 12px;
  color: #43a047;
}

/* 扩展 */
.ext-load-row {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}
.ext-load-row input {
  flex: 1;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 7px 11px;
  font-size: 13px;
  outline: none;
  -webkit-appearance: none;
  appearance: none;
}
.ext-load-row input:focus {
  border-color: var(--accent);
}
.ext-error {
  color: #ff6b6b;
  font-size: 12px;
  margin-bottom: 6px;
}
.ext-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ext-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  padding: 8px 10px;
  gap: 8px;
  transition: border-color 0.15s;
}
.ext-item:hover {
  border-color: var(--border-color);
}
.ext-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.ext-name {
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
}
.ext-desc {
  color: var(--text-muted);
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ext-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.ext-badge {
  font-size: 10px;
  color: var(--accent);
  background: rgba(74, 158, 255, 0.15);
  padding: 2px 6px;
  border-radius: 4px;
}
.ext-empty {
  color: var(--text-muted);
  font-size: 12px;
  text-align: center;
  padding: 14px 0;
}
.mini-btn {
  background: var(--bg-active);
  color: var(--text-secondary);
  border: none;
  border-radius: 6px;
  padding: 5px 11px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.mini-btn:hover {
  background: var(--text-secondary);
  color: var(--bg-primary);
}
.mini-btn.primary {
  background: rgba(74, 158, 255, 0.18);
  color: #cfe4ff;
}
.mini-btn.primary:hover {
  background: rgba(74, 158, 255, 0.3);
}
.mini-btn.danger {
  color: #ff8a8a;
  background: rgba(229, 57, 53, 0.1);
}
.mini-btn.danger:hover {
  background: rgba(229, 57, 53, 0.25);
}

/* 关键词 */
.kw-block {
  margin: 4px 0 12px 0;
  padding: 10px 12px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
}
.kw-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 8px;
  line-height: 1.4;
}
.kw-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}
.kw-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: rgba(74, 158, 255, 0.15);
  color: #cfe4ff;
  border-radius: 12px;
  padding: 3px 8px;
  font-size: 12px;
}
.kw-x {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 11px;
  padding: 0;
  line-height: 1;
}
.kw-x:hover {
  color: #ff8a8a;
}
.kw-add {
  display: flex;
  gap: 8px;
}
.kw-add input {
  flex: 1;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 6px 10px;
  font-size: 13px;
  outline: none;
  -webkit-appearance: none;
  appearance: none;
}
.kw-add input:focus {
  border-color: var(--accent);
}
.kw-textarea {
  width: 100%;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 6px 10px;
  font-size: 12px;
  line-height: 1.5;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  outline: none;
  resize: vertical;
  margin-bottom: 8px;
  box-sizing: border-box;
}
.kw-textarea:focus {
  border-color: var(--accent);
}
</style>
