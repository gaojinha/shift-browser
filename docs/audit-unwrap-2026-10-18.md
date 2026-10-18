# Rust 生产代码 unwrap / 吞错审计报告

- 日期：2026-10-18（F-79 卡片，审计卡：**只出报告，不改代码**）
- 依据：AGENTS.md 工作守则第 21 行「锁、panic、unwrap：一律毒化恢复或降级，`#[allow]` 遮羞的不许留」
- 产出：统计 + 风险分类 + 修复建议。**修不修、怎么修，等高哥拍板**

## 1. 统计口径与方法

- 范围：`src-tauri/src`（含 bin）
- 生产段 = 剔除 ① 独立测试文件（`tests.rs` / `*_tests.rs` / `api/http_tests/`）② 文件尾 `#[cfg(test)]` 段
- 方法：行级正则统计 + **逐处人工核上下文**（跨行形态、注释假阳性、前置条件均手工补正）
- 卡片粗数对照：卡片估 unwrap「排除测试后 589」——实测裸总量 660，其中 547 在测试代码（独立测试文件 82 + 内联 `#[cfg(test)]` 模块 465），**生产段真实只有 31**。粗数的"排除测试"只排了独立测试文件，内联测试模块未排。

## 2. 总览

| 模式 | 裸总量（含测试） | 生产段 | 定性 |
|------|------|------|------|
| `.unwrap()` | 660 | **31** | 锁毒化 15（P0）/ 协议构造 12（P1）/ 其他 4 |
| `unwrap_or_default()` | 83 | 73 | 多数安全兜底，抽查即可 |
| `.ok();`（语句级） | 12 | 11（含 2 假阳性） | 9 真为 channel 发送，保留 |
| `.ok().flatten()` | 1 | 1 | 注释行假阳性（记录的是已修复历史） |
| `let _ =`（丢弃） | 199 | **177** | 可保留 ~120 / 建议加日志 ~47 / **须修 5** |
| `[0]` 裸下标 | — | 10 | 逐一核过前置条件，**全部安全** |

## 3. A. 锁毒化 `lock().unwrap()` —— 15 处（P0，须修）

线程在临界区内 panic → 锁被毒化 → 此后每次 `unwrap()` 连锁 panic，请求路径整条瘫痪。

| 文件 | 行 | 锁 |
|------|----|----|
| `src/agent_cmd.rs` | 37 / 57 / 67 / 90 / 119 / 129 | `pending()` `std::Mutex<HashMap>`（agent 命令 in-flight 表） |
| `src/engine/mcp.rs` | 172 / 193 | `connections()` `std::Mutex`（MCP 连接表） |
| `src/engine/mcp/transport.rs` | 210 / 217 / 294 | `connections()` `std::Mutex` |
| `src/engine/platform/mod.rs` | 37 / 46 | `DOWNLOAD_DIR_OVERRIDE` `std::RwLock` |
| `src/webqa.rs` | 627-630（跨行）/ 651 | `cancel_flags()` `std::Mutex` |

**修复**：机械替换 `unwrap_or_else(|e| e.into_inner())`（毒化恢复——持锁 panic 说明内部已出事，但数据结构大概率仍可用，继续服务优于连锁崩溃）。

**同仓正面先例**（同一模式、别处已按守则修好）：
- `src/ai.rs:394 / 423 / 434`：`cancel_flags().lock().unwrap_or_else(|e| e.into_inner())`，注释「锁毒化时照常用，不崩进程」
- `src/proto/shift/protocol.rs:235`：`PROGRESS_THROTTLE…lock().unwrap_or_else(|e| e.into_inner())`

**关键发现：`ai.rs` 的同类锁已修，其余 5 个文件未修——同一代码库同一模式处理不一致。** 这 15 处是本次审计最高优先级。

## 4. B. 协议响应构造 `Response::builder()…body().unwrap()` —— 12 处（P1，建议修）

`src/proto/shift/protocol.rs`：99 / 104 / 111 / 143 / 156 / 170 / 190 / 244 / 372 / 389 / 418 / 425。

- tauri 的 `Response` builder `.body()` 理论可失败（body 类型转换），实际几乎不会。
- 但一旦 panic，**整个 shift:// 协议 handler 线程崩**——所有内部页（启动页/世界导航）与翻译 IPC 请求全灭。
- **建议**：抽一个小 helper `fn respond(err) -> Response`（log::error + 500），12 处统一替换。

