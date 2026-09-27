// 二次点击确认（删除类不可恢复操作统一用，跟 SpaceSwitcher 行内二次点击同模式）
// 第一次点进入待确认态（按钮变"确认删除"），第二次真删；超时无操作自动撤防。
// 不用弹窗式确认：避免再叠一层 overlay/z-index，也不依赖 window.confirm（WebKitGTK 下无显示）。
import { ref } from 'vue'

export function useConfirmDelete(timeoutMs = 3000) {
  const pendingId = ref<number | string | null>(null)
  // globalThis 兼容浏览器与单测 node 环境（window 在 node 下不存在）
  let timer: ReturnType<typeof setTimeout> | null = null

  function disarm() {
    pendingId.value = null
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }

  /**
   * 尝试删除：已在待确认态返回 true（调用方执行删除），否则进入待确认返回 false。
   * 不同 id 之间切换时旧的自动撤防。
   */
  function arm(id: number | string): boolean {
    if (pendingId.value === id) {
      disarm()
      return true
    }
    disarm()
    pendingId.value = id
    timer = setTimeout(disarm, timeoutMs)
    return false
  }

  function isPending(id: number | string): boolean {
    return pendingId.value === id
  }

  return { pendingId, isPending, arm, disarm }
}
