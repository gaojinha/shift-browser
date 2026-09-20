// 页面理解层类型（阶段 1.1，与 Rust engine::content::Snapshot 一致）

/** 语义快照：从活动标签实时 DOM 提取的页面结构化信息 */
export interface Snapshot {
  title: string
  url: string
  lang: string | null
  /** 正文可见文本（去空白压缩，最多 4000 字符） */
  text: string
  /** 可交互元素统计 */
  interactive: {
    links: number
    buttons: number
    inputs: number
    textareas: number
    selects: number
    iframes: number
  }
  /** 表单字段（最多 20 个） */
  forms: Array<{ name: string; field_type: string; placeholder: string }>
  /** 可见链接（最多 30 个） */
  links: Array<{ text: string; href: string }>
  /** 页面暴露的 WebMCP 工具声明（阶段 1.5 polyfill 试验，最多 10 个，可缺省） */
  webmcpTools?: Array<{ name: string; description: string; parameters: unknown | null }>
  /** 无障碍语义信号（阶段 1.5 · 对齐 Lighthouse Agentic 信号②"代理为中心的无障碍"——
    程序化名称 aria-label/labelledby + 有效角色 + 隐藏态，供 Agent 用**语义定位**定位元素，
    不再只靠标签文本猜。最多 30 个，可缺省） */
  aria?: Array<{ role: string; label: string; tag: string; hidden: boolean }>
}