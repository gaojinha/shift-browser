<script setup lang="ts">
// 设置页公共开关：接口不变（checked + change），内核换 UiSwitch（Reka 底座）。
// 调用方（Appearance/General）一行不用改；其余 7 处手搓 .switch 留待推广期统一收口。
import { computed } from 'vue'
import UiSwitch from '@/components/ui/UiSwitch.vue'

const props = defineProps<{
  checked: boolean
  label: string
}>()
const emit = defineEmits<{
  (e: 'change', value: boolean): void
}>()

const model = computed({
  get: () => props.checked,
  set: (v: boolean) => emit('change', v),
})
</script>

<template>
  <UiSwitch v-model="model" :label="label" />
</template>
