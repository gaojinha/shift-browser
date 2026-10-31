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
  // 同时抓 invoke() 和 invokeOk()：invokeOk 才是业务代码的实际入口
  // （P3 踩过：只匹配 invoke 时，invokeOk('read_selection') 这种
  //   "调了 Rust 未注册命令"的漂移会整批逃过本守卫）
  // 泛型参数里可能含 >/< 和换行（如 invokeOk<{a: string}>('cmd', {...})），
  // 用 [\s\S]*? 跨行匹配到第一个 '('，再取其后的引号命令名。
  const re = /(?:invoke|invokeOk)(?:<[\s\S]*?>)?\(\s*['"]([a-z_]+)['"]/g
  for (const f of collectFiles(join(ROOT, 'src'))) {
    const s = readFileSync(f, 'utf-8')
    let m: RegExpExecArray | null
    while ((m = re.exec(s)) !== null) names.add(m[1])
  }
  return names
}

/**
 * 提取 `#[tauri::command(name = "ipc_name")] fn rust_fn` 的显式改名映射。
 *
 * 为什么需要：Tauri 允许命令函数名与 IPC 名不同（`db/userscripts.rs` 里
 * `list_userscripts_cmd` 用 `name = "list_userscripts"` 保持 WebKit 侧
 * `invoke('list_userscripts')` 不变）。generate_handler! 登记的是**函数名**，
 * 若只认函数名就会把已注册的命令误判为未注册（2026-10-31 实测：901 例中 1 例红）。
 */
function commandRenames(): Map<string, string> {
  const renames = new Map<string, string>()
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const p = join(dir, name)
      const st = statSync(p)
      if (st.isDirectory()) {
        walk(p)
      } else if (name.endsWith('.rs')) {
        const s = readFileSync(p, 'utf-8')
        const re =
          /#\[tauri::command\s*\(\s*name\s*=\s*"([a-z_][a-z0-9_]*)"\s*\)\][\s\S]{0,200}?fn\s+([a-z_][a-z0-9_]*)/g
        let m: RegExpExecArray | null
        while ((m = re.exec(s)) !== null) renames.set(m[2], m[1])
      }
    }
  }
  walk(join(ROOT, 'src-tauri/src'))
  return renames
}

/** 提取 Rust lib.rs 的 generate_handler! 注册命令名（IPC 名，含显式改名） */
function registeredCommands(): Set<string> {
  const renames = commandRenames()
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
    if (/^[a-z_][a-z0-9_]*$/.test(name)) {
      // 带显式 name 的命令：IPC 名是属性里的那个，不是函数名。
      // 只加 IPC 名（不加函数名），否则前端误调 `xxx_cmd` 也会被放行。
      const ipc = renames.get(name)
      names.add(ipc ?? name)
    }
  }
  // 补充：lib.rs 里的裸 #[tauri::command] 函数（如 front_log/is_locked/selfcheck_cmd）
  const cmdRe = /#\[tauri::command\][\s\S]{0,80}?fn\s+([a-z_][a-z0-9_]*)/g
  let m: RegExpExecArray | null
  while ((m = cmdRe.exec(src)) !== null) names.add(m[1])
  return names
}

describe('命令契约（P1-2）', () => {
  it('显式改名的命令都已在 generate_handler! 注册', () => {
    // 反向守卫：`#[tauri::command(name = "x")]` 只改 IPC 名，仍需在 lib.rs 注册函数名，
    // 否则前端能调通契约检查却调不通运行时。
    const renames = commandRenames()
    expect(renames.size, '未解析到任何显式改名，正则可能已失效').toBeGreaterThan(0)
    const src = readFileSync(join(ROOT, 'src-tauri/src/lib.rs'), 'utf-8')
    const start = src.indexOf('generate_handler![')
    const rest = src.slice(start + 'generate_handler!['.length)
    const end = rest.indexOf('])')
    const registeredFns = new Set(
      rest
        .slice(0, end)
        .split(',')
        .map((seg) => seg.trim().split('::').pop()?.trim() ?? '')
        .filter((n) => n.length > 0),
    )
    const orphan = [...renames.keys()].filter((fn) => !registeredFns.has(fn)).sort()
    expect(orphan, `以下命令有显式 IPC 名但未在 generate_handler! 注册: ${orphan.join(', ')}`).toEqual([])
  })

  it('前端 invoke 的命令都已在 Rust 侧注册', () => {
    const frontend = frontendCommands()
    const registered = registeredCommands()
    expect(registered.size).toBeGreaterThan(100)
    const missing = [...frontend].filter((c) => !registered.has(c)).sort()
    expect(missing, `以下命令前端在调用但 Rust 未注册: ${missing.join(', ')}`).toEqual([])
  })
})
