// Tauri 命令统一调用封装（R1 错误类型化 · 2026-09-25）
// 契约：Rust CommandError 序列化为 {code, message}（八码见 src-tauri/src/error.rs）；
// 迁移期旧命令仍以裸字符串/Error reject，这里统一归一为 AppErrorShape.
import { invoke } from '@tauri-apps/api/core'

/** 稳定错误码（与 Rust error.rs 八码一一对应，改名需走契约评审） */
export type ErrorCode =
  | 'E_DB'
  | 'E_PERM'
  | 'E_TIMEOUT'
  | 'E_AUTH'
  | 'E_RATELIMIT'
  | 'E_NET'
  | 'E_VALIDATION'
  | 'E_INTERNAL'

export interface AppErrorShape {
  code: ErrorCode
  message: string
}

const CODES: readonly string[] = [
  'E_DB',
  'E_PERM',
  'E_TIMEOUT',
  'E_AUTH',
  'E_RATELIMIT',
  'E_NET',
  'E_VALIDATION',
  'E_INTERNAL',
]

/** 任意 reject 值归一为 {code,message}：
 *  - 新命令的 CommandError 对象：原样通过
 *  - 带 message 但 code 不在八码内的对象：code 兜底 E_INTERNAL，保留 message
 *  - Error 实例：取 e.message
 *  - 裸字符串/其他：String(e) 兜底 E_INTERNAL
 */
export function toAppError(e: unknown): AppErrorShape {
  if (e && typeof e === 'object') {
    const o = e as Record<string, unknown>
    if (
      typeof o.code === 'string' &&
      CODES.includes(o.code) &&
      typeof o.message === 'string'
    ) {
      return { code: o.code as ErrorCode, message: o.message }
    }
    if (typeof o.message === 'string') {
      return { code: 'E_INTERNAL', message: o.message }
    }
    if (e instanceof Error) return { code: 'E_INTERNAL', message: e.message }
  }
  return { code: 'E_INTERNAL', message: String(e) }
}

/** 取人读消息（toast/日志用），等价于旧代码 String(e)，但兼容新错误对象 */
export function errText(e: unknown): string {
  return toAppError(e).message
}

/** invoke 包装：成功原样返回；失败统一抛 AppErrorShape，调用方按 code 分流 */
export async function invokeOk<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  try {
    // 无参命令不显式传 undefined 第二参——与裸 invoke(cmd) 行为一致（mock 断言兼容）
    return args === undefined ? await invoke<T>(cmd) : await invoke<T>(cmd, args)
  } catch (e) {
    throw toAppError(e)
  }
}
