<script setup lang="ts">
// 设置弹窗 · 扩展与自动化分区「用户脚本」入口卡（导航收口：打开独立 UserscriptsDialog）
import './ui/settings-shared.css'
defineEmits<{ (e: 'open'): void }>()
</script>

<template>
  <div class="card" data-anchor="userscripts">
    <div class="card-title"><AppIcon name="scroll" :size="14" /> 用户脚本</div>
    <div class="field">
      <span class="field-desc">
        Tampermonkey 风格的用户脚本管理：安装、启停、删除随页面注入的 JS 脚本。
      </span>
      <button class="mini-btn primary" @click="$emit('open')">打开用户脚本管理</button>
    </div>
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>
