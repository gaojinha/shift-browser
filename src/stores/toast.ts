// 轻量 toast 通知（右下角弹出，3 秒自动消失）
// 用于统一提示操作失败/成功，替代各处 .catch(() => {}) 静默吞错
import { defineStore } from 'pinia'

export type ToastType = 'error' | 'success' | 'info'

export interface ToastItem {
  id: number
  type: ToastType
  message: string
}

interface ToastState {
  items: ToastItem[]
}

let nextId = 1

/** 上限 3 条：超了挤掉最早的；同文重复不堆积（连点不刷屏） */
const MAX_ITEMS = 3

export const useToastStore = defineStore('toast', {
  state: (): ToastState => ({
    items: [],
  }),
  actions: {
    show(message: string, type: ToastType = 'info', duration = 3000) {
      if (this.items.some((t) => t.type === type && t.message === message)) return
      const id = nextId++
      this.items.push({ id, type, message })
      while (this.items.length > MAX_ITEMS) this.items.shift()
      setTimeout(() => {
        this.items = this.items.filter((t) => t.id !== id)
      }, duration)
    },
    error(message: string) {
      this.show(message, 'error', 4000)
    },
    success(message: string) {
      this.show(message, 'success', 2500)
    },
  },
})
