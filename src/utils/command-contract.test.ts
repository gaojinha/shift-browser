// P1-2 契约守卫（2026-09-22）：断言前端 invoke 调用的命令都已在 Rust 侧注册。
// 防"前端调了未注册的命令"（运行时才报错的漂移）。
import { describe, it, expect } from 'vitest'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'

const ROOT = resolve(__dirname, '../..')

/** 递归收集 src 下的 .ts/.vue 文件 */
function collectFiles(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    const st = statSync(p)
    if (st.isDirectory()) {
      if (name === 'node_modules' || name.startsWith('.')) continue
      collectFiles(p, out)
    } else if (
      (name.endsWith('.ts') || name.endsWith('.vue')) &&
      !name.endsWith('.test.ts') // 排除测试文件自身（含示例字符串）
    ) {
      out.push(p)
    }
  }
  return out
}

/** 提取前端所有 invoke('xxx') 的命令名 */
function frontendCommands(): Set<string> {
  const names = new Set<string>()
  const re = /invoke(?:<[^>]*>)?\(\s*['"]([a-z_]+)['"]/g
  for (const f of collectFiles(join(ROOT, 'src'))) {
    const s = readFileSync(f, 'utf-8')
    let m: RegExpExecArray | null
    while ((m = re.exec(s)) !== null) names.add(m[1])
  }
  return names
}

/** 提取 Rust lib.rs 的 generate_handler! 注册命令名 */
function registeredCommands(): Set<string> {
  const src = readFileSync(join(ROOT, 'src-tauri/src/lib.rs'), 'utf-8')
  const start = src.indexOf('generate_handler![')
  expect(start, '未找到 generate_handler').toBeGreaterThan(-1)
  const rest = src.slice(start + 'generate_handler!['.length)
  const end = rest.indexOf('])')
  expect(end, 'generate_handler 未闭合').toBeGreaterThan(-1)
  const names = new Set<string>()
  for (const seg of rest.slice(0, end).split(',')) {
    const t = seg.trim()
    if (!t) continue
    const name = t.split('::').pop()?.trim() ?? ''
    if (/^[a-z_][a-z0-9_]*$/.test(name)) names.add(name)
  }
  // 补充：lib.rs 里的裸 #[tauri::command] 函数（如 front_log/is_locked/selfcheck_cmd）
  const cmdRe = /#\[tauri::command\][\s\S]{0,80}?fn\s+([a-z_][a-z0-9_]*)/g
  let m: RegExpExecArray | null
  while ((m = cmdRe.exec(src)) !== null) names.add(m[1])
  return names
}

describe('命令契约（P1-2）', () => {
  it('前端 invoke 的命令都已在 Rust 侧注册', () => {
    const frontend = frontendCommands()
    const registered = registeredCommands()
    expect(registered.size).toBeGreaterThan(100)
    const missing = [...frontend].filter((c) => !registered.has(c)).sort()
    expect(missing, `以下命令前端在调用但 Rust 未注册: ${missing.join(', ')}`).toEqual([])
  })
})
