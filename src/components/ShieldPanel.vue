<script setup lang="ts">
// 护盾面板（B-04 MVP，对齐 Brave Shields 面板形态 · 2026-10-15）
// 位置：地址栏站点区（由 AddressBar 挂载展开）
// 内容：本页近似计数 · 站点降盾（兼容白名单复用）· 三项快捷开关 · 详细设置入口
// 口径说明：计数数据源为 Rust 内存统计（get_privacy_stats，重启归零）；
//          "本页" = 进入本页后的统计增量（近似，并发归属不逐一区分，UI 有提示）
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'
import { useShieldStore } from '../stores/shield'
import { useToastStore } from '../stores/toast'
import { useSettingsSet } from '../composables/useSettingsSet'
import { invokeOk } from '../utils/invoke'
import AppIcon from './AppIcon.vue'
import UiSwitch from './ui/UiSwitch.vue'

const emit = defineEmits<{
  (e: 'openPrivacy'): void
  (e: 'close'): void
}>()

const { t } = useI18n()
const tabsStore = useTabsStore()
const settingsStore = useSettingsStore()
const shieldStore = useShieldStore()
const toast = useToastStore()
const { onSet } = useSettingsSet()

const root = ref<HTMLElement | null>(null)

const host = computed(() => {
  const url = tabsStore.activeTab?.url ?? ''
  try {
    const u = new URL(url)
    return u.protocol === 'https:' || u.protocol === 'http:' ? u.hostname : ''
  } catch {
    return ''
  }
})

// 站点防护状态：true = 防护中，false = 已降盾（兼容白名单）
const shieldUp = ref(true)
const busy = ref(false)

// 三项开关（值由设置驱动；写入失败时 store 回滚，UI 自动跟随）
const adblockOn = computed(() => settingsStore.get('adblock_enabled') !== 'false')
const gpcOn = computed(() => settingsStore.get('gpc_header') !== 'false')
const shieldLevel = computed(() => {
  const l = settingsStore.get('privacy_shield_level')
  if (l === 'off' || l === 'standard' || l === 'strict') return l
  // 老值迁移兼容：与 Rust adblock.rs / privacy.rs 回退同口径
  return settingsStore.get('privacy_shield') === 'false' ? 'off' : 'standard'
})

// 本页计数（近似增量）
const delta = computed(() => shieldStore.pageDelta)

let timer: number | null = null

onMounted(async () => {
  document.addEventListener('click', onDocClick)
  await refresh()
  // 面板打开期间每 2 秒刷新计数（StatusBar 为 10s；面板场景更密）
  timer = window.setInterval(() => {
    void shieldStore.loadStats()
  }, 2000)
})

onUnmounted(() => {
  document.removeEventListener('click', onDocClick)
  if (timer !== null) window.clearInterval(timer)
})

function onDocClick(e: MouseEvent) {
  if (root.value && !root.value.contains(e.target as Node)) emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.stopPropagation()
    emit('close')
  }
}

async function refresh() {
  await shieldStore.loadStats()
  shieldStore.ensureBaseline(tabsStore.activeTab?.url ?? '')
  if (host.value) {
    shieldUp.value = !(await shieldStore.isSiteShieldOff(host.value))
  }
}

async function onToggleAdblock(v: boolean) {
  try {
    await settingsStore.set('adblock_enabled', v ? 'true' : 'false')
    await invokeOk('set_adblock_enabled', { enabled: v })
    toast.success(v ? t('shield.adblockOn') : t('shield.adblockOff'))
  } catch (e) {
    console.error('[shield] 广告拦截同步失败:', e)
    toast.error(t('shield.setFailed'))
  }
}

async function onToggleGpc(v: boolean) {
  try {
    await onSet('gpc_header', v ? 'true' : 'false')
  } catch (e) {
    console.error('[shield] GPC 开关同步失败:', e)
    toast.error(t('shield.setFailed'))
  }
}

async function onChangeLevel(e: Event) {
  const level = (e.target as HTMLSelectElement).value
  try {
    await settingsStore.set('privacy_shield_level', level)
    await invokeOk('set_privacy_shield_level', { level })
    const msg =
      level === 'off'
        ? t('shield.levelOff')
        : level === 'strict'
          ? t('shield.levelStrict')
          : t('shield.levelStandard')
    toast.success(msg)
  } catch (err) {
    console.error('[shield] 隐私防护档位同步失败:', err)
    toast.error(t('shield.setFailed'))
  }
}

