<script setup lang="ts">
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()
</script>

<template>
  <div class="settings-scroll">
            <div class="card">
              <div class="card-title"><AppIcon name="palette" :size="14" /> 外观</div>
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
              <div class="card-title" style="margin-top: 16px"><AppIcon name="house" :size="14" /> 启动页</div>
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
.api-actions .selfcheck-btn,
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
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'><path fill='%23c6d0df' d='M2.5 4.5L6 8l3.5-3.5'/></svg>");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 12px;
}
.field input:focus,
.field select:focus {
  border-color: var(--accent);
}
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
  background: var(--accent);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
  background: #fff;
}
</style>
