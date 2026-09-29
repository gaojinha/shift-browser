// 地址栏认址纯函数（从 AddressBar.vue 抽出，便于单测）
//
// 规则（对齐 Chrome 大致行为，内网优先 http：路由器/NAS 后台多为 http，
// 硬补 https 会 TLS 失败导致"打不开"）：
// - 显式协议（http://、https://、shift://…）原样放行
// - 含空格 → 搜索
// - 公网域名 → https；内网字面量（IPv4/IPv6/localhost/单标签主机名/非443端口）→ http
import { SEARCH_URLS } from '../constants'

/** 输入是否像 URL（域名/含点/冒号/localhost/IP/单标签主机名），而不是搜索词 */
export function looksLikeUrl(input: string): boolean {
  if (input.includes('.')) return true
  if (input.includes(':')) return true
  if (/^localhost(:\d+)?$/i.test(input)) return true
  if (/^\d{1,3}(\.\d{1,3}){3}(:\d+)?$/.test(input)) return true
  // 单标签内网主机名（如 nas、router）：纯数字除外（纯数字走搜索，如 12345）
  if (/^[A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?$/.test(input) && !/^\d+$/.test(input)) {
    return true
  }
  return false
}

/** 无协议输入的默认协议：内网/字面量/host:port 走 http，公网域名走 https */
export function defaultScheme(host: string): 'http' | 'https' {
  const seg = host.split('/')[0]
  // IPv6 字面量（带括号或裸写多冒号）：多为内网，走 http
  if (seg.startsWith('[')) return 'http'
  if ((seg.match(/:/g) || []).length >= 2) return 'http'
  if (/^localhost/i.test(seg)) return 'http'
  if (/^\d{1,3}(\.\d{1,3}){3}$/.test(seg)) return 'http'
  // 单标签主机名（内网）：走 http
  if (!seg.includes('.') && !seg.includes(':')) return 'http'
  // host:port：443 走 https，其余端口（多为 http 服务）走 http
  const lastColon = seg.lastIndexOf(':')
  if (lastColon !== -1) {
    const port = seg.slice(lastColon + 1)
    if (/^\d+$/.test(port)) return port === '443' ? 'https' : 'http'
  }
  return 'https'
}

export interface ResolvedTarget {
  url: string
  isSearch: boolean
}

/** 地址栏输入 → 导航目标（URL 或搜索引擎链接），空输入返回空 url */
export function resolveInputTarget(input: string, engine = 'bing'): ResolvedTarget {
  const text = input.trim()
  if (!text) return { url: '', isSearch: false }
  // 显式协议原样放行
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(text)) return { url: text, isSearch: false }
  // 带空格或不像 URL → 走搜索
  if (/\s/.test(text) || !looksLikeUrl(text)) {
    const base = SEARCH_URLS[engine] ?? SEARCH_URLS.bing
    return { url: `${base}${encodeURIComponent(text)}`, isSearch: true }
  }
  // 裸写 IPv6（多冒号、无括号）：主机段加括号
  let rest = text
  const seg = text.split('/')[0]
  if (!seg.startsWith('[') && (seg.match(/:/g) || []).length >= 2) {
    rest = `[${seg}]` + text.slice(seg.length)
  }
  const scheme = defaultScheme(seg)
  return { url: `${scheme}://${rest}`, isSearch: false }
}
