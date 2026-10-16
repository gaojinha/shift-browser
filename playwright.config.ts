import { defineConfig } from '@playwright/test'

// F-1 a11y 全量回归 e2e（2026-10-16 补建）。
// 纯浏览器（headless chrome）+ e2e/helpers/tauri-mock.ts 的 IPC mock 跑 dev 前端。
// 注意：Tauri IPC 链路（invoke 真实现）不在 e2e 覆盖范围，走 src-tauri live 测试。
export default defineConfig({
  testDir: './e2e',
  timeout: 90_000,
  fullyParallel: false,
  retries: 0,
  reporter: [
    ['list'],
    ['json', { outputFile: 'e2e/.last-report.json' }],
  ],
  use: {
    headless: true,
    channel: 'chrome',
    baseURL: 'http://localhost:5173',
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:5173',
    reuseExistingServer: true,
    timeout: 60_000,
  },
})