## 5. C. 其他 `unwrap()` —— 4 处

| 文件:行 | 内容 | 定性 |
|---------|------|------|
| `src/tabs/backend/create.rs:180/181` | `child.position().unwrap()` / `child.size().unwrap()`（刚建 webview 取几何） | 建议修：`unwrap_or_default` + log，或 map_err 上抛；panic 会中止整个标签创建 |
| `src/tabs/backend/layout.rs:352/358` | `SHORTCUTS.iter().find(\|s\| s.gtk_always).unwrap()` / `!s.gtk_always` | **保留**：依赖 contracts.rs 数据不变量（表必含两类条目）。panic 即 fail-fast 暴露配置错，好过静默错行为；建议确认无守卫单测则补一条 |

## 6. D. `let _ =` 吞错 —— 177 处，三档分类

### D1. 须修（5 处，P0）

| 文件:行 | 内容 | 为什么危险 |
|---------|------|-----------|
| `src/tabs/commands/tab_state_cmds.rs:182/189/193` | `DELETE FROM history / downloads / sessions` 三处 | 「清除数据」失败被吞，但 `done.push(...)` 仍向调用方汇报"已清除"——**用户可见的行为不一致**（以为清了，实际没清） |
| `src/perms.rs:193` | `write_allow_map`：权限 allow 表写库 | 内存放行表与 DB 失同步，下次启动权限状态不一致——安全相关 |
| `src/db/users/profile.rs:203` | `user_prefs` 保存（月相/天气/字号/主题/壁纸） | 用户主动存设置，失败被吞 → 设置静默丢失 |

**建议**：失败收集后如实上抛（对齐 `feed.rs:223` 的既有口径「提交失败要如实上报，不能把未落库的数据当成功」）。

### D2. 建议加日志（~47 处，P1，降级不崩溃）

| 组 | 处数 | 说明 |
|----|------|------|
| `TabManager::set_webview_visible` / `switch_visibility` | 10 | webview 显隐失败 = UI 状态不一致（该隐未隐/该显未显），应 log |
| `wv.with_webview` / `webview.with_webview` | 12 | WebKit 操作失败（eval/导航/截图），应 log |
| 窗口 `show/hide/set_focus/unminimize/close` | 15 | GTK/WebKit 失败，低-中，建议 debug log |
| `downloads::update_download_bytes/state`（layout/gtk.rs 360/382/404） | 3 | 下载进度写库失败 → UI 进度卡死，应 log |
| `newtab::generate`（newtab.rs:38 / feed.rs:324） | 2 | 启动页生成失败 → 旧页残留，应 log |
| `std::fs::set_permissions`（db/mod.rs:97 等 6 处） | 6 | 安全加固：db 含加密密码，权限收紧失败应 **warn**（否则回退 umask 644） |
| `api/auth.rs:25` `atomic_write_token` | 1 | dashboard token 写失败（旧 token 继续有效，不 fail-open），warn |
| `api/http/util.rs:23/24` `write_http` 两次写 | 2 | 写失败基本 = 对端已断连，debug log 即可 |
| `certificates.rs` 4 处监控写 / `feed.rs:219` 裁剪 / `plugins.rs:121` 审计写 | 6 | best-effort 审计/监控表（其中 1 处已有注释「尽力裁剪：失败不影响主流程」），debug log |
| `wv.navigate` / `apply_split_layout` / `run_on_main_thread` / `close_tab` / `reload_tab` 等零散 | ~6 | 应 log |
| `db/mod.rs:66`（预创建 600 占位）/ `db/sync.rs:118`（MKCOL）/ `actions.rs:206`（idle 清理）/ `opencode_chat.rs:165`（`on_delta` 回调，返回类型已无损） | 4 | 注释已解释或语义无害，**可保留**（列此表只为完整） |

### D3. 可保留（~120 处，有意语义/已知无害）

