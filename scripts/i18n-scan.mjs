#!/usr/bin/env node
// i18n 硬编码中文扫描器（R4-5 迁移工具链 · 022）
//
// 扫描 src/**/*.vue / *.ts，找出硬编码中文字符串：
//   - HTML 属性：title / aria-label / placeholder
//   - 模板文本节点：> 中文 <
//   - JS 字符串字面量（含中文）
// 排除：注释行、i18n 语言包、测试文件、node_modules
//
// 用法：
//   node scripts/i18n-scan.mjs
//   node scripts/i18n-scan.mjs --json
//
// 口径与 017 盘点一致
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative, basename } from 'node:path'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

const __dirname = resolve(fileURLToPath(new URL('.', import.meta.url)))
const root = resolve(__dirname, '..')
const srcDir = join(root, 'src')

const EXCLUDE_DIRS = new Set(['node_modules', 'dist', '.git', '__tests__', 'locales'])
const CJK = /[\u4e00-\u9fff]/

function sectionOf(rel) {
  if (rel.startsWith('components/settings/')) return 'settings'
  if (rel.startsWith('components/ui/')) return 'ui'
  if (rel.startsWith('components/')) return 'components'
  if (rel.startsWith('stores/')) return 'stores'
  if (rel.startsWith('composables/')) return 'composables'
  if (rel.startsWith('utils/')) return 'utils'
  if (rel.startsWith('lib/')) return 'lib'
  return 'misc'
}

function componentOf(file) {
  return basename(file).replace(/\.(vue|ts)$/, '').replace(/([a-z])([A-Z])/g, '$1-$2').replace(/_/g, '-').toLowerCase()
}

function collectFiles(dir, files = []) {
  let entries
  try { entries = readdirSync(dir) } catch { return files }
  for (const e of entries) {
    const full = join(dir, e)
    try {
      const st = statSync(full)
      if (st.isDirectory()) {
        if (!EXCLUDE_DIRS.has(e)) collectFiles(full, files)
      } else if (/\.(vue|ts)$/.test(e) && !e.endsWith('.test.ts') && !e.includes('.d.ts')) {
        const rel = relative(srcDir, full)
        if (!rel.includes('locales/')) files.push(full)
      }
    } catch { /* skip */ }
  }
  return files
}

function scanFile(file) {
  const rel = relative(root, file)
  const section = sectionOf(relative(srcDir, file))
  const component = componentOf(file)
  let content
  try { content = readFileSync(file, 'utf-8') } catch { return [] }
  const lines = content.split('\n')
  const results = []
  const seen = new Set()

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]
    const trimmed = line.trim()
    // 跳过注释
    if (trimmed.startsWith('//') || trimmed.startsWith('*') || trimmed.startsWith('/*')) continue

    // ① title="中文" / aria-label="中文" / placeholder="中文"
    for (const attr of ['title', 'aria-label', 'placeholder']) {
      const idx = line.indexOf(`${attr}="`)
      if (idx === -1) continue
      const start = idx + attr.length + 2
      const end = line.indexOf('"', start)
      if (end === -1) continue
      const val = line.slice(start, end)
      if (CJK.test(val) && !seen.has(val)) {
        seen.add(val)
        results.push({ file: rel, line: i + 1, section, component, type: 'attr', value: val, suggestedKey: `${section}.${component}.${attr.replace(/-/g, '_')}` })
      }
    }

    // ② > 中文文本 <（模板文本节点）
    const gtIdx = line.indexOf('>')
    if (gtIdx > -1) {
      const ltIdx = line.indexOf('<', gtIdx + 1)
      if (ltIdx > gtIdx + 1) {
        const text = line.slice(gtIdx + 1, ltIdx).trim()
        if (text.length > 1 && CJK.test(text) && !text.includes('{{') && !text.includes('}}') && !seen.has(text)) {
          seen.add(text)
          results.push({ file: rel, line: i + 1, section, component, type: 'text', value: text.slice(0, 60), suggestedKey: `${section}.${component}.text` })
        }
      }
    }
  }
  return results
}

