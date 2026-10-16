/**
 * Tauri IPC mock（F-1 a11y e2e 专用，纯浏览器环境）。
 *
 * 页面自带 ipc-bridge（index.html 内联）：invoke 经
 * `fetch('ipc://localhost/<cmd>')` 发请求，成功判定看响应头
 * `Tauri-Response: ok`，响应体 = Rust 命令返回值的原始 JSON（无信封）。
 * 本 mock 在页面脚本前注入，拦截 ipc:// 的 fetch 并回默认值；
 * 未登记命令按读命令惯例回退（get_ 与 list_ 前缀回空数组，其余 null）。
 *
 * 默认值口径：src/constants.ts 的 SETTINGS_SCHEMA（contracts.rs 单一来源，
 * build.rs 自动生成）+ 真实库只读采样。结构猜错会以前端报错暴露，迭代修正。
 */
import type { Page } from '@playwright/test'

const DEFAULTS: Record<string, unknown> = {
  // 设置读回是「非默认项行数组」（settings.ts rows 迭代），空 = 全默认
  get_all_settings: [],
  get_bookmarks: [],
  get_tabs: [],
  is_locked: false,
  get_privacy_stats: {},
  front_log: null,
  get_privacy_fp_mode: 'noise',
  get_privacy_whitelist: [],
  ai_list_providers: [],
  get_user_agent_config: {},
  // 启动流程会建一个标签，前端读返回值的 .id
  create_tab: { id: 'mock-tab-1', url: 'about:blank', title: '新标签页' },
}

export async function mockTauriIpc(page: Page) {
  await page.addInitScript((defaults: Record<string, unknown>) => {
    const origFetch = window.fetch.bind(window)
    window.fetch = (input: RequestInfo | URL, init?: RequestInit) => {
      const url = typeof input === 'string' ? input : input.toString()
      if (url.startsWith('ipc://')) {
        const cmd = decodeURIComponent(url.replace('ipc://localhost/', ''))
        let v = defaults[cmd]
        if (v === undefined) {
          v = cmd.startsWith('get_') || cmd.startsWith('list_') ? [] : null
        }
        return Promise.resolve(
          new Response(JSON.stringify(v), {
            status: 200,
            headers: { 'Tauri-Response': 'ok', 'Content-Type': 'application/json' },
          }),
        )
      }
      return origFetch(input, init)
    }
  }, DEFAULTS)
}
