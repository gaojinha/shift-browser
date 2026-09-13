<script setup lang="ts">
// 全局 toast 通知：右下角固定，自动消失
import { useToastStore } from '../stores/toast'

const toastStore = useToastStore()
</script>

<template>
  <div class="toast-container">
    <TransitionGroup name="toast">
      <div v-for="t in toastStore.items" :key="t.id" class="toast-item" :class="t.type">
        <span class="toast-icon">{{
          t.type === 'error' ? '⚠️' : t.type === 'success' ? '✅' : 'ℹ️'
        }}</span>
        <span class="toast-msg">{{ t.message }}</span>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-container {
  position: fixed;
  right: 16px;
  bottom: 40px;
  z-index: 9999;
  display: flex;
  flex-direction: column;
  gap: 8px;
  pointer-events: none;
  max-width: 360px;
}
.toast-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 14px;
  border-radius: 8px;
  font-size: 13px;
  line-height: 1.5;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  pointer-events: auto;
  word-break: break-word;
}
.toast-item.error {
  background: var(--bg-secondary);
  border: 1px solid var(--danger);
  color: var(--text-primary);
}
.toast-item.success {
  background: var(--bg-secondary);
  border: 1px solid var(--success);
  color: var(--text-primary);
}
.toast-item.info {
  background: var(--bg-secondary);
  border: 1px solid var(--accent);
  color: var(--text-primary);
}
.toast-icon {
  flex-shrink: 0;
}
.toast-msg {
  flex: 1;
}
.toast-enter-active,
.toast-leave-active {
  transition: all 0.25s ease;
}
.toast-enter-from {
  opacity: 0;
  transform: translateX(24px);
}
.toast-leave-to {
  opacity: 0;
  transform: translateX(24px);
}
.toast-move {
  transition: transform 0.25s ease;
}
</style>
