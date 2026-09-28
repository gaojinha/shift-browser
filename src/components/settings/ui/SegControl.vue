<script setup lang="ts">
// 设置页公共分段选择：单选按钮组（样式见 settings-shared.css）
export interface SegOption {
  value: string
  label: string
}
defineProps<{
  modelValue: string
  options: SegOption[]
  label: string
}>()
const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()
</script>

<template>
  <div class="seg-group" role="radiogroup" :aria-label="label">
    <button
      v-for="o in options"
      :key="o.value"
      type="button"
      class="seg-btn"
      :class="{ active: modelValue === o.value }"
      role="radio"
      :aria-checked="modelValue === o.value"
      @click="emit('update:modelValue', o.value)"
    >
      {{ o.label }}
    </button>
  </div>
</template>
