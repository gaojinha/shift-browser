<script setup lang="ts">
// 全局 toast 通知：右下角固定，自动消失
import { useToastStore } from '../stores/toast'

const toastStore = useToastStore()
</script>

<template>
  <!-- 读屏播报（R4-6 §3.3）：error 用 assertive，其余 polite -->
  <div class="toast-container" role="status" :aria-live="toastStore.items.some((t) => t.type === 'error') ? 'assertive' : 'polite'">
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
  z-index: var(--z-toast);
  display: flex;
  flex-direction: column;
  gap: 8px;
  pointer-events: none;
  max-width: 360px;
}
.toast-item {
  display: flex;
  align-items: flex-start;
  gap: 9px;
  padding: 11px 14px 11px 11px;
  border-radius: var(--radius-md);
  font-size: var(--text-base);
  line-height: var(--leading-base);
  background: var(--bg-elevated);
  border: 1px solid var(--hairline);
  box-shadow: var(--shadow-pop);
  pointer-events: auto;
  word-break: break-word;
  animation: toast-in var(--dur) var(--ease-out);
}
@keyframes toast-in {
  from {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}
.toast-icon {
  flex-shrink: 0;
  width: 22px;
  height: 22px;
  border-radius: var(--radius-full);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 11px;
}
.toast-item.error {
  color: var(--text-primary);
}
.toast-item.error .toast-icon {
  color: var(--danger);
  background: var(--danger-soft);
}
.toast-item.success {
  color: var(--text-primary);
}
.toast-item.success .toast-icon {
  color: var(--success);
  background: var(--success-soft);
}
.toast-item.info {
  color: var(--text-primary);
}
.toast-item.info .toast-icon {
  color: var(--accent);
  background: var(--accent-soft);
}
.toast-msg {
  flex: 1;
  padding-top: 2px;
}
.toast-enter-active,
.toast-leave-active {
  transition:
    opacity var(--dur) var(--ease),
    transform var(--dur) var(--ease);
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
  transition: transform var(--dur) var(--ease);
}
</style>
