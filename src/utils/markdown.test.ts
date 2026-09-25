// markdown 渲染器安全用例集（R1 测试补课）
// renderMarkdown 的产物会经 v-html 注入 DOM，XSS 必须在字符串层被堵死。
// 策略：先整体 escapeHtml，再做语法替换；链接只放行 http/https。
import { describe, it, expect } from "vitest";
import { renderMarkdown } from "./markdown";

/** 渲染器允许产出的标签白名单；任何注入的额外标签名都意味着逃逸 */
const ALLOWED_TAGS = new Set([
  "p", "a", "code", "pre", "div", "span", "button",
  "h1", "h2", "h3", "h4", "h5", "h6",
  "ul", "ol", "li", "blockquote", "strong", "em",
]);

/** 产物中不得出现白名单外标签、事件处理器或危险协议 href */
function expectNoXss(html: string) {
  for (const m of html.matchAll(/<\/?([a-zA-Z][a-zA-Z0-9]*)/g)) {
    expect(ALLOWED_TAGS.has(m[1].toLowerCase()), `出现非白名单标签 <${m[1]}>`).toBe(true);
  }
  // 开标签内部不得出现事件处理器（文本内容里的 onXxx= 只是被转义展示的普通字符）
  for (const m of html.matchAll(/<[a-zA-Z][^>]*>/g)) {
    expect(m[0], `标签内出现事件处理器: ${m[0]}`).not.toMatch(/\son\w+\s*=/i);
  }
  // 所有 href 只允许 http(s) 或降级占位 #
  for (const m of html.matchAll(/href="([^"]*)"/g)) {
    if (m[1] !== "#") {
      expect(m[1], `href 协议非法: ${m[1]}`).toMatch(/^https?:/);
    }
  }
}

