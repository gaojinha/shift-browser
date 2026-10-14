// a11y 真机补验：ariaSnapshot 采集（019 · 2026-10-14）
//
// 覆盖场景：主页、设置弹窗、命令面板、地址栏建议、AI 侧边栏、历史对话框
// 方法：playwright Chromium headless → page.ariaSnapshot() → 输出到 docs/a11y-aria-snapshots.md
//
// 跑法：npm run dev 起 vite 后执行此脚本
//   node scripts/aria-snapshot-audit.mjs
import { chromium } from 'playwright'
import { writeFileSync, mkdirSync } from 'node:fs'

const base = process.env.AXU_BASE || 'http://localhost:5199'

const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  args: ['--no-sandbox', '--disable-gpu', '--use-gl=swiftshader'],
})
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } })

const snapshots = []

async function scan(name, trigger, waitMs = 800) {
  const p = await ctx.newPage()
  await p.goto(base, { waitUntil: 'networkidle', timeout: 30000 })
  await p.waitForTimeout(1200)
  try {
    await trigger(p)
  } catch (e) {
    snapshots.push({
      scenario: name,
      ts: new Date().toISOString(),
      snapshot: '(trigger failed: ' + e.message?.slice(0, 200) + ')',
      notes: ['trigger error: ' + e.message?.slice(0, 200)],
    })
    await p.close()
    return
  }
  await p.waitForTimeout(waitMs)
  const snap = await p.ariaSnapshot({ timeout: 5000 })
  snapshots.push({
    scenario: name,
    ts: new Date().toISOString(),
    snapshot: snap.slice(0, 8000),
    notes: [],
  })
  await p.close()
}

const scenarios = [
  {
    name: '主页（空白标签页）',
    trigger: async (p) => {
      await p.waitForSelector('.tab', { timeout: 5000 }).catch(() => {})
    },
    wait: 1000,
  },
  {
    name: '设置弹窗（Ctrl+Shift+O）',
    trigger: async (p) => {
      await p.keyboard.press('Control+Shift+O')
    },
    wait: 1500,
  },
  {
    name: '命令面板（Ctrl+K）',
    trigger: async (p) => {
      await p.keyboard.press('Control+K')
    },
    wait: 1000,
  },
  {
    name: '地址栏建议（输入 test）',
    trigger: async (p) => {
      const input = p.locator('input[aria-label="地址栏"]')
      await input.click()
      await input.fill('test')
      await p.waitForTimeout(600)
    },
    wait: 800,
  },
  {
    name: 'AI 侧边栏（点击按钮）',
    trigger: async (p) => {
      await p.locator('button[aria-label="AI 助手"]').click()
    },
    wait: 800,
  },
  {
    name: '历史记录对话框（Ctrl+Shift+H）',
    trigger: async (p) => {
      await p.keyboard.press('Control+Shift+H')
    },
    wait: 800,
  },
  {
    name: '书签管理器（manage-btn）',
    trigger: async (p) => {
      await p.locator('button.manage-btn').click()
    },
    wait: 800,
  },
  {
    name: '密码管理（aria-label）',
    trigger: async (p) => {
      await p.locator('button[aria-label="密码管理"]').click()
    },
    wait: 800,
  },
]

for (const s of scenarios) {
  console.log(`\n=== ${s.name} ===`)
  await scan(s.name, s.trigger, s.wait)
}

await browser.close()

const outPath = 'docs/a11y-aria-snapshots.md'
mkdirSync('docs', { recursive: true })
let md = '# a11y AriaSnapshot 补验报告（019 · 2026-10-14）\n\n'
md += '> 方法：Playwright Chromium headless，' + base + '\n'
md += '> 工具：page.ariaSnapshot()，viewport 1440×900\n'
md += '> 备注：仅采集快照，不执行 axe-core 规则扫描；读屏交互需真机验证\n\n---\n\n'

for (const s of snapshots) {
  md += `## ${s.scenario}\n\n`
  md += `- 时间：${s.ts}\n\n`
  md += '```\n'
  md += s.snapshot
  md += '\n```\n\n'
  if (s.notes.length > 0) {
    md += '**注意：** ' + s.notes.join('; ') + '\n\n'
  }
  md += '---\n\n'
}

writeFileSync(outPath, md, 'utf-8')
console.log(`\nwritten to ${outPath} (${snapshots.length} scenarios)`)
