<script setup lang="ts">
// 设置页公共卡片：标题图标 + 锚点 + 统一样式（样式见 settings-shared.css）
defineProps<{
  title: string
  icon?: string
  anchor?: string
}>()
</script>

<template>
  <div class="card" :data-anchor="anchor">
    <div class="card-title">
      <AppIcon v-if="icon" :name="icon" :size="14" />
      {{ title }}
      <span class="card-title-extra"><slot name="extra" /></span>
    </div>
    <slot />
  </div>
</template>

<style scoped>
.card-title-extra {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
</style>
