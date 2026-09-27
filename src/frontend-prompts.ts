// 前端提示词与话术单源（侧栏系统词的组装仍在 ai.ts，预算口径看这里注释）
//
// 上下文预算（字符）：与后端 content_prompts 同口径 ——
// 总预算 8000，快照 800，选区 4000，历史 20 条。超了先砍历史，选区永远保。
import { toAppError, type ErrorCode } from './utils/invoke'

export const CONTEXT_BUDGET_CHARS = 8000
export const SNAPSHOT_CHARS = 800
export const SELECTION_CHARS = 4000
export const HISTORY_KEPT = 20

/** 快捷句：总结（显示文本；模型侧配 content_summary_prompt 回的整段） */
export function shortcutSummarize(title: string, url: string): string {
  return `请总结当前页面「${title}」（${url}）的主要内容，用 5 条以内要点概括。`
}

/** 快捷句：解释（显示文本；模型侧配 buildQaPrompt 回的组装提示词） */
export function shortcutExplain(title: string, url: string): string {
  return `请用通俗易懂的方式解释当前页面「${title}」（${url}）讲的是什么、对用户有什么价值。`
}

/** 八码人话：toast 只出人话，原文 parentheses 附注备查 */
export const ERROR_COPY: Record<ErrorCode, string> = {
  E_AUTH: '没登录或钥匙不对，去设置里看看',
  E_TIMEOUT: '等太久没回音，重试一次；还不行就问短点',
  E_NET: '网络没连上，先点右下角看看断在哪一层',
  E_PERM: '权限不够，按提示开白名单再试',
  E_RATELIMIT: '问太勤被限了，歇一分钟再来',
  E_DB: '本地数据 hiccup，重启不行去自检页看红项',
  E_INTERNAL: '内部 hiccup，重试一次；还红去自检页看红项',
  E_VALIDATION: '输入不对，照提示改改再发',
}

/** 任意错误翻人话：话术打底，后端原文括号附注（空原文则只有话术） */
export function humanizeError(e: unknown): string {
  const { code, message } = toAppError(e)
  const human = ERROR_COPY[code] ?? ERROR_COPY.E_INTERNAL
  const raw = (message || '').trim()
  if (!raw || raw === human) return human
  return `${human}（${raw}）`
}

/** 空状态引导话术 */
export const EMPTY_GUIDE: Record<string, string> = {
  noTabs: '空着，去地址栏输网址或按 Ctrl+T 新建标签。',
  noResult: '没搜到，换个词，或去历史记录里翻。',
  notLogged: '登录后偏好跟着走，不登录也能用。',
  offline: '断网了，点右下角胶囊看断在哪一层。',
}

/** 翻译面板分工说明（粘贴走机翻，整页走侧栏汉化） */
export const TRANSLATE_PANEL_HINT =
  '粘贴翻译走机翻极速出结果；整页汉化去 AI 侧栏点「翻译当前页」，译文直接长在页面上。'
