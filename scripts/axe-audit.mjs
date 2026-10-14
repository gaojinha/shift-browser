// a11y 审计跑分：axe-core 全量规则扫描 dev server 页面
// 用法：npm run axe:audit （先起 vite）
import { chromium } from 'playwright'
import { AxeBuilder } from '@axe-core/playwright'
import { readFileSync } from 'node:fs'

const base = process.env.AXU_BASE || 'http://localhost:5199'
const pages = process.argv[2]
  ? process.argv[2].split(',')
  : ['/']

const outPath = process.env.AXU_OUT || 'docs/a11y-axe-results.json'

const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.CHROME_PATH || '/usr/bin/google-chrome',
  args: ['--no-sandbox', '--disable-gpu', '--use-gl=swiftshader'],
})
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } })

const results = []
for (const path of pages) {
  const p = await ctx.newPage()
  const url = base + path
  console.log(`\n=== scanning ${url} ===`)
  try {
    await p.goto(url, { waitUntil: 'networkidle', timeout: 30000 })
  } catch (e) {
    console.log(`  goto failed: ${e.message?.slice(0, 160)}`)
  }
  await p.waitForTimeout(1200)
  const axe = await new AxeBuilder({ page: p })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'])
    .analyze()
  const counts = axe.violations.reduce(
    (acc, v) => { acc[v.impact] = (acc[v.impact] || 0) + v.nodes.length; return acc },
    {},
  )
  console.log(`  violations: ${axe.violations.length} rules, by impact ${JSON.stringify(counts)}`)
  for (const v of axe.violations) {
    console.log(`  [${v.impact}] ${v.id} — ${v.description.slice(0, 80)} (${v.nodes.length} nodes)`)
  }
  results.push({ url, ts: new Date().toISOString(), axeVersion: axe.axeVersion, counts, violations: axe.violations })
  await p.close()
}

await browser.close()

// 写 JSON 结果
import { writeFileSync, mkdirSync } from 'node:fs'
mkdirSync('docs', { recursive: true })
writeFileSync(outPath, JSON.stringify(results, null, 2))
console.log(`\nresults written to ${outPath}`)
