# 本地开放 API（阶段 4 · 开放生态）

让外部程序（脚本 / 工具 / 其它应用）驱动 ShiftBrowser。

## 设计

- **传输**：**Unix 域套接字**（不做网络监听）→ 天然本地隔离。
- **权限**：套接字文件 `0600`，**仅本用户**可连。
- **协议**：**行式 JSON**——每行一个请求 → 每行一个响应。
- **路径**：`~/.local/share/com.shiftbrowser.app/api.sock`

## 两种形态

| 形态 | 地址 | 鉴权 | 用途 |
|---|---|---|---|
| **Unix 套接字** | `~/.local/share/com.shiftbrowser.app/api.sock` | 文件权限 0600 | 本机脚本/工具 |
| **HTTP**（阶段4 自托管） | `http://127.0.0.1:19701/api` | `Authorization: Bearer <token>` | 浏览器扩展/HTTP 客户端等 |

HTTP 形态**只绑 127.0.0.1**（不对外），token 随机生成、落 `api-token`（权限 0600）。
两形态**命令完全一致**（同一套处理逻辑）。

### MCP Server（AI 驱动浏览器 · 标准协议）

把本地开放 API 暴露为**标准 MCP 工具**，任何 MCP 客户端（opencode / Claude Desktop /
Cursor / 自研 agent）配一条 command 即可驱动浏览器——不用自己写 HTTP 客户端。

- **二进制**：`~/.local/bin/shift-browser-mcp`（`cargo build --release --bin shift-browser-mcp` 编译）
- **协议**：MCP 2025-03-26，stdio 传输（newline-delimited JSON-RPC）
- **转发**：直连浏览器 Unix socket（api.sock，0600 权限鉴权）——**零额外依赖、不经网络**
- **工具**：26 个 = 15 读 + 11 写（与 API 命令同名同参；写类受 `api_readonly` 只读闸门约束）
- **不暴露**：`eval`（执行任意 JS）与 `tool_call`（嵌套 MCP）——风险高，需要时走浏览器 API 直连（有显式闸门）

配置示例（opencode / Claude Desktop 的 `mcpServers`）：

```json
{
  "mcpServers": {
    "shift-browser": { "command": "/home/gao/.local/bin/shift-browser-mcp" }
  }
}
```

环境变量 `SHIFT_BROWSER_SOCK` 可覆盖 api.sock 路径（默认
`~/.local/share/com.shiftbrowser.app/api.sock`）。

**工具清单**：

| 类别 | 工具 |
|---|---|
| 读 | `status` · `list_tabs` · `active_tab` · `history` · `bookmarks` · `search_history` · `list_downloads` · `list_sessions` · `list_groups` · `list_spaces` · `list_userscripts` · `get_setting` · `list_settings` · `screenshot` · `read_page` · `read_status` · `open_and_read` · `extract` · `reader/extract` · `translate` · `content/snapshot` · `audit/reads` · `audit/egress` |
| 写 | `open_url` · `new_tab` · `close_tab` · `switch_tab` · `reload_tab` · `navigate` · `search` · `pin_tab` · `mute_tab` · `minimize_window` · `maximize_window` · `tabs/close_ai` · `action/dismiss` |

手动测试（三行握手 + 调工具）：

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}' \
  '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"status","arguments":{}}}' \
  | ~/.local/bin/shift-browser-mcp
```

```bash
TOKEN=$(cat ~/.local/share/com.shiftbrowser.app/api-token)
curl -s -X POST http://127.0.0.1:19701/api -H "Authorization: Bearer $TOKEN" \
     -d '{"cmd":"status"}'
# → {"ok":true,"data":{"version":"…","tabs":N}}
```
无/错 token 返回 `{"ok":false,"error":"未授权…"}`。

> **端口 / 绑定可配**（设置 → 隐私）：
> - 「开放 API 端口」默认 `19701`；
> - 「开放 API 绑局域网（危险）」默认关（只绑 `127.0.0.1`）；开启后绑 `0.0.0.0`，
>   局域网可达（手机/其它设备可驱动浏览器）——**务必保管好 token**。
> 两者改完**需重启生效**。

### 模型网关（OpenAI 兼容）

`POST http://127.0.0.1:19701/v1/chat/completions`（`Authorization: Bearer <token>`）→
转发到设置里的 AI provider（`ai_base_url` / `ai_api_key` / `ai_model`），**其它程序可用同一模型**。

