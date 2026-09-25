// webview 显隐控制（P1-9）
// 弹窗/右键菜单等 HTML 覆盖层需要隐藏 GTK 层 webview（否则盖不住）。
// 快速开关时多次 invoke 并发乱序会导致最终状态错位，这里用单调递增
// seq 令牌：后端只应用最新 seq 的指令，过期指令直接丢弃。
import { invokeOk } from './invoke'

let seq = 0

/**
 * 设置所有标签页 webview 显隐
 * @param visible 目标状态（true=显示，false=隐藏）
 * @param source 调用来源标识（调试用）
 */
export function setWebviewVisible(visible: boolean, source?: string) {
  seq += 1
  return invokeOk('set_webview_visible', { visible, seq, source }).catch(() => {})
}