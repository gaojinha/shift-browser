# 本地开放 API（阶段 4 · 开放生态）

让外部程序（脚本 / 工具 / 其它应用）驱动 ShiftBrowser。

## 设计

- **传输**：**Unix 域套接字**（不做网络监听）→ 天然本地隔离。
- **权限**：套接字文件 `0600`，**仅本用户**可连。
- **协议**：**行式 JSON**——每行一个请求 → 每行一个响应。
- **路径**：`~/.local/share/com.shiftbrowser.app/api.sock`

## 命令

| 请求 | 响应 |
|---|---|
| `{"cmd":"status"}` | `{"ok":true,"data":{"version":"…","tabs":N}}` |
| `{"cmd":"list_tabs"}` | `{"ok":true,"data":[Tab…]}` |
| `{"cmd":"open_url","url":"https://…"}` | `{"ok":true,"data":Tab}` |

出错时：`{"ok":false,"error":"…"}`。

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
