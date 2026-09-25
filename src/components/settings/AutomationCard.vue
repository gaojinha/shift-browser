<script setup lang="ts">
// 设置弹窗 · 扩展与自动化分区「定时任务」卡（IA 重组：原 GeneralSection 三块）
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()
</script>

<template>
  <div class="card" data-anchor="automation">
    <div class="card-title"><AppIcon name="clock" :size="14" /> 定时任务</div>
    <div class="field">
      <label>定时清理历史</label>
      <input
        class="task-num"
        type="number"
        min="0"
        :value="settingsStore.get('auto_clear_history_days') || '0'"
        @change="onSet('auto_clear_history_days', ($event.target as HTMLInputElement).value)"
      />
      <span class="field-desc">天（0=关闭；每天自动删更早的历史）</span>
    </div>
    <div class="field">
      <label>定时刷新订阅</label>
      <input
        class="task-num"
        type="number"
        min="0"
        :value="settingsStore.get('auto_refresh_feeds_minutes') || '0'"
        @change="onSet('auto_refresh_feeds_minutes', ($event.target as HTMLInputElement).value)"
      />
      <span class="field-desc">分钟（0=关闭）</span>
    </div>
    <div class="field">
      <label>定时跑 Agent</label>
      <input
        class="task-num"
        type="number"
        min="0"
        :value="settingsStore.get('auto_agent_minutes') || '0'"
        @change="onSet('auto_agent_minutes', ($event.target as HTMLInputElement).value)"
      />
      <span class="field-desc">分钟（0=关闭；到点打开 AI 侧栏并发送下面的提示词）</span>
    </div>
    <div class="kw-block">
      <div class="kw-hint">
        <b>Agent 定时提示词</b>（阶段3 编排）：到点自动发给 AI；AI 的多步动作仍走原有确认流程。
      </div>
      <textarea
        class="net-rules"
        :value="settingsStore.get('auto_agent_prompt')"
        placeholder="例如：看看当前页面讲了啥，总结三条要点"
        spellcheck="false"
        rows="3"
        @change="onSet('auto_agent_prompt', ($event.target as HTMLTextAreaElement).value)"
      ></textarea>
    </div>
  </div>
</template>

<style scoped>
.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; }.field-desc { flex: 1; font-size: 12px; color: var(--text-secondary); }.field label { font-size: 13px; color: var(--text-secondary); flex-shrink: 0; }.task-num { width: 72px; height: 28px; padding: 0 8px; border: 1px solid var(--border-color); border-radius: 6px; background: var(--bg-primary); color: var(--text-primary); font-size: 12px; outline: none; }.task-num:focus { border-color: var(--accent); }.kw-block { margin: 4px 0 12px 0; padding: 10px 12px; background: var(--bg-primary); border: 1px solid var(--border-subtle); border-radius: 8px; }.kw-hint { font-size: 11px; color: var(--text-muted); margin-bottom: 8px; line-height: 1.4; }.net-rules { width: 100%; box-sizing: border-box; min-height: 64px; padding: 8px; border: 1px solid var(--border-color); border-radius: 6px; background: var(--bg-secondary); color: var(--text-primary); font-size: 12px; font-family: monospace; resize: vertical; outline: none; }.net-rules:focus { border-color: var(--accent); }
</style>
