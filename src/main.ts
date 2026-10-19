import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import AppIcon from './components/AppIcon.vue'
import { i18n } from './i18n'
import { frontLog } from './utils/front-log'
import './styles/main.css'

// ===== 全局错误哨兵（2026-10-19）=====
// 崩溃/白屏前把前端异常与资源加载失败落进 Rust 日志，事后有据可查。
// frontLog 内部已做静默，日志桥自身失败不会再抛。

window.addEventListener(
  'error',
  (e: Event) => {
    if (e instanceof ErrorEvent) {
      frontLog(
        `[global-error] ${e.message} @ ${e.filename}:${e.lineno}:${e.colno} ${String(e.error?.stack ?? '')}`,
      )
      return
    }
    // 资源加载失败（img/script/link）走普通 Event，没有 message
    if (e.target instanceof HTMLElement) {
      const el = e.target
      const src =
        el instanceof HTMLImageElement ||
        el instanceof HTMLScriptElement ||
        el instanceof HTMLIFrameElement
          ? el.src
          : el instanceof HTMLLinkElement
            ? el.href
            : ''
      frontLog(`[resource-error] <${el.tagName.toLowerCase()}> ${src}`)
    }
  },
  true,
)

window.addEventListener('unhandledrejection', (e: PromiseRejectionEvent) => {
  const reason: unknown = e.reason
  frontLog(
    `[unhandledrejection] ${reason instanceof Error ? String(reason.stack ?? reason.message) : String(reason)}`,
  )
})

const app = createApp(App)
app.use(createPinia())
app.use(i18n)
app.component('AppIcon', AppIcon)
app.mount('#app')
