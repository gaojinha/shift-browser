// i18n 装配（R4-5 基础框架 · 2026-10-14）
//
// 选型：vue-i18n 10，composition 模式（legacy:false），默认 zh-CN，
// fallback zh-CN（en 包缺键时回退中文，不空串）。
// 语言包按功能节拆模块（locales/*.ts），全量组件迁移与"设置节懒加载"
// 是 R4-5 完整范围；本期只落骨架 + TranslateProgress 样板迁移。
import { createI18n } from 'vue-i18n'
import zhCN from './locales/zh-CN'
import en from './locales/en'

export type Locale = 'zh-CN' | 'en'
export const SUPPORTED_LOCALES: readonly Locale[] = ['zh-CN', 'en'] as const

export const i18n = createI18n({
  legacy: false,
  locale: 'zh-CN',
  fallbackLocale: 'zh-CN',
  messages: { 'zh-CN': zhCN, en },
})

/** 切换语言（R4-5 完整版接设置键与持久化；骨架期供测试与后续接线用） */
export function setLocale(l: Locale) {
  i18n.global.locale.value = l
}
