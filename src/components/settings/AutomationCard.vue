<script setup lang="ts">
// 设置弹窗 · 扩展与自动化分区「定时任务」卡（IA 重组：原 GeneralSection 三块）
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import './ui/settings-shared.css'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

/** 三模板：点一下填进提示词框，用户改时间改字就行 */
const TPLS: Record<string, string> = {
  digest: '把今天订阅里最重要的3条资讯各用一句话说清，注明来源。',
  summary: '看看当前页面讲了啥，总结三条要点，每条一句话。',
  remind: '提醒我站起来活动一下，并说一句今天的日期和星期。',
}
function fillTpl(k: string) {
  onSet('auto_agent_prompt', TPLS[k] ?? '')
}
</script>

<template>
  <div class="card" data-anchor="automation">
    <div class="card-title"><AppIcon name="clock" :size="14" /> 定时任务</div>
    <div class="field">
      <label for="auto-clear-history-days">定时清理历史</label>
      <input
        id="auto-clear-history-days"
        class="task-num"
        type="number"
        min="0"
        :value="settingsStore.get('auto_clear_history_days') || '0'"
        @change="onSet('auto_clear_history_days', ($event.target as HTMLInputElement).value)"
      />
      <span class="field-desc">天（0=关闭；每天自动删更早的历史）</span>
    </div>
    <div class="field">
      <label for="auto-refresh-feeds">定时刷新订阅</label>
      <input
        id="auto-refresh-feeds"
        class="task-num"
        type="number"
        min="0"
        :value="settingsStore.get('auto_refresh_feeds_minutes') || '0'"
        @change="onSet('auto_refresh_feeds_minutes', ($event.target as HTMLInputElement).value)"
      />
      <span class="field-desc">分钟（0=关闭）</span>
    </div>
    <div class="field">
      <label for="auto-agent-minutes">定时跑 Agent</label>
      <input
        id="auto-agent-minutes"
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
        不会写就点模板改字，不用从零写。
      </div>
      <div class="tpl-row">
        <button class="mini-btn" @click="fillTpl('digest')">资讯早报模板</button>
        <button class="mini-btn" @click="fillTpl('summary')">页面总结模板</button>
        <button class="mini-btn" @click="fillTpl('remind')">到点提醒模板</button>
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
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>