- **默认关**：需在 设置 → 隐私 → 「允许模型网关（消耗额度）」显式开启。
- 未指定 `model` 时用设置里的 `ai_model`；`ai_api_key` 空则回退环境变量 `AGNES_API_KEY`。
- **注意**：会消耗你的模型额度，故默认关。

```bash
curl -s -X POST http://127.0.0.1:19701/v1/chat/completions \
  -H "Authorization: Bearer $(cat ~/.local/share/com.shiftbrowser.app/api-token)" \
  -H "Content-Type: application/json" \
  -d '{"messages":[{"role":"user","content":"你好"}]}'
```

### 状态面板（浏览器用）

浏览器打开 `http://127.0.0.1:19701/` → 登录页，两种安全方式进入**状态面板**
（版本、标签数、当前活动标签、命令清单）：

1. **应用内一键打开**（推荐）：设置 → 开放 API →「在浏览器打开状态面板」。
   应用签发一次性门票拼在 `?ticket=<t>` 里，**5 分钟有效、打开一次即焚**。
2. **登录表单粘贴 token**：在打开的登录页把 `api-token` 文件内容贴进输入框提交
   （走 POST body，不进浏览器历史/Referer/代理日志）。

> 长期 token **不再支持** `?token=` 查询串（会落历史、Referer、代理日志）；
> Bearer 头依然有效。LAN 暴露下凭据错误按来源 IP 指数退避（1s→2s→…→300s 封顶，
> 成功认证清零），面板响应带 `X-Frame-Options: DENY`。
> token 可在设置 → 开放 API →「轮换 API token」一键热更换，旧 token 即时失效。

## 命令

