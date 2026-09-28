<script setup lang="ts">
// 设置页公共开关：无障碍 label + focus 光圈 + 统一样式
defineProps<{
  checked: boolean
  label: string
}>()
const emit = defineEmits<{
  (e: 'change', value: boolean): void
}>()
function onChange(e: Event) {
  emit('change', (e.target as HTMLInputElement).checked)
}
</script>

<template>
  <label class="switch">
    <span class="sr-only">{{ label }}</span>
    <input type="checkbox" :checked="checked" :aria-label="label" @change="onChange" />
    <span class="slider"></span>
  </label>
</template>
