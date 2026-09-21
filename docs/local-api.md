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

浏览器打开 `http://127.0.0.1:19701/?token=<api-token 里的值>` → 一个**状态面板**：
版本、标签数、当前活动标签、命令清单、用法。不带 token 只显示提示。
（浏览器不便发 Bearer 头，故面板支持 `?token=` 查询串。）

## 命令

| 请求 | 响应 | 说明 |
|---|---|---|
| `{"cmd":"status"}` | `{"ok":true,"data":{"version":"…","tabs":N}}` | 版本 + 标签数 |
| `{"cmd":"list_tabs"}` | `{"ok":true,"data":[Tab…]}` | 列全部标签 |
| `{"cmd":"active_tab"}` | `{"ok":true,"data":Tab\|null}` | 取当前活动标签 |
| `{"cmd":"open_url","url":"https://…"}` | `{"ok":true,"data":Tab}` | 新标签打开网址 |
| `{"cmd":"close_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 关标签（返回接管的活动标签） |
| `{"cmd":"switch_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 切换活动标签 |
| `{"cmd":"reload_tab","id":"…"}` | `{"ok":true,"data":Tab\|null}` | 刷新标签 |
| `{"cmd":"navigate","id":"…","url":"…"}` | `{"ok":true,"data":Tab\|null}` | 让指定标签导航到网址 |
| `{"cmd":"search","query":"…"}` | `{"ok":true,"data":Tab}` | 用当前搜索引擎搜（新标签打开） |
| `{"cmd":"new_tab"}` | `{"ok":true,"data":Tab}` | 打开内置新标签页 |
| `{"cmd":"history","limit":50}` | `{"ok":true,"data":[{url,title,visit_count,last_visit_at}…]}` | 读最近历史（limit ≤500） |
| `{"cmd":"bookmarks"}` | `{"ok":true,"data":[{title,url,folder,sort_order}…]}` | 读书签（含文件夹名） |
| `{"cmd":"screenshot"}` | `{"ok":true,"data":{"path":"…/screenshots/shot-*.png"}}` | 截当前活动标签可见区 → PNG 落盘 |
| `{"cmd":"eval","code":"…"}` | `{"ok":true,"data":{"result":"…"}}` | 在活动标签执行 JS（**默认关**，见下） |
| `{"cmd":"tools"}` | `{"ok":true,"data":[{name,server_name,description,input_schema}…]}` | 列出可用 MCP 工具（工具网关） |
| `{"cmd":"tool_call","name":"…","arguments":{…}}` | `{"ok":true,"data":{"result":"…"}}` | 调用一个 MCP 工具（受 server 白名单约束） |

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
- **动作面**：目前只有"读状态 / 列标签 / 开网址"三个**低风险**命令；写操作（关标签、执行 JS 等）
  若后续加入，需评估是否要再加一层 token 鉴权。
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

## 后续

- 更多命令：`close_tab` / `switch_tab` / `search`（按需）。
- HTTP 形态（`127.0.0.1:<port>` + token）若确有必要再加；当前 Unix 套接字已够本地脚本用。
