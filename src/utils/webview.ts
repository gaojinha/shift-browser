// webview 显隐控制（P1-9 + 第一期止血：全局引用计数）
// 弹窗/右键菜单/地址栏下拉等 HTML 覆盖层需要隐藏 GTK 层 webview（否则盖不住）。
// 多弹窗并发时旧方案各管各的：A开→隐藏，B开→隐藏，A关→显示，会把 B 正在用的 webview
// 误放出来导致死锁。这里改全局引用计数：所有隐藏请求按来源登记，只有全部关闭才真正显示。
// 后端仍用单调递增 seq 令牌丢弃乱序指令。
import { invokeOk } from './invoke'
import { perfLog } from './front-log'
import { useBackdropStore } from '../stores/backdrop'
import { useTabsStore } from '../stores/tabs'

let seq = 0
/** 当前隐藏网页的覆盖层来源集合，为空 = 网页可见 */
const hiders = new Set<string>()
/** 底垫抓取令牌：菜单关得比快照快时，晚到的快照直接丢掉，不盖新页面 */
let backdropToken = 0
/** 底垫暖缓存：藏着时抓的是蓝屏，所以只在页面可见且稳定时后台预热（菜单开直接贴，不闪） */
let backdropCache = { key: '', url: '', at: 0 }
/** 暖缓存有效期（毫秒）：过期重抓；页面切了/地址变了直接失效 */
const BACKDROP_CACHE_MS = 30000
/** 两次后台预热最小间隔（毫秒）：同步事件频繁时不打爆截图 */
const BACKDROP_WARM_GAP_MS = 15000
let lastWarmAt = 0
/** 显隐指令失败重试间隔（毫秒）：主线程繁忙/布局重排瞬间可能瞬时失败 */
const VISIBLE_RETRY_MS = 150
/** 显隐指令最多重试次数（1 次原发 + 2 次重试） */
const VISIBLE_RETRIES = 2

/**
 * 下发显隐指令（带有限重试）。
 *
 * 复用同一个 seq：若上一次其实已生效、只是回包丢了，重试会被后端按"过期指令"丢弃（幂等）；
 * 若期间已有新指令（seq 更大），重试同样被丢弃，由新指令接管——不会把状态改回去。
 *
 * 原实现失败一律静默：指令丢了、页面却一直藏着，用户只能看到"打不开"。
 * 现在有限重试 + 最终失败留痕，既自愈又不丢证据。
 */
function sendVisible(
  visible: boolean,
  token: number,
  source: string,
  left: number = VISIBLE_RETRIES,
): Promise<void> {
  return invokeOk('set_webview_visible', { visible, seq: token, source }).then(
    () => undefined,
    (e: unknown) => {
      if (left > 0) {
        return new Promise<void>((r) => setTimeout(r, VISIBLE_RETRY_MS)).then(() =>
          sendVisible(visible, token, source, left - 1),
        )
      }
      // 最终失败：留痕（日志桥自身失败会静默，不影响业务）
      console.error('[webview] 显隐指令失败:', e)
      perfLog(`[perf][webview] failed visible=${visible} src=${source} err=${String(e)}`)
    },
  )
}

/** 当前活动标签的底垫键（id+url 对不上=页面变了，缓存作废） */
function backdropKey(): string | null {
  const active = useTabsStore().activeTab
  if (!active || !active.url) return null
  return `${active.id}\n${active.url}`
}

/** 后台预热底垫（syncTabs 尾巴调用）：可见才抓，失败静默 */
export function warmBackdrop() {
  if (hiders.size > 0) return
  const key = backdropKey()
  if (!key) return
  const now = Date.now()
  if (backdropCache.key === key && now - backdropCache.at < BACKDROP_CACHE_MS) return
  if (now - lastWarmAt < BACKDROP_WARM_GAP_MS) return
  lastWarmAt = now
  invokeOk<string>('page_backdrop_shot')
    .then((shot) => {
      if (shot) backdropCache = { key, url: shot, at: Date.now() }
    })
    .catch(() => {})
}

/**
 * 设置所有标签页 webview 显隐（引用计数版）
 * @param visible 目标状态（true=显示，false=隐藏）
 * @param source 调用来源标识（调试用，必填以便计数；缺省记 anonymous）
 */
export function setWebviewVisible(visible: boolean, source: string = 'anonymous') {
  seq += 1
  // 性能埋点（2026-10-19）：GTK 显隐+重排是手输导航的关键路径，记录往返耗时
  const t0 = performance.now()
  /** 有意静默：埋点失败不能影响显隐主流程 */
  const perf = (v: boolean, t: number, src: string) => {
    perfLog(`[perf][webview] set_visible=${v} ${(performance.now() - t).toFixed(1)}ms src=${src}`)
  }
  if (!visible) {
    const wasVisible = hiders.size === 0
    hiders.add(source)
    // 首个覆盖层：暖缓存命中直接贴（零闪烁）；未命中则现抓（令牌防晚到盖错），
    // 抓到即入缓存，下次同页直接贴。失败静默（无底垫=老样子）。
    if (wasVisible) {
      const key = backdropKey()
      if (key && backdropCache.key === key && Date.now() - backdropCache.at < BACKDROP_CACHE_MS) {
        useBackdropStore().set(backdropCache.url)
      } else {
        const token = ++backdropToken
        invokeOk<string>('page_backdrop_shot')
          .then((url) => {
            if (token === backdropToken && hiders.size > 0 && url) {
              useBackdropStore().set(url)
              if (key) backdropCache = { key, url, at: Date.now() }
            }
          })
          .catch(() => {})
      }
    }
    // 有意静默（失败会重试并留痕，见 sendVisible）：显隐是高频可丢弃指令（有 seq 令牌保证最终一致）
    return sendVisible(false, seq, source).then((r) => {
      perf(false, t0, source)
      return r
    })
  }
  hiders.delete(source)
  if (hiders.size > 0) {
    // 还有别的覆盖层开着，继续保持隐藏，不发显示指令
    return Promise.resolve()
  }
  // 覆盖层清零：底垫即撤，令牌作废
  backdropToken += 1
  useBackdropStore().clear()
  return sendVisible(true, seq, source).then((r) => {
    perf(true, t0, source)
    return r
  })
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
