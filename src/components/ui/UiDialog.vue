<script setup lang="ts">
// UiDialog：shadcn 风格弹窗（Reka UI 底座 + 自家设计令牌画皮）
// 用法：<UiDialog v-model:open="show" title="标题" description="说明">
//         正文… <template #footer>按钮…</template>
//       </UiDialog>
// 行为（Reka 白送）：Esc 关闭、点遮罩关闭、焦点陷阱、关闭后焦点归位。
// 样式语言与 App.vue 全局 .dialog 对齐：scrim+blur 遮罩、bg-elevated 面板、radius-lg、shadow-dialog。
import {
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogOverlay,
  DialogPortal,
  DialogRoot,
  DialogTitle,
} from 'reka-ui'
import { cn } from '@/lib/utils'

withDefaults(
  defineProps<{
    title: string
    description?: string
    /** 面板宽度，默认 min(400px, 100vw-48px) */
    width?: string
  }>(),
  { description: '', width: '' },
)

const open = defineModel<boolean>('open', { required: true })
</script>

<template>
  <DialogRoot v-model:open="open">
    <DialogPortal>
      <DialogOverlay class="ui-dialog-overlay" />
      <DialogContent
        :class="cn('ui-dialog')"
        :style="width ? { width } : undefined"
        @open-auto-focus.prevent
      >
        <div class="ui-dialog-header">
          <DialogTitle class="ui-dialog-title">{{ title }}</DialogTitle>
          <DialogClose class="close-btn" aria-label="关闭">×</DialogClose>
        </div>
        <DialogDescription v-if="description" class="ui-dialog-desc">
          {{ description }}
        </DialogDescription>
        <div class="ui-dialog-body">
          <slot />
        </div>
        <div v-if="$slots.footer" class="ui-dialog-footer">
          <slot name="footer" />
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>

<style scoped>
.ui-dialog-overlay {
  position: fixed;
  inset: 0;
  background: var(--scrim);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  z-index: var(--z-overlay);
}
.ui-dialog-overlay[data-state='open'] {
  animation: ui-fade-in var(--dur) var(--ease);
}
.ui-dialog {
  position: fixed;
  left: 50%;
  top: 50%;
  transform: translate(-50%, -50%);
  width: min(400px, calc(100vw - 48px));
  max-height: 80vh;
  overflow-y: auto;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-dialog);
  z-index: var(--z-overlay);
  padding: 16px 18px;
}
.ui-dialog[data-state='open'] {
  animation: ui-zoom-in var(--dur) var(--ease-out);
}
.ui-dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.ui-dialog-title {
  margin: 0;
  color: var(--text-primary);
  font-size: var(--text-md);
  font-weight: 600;
  letter-spacing: 0.01em;
}
.ui-dialog-desc {
  margin: 0 0 10px;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  line-height: 1.6;
}
.ui-dialog-body {
  color: var(--text-primary);
  font-size: var(--text-base);
  line-height: 1.6;
}
.ui-dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 14px;
}
@keyframes ui-fade-in {
  from {
    opacity: 0;
  }
}
@keyframes ui-zoom-in {
  from {
    opacity: 0;
    transform: translate(-50%, -48%) scale(0.97);
  }
}
</style>