| 请求 | 响应 | 说明 |
|---|---|---|
| `{"cmd":"status"}` | `{"ok":true,"data":{"version":"…","tabs":N}}` | 版本 + 标签数 |
| `{"cmd":"list_tabs"}` | `{"ok":true,"data":[Tab…]}` | 列全部标签 |
| `{"cmd":"active_tab"}` | `{"ok":true,"data":Tab\|null}` | 取当前活动标签 |
| `{"cmd":"open_url","url":"https://…"}` | `{"ok":true,"data":Tab}` | 新标签打开网址 |
| `{"cmd":"close_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 关标签（返回接管的活动标签） |
| `{"cmd":"tabs/close_ai"}` | `{"ok":true,"data":{"closed":[…],"count":N}}` | 关闭**所有 AI 标签**（`origin=ai`，开放 API 打开的那些）——绝不碰用户标签。AI 读完回收现场用 |
| `{"cmd":"audit/reads","limit":50}` | `{"ok":true,"data":[{id,ts,cmd,url,tab_id,ok,error_code,origin}…]}` | 查询最近 AI 操作审计（**只记命令/url/结果码，不含页面内容**）。所有 API 命令自动入库（元命令与 `audit/reads` 自身除外） |
| `{"cmd":"audit/egress"}` | `{"ok":true,"data":{processes,connections:[{pid,proto,remote_ip,remote_port,state}…],distinct_remotes,loopback,external,checked_at}}` | 网络外发审计：列出本应用及子进程当前 TCP 外联端点（只读 /proc、零出网），核对"浏览器连了哪儿" |
| `{"cmd":"extract","selector":".item","fields":["text","href"],"limit":100}` | `{"ok":true,"data":{selector,total,returned,truncated,items:[…]}}` | 按选择器结构化提取（固定模板，非任意 JS）。`fields` 白名单 `text`/`href`/`src`/`alt`/`title`/`data-*`；`tab_id` 可选（缺省活动标签） |
| `{"cmd":"reader/extract","url":"…"}` | `{"ok":true,"data":{title,content,text}}` | 给 URL 抓取并净化正文（阅读模式 readability，独立于标签） |
| `{"cmd":"translate","text":"…","target":"zh-CN"}` | `{"ok":true,"data":{ok,data,msg}}` | 翻译文本（复用本地翻译链路） |
| `{"cmd":"content/snapshot"}` | `{"ok":true,"data":{…}}` | 活动标签语义快照（本地、不出网） |
| `{"cmd":"switch_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 切换活动标签 |
| `{"cmd":"reload_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 刷新标签 |
| `{"cmd":"navigate","id":"…","url":"…"}` | `{"ok":true,"data":Tab\|null}` | 让指定标签导航到网址 |
| `{"cmd":"search","query":"…"}` | `{"ok":true,"data":Tab}` | 用当前搜索引擎搜（新标签打开） |
| `{"cmd":"new_tab"}` | `{"ok":true,"data":Tab}` | 打开内置新标签页 |
| `{"cmd":"history","limit":50}` | `{"ok":true,"data":[{url,title,visit_count,last_visit_at}…]}` | 读最近历史（limit ≤500） |
| `{"cmd":"bookmarks"}` | `{"ok":true,"data":[{title,url,folder,sort_order}…]}` | 读书签（含文件夹名） |
| `{"cmd":"screenshot","id":"…"}` | `{"ok":true,"data":{"path":"…/screenshots/shot-*.png"}}` | 截标签可见区 → PNG 落盘（`id` 可选，缺省当前活动标签） |
| `{"cmd":"read_page","mode":"text"}` | `{"ok":true,"data":{mode,url,title,meta,hints,ts,truncated,total,…}}` | 读页面内容（缺省活动标签，可用 `tab_id` 指定），8 模式：`text`/`elements`/`links`/`images`/`tables`/`forms`/`article`/`html`。参数：`mode`、`selector`、`tab_id`、`max_chars`、`max_items`，另 P1b：`expand`(展开 details)、`shadow`(穿透 shadow root)、`iframe`(并入同源 iframe)。固定采集脚本非任意 JS，password/hidden 的 value 一律不返回；tables 带 `merged` 标记 |
| `{"cmd":"action/dismiss","tab_id":"…"}` | `{"ok":true,"data":{dismissed:true,by:"text",text:"同意"}}` | 关闭常见弹窗（Cookie 同意/遮罩浮层）。保守识别，识别不了返回 `{dismissed:false,hint}`，**绝不盲点**。写类，受只读闸门约束 |
| `{"cmd":"read_status","id":"…"}` | `{"ok":true,"data":{tab_id,url,title,status,ready_state,has_body,hints,counts}}` | 读标签状态与元信息（纯读、不返回内容；`id` 可选，缺省活动标签）。只读模式（`api_readonly=true`）下放行 |
| `{"cmd":"open_and_read","url":"…","mode":"text","close":false,"reuse":false,"ua":"desktop"}` | `{"ok":true,"data":{tab_id,url,final_url,title,ready_state,content_type,reused,ua,read,…}}` | **一次完成「开标签→等加载→读取」**。参数：`url`（必填，仅 http/https）、`mode`/`selector`/`max_chars`/`max_items`（同 read_page）、`close`（读完是否关闭，默认 false）、`reuse`（复用同 URL 的 AI 标签，默认 false）、`ua`（`desktop`/`mobile`，mobile 切移动版 UA 读移动版内容）。内网/私有地址默认拒绝（`api_allowed_ips` 白名单可放行）。语义是读 → 只读模式下放行 |
| `{"cmd":"eval","code":"…"}` | `{"ok":true,"data":{"result":"…"}}` | 在活动标签执行 JS（**默认关**，见下） |
| `{"cmd":"tools"}` | `{"ok":true,"data":[{name,server_name,description,input_schema}…]}` | 列出可用 MCP 工具（工具网关） |
| `{"cmd":"tool_call","name":"…","arguments":{…}}` | `{"ok":true,"data":{"result":"…"}}` | 调用一个 MCP 工具（受 server 白名单约束） |
| `{"cmd":"search_history","query":"…","limit":50}` | `{"ok":true,"data":[HistoryEntry…]}` | 搜索历史（query 必填） |
| `{"cmd":"list_downloads"}` | `{"ok":true,"data":[Download…]}` | 列下载记录（最近 100） |
| `{"cmd":"list_sessions"}` | `{"ok":true,"data":[Session…]}` | 列会话快照（最近 50） |
| `{"cmd":"list_groups"}` | `{"ok":true,"data":[TabGroup…]}` | 列标签分组 |
| `{"cmd":"list_spaces"}` | `{"ok":true,"data":[Workspace…]}` | 列空间（工作区） |
| `{"cmd":"list_userscripts"}` | `{"ok":true,"data":[Userscript…]}` | 列用户脚本 |
| `{"cmd":"get_setting","key":"theme"}` | `{"ok":true,"data":"…"\|null}` | 读单个设置（敏感键返回 null） |
| `{"cmd":"list_settings"}` | `{"ok":true,"data":[{key,value}…]}` | 列全部设置（敏感键已过滤） |
| `{"cmd":"pin_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 固定 / 取消固定标签 |
| `{"cmd":"mute_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 静音 / 取消静音标签 |
| `{"cmd":"minimize_window"}` | `{"ok":true,"data":null}` | 最小化主窗口 |
| `{"cmd":"maximize_window"}` | `{"ok":true,"data":bool}` | 最大化 / 还原主窗口（返回当前是否最大化） |

出错时：`{"ok":false,"error":"…"}`。

**取 id**：先 `list_tabs` / `active_tab` 拿 `id`，再传给写命令。

## 用法示例

```bash
SOCK=~/.local/share/com.shiftbrowser.app/api.sock

# 查状态
printf '{"cmd":"status"}\n' | socat - UNIX-CONNECT:$SOCK

# 列标签
printf '{"cmd":"list_tabs"}\n' | socat - UNIX-CONNECT:$SOCK

# 开网址
printf '{"cmd":"open_url","url":"https://example.com"}\n' | socat - UNIX-CONNECT:$SOCK
```

Python（无需 socat）：

```python
import socket, json
p = "/home/gao/.local/share/com.shiftbrowser.app/api.sock"
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(p)
s.sendall(b'{"cmd":"open_url","url":"https://example.com"}\n')
print(s.recv(65536).decode())
```

## 安全边界

- **只监听本地**：Unix 套接字，不经网络，外部主机连不上。
- **只限本用户**：文件权限 0600（同机其它用户无权限）。
- **动作面**：读命令（`status` / `list_*` / `history` / `bookmarks` / `search_history` /
  `get_setting` / `list_settings`）为低风险；写命令（`open_url` / `close_tab` / `switch_tab` /
  `navigate` / `search` / `new_tab` / `pin_tab` / `mute_tab` / `minimize_window` /
  `maximize_window` / `eval` / `tool_call`）会改变浏览器状态。
- **只读模式**（设置 `api_readonly=true`）：一键禁掉**所有写命令**（返回 `ok:false`），
  只放读命令——给"只想让脚本读数据"的场景兜底。
- **IP 白名单**（设置 `api_allowed_ips`）：HTTP 形态可绑 `0.0.0.0` 暴露到局域网，
  此时用白名单限定来源 IP，逗号分隔，支持精确 IP（`127.0.0.1`）、CIDR
  （`192.168.1.0/24`、`fe80::/10`）与旧式尾点段前缀（`192.168.1.`）；
  空 = 不限制。注意裸前缀 `192.168.1` 已不再生效（旧实现会误放行 `192.168.10.x`）。
  非白名单来源的连接直接被拒（日志留痕）。
- **不绕过安全闸门**：本 API 不提供 agent 动作执行入口（多步动作仍只走 AI 面板的确认流程）。
- **执行 JS（eval）默认关**：最强能力（可读页面/驱动交互），故加**显式闸门**——必须在
  设置 → 隐私 → 「允许 API 执行 JS（危险）」里开启，命令才生效；未开返回 `ok:false`。
  另有限长（≤100KB）。开启前请确认：只有可信的本机脚本 / 持有 token 者能连进来。

## 验证

```bash
# 套接字权限
ls -la ~/.local/share/com.shiftbrowser.app/api.sock   # 应为 srw-------
```

运行实证（2026-09-21）：`status` / `list_tabs` / `open_url` 三命令均返回 `ok:true`，
`open_url` 成功新建标签。

运行实证（2026-09-21 · C2 扩展）：12 个新命令全部 `ok:true`——`list_downloads` /
`list_sessions` / `list_spaces` / `list_settings` / `search_history` / `get_setting`
均取到真实数据；`pin_tab` / `mute_tab` / `maximize_window` 生效。
**只读模式**：`api_readonly=true` 时 `open_url` 被拒（`只读模式已开启…`）、`status` 正常。
**IP 白名单**：`api_allowed_ips=1.2.3.4` 时本机请求被拒（日志 `拒绝非白名单 IP: 127.0.0.1`），
改为 `127.0.0.1` 后恢复正常。

## 后续

- 更多命令：按需继续扩（如书签增删、会话恢复、空间切换）。
- 鉴权细化（已完成 · C2）：只读模式（`api_readonly`）+ IP 白名单（`api_allowed_ips`）；
  如仍需要，可再加速率限制 / 命令级权限表。
- HTTP 形态已支持（`127.0.0.1:<port>` + token，端口可配、可绑局域网 + IP 白名单）。
- MCP Server（已完成 · 第八十七章）：`shift-browser-mcp` 二进制，标准 MCP stdio 协议，
  26 个工具（15 读 + 11 写），任何 MCP 客户端可驱动浏览器；eval/tool_call 不暴露。
