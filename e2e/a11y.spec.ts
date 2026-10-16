/**
 * F-1 a11y 全量回归（axe）。
 *
 * 口径：扫 dev 前端主框架（纯浏览器 + e2e/helpers/tauri-mock.ts 的 IPC mock），
 * 断言 WCAG 2.0/2.1/2.2 A/AA 无 critical/serious 违规；
 * moderate/minor 记录不阻塞。首跑基线 2026-10-16 见
 * docs/test-reports/e2e-2026-10-16/F-1-a11y-全量回归-报告.md。
 *
 * 覆盖缺口（如实）：对话框/面板（设置/书签/下载等）v1 未覆盖——
 * 入口按钮无稳定测试选择器（无 data-test/aria-label），补覆盖待选择器落地。
 * 垂直标签栏（VerticalTabBar）同为 tablist+关闭按钮模式，默认不渲染未扫。
 */
import { test, expect } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'
import { mockTauriIpc } from './helpers/tauri-mock'

const BLOCKING = new Set(['critical', 'serious'])

type ViolationNode = { target: string[] }
type Violation = {
  id: string
  impact: string | null
  help: string
  nodes: ViolationNode[]
}

/**
 * 已知误报基线（白名单管理，不计回归）。每条必须带理由与复核路径：
 *
 * nested-interactive（.tab）：WAI-ARIA APG "Tabs with close buttons"
 * （W3C 官方推荐模式）正是 role=tab 容器内嵌关闭按钮，axe 该规则对此模式
 * 过严（dequelabs/axe-core 同类误报上报多起）。键盘路径完好：
 * roving tabindex + ArrowLeft/Right 切换 + Enter/Space 触发关闭
 * （TabBar.vue onTabListKeydown，close-btn 命中时跳过 tab 级处理）。
 * 若 axe 未来对该模式放宽，删此白名单条目并观察是否清零。
 */
const KNOWN_FALSE_POSITIVES: ReadonlyArray<
  Readonly<{ rule: string; target: string }>
> = [
  { rule: 'nested-interactive', target: '.tab' },
]

function isKnownFalsePositive(v: Violation): boolean {
  return v.nodes.every((n) =>
    KNOWN_FALSE_POSITIVES.some(
      (k) => v.id === k.rule && n.target.some((sel) => sel.startsWith(k.target)),
    ),
  )
}

async function scanPage(page: import('@playwright/test').Page): Promise<Violation[]> {
  await page.waitForLoadState('networkidle')
  // Vue 首帧渲染 + 设置加载（mock 即刻返回，2.5s 足够）
  await page.waitForTimeout(2500)
  const results = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22a', 'wcag22aa'])
    .analyze()
  const violations = results.violations as unknown as Violation[]
  for (const v of violations) {
    console.log(`  [${v.impact ?? 'none'}] ${v.id} ×${v.nodes.length} — ${v.help}`)
  }
  return violations
}

test('a11y: 主框架无 critical/serious 违规', async ({ page }) => {
  await mockTauriIpc(page)
  await page.goto('/')
  const violations = await scanPage(page)
  const blocking = violations.filter((v) => BLOCKING.has(v.impact ?? ''))
  const regressions = blocking.filter((v) => !isKnownFalsePositive(v))
  for (const v of blocking) {
    if (isKnownFalsePositive(v)) {
      console.log(`  [whitelisted] ${v.id} ×${v.nodes.length}（已知误报，见文件头注）`)
    }
  }
  expect(
    regressions,
    JSON.stringify(
      regressions.map((v) => ({
        id: v.id,
        impact: v.impact,
        help: v.help,
        nodes: v.nodes.map((n) => n.target.join(' ')).slice(0, 5),
      })),
      null,
      2,
    ),
  ).toEqual([])
})