describe("renderMarkdown · XSS 防护", () => {
  it("普通段落中的 <script> 被整体转义", () => {
    const html = renderMarkdown("<script>alert(1)</script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
    expectNoXss(html);
  });

  it("<img onerror> / <svg onload> 等标签注入被转义", () => {
    const cases = [
      '<img src=x onerror="alert(1)">',
      "<svg/onload=alert(1)>",
      '<iframe src="javascript:alert(1)"></iframe>',
      "<details open ontoggle=alert(1)>",
    ];
    for (const c of cases) {
      const html = renderMarkdown(c);
      expectNoXss(html);
      expect(html).toContain("&lt;");
    }
  });

  it("javascript: 链接降级为 #，大小写/无空格混淆同样拦截", () => {
    const cases = [
      "[点击](javascript:alert(1))",
      "[点击](JAVASCRIPT:alert(1))",
      "[点击](JaVaScRiPt:alert(document.cookie))",
      "[点击](javascript:alert(1)//)",
    ];
    for (const c of cases) {
      const html = renderMarkdown(c);
      expect(html).toContain('href="#"');
      expect(html).not.toMatch(/javascript/i);
    }
  });

  it("vbscript: 与 data:text/html 协议链接被拦截", () => {
    const html1 = renderMarkdown("[x](vbscript:msgbox(1))");
    expect(html1).toContain('href="#"');
    const html2 = renderMarkdown("[x](data:text/html;base64,PHNjcmlwdD4=)");
    expect(html2).toContain('href="#"');
    expectNoXss(html1);
    expectNoXss(html2);
  });

  it("HTML 实体混淆协议（javascript&colon;）不可绕过", () => {
    const html = renderMarkdown("[x](javascript&colon;alert(1))");
    // new URL 无法解析 → 降级 #；且实体形态不得出现在 href 中
    expect(html).toContain('href="#"');
    expect(html).not.toMatch(/javascript/i);
  });

  it("链接 url 中的引号无法逃逸 href 属性", () => {
    // 输入先经 escapeHtml：" → &quot;，属性内实体不会终止属性
    const html = renderMarkdown('[x](https://example.com/"onclick="alert(1))');
    expect(html).not.toContain('onclick="');
    expect(html).toContain("&quot;");
    expectNoXss(html);
  });

  it("链接文本中的 HTML 被转义", () => {
    const html = renderMarkdown("[<script>alert(1)</script>](https://example.com/)");
    expect(html).toContain("https://example.com/");
    expect(html).toContain("&lt;script&gt;");
    expectNoXss(html);
  });

  it("畸形/嵌套链接不产生可执行结构", () => {
    // 内层链接先匹配，外层残片按文本处理
    const html = renderMarkdown("[a [b](https://b.example)](https://a.example)");
    expect(html).toContain('href="https://b.example"');
    expectNoXss(html);
    // url 含空格/制表符：链接语法不成立，整段按已转义文本输出
    const tab = renderMarkdown("[x](java\tscript:alert(1))");
    expect(tab).not.toContain("<a ");
    expectNoXss(tab);
  });

  it("围栏代码块内的脚本与标签原样转义展示", () => {
    const md = "```html\n<script>alert(1)</script>\n<img src=x onerror=alert(2)>\n```";
    const html = renderMarkdown(md);
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img");
    expect(html).not.toContain("<script>");
    expectNoXss(html);
  });

  it("未闭合代码块走兜底分支，注入仍被转义", () => {
    const html = renderMarkdown("```\n<script>alert(1)</script>");
    expect(html).toContain("&lt;script&gt;");
    expectNoXss(html);
  });

  it("行内代码中的注入被转义", () => {
    const html = renderMarkdown("执行 `<script>alert(1)</script>` 看看");
    expect(html).toContain("<code>");
    expect(html).toContain("&lt;script&gt;");
    expectNoXss(html);
  });

  it("标题/引用/列表中的注入同样被转义", () => {
    const h = renderMarkdown("# <script>alert(1)</script>");
    const q = renderMarkdown("> <img src=x onerror=alert(1)>");
    const l = renderMarkdown("- <script>x</script>");
    for (const html of [h, q, l]) {
      expectNoXss(html);
      expect(html).toContain("&lt;");
    }
  });

  it("data-code 中的双引号经 encodeURIComponent 编码，不逃逸按钮属性", () => {
    const md = '```\nconst x = "\\" onclick=alert(1)\n```';
    const html = renderMarkdown(md);
    // encodeURIComponent 会把 " 编为 %22，属性值内不得出现裸引号
    const attr = html.match(/data-code="([^"]*)"/)?.[1] ?? "";
    expect(attr).not.toContain('"');
    expect(attr).toContain("%22");
    expectNoXss(html);
  });
});

describe("renderMarkdown · 正常语法回归", () => {
  it("http/https 正常链接放行且带安全 rel", () => {
    const html = renderMarkdown("[官网](https://example.com/a?b=1)");
    expect(html).toContain('href="https://example.com/a?b=1"');
    expect(html).toContain('rel="noopener noreferrer"');
    expect(html).toContain('target="_blank"');
  });

  it("标题/加粗/斜体/代码块结构完整", () => {
    const html = renderMarkdown("# 标题\n\n**粗**与*斜*与 `code`");
    expect(html).toContain("<h1>");
    expect(html).toContain("<strong>粗</strong>");
    expect(html).toContain("<em>斜</em>");
    expect(html).toContain("<code>code</code>");
  });

  it("无序列表与有序列表正确成组", () => {
    const html = renderMarkdown("- a\n- b\n\n1. 一\n2. 二");
    expect(html).toContain("<ul><li>a</li><li>b</li></ul>");
    expect(html).toContain("<ol><li>一</li><li>二</li></ol>");
  });

  it("代码块语言标签被转义后展示", () => {
    const html = renderMarkdown("```ts\nconst a = 1\n```");
    expect(html).toContain("code-lang");
    expect(html).toContain(">ts<");
    expect(html).toContain("const a = 1");
  });
});
