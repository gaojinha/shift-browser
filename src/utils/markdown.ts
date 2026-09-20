// 轻量 Markdown 渲染（AI 回复专用）
// 安全策略：先转义 HTML 特殊字符，再处理语法标记，杜绝 XSS
// 支持：代码块/行内代码/加粗/斜体/链接/列表/标题/引用/段落

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/** 链接只放行 http/https，其余一律 #（防 javascript: 等注入） */
function safeUrl(url: string): string {
  try {
    const u = new URL(url)
    return u.protocol === 'http:' || u.protocol === 'https:' ? url : '#'
  } catch {
    return '#'
  }
}

/** 行内语法：行内代码/加粗/斜体/链接（输入已是转义后的文本） */
function renderInline(s: string): string {
  let t = s
  // 行内代码最先处理——避免代码里的 * _ [ ] 被误解析
  t = t.replace(/`([^`]+)`/g, (_, code: string) => `<code>${code}</code>`)
  // 加粗
  t = t.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  // 斜体
  t = t.replace(/\*([^*]+)\*/g, '<em>$1</em>')
  // 链接 [text](url)
  t = t.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_m, text: string, url: string) => {
    const safe = safeUrl(url)
    return `<a href="${safe}" target="_blank" rel="noopener noreferrer">${text}</a>`
  })
  return t
}

/** 代码块（带语言标签 + 复制按钮，复制内容经 data-code 传给点击处理） */
function renderCodeBlock(code: string, lang: string): string {
  const escaped = escapeHtml(code)
  const langLabel = lang
    ? `<span class="code-lang">${escapeHtml(lang)}</span>`
    : '<span class="code-lang"></span>'
  return `<div class="code-block"><div class="code-header">${langLabel}<button class="code-copy" data-code="${encodeURIComponent(code)}" title="复制代码">复制</button></div><pre><code>${escaped}</code></pre></div>`
}

/** 把 Markdown 文本渲染为 HTML 字符串（调用方用 v-html 注入） */
export function renderMarkdown(text: string): string {
  const lines = text.split('\n')
  let html = ''
  let i = 0
  let inCode = false
  let codeBuf: string[] = []
  let codeLang = ''

  while (i < lines.length) {
    const line = lines[i]

    // 围栏代码块 ```lang ... ```
    const codeMatch = line.match(/^```(\w*)\s*$/)
    if (codeMatch) {
      if (!inCode) {
        inCode = true
        codeLang = codeMatch[1] || ''
        codeBuf = []
      } else {
        inCode = false
        html += renderCodeBlock(codeBuf.join('\n'), codeLang)
      }
      i++
      continue
    }
    if (inCode) {
      codeBuf.push(line)
      i++
      continue
    }

    // 标题 # ~ ######
    const hMatch = line.match(/^(#{1,6})\s+(.*)$/)
    if (hMatch) {
      const level = hMatch[1].length
      html += `<h${level}>${renderInline(escapeHtml(hMatch[2]))}</h${level}>`
      i++
      continue
    }

    // 引用 > text
    if (line.startsWith('> ')) {
      html += `<blockquote>${renderInline(escapeHtml(line.slice(2)))}</blockquote>`
      i++
      continue
    }

    // 无序列表 - / * / +
    if (/^\s*[-*+]\s+/.test(line)) {
      html += '<ul>'
      while (i < lines.length && /^\s*[-*+]\s+/.test(lines[i])) {
        html += `<li>${renderInline(escapeHtml(lines[i].replace(/^\s*[-*+]\s+/, '')))}</li>`
        i++
      }
      html += '</ul>'
      continue
    }

    // 有序列表 1. 2. ...
    if (/^\s*\d+\.\s+/.test(line)) {
      html += '<ol>'
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) {
        html += `<li>${renderInline(escapeHtml(lines[i].replace(/^\s*\d+\.\s+/, '')))}</li>`
        i++
      }
      html += '</ol>'
      continue
    }

    // 空行
    if (!line.trim()) {
      html += '<div class="md-blank"></div>'
      i++
      continue
    }

    // 普通段落
    html += `<p>${renderInline(escapeHtml(line))}</p>`
    i++
  }

  // 未闭合的代码块兜底
  if (inCode) {
    html += renderCodeBlock(codeBuf.join('\n'), codeLang)
  }
  return html
}

/** 复制文本（navigator.clipboard 优先，失败回退 execCommand） */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    try {
      const ta = document.createElement('textarea')
      ta.value = text
      ta.style.position = 'fixed'
      ta.style.opacity = '0'
      document.body.appendChild(ta)
      ta.select()
      const ok = document.execCommand('copy')
      document.body.removeChild(ta)
      return ok
    } catch {
      return false
    }
  }
}