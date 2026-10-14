// a11y 审计：扫描对话框/面板场景
// 在 axe-audit.mjs 基础上：先开面板再扫描，把对话框里的违规也抓出来
import { chromium } from 'playwright'
import { AxeBuilder } from '@axe-core/playwright'
import { writeFileSync, mkdirSync } from 'node:fs'

const base = 'http://localhost:5199'

const scenarios = [
  { name: 'settings-dialog', trigger: (p) => p.keyboard.press('Control+Shift+O'), wait: 1500 },
  { name: 'command-palette', trigger: (p) => p.keyboard.press('Control+K'), wait: 1000 },
  { name: 'address-suggest', trigger: async (p) => {
    const input = p.locator('input[aria-label="地址栏"]')
    await input.fill('test')
    await p.waitForTimeout(600)
  }, wait: 0 },
  // 补扫包：§ 六 列出的 6 个对话框（2026-10-14 T-017 补扫）
  { name: 'ai-sidebar', trigger: async (p) => {
    await p.locator('button[aria-label="AI 助手"]').click()
    await p.waitForTimeout(800)
  }, wait: 0 },
  { name: 'history-dialog', trigger: async (p) => {
    await p.locator('button[aria-label="历史记录"]').click()
    await p.waitForTimeout(800)
  }, wait: 0 },
  { name: 'translate-panel', trigger: async (p) => {
    await p.locator('button[aria-label="翻译"]').click()
    await p.waitForTimeout(800)
  }, wait: 0 },
  { name: 'bookmarks-dialog', trigger: async (p) => {
    // 书签管理器按钮在 BookmarksBar 里，需先让书签栏显示
    await p.locator('button.manage-btn').click()
    await p.waitForTimeout(800)
  }, wait: 0 },
  { name: 'passwords-dialog', trigger: async (p) => {
    await p.locator('button[aria-label="密码管理"]').click()
    await p.waitForTimeout(800)
  }, wait: 0 },
  { name: 'sessions-dialog', trigger: async (p) => {
    await p.keyboard.press('Control+Shift+S')
    await p.waitForTimeout(800)
  }, wait: 0 },
  { name: 'downloads-dialog', trigger: async (p) => {
    await p.locator('button.dl-btn').click()
    await p.waitForTimeout(800)
  }, wait: 0 },
]

const browser = await chromium.launch({
  headless: true,
  executablePath: '/usr/bin/google-chrome',
  args: ['--no-sandbox', '--disable-gpu', '--use-gl=swiftshader'],
})
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } })
const results = []

for (const s of scenarios) {
  const p = await ctx.newPage()
  await p.goto(base, { waitUntil: 'networkidle', timeout: 30000 })
  await p.waitForTimeout(1000)
  await s.trigger(p)
  if (s.wait) await p.waitForTimeout(s.wait)

  const axe = await new AxeBuilder({ page: p })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'])
    .analyze()

  const counts = axe.violations.reduce(
    (acc, v) => { acc[v.impact] = (acc[v.impact] || 0) + v.nodes.length; return acc },
    {},
  )
  console.log(`\n=== ${s.name} === violations: ${axe.violations.length} rules, ${JSON.stringify(counts)}`)
  for (const v of axe.violations) {
    console.log(`  [${v.impact}] ${v.id} — ${v.nodes.length} nodes`)
    for (const n of v.nodes.slice(0, 4)) {
      console.log(`     ${n.html?.slice(0, 120)}`)
    }
  }
  results.push({ scenario: s.name, ts: new Date().toISOString(), counts, violations: axe.violations })
  await p.close()
}

await browser.close()
mkdirSync('docs', { recursive: true })
writeFileSync('docs/a11y-axe-dialogs.json', JSON.stringify(results, null, 2))
console.log('\nwritten to docs/a11y-axe-dialogs.json')
