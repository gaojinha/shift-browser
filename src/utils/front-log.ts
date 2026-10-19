// 前端日志桥（2026-10-19）
// 统一走 Rust 的 front_log，沉到应用日志里，便于事后定位慢点/崩溃。
import { invoke } from '@tauri-apps/api/core'

/**
 * 写一条前端日志到 Rust 日志。
 *
 * 有意静默：日志桥失败（测试环境 mock、IPC 未就绪、Rust 侧报错）绝不能影响主流程，
 * 也不能形成"报错→再报错"的递归噪音。
 */
export function frontLog(msg: string) {
  try {
    void Promise.resolve(invoke('front_log', { msg })).catch(() => {})
  } catch {
    // 有意静默：埋点失败不影响业务
  }
}

/** 性能埋点：格式统一为 `[perf][模块] ...`，便于 grep */
export function perfLog(msg: string) {
  frontLog(msg)
}
