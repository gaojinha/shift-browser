<script setup lang="ts">
// UiSwitch：shadcn 风格开关（Reka Switch 底座 + 自家设计令牌画皮）
// 用法：<UiSwitch v-model="on" label="某某开关" />
// 行为（Reka 白送）：role=switch、aria-checked、空格/回车切换、focus 光圈走原生 :focus-visible。
// 尺寸与 settings-shared.css 旧 .switch 完全一致（38x22），视觉无缝。
import { SwitchRoot, SwitchThumb } from 'reka-ui'

defineProps<{
  label: string
}>()

const model = defineModel<boolean>({ required: true })
</script>

<template>
  <SwitchRoot v-model="model" class="ui-switch" :aria-label="label">
    <SwitchThumb class="ui-switch-thumb" />
  </SwitchRoot>
</template>

<style scoped>
.ui-switch {
  position: relative;
  display: inline-block;
  width: 38px;
  height: 22px;
  flex-shrink: 0;
  padding: 0;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: var(--bg-active);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease);
}
.ui-switch[data-state='checked'] {
  background: var(--accent);
  border-color: var(--accent);
}
.ui-switch:focus-visible {
  outline: none;
  box-shadow: 0 0 0 3px var(--accent-ring);
}
.ui-switch-thumb {
  display: block;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--text-muted);
  transform: translateX(2px);
  transition:
    transform var(--dur-fast) var(--ease),
    background var(--dur-fast) var(--ease);
}
.ui-switch[data-state='checked'] .ui-switch-thumb {
  transform: translateX(16px);
  background: var(--accent-contrast);
}
</style>
