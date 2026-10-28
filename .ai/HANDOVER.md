# 交接说明（2026-10-28 18:30，重启前）

## 一、当前版本

| 项 | 值 |
|---|---|
| HEAD | `3d14430` |
| 安装版 | `/opt/shift-browser-cef/shift-browser`（Oct 28 18:07） |
| 启动命令 | `shift-browser` |
| 二进制指纹 | wrapper 在 `/usr/bin/shift-browser`，`LD_LIBRARY_PATH=/opt/shift-browser-cef` |
| 工作区 | 干净（只剩 `.ai/` 未跟踪的派单/回执，正常） |

## 二、今天交付了什么

### 修复（4 个卡死/崩溃，同一类病根）

| 问题 | 根因 | 提交 |
|---|---|---|
| 点书签/地址栏导航 → 整窗卡死 | `handle_action` 持 `browsers()` 锁调 `load_url`，同步回调再拿同锁 = 自死锁 | `133e1c0` 前身 + 后续 |
| 刷新/前进/后退 → 整窗卡死 | 同上，`reload`/`go_back`/`go_forward`/`stop_load` 都在锁内 | `9a1795c` |
| 关窗 → `[crash] FATAL SIGABRT` | `on_window_destroyed` 里 100ms 直接 `exit(0)`，CEF 还在析构 | `4c25cc4` |
| 标签右键菜单被裁 + 一大块灰底 | 菜单 200px > 视口 122px；`top = innerHeight-200` 算出 **-78** | `ca54c6e`（第二版方案） |

### 机制（防复发）

- **自死锁检测**（`279a562`）：`lock_state` 记录线程本地持锁表，同线程二次取同锁立即 panic（带线程名+锁类型），不再静默卡死
- **统一错误上报**（`48d27af`）：`err_report.rs` + `errors.log` + 诊断面板
- **日志落盘**（`b05e993`）：stdout/stderr → `logs/stdio.log`（CEF 原生输出）；CEF 初始化后重装信号处理器（Chromium 会覆盖）；日志轮转 + `SHIFT_LOG_LEVEL`
- **一键收集**：`bash scripts/collect-logs.sh` → `/tmp/shift-logs-*.tar.gz`

### 功能

- 命令面板 `Ctrl+K`（四组候选：标签/历史/书签/命令）
- 标签固定 / 静音 / 拖动重排
- 会话恢复
- 下载面板实时刷新 + 清空已结束
- 附件链接直接下载（`start_download`，不再卡死）
- **启动页侧边栏**：世界导航（32 类 721 站）/ 我的 / 设置
- 启动页 + 世界导航页过 **hallmark** 与 **taste-skill** 双 linter，均 clean

### 技能（已装）

- `~/.agents/skills/hallmark`（结构卫生：token/4px 网格/emoji/渐变）
- `~/.agents/skills/design-taste-frontend`（布局语义：h1/对比度/icon chips/字号，支持 app screen）

## 三、待办

| 编号 | 内容 | 优先级 |
|---|---|---|
| **F-NEWTAB-02** | 「我的」接入账号系统（登录/注册/资料/偏好/备份），目前只 console 占位 | 中 |
| **F-CEF-25** | CEF 版接回 shift-browser MCP（高哥定的：等功能完善再接） | 低 |
| **F-CEF-31 遗留** | 附件导航已修（`start_download`），但普通 URL 导航到国内不可达域名（如 example.com）时 DevTools 端口也会无响应，属网络等待，未深挖 | 低 |
| **右键菜单第一版方案** | 「弹出时撑高工具栏」实测失败（撑高没生效且菜单被关），已回滚改用紧凑+滚动。若以后要恢复撑高，需先查清为什么撑高会把菜单关掉 | 记录 |
| 端到端设计 | 工具栏/面板/设置页还没用 taste-skill 的 `--app` 模式系统检查过（只查了启动页） | 中 |

## 四、重启后检查清单（5 分钟）

```bash
# 1. 启动
shift-browser

# 2. 刷新（Ctrl+R）连续 3 次 —— 应秒回，不卡
# 3. 前进/后退 —— 不卡
# 4. 右键标签 —— 菜单完整、紧贴标签栏下方、可滚动到 7 项
# 5. 关窗 —— 干净退出（进程归零，crash.log 不新增）
# 6. 侧边栏：世界导航 / 我的 / 设置
# 7. 点书签栏的站点 —— 应正常打开
```

出问题先跑：

```bash
bash scripts/collect-logs.sh
```

把 `/tmp/shift-logs-*.tar.gz` 发我。日志覆盖：主日志 / stdio（CEF 原生）/ errors / panic / crash / diagnostics / 系统信息 / dmesg。

## 五、关键文件

| 路径 | 说明 |
|---|---|
| `src-tauri/src/engine/cef/shell.rs` | CEF 主逻辑（锁纪律、动作路由、回调） |
| `src-tauri/src/engine/cef/toolbar.rs` | 工具栏 HTML/CSS/JS（面板、菜单、命令面板） |
| `src-tauri/src/engine/cef/newtab.rs` | 启动页生成（含世界导航页落盘） |
| `src-tauri/src/app/err_report.rs` | 错误上报 + guard + 环形缓冲 |
| `src-tauri/src/app/crash_signal.rs` | 致命信号捕获 |
| `scripts/collect-logs.sh` | 一键收日志 |
| `scripts/cef-coordinator/regress-24d.sh` | 8 项自动回归 |
| `docs/错误排查指南_2026-10-28.md` | 症状对照表 + F-CEF-35 复盘 |
| `docs/交付汇总_2026-10-28.md` | 今日总账 |
| `.ai/WORKBOARD.md` / `.ai/MESSAGES.md` | 协作状态源 |

## 六、踩过的坑（别再踩）

1. **持锁调 CEF = 自死锁**：CEF 会同步回调，回调里再拿同一把锁。锁内只 clone 句柄，放锁后再调。
2. **`spawn_local` 只能 MainContext 所属线程调**：tokio 线程调必 panic。
3. **CEF 初始化会覆盖 SIGSEGV 处理器**（crashpad），必须在 CEF 起来后重装。
4. **`CefShutdown` 必须在初始化 CEF 的线程**调；强杀进程会 SIGABRT。
5. **MSRV 1.77.2 不支持 `LazyLock`**（1.80 稳定），用 `Mutex::new` / `OnceLock`。
6. **`wrap_task!` 宏内部递归用裸名**，必须 `use cef::wrap_task;`。
