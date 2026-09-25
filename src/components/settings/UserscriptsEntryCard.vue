<script setup lang="ts">
// 设置弹窗 · 扩展与自动化分区「用户脚本」入口卡（导航收口：打开独立 UserscriptsDialog）
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
.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 4px; }.field-desc { flex: 1; font-size: 12px; color: var(--text-secondary); }.mini-btn { background: var(--bg-active); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 7px; padding: 5px 12px; font-size: 12px; font-weight: 500; cursor: pointer; transition: all 0.15s; white-space: nowrap; }.mini-btn:hover { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }.mini-btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }.mini-btn.primary:hover { background: var(--accent-2); color: #fff; border-color: var(--accent-2); }
</style>