function main() {
  const jsonMode = process.argv.includes('--json')
  const files = collectFiles(srcDir)
  const all = []
  for (const f of files) all.push(...scanFile(f))

  const bySection = {}
  for (const r of all) { bySection[r.section] = (bySection[r.section] || 0) + 1 }

  if (jsonMode) {
    process.stdout.write(JSON.stringify({ total: all.length, bySection, entries: all }, null, 2) + '\n')
    return
  }

  console.log(`\n=== i18n 硬编码中文扫描（${files.length} 文件）===`)
  console.log(`总计：${all.length} 处\n`)

  const grouped = {}
  for (const r of all) {
    if (!grouped[r.section]) grouped[r.section] = {}
    if (!grouped[r.section][r.component]) grouped[r.section][r.component] = []
    grouped[r.section][r.component].push(r)
  }

  for (const [sec, comps] of Object.entries(grouped)) {
    console.log(`\n[${sec}]`)
    for (const [comp, items] of Object.entries(comps)) {
      console.log(`  ${comp} (${items.length})`)
      for (const item of items.slice(0, 4)) {
        const v = item.value.length > 40 ? item.value.slice(0, 40) + '…' : item.value
        console.log(`    ${item.file}:${item.line} [${item.type}] ${v}`)
      }
      if (items.length > 4) console.log(`    ... +${items.length - 4}`)
    }
  }

  console.log(`\n=== 按 section 统计 ===`)
  for (const [sec, cnt] of Object.entries(bySection)) console.log(`  ${sec}: ${cnt}`)
  console.log('')
}

// ─── 入口：scaffold 优先，否则跑扫描 ─────────────────────────────────────
const argScaffold = process.argv.indexOf('--scaffold')
if (argScaffold !== -1) {
  const comp = process.argv[argScaffold + 1]
  if (!comp) {
    console.error('用法：node scripts/i18n-scan.mjs --scaffold <component-name>')
    process.exit(1)
  }
  scaffoldMode(comp)
} else {
  main()
}

// ─── 脚手架模式：--scaffold <component-name> ───────────────────────────────
// 生成指定组件的 key 骨架 TS 文件（stdout）
// 用法：node scripts/i18n-scan.mjs --scaffold address-bar > /tmp/address-bar.keys.ts
function scaffoldMode(componentName) {
  const files = collectFiles(srcDir)
  const all = []
  for (const f of files) {
    if (componentOf(f) === componentName) all.push(...scanFile(f))
  }
  if (all.length === 0) {
    console.error(`未找到组件 ${componentName}（扫描到 0 处）`)
    process.exit(1)
  }

  // 生成 snake_case key 名：attr → {attr}_{seq}，text → text_{seq}
  const keys = {}
  let attrSeq = 0, textSeq = 0
  const seenValues = new Set()

  for (const item of all) {
    if (seenValues.has(item.value)) continue
    seenValues.add(item.value)
    let keyName
    if (item.type === 'attr') {
      attrSeq++
      const attrShort = item.value.length <= 6 ? item.value.replace(/\s+/g, '_') : `label_${attrSeq}`
      keyName = attrShort
    } else {
      textSeq++
      keyName = `text_${String(textSeq).padStart(3, '0')}`
    }
    // 清理 key 名
    keyName = keyName.replace(/[^a-zA-Z0-9_]/g, '_').replace(/_+/g, '_').replace(/^_|_$/g, '').toLowerCase() || `key_${attrSeq + textSeq}`
    if (!keys[keyName]) keys[keyName] = item.value
  }

  // 输出 TS 骨架
  const compKey = componentName.replace(/-/g, '_')
  console.log(`// 自动生成 key 骨架：${componentName}`)
  console.log(`// 生成时间：${new Date().toISOString().slice(0, 10)}`)
  console.log(`// 用法：将以下内容合并到 locales/zh-CN.ts 对应 section 下，en.ts 填入英文翻译`)
  console.log(`// 共 ${Object.keys(keys).length} 个 key\n`)
  console.log(`export const ${compKey} = {`)
  for (const [k, v] of Object.entries(keys)) {
    const escaped = v.replace(/'/g, "\\'").slice(0, 60)
    console.log(`  ${k}: '${escaped}',`)
  }
  console.log(`}\n`)
}
