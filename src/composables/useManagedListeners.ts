// Tauri 事件监听的生命周期托管（R1-5 · 2026-09-25）
//
// 背景：App.vue 历史上 12 处 listen(...) 都只 catch 注册失败、不保留 unlisten，
// 组件卸载（HMR/测试重挂载/未来多窗口）后监听器残留 → 事件被旧实例重复处理。
// 本 composable 统一收集 unlisten，在所属 effect 作用域停止（组件卸载）时全量解绑；
// 注册是异步的，若 await 期间作用域已销毁，立即解绑，杜绝"晚到的注册"泄漏。
import { onScopeDispose, getCurrentScope } from 'vue'
import { listen, type Event, type UnlistenFn } from '@tauri-apps/api/event'

export type EventHandler<T> = (event: Event<T>) => void

export function useManagedListeners() {
  const unlisteners: UnlistenFn[] = []
  let disposed = false

  // 无活动作用域（组件 setup / effectScope）时 onScopeDispose 无法注册清理
  if (!getCurrentScope()) {
    console.warn('[useManagedListeners] 必须在 setup/effectScope 期间调用，否则无法自动解绑')
  }

  /** 注册一个监听；返回的 Promise 仅用于"必须确保注册完成"的场景（如锁屏事件先注后等） */
  async function add<T>(event: string, handler: EventHandler<T>): Promise<void> {
    try {
      const un = await listen<T>(event, handler)
      if (disposed) {
        un()
      } else {
        unlisteners.push(un)
      }
    } catch {
      // listen 注册失败（IPC 桥异常）与历史行为一致：事件不可用不阻塞应用启动
    }
  }

  // onScopeDispose：组件卸载（setup 作用域销毁）与测试中的 effectScope.stop() 都会触发
  onScopeDispose(() => {
    disposed = true
    for (const un of unlisteners.splice(0)) un()
  })

  return { add }
}
