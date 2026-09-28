<script setup lang="ts">
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import SettingCard from './ui/SettingCard.vue'
import SettingSwitch from './ui/SettingSwitch.vue'
import HintBlock from './ui/HintBlock.vue'
import './ui/settings-shared.css'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()
</script>

<template>
  <div class="section-stack">
    <SettingCard title="外观" icon="palette">
      <div class="field">
        <label id="theme-label">主题</label>
        <div class="theme-picker" role="radiogroup" aria-labelledby="theme-label">
          <button
            type="button"
            class="theme-card"
            :class="{ active: (settingsStore.get('theme') || 'dark') === 'dark' }"
            role="radio"
            :aria-checked="(settingsStore.get('theme') || 'dark') === 'dark'"
            @click="onSet('theme', 'dark')"
          >
            <span class="theme-swatch dark-swatch" aria-hidden="true"></span>
            <span>深色</span>
          </button>
          <button
            type="button"
            class="theme-card"
            :class="{ active: (settingsStore.get('theme') || 'dark') === 'light' }"
            role="radio"
            :aria-checked="(settingsStore.get('theme') || 'dark') === 'light'"
            @click="onSet('theme', 'light')"
          >
            <span class="theme-swatch light-swatch" aria-hidden="true"></span>
            <span>浅色</span>
          </button>
        </div>
      </div>
      <div class="field">
        <label for="bookmarks-bar">显示书签栏</label>
        <SettingSwitch
          label="显示书签栏"
          :checked="settingsStore.get('show_bookmarks_bar') !== 'false'"
          @change="onSet('show_bookmarks_bar', $event ? 'true' : 'false')"
        />
      </div>
    </SettingCard>
    <!-- IA 重组（2026-09）：DMA-BUF 开关已移至「高级 → 兼容性」CompatCard.vue -->
    <SettingCard title="启动页" icon="house" anchor="newtab">
      <div class="field">
        <label for="show-bg">动态背景</label>
        <span class="field-desc">Bing 每日高清壁纸 + 缓慢缩放动画</span>
        <SettingSwitch
          label="动态背景"
          :checked="settingsStore.get('show_bg') !== 'false'"
          @change="onSet('show_bg', $event ? 'true' : 'false')"
        />
      </div>
      <div class="field">
        <label for="font-size">字体大小</label>
        <select
          id="font-size"
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
      <div class="field">
        <label for="theme-color">主题色</label>
        <div class="color-row">
          <input
            id="theme-color"
            type="color"
            class="color-pick"
            aria-label="主题色"
            :value="settingsStore.get('theme_color') || '#4a9eff'"
            @change="onSet('theme_color', ($event.target as HTMLInputElement).value)"
          />
          <button class="mini-btn" @click="onSet('theme_color', '')">恢复默认</button>
        </div>
      </div>
      <HintBlock>启动页强调色；留空使用默认蓝。</HintBlock>
      <div class="field">
        <label for="wallpaper">自定义壁纸 URL</label>
        <input
          id="wallpaper"
          type="text"
          :value="settingsStore.get('wallpaper')"
          placeholder="留空 = Bing 每日高清图"
          spellcheck="false"
          @change="onSet('wallpaper', ($event.target as HTMLInputElement).value.trim())"
        />
      </div>
      <div class="field">
        <label for="show-lunar">显示农历</label>
        <span class="field-desc">启动页时钟下方显示农历日期</span>
        <SettingSwitch
          label="显示农历"
          :checked="settingsStore.get('show_lunar') !== 'false'"
          @change="onSet('show_lunar', $event ? 'true' : 'false')"
        />
      </div>
      <div class="field">
        <label for="show-quote">显示每日一句</label>
        <span class="field-desc">启动页展示一句英文金句</span>
        <SettingSwitch
          label="显示每日一句"
          :checked="settingsStore.get('show_quote') !== 'false'"
          @change="onSet('show_quote', $event ? 'true' : 'false')"
        />
      </div>
      <div class="field">
        <label for="show-weather">显示天气</label>
        <span class="field-desc">启动页展示天气卡片</span>
        <SettingSwitch
          label="显示天气"
          :checked="settingsStore.get('show_weather') !== 'false'"
          @change="onSet('show_weather', $event ? 'true' : 'false')"
        />
      </div>
      <div class="field">
        <label for="weather-city">天气城市</label>
        <input
          id="weather-city"
          type="text"
          :value="settingsStore.get('weather_city')"
          placeholder="留空 = 自动定位"
          spellcheck="false"
          @change="onSet('weather_city', ($event.target as HTMLInputElement).value.trim())"
        />
      </div>
      <HintBlock>
        以上启动页偏好保存后<strong>重新打开启动页生效</strong>；登录本地账号后，这些偏好随账号保存并以账号设置为准。
      </HintBlock>
    </SettingCard>
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>
