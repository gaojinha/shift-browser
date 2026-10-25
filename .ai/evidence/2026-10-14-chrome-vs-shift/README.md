# 证据归档说明（2026-10-14）

> 本目录由王二狗归档，存放 "Chrome vs ShiftBrowser 对比分析" 的原始证据文件。
> 对应的分析文档在项目 `docs/` 下：
> - `docs/Chrome对比-可借鉴工程实践_2026-10-14.md`
> - `docs/兼容性诊断-Cloudflare检测与视频闪烁根因_2026-10-14.md`

## 文件清单

| 文件 | 来源 | 说明 |
|---|---|---|
| `shift-mcp-layout.json` | REA `inspect_binary_layout` | `shift-browser-mcp` 完整结构证据（2.4MB，含 8 个 debug 段、符号表、依赖） |
| `chrome-chrome-sandbox.json` | 同上 | Chrome 沙箱组件证据（代码段 4KB、suid） |
| `chrome-chrome_crashpad_handler.json` | 同上 | Chrome 崩溃处理组件证据 |
| `chrome-chrome-management-service.json` | 同上 | Chrome 企业管理组件证据 |
| `chrome-peet.json` | 真 Chrome 154 实测 | tls.peet.ws 指纹回显全文（JA3/JA4/HTTP2/头） |
| `chrome-peet.html` | 同上 | 原始 dump-dom 输出（留档） |

## 复现方法

```bash
# REA 分析（前置：~/.local/venvs/rea-pwntools 隔离环境）
REA_PWNTOOLS_PYTHON=$HOME/.local/venvs/rea-pwntools/bin/python \
  /home/gao/rea/scripts/rea.mjs inspect-binary-layout <目标路径> --format json

# Chrome 指纹对照
google-chrome --headless=new --disable-gpu --user-data-dir=/tmp/opencode/chrome-peet-profile \
  "https://tls.peet.ws/api/all" --dump-dom

# ShiftBrowser 指纹（经浏览器 MCP）
# open_and_read → https://tls.peet.ws/api/all、https://httpbin.org/headers
```

## 关联实测数据（会话记录，未单独存文件）
- ShiftBrowser 实测 JA4：`t13d2913h2_723694b0fccc_5671b5df5029`
- Chrome 154 实测 JA4：`t13d1517h2_8daaf6152771_cb7bf5808d99`
- ShiftBrowser 请求头实测（httpbin）：缺 Sec-CH-UA 全家、多 Dnt: 1（详见诊断文档）

## 注意
- 本目录位于 `.ai/` 下（.gitignore 已忽略），**不会进入 git 仓库**；如需长期归档请另行安排。
- 数字引用请以两份 docs 文档为准（已做交叉核对）。
