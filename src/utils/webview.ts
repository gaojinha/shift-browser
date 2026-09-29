// webview 显隐控制（P1-9 + 第一期止血：全局引用计数）
// 弹窗/右键菜单/地址栏下拉等 HTML 覆盖层需要隐藏 GTK 层 webview（否则盖不住）。
// 多弹窗并发时旧方案各管各的：A开→隐藏，B开→隐藏，A关→显示，会把 B 正在用的 webview
// 误放出来导致死锁。这里改全局引用计数：所有隐藏请求按来源登记，只有全部关闭才真正显示。
// 后端仍用单调递增 seq 令牌丢弃乱序指令。
import { invokeOk } from './invoke'
import { useBackdropStore } from '../stores/backdrop'

let seq = 0
/** 当前隐藏网页的覆盖层来源集合，为空 = 网页可见 */
const hiders = new Set<string>()
/** 底垫抓取令牌：菜单关得比快照快时，晚到的快照直接丢掉，不盖新页面 */
let backdropToken = 0

/**
 * 设置所有标签页 webview 显隐（引用计数版）
 * @param visible 目标状态（true=显示，false=隐藏）
 * @param source 调用来源标识（调试用，必填以便计数；缺省记 anonymous）
 */
export function setWebviewVisible(visible: boolean, source: string = 'anonymous') {
  seq += 1
  if (!visible) {
    const wasVisible = hiders.size === 0
    hiders.add(source)
    // 首个覆盖层：抓一张当前页快照垫底，免得露出深蓝兜底。fire-and-forget，
    // 失败静默（无底垫=老样子）；令牌防"菜单已关、快照晚到"盖错地方。
    if (wasVisible) {
      const token = ++backdropToken
      invokeOk<string>('page_backdrop_shot')
        .then((url) => {
          if (token === backdropToken && hiders.size > 0 && url) {
            useBackdropStore().set(url)
          }
        })
        .catch(() => {})
    }
    // 有意静默：显隐是高频可丢弃指令（有 seq 令牌保证最终一致），单次失败由后续指令覆盖自愈
    return invokeOk('set_webview_visible', { visible: false, seq, source }).catch(() => {})
  }
  hiders.delete(source)
  if (hiders.size > 0) {
    // 还有别的覆盖层开着，继续保持隐藏，不发显示指令
    return Promise.resolve()
  }
  // 覆盖层清零：底垫即撤，令牌作废
  backdropToken += 1
  useBackdropStore().clear()
  return invokeOk('set_webview_visible', { visible: true, seq, source }).catch(() => {})
}

/** 调试用：当前还有几个覆盖层压着网页 */
export function webviewHiderCount() {
  return hiders.size
}

/** 调试用：清空计数（测试/异常恢复用） */
export function resetWebviewHiders() {
  hiders.clear()
  backdropToken += 1
  useBackdropStore().clear()
}