async function onToggleShield() {
  if (!host.value || busy.value) return
  busy.value = true
  try {
    const nowUp = await shieldStore.toggleSiteShield(host.value)
    shieldUp.value = nowUp
    toast.success(nowUp ? t('shield.siteResumed') : t('shield.sitePaused'))
  } catch (e) {
    console.error('[shield] 降盾切换失败:', e)
    toast.error(t('shield.setFailed'))
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div
    ref="root"
    class="shield-panel"
    role="dialog"
    :aria-label="t('shield.panelLabel')"
    @keydown="onKeydown"
  >
    <!-- 头部：状态 + 站点 -->
    <div class="shield-head">
      <AppIcon name="shield" :size="14" :class="['shield-glyph', shieldUp ? 'up' : 'down']" />
      <span class="shield-title">{{ t('shield.title') }}</span>
      <span class="shield-state" :class="shieldUp ? 'up' : 'down'">
        {{ shieldUp ? t('shield.stateUp') : t('shield.stateDown') }}
      </span>
      <button class="close-btn" :aria-label="t('shield.close')" @click="emit('close')">
        <AppIcon name="x" :size="12" />
      </button>
    </div>
    <div class="shield-site" :title="host">{{ host || t('shield.internalPage') }}</div>

    <!-- 本页计数（近似口径） -->
    <div class="stat-block">
      <template v-if="shieldUp">
        <div class="stat-main">
          <span class="stat-num">{{ delta.adblock_injections }}</span>
          <span class="stat-desc">{{ t('shield.blockedOnPage') }}</span>
        </div>
        <div class="stat-sub" :title="t('shield.approxTip')">
          {{ t('shield.pageExtra', { clean: delta.cleaned_params, inject: delta.privacy_injections }) }}
        </div>
      </template>
      <div v-else class="stat-paused">{{ t('shield.pausedHint') }}</div>
    </div>

    <!-- 快捷开关 -->
    <div class="row">
      <span class="row-label">{{ t('shield.adblock') }}</span>
      <UiSwitch
        :model-value="adblockOn"
        :label="t('shield.adblock')"
        @update:model-value="onToggleAdblock"
      />
    </div>
    <div class="row">
      <label class="row-label" for="shield-level">{{ t('shield.level') }}</label>
      <select
        id="shield-level"
        class="level-select"
        :value="shieldLevel"
        @change="onChangeLevel"
      >
        <option value="off">{{ t('shield.levelOffOpt') }}</option>
        <option value="standard">{{ t('shield.levelStandardOpt') }}</option>
        <option value="strict">{{ t('shield.levelStrictOpt') }}</option>
      </select>
    </div>
    <div class="row">
      <span class="row-label">{{ t('shield.gpc') }}</span>
      <UiSwitch
        :model-value="gpcOn"
        :label="t('shield.gpc')"
        @update:model-value="onToggleGpc"
      />
    </div>

    <!-- 降盾救援 -->
    <div class="rescue">
      <button class="rescue-btn" :disabled="!host || busy" @click="onToggleShield">
        {{ shieldUp ? t('shield.pauseSite') : t('shield.resumeSite') }}
      </button>
      <p class="rescue-tip">{{ t('shield.rescueTip') }}</p>
    </div>

    <!-- 底部入口 -->
    <div class="foot">
      <button class="link-btn" @click="emit('openPrivacy')">{{ t('shield.openSettings') }}</button>
    </div>
  </div>
</template>

<style scoped>
.shield-panel {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  width: 300px;
  padding: 10px 12px 8px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-pop);
  z-index: var(--z-dropdown);
  color: var(--text-primary);
  font-size: var(--text-sm);
  text-align: left;
}
.shield-head {
  display: flex;
  align-items: center;
  gap: 6px;
}
.shield-glyph.up {
  color: var(--accent);
}
.shield-glyph.down {
  color: var(--text-faint);
}
.shield-title {
  font-weight: 600;
  font-size: var(--text-md);
}
.shield-state {
  font-size: var(--text-xs);
  padding: 1px 6px;
  border-radius: var(--radius-full);
  border: 1px solid var(--border-subtle);
}
.shield-state.up {
  color: var(--success);
  border-color: var(--success-soft);
  background: var(--success-soft);
}
.shield-state.down {
  color: var(--text-muted);
  background: var(--bg-hover);
}
.close-btn {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: var(--radius-xs);
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
}
.close-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.close-btn:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.shield-site {
  margin: 2px 0 8px;
  font-size: var(--text-xs);
  color: var(--text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.stat-block {
  padding: 8px 10px;
  border-radius: var(--radius-md);
  background: var(--bg-hover);
  margin-bottom: 8px;
}
.stat-main {
  display: flex;
  align-items: baseline;
  gap: 6px;
}
.stat-num {
  font-size: var(--text-xl);
  font-weight: 700;
  color: var(--accent);
}
.stat-desc {
  color: var(--text-secondary);
}
.stat-sub {
  margin-top: 2px;
  font-size: var(--text-xs);
  color: var(--text-muted);
}
.stat-paused {
  font-size: var(--text-sm);
  color: var(--warning);
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 5px 2px;
}
.row-label {
  color: var(--text-secondary);
}
.level-select {
  background: var(--bg-secondary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-xs);
  padding: 2px 6px;
  font-size: var(--text-sm);
}
.level-select:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.rescue {
  margin-top: 6px;
  padding-top: 8px;
  border-top: 1px solid var(--border-subtle);
}
.rescue-btn {
  width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  font-size: var(--text-sm);
}
.rescue-btn:hover:not(:disabled) {
  background: var(--bg-hover);
}
.rescue-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.rescue-btn:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.rescue-tip {
  margin: 6px 0 0;
  font-size: var(--text-xs);
  color: var(--text-muted);
  line-height: var(--leading-tight);
}
.foot {
  margin-top: 8px;
  padding-top: 6px;
  border-top: 1px solid var(--border-subtle);
}
.link-btn {
  border: none;
  background: transparent;
  color: var(--accent);
  cursor: pointer;
  padding: 2px 0;
  font-size: var(--text-sm);
}
.link-btn:hover {
  color: var(--accent-hover);
}
.link-btn:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
</style>
