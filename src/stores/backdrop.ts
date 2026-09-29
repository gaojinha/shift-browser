// 菜单底垫：弹 HTML 菜单时原生 webview 必须隐藏，底下垫一张当前页快照，
// 免得露出内容区深蓝兜底（整屏纯蓝）。快照由 webview.ts 在隐藏瞬间抓取，
// 菜单关即撤；抓取失败静默（无底垫=老样子），绝不挡菜单。
import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useBackdropStore = defineStore('backdrop', () => {
  /** 快照 data URL，为空 = 无底垫 */
  const image = ref<string | null>(null)

  function set(url: string) {
    image.value = url
  }

  function clear() {
    image.value = null
  }

  return { image, set, clear }
})
