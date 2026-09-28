<script setup lang="ts">
// 设置页公共提示块：info / warn / danger / success 四档（样式见 settings-shared.css）
withDefaults(
  defineProps<{
    tone?: 'info' | 'warn' | 'danger' | 'success'
  }>(),
  { tone: 'info' },
)
</script>

<template>
  <div
    class="hint-block"
    :class="tone === 'info' ? '' : tone"
    role="note"
  >
    <slot />
  </div>
</template>