| 组 | 处数 | 理由 |
|----|------|------|
| `app.emit` / `app_*.emit` 系列 | ~26 | UI 广播，无监听者 = 正常 |
| `tx.send` / `self.tx.send`（mpsc/tray） | ~17 | channel 半闭语义：接收端 drop 是正常结束 |
| `child.kill().await` / `child.wait().await` | 7 | 顽固进程兜底（reader/fetch、mcp/transport，注释明确） |
| `OnceLock/OnceCell .set`（CHAT_IMPL 等 4 + PANIC_LOG_DIR 1） | 5 | 第二次 set 必失败，有意 |
| 窗口/webview `show/hide/focus` 中启动期一次性调用 | ~8 | 与 D2 同形但调用方已无后续依赖，保留可接受 |
| `gtk::init()`（linux.rs:16） | 1 | 幂等初始化 |
| `std::fs::create_dir_all`（5）/ `remove_file`（3） | 8 | 幂等 / 清理不存在无害 |
| `GDK_TAB;` / `SHORTCUTS.len();`（layout.rs:343/344） | 2 | **不是吞错**——引用常量抑制 unused 警告（写法可再讲究，无害） |
| `conn.notify`（mcp 初始化通知）/ `provider.load_from_data`（lock_dialog CSS）/ `stdout().flush()` / panic_hook `writeln!` / `update-desktop-database` / `std::fs::write`（tray hint） | 6 | 低影响 best-effort |

## 7. E. `.ok();` —— 11 处（含 2 假阳性）

| 位置 | 内容 | 定性 |
|------|------|------|
| `webqa.rs` 568/579/591/602/610/634 | `on_event.send(e).ok()` | channel 发送，保留 |
| `ai.rs` 368/420/444/454 | `on_event.send(StreamEvent::…).ok()` | channel 发送，保留 |
| `app/bootstrap.rs:182` | `app.path().resource_dir().ok()` | **假阳性**：Result→Option 正常用法，值被使用 |
| `db/settings_cache.rs:105` | 注释行「此前 .ok().flatten() 静默吞错」 | **假阳性**：注释记录 2026-09-29 已修复 |

## 8. F. `unwrap_or_default()` —— 生产段 73 处

多数是安全兜底（默认值合理）。风险类别是**兜底值掩盖真实错误**（失败时静默走默认路径，调用方无感知）。抽查样例：`protocol.rs:129` 的 `wv_url` 兜底空串 → 后续有来源白名单校验，安全。不逐处列，建议修复卡里按"兜底后是否有校验/日志"抽查。

## 9. G. `[0]` 裸下标 —— 生产段 10 处，逐一核过，**全部安全**

| 位置 | 前置条件 |
|------|----------|
| `ai/ssrf.rs:70/71/72/85` | `IpAddr::segments()/octets()` 类型保证（v4 恒 4 段、v6 恒 8 段） |
| `audit.rs:66` | `n.to_le_bytes()` 返回 `[u8; 4]`，类型保证 |
| `opencode_chat.rs:342/343` | `turns.len() == 1` 预检 |
| `reader.rs:109` | `while old.len() > 19` 预检 |
| `selfcheck.rs:82` | `if f.len() < 6 { continue }` 预检 |
| `netmon.rs:42` | `if fields.len() >= 9` 预检 |

## 10. 高危清单汇总（按修复优先级）

| 优先级 | 处数 | 内容 | 修法 |
|--------|------|------|------|
| **P0 须修** | 15 | 锁毒化 `lock().unwrap()`（A 节清单） | 机械替换 `unwrap_or_else(\|e\| e.into_inner())`（ai.rs 已示范） |
| **P0 须修** | 5 | `let _ =` 吞关键错误（D1：清数据 3 / 权限表 1 / 用户设置 1） | 失败收集后如实上抛 |
| **P1 建议修** | 12 | shift:// 响应构造 unwrap（B 节） | helper 化：log + 500 |
| **P1 建议修** | 2 | webview 几何 `position()/size()` | `unwrap_or_default` + log |
| **P1 建议** | ~40 | D2 日志补齐（webview 操作/窗口/下载状态/权限收紧等） | 按组加 `log::warn!`/`log::debug!` |
| **保留** | ~135 | D3 + C 节 SHORTCUTS + E 节 channel 发送 + G 节下标 | 无需动；SHORTCUTS 处建议补守卫单测（若无） |

**附注**：
- 正面先例三处可直接照抄范式：`ai.rs:394`、`protocol.rs:235`（毒化恢复）、`feed.rs:223` 注释（失败如实上报口径）。
- 建议拆两张修复卡：**卡 1** = P0 锁毒化 15 处（纯机械、低风险、可顺手给生产代码开 `#![deny(clippy::unwrap_used)]` 防回潮）；**卡 2** = P0 吞错 5 处 + P1 日志补齐（需要逐处看上下文，工作量中等）。
- 本卡不改任何代码；以上全部待拍板。

