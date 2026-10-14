# 兼容性诊断 · Cloudflare 检测与视频闪烁根因（2026-10-14）

> 分析：王二狗（协调器）
> 方法：代码审计（useragent.rs / compat_chrome.js / env_setup.rs / feed.rs / open_read.rs）+ **实机实测**（shift MCP 驱动 + 真 Chrome 对照）+ 上游检索
> **结论先行**：
> 1. **Cloudflare / AI 检测**：核心矛盾在 **TLS / HTTP2 指纹层**（实测 GnuTLS 指纹与 Chrome 完全不符，JS 层补不到）——这是物理上限；但 **HTTP 请求头层有 5 处可补齐**（缺 Client Hints 全家等），是当前性价比最高的工程动作。
> 2. **视频闪烁**：锁定 **WebKitGTK DMA-BUF renderer 在 Wayland 下的 explicit sync 缺陷**（提交 surface 时不总设 acquire point）；第一步做 `GDK_BACKEND=x11` 对照实验即可鉴别路线。

---

## 一、Cloudflare / AI 检测问题

### 1.1 实测对比（2026-10-14，tls.peet.ws + httpbin.org）

用 ShiftBrowser 实机 与 真 Chrome 154（headless）分别访问指纹回显服务：

| 指纹层 | 真 Chrome 154 | ShiftBrowser | 判定 |
|---|---|---|---|
| JA3 | `ec6b4c1872353310772761665d27c840` | `500acc5cb01cf2866f8f1c36647ac69e` | ❌ 完全不同 |
| JA4 | `t13d1517h2_8daaf6152771_cb7bf5808d99` | `t13d2913h2_723694b0fccc_5671b5df5029` | ❌ 密码套件 **15 vs 29**、扩展 17 vs 13 |
| HTTP/2 (Akamai) | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` | `4:6291456;1:65536;2:0\|15663105\|0\|m,s,a,p` | ❌ 设置顺序不同、缺 `6:262144`、伪头顺序 s/a 反了 |
| UA | `Chrome/154` | `Chrome/142`（滞后 12 个大版本）| ⚠️ |

ShiftBrowser 实际发送的请求头（httpbin.org/headers 实读）：
`Accept / Accept-Encoding / Accept-Language: zh-CN / Dnt: 1 / Host / Sec-Fetch-Dest / Sec-Fetch-Mode / Sec-Fetch-Site / User-Agent`

**相对 Chrome 缺失**：`Sec-CH-UA`、`Sec-CH-UA-Mobile`、`Sec-CH-UA-Platform`（Client Hints 全家）、`Sec-Fetch-User`、`Upgrade-Insecure-Requests`
**多余**：`Dnt: 1`（Chrome 默认**不发**）
**模板差异**：Accept 为 WebKit 短模板（缺 `image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7` 段）；Accept-Language 无子项（Chrome 通常 `zh-CN,zh;q=0.9`）

### 1.2 分层结论

**第 1 层 · TLS 指纹（硬伤，JS 无法修）**
- Cloudflare 官方把 JA3/JA4 作为 bot 评分输入，并有 "**JA4 + UA 声明不符**" 的现成规则思路（官方博客 / WAF 规则文档 / 第三方抓 AI 爬虫实践）
- GnuTLS 与 BoringSSL 的 ClientHello 是**结构性差异**（套件数 29 vs 15），非配置能解
- 团队 2026-09-18 已尝试规范化（`G_TLS_GNUTLS_PRIORITY`）并回退（邮箱正文空白），结论与本次一致：**保持现有网络栈 => 无法伪装成 Chrome**
- 影响：开"激进 bot/AI 模式"的站点直接给高 bot 分；B站类服务端 TLS 风控同理（已有记录）

**第 2 层 · HTTP/2 指纹（硬伤同上）**
- libsoup 的帧序/伪头顺序与 Chromium 不同，无 JS 层修法

**第 3 层 · HTTP 请求头（可补，建议优先做）**
- 技术路径：WebKitWebExtension 的 `WebKitWebPage::send-request` 信号（请求发出前可改头）——需 PoC 验证 webkit2gtk-rs 封装情况（可能要少量 C 侧代码）
- 补齐清单（与 UA/userAgentData 版本号对齐）：
  1. `Sec-CH-UA: "Chromium";v="…", "Google Chrome";v="…", "Not-A.Brand";v="99"`
  2. `Sec-CH-UA-Mobile: ?0`
  3. `Sec-CH-UA-Platform: "Linux"`（按站点覆盖表联动 Windows/macOS）
  4. `Sec-Fetch-User: ?1`（导航请求）
  5. Accept 换 Chrome 长模板；Accept-Language 补 `zh;q=0.9` 子项
- **决策点（呈高哥）**：`Dnt: 1`（含 Sec-GPC 策略）的取舍——是我们的隐私立场招牌，也是风控侧异常特征。建议：默认对齐 Chrome 行为（不发），隐私增强独立开关，或最低限度对风控敏感站点不发。

**第 4 层 · JS 指纹（继续补，第二批）**
- 已有：userAgentData、deviceMemory（compat_chrome.js，2026-09-29）
- 缺口：`window.chrome` 对象、WebGL renderer 归一、plugins/mimeTypes 一致性、scheduler 完整化评估（现为禁用状态，语义不完整会卡 GitHub React——维持禁用合理）
- 验证工具：CreepJS / Cover Your Tracks（指纹研究文档既定验收标准）

**第 5 层 · 行为噪声（降噪）**
- AI 读取路径的 `verify_reachable` 先导 HEAD（reqwest、非浏览器特征）——现已失败降级为 warn 放行；建议给该请求配浏览器化头（低优先，安全语义不动）
- **多客户端指纹杂居**：同一 IP 上同时存在 WebKit(GnuTLS) / reqwest / curl / RSS(reqwest，自报 `ShiftBrowser/<ver> (RSS reader)`) 四种指纹
- **建议**：RSS/reader 自报 UA 评估去标识（改通用 UA），消除"ShiftBrowser 用户群体"被整包标记的风险——透明度 vs 过检的决策点，呈高哥

**第 6 层 · 版本号**
- UA 142 vs 当前 154：差 12 个大版本 = "年久失修 Chrome"画像，轻度异常信号
- 建议：季度跟版机制（UA / userAgentData / hints 三处同步，已有站点覆盖表机制，维护成本可控）

### 1.3 行动清单（建议排期）

| 优先级 | 行动 | 类型 | 验证方式 |
|---|---|---|---|
| P0 | HTTP 头补齐 PoC（5 项清单）| 工程 | peet/httpbin 复测 + CreepJS + site-compat 回归 |
| P0 | DNT/GPC 策略拍板 | 决策（高哥）| — |
| P1 | UA 版本跟版机制（142→当前区间）| 工程 | userAgentData 对齐复查 |
| P1 | JS 缺口第二批（window.chrome 等）| 工程 | CreepJS |
| P2 | 先导 HEAD 降噪 + RSS UA 去标识 | 工程/决策 | 抓包复测 |
| P2 | Cloudflare 实站验证集（需指定样本站点）| 测试 | 端到端复测 |

### 1.4 诚实结论
保持 WebKitGTK/GnuTLS 网络栈的前提下：**TLS/HTTP2 层矛盾无解**。将其余各层对齐到最优后，可过大多数轻中度检测；对"只看 TLS 指纹"的激进站点属物理上限（换网络栈为架构级决策，本报告不含结论）。

---

## 二、视频闪烁问题

### 2.1 事实链（均有出处）
1. **环境**：GNOME 49 / mutter 49 / **Wayland 原生**（进程实测连接 `memfd:wayland-cursor`、`libwayland-egl`）+ Intel HD 630 (i915) + 内核 6.17，WebKitGTK 2.52.3（系统最新，无可升级）
2. **解码路径**：全部视频走软件解码（env_setup.rs：`GST_PLUGIN_FEATURE_RANK` 压掉 v4l2/vaapi 硬解，修 H.264 对齐断言崩溃）
3. **团队实测（09-25/26）**：`WEBKIT_DISABLE_DMABUF_RENDERER=1` → 闪烁消失 + CPU -29%；但触发加速合成空指针崩溃（微博 100% 复现）→ 已回滚为默认关（设置项保留）
4. **上游机制（关键出处）**：`webkit2gtk_nvidia_quirk` 文档原文——"WebKitGTK 的 DMA-BUF renderer 在窗口 surface 上启用 Wayland explicit sync，但**提交前不总是设置 acquire point**"；不同合成器严格度不同：Hyprland 直接协议错误杀连接（WebKitGTK bug #280210），niri 容忍，**mutter 表现待验证**
5. **版本演进**：2.45.4 曾修 "video flickering with DMA-BUF sink"（2024-06）→ 2.54 又现新形态（禁用 dmabuf 时 overlay 闪烁，2026 社区 issue）——缺陷域活跃演进中；发行版普遍打补丁兜底（Gentoo 对 2.52.3 打 disable-nvidia-dmabuf）
6. 当前规避：`WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT=XR24:0`（修另一处空指针崩溃，进程内默认注入）

### 2.2 机制假设（置信度标注）
- 【高置信】闪烁与 **Wayland 合成路径的 buffer 同步**相关：视频帧/backing store 提交 mutter 时 acquire point 缺失或时序抖动 → 合成器短暂用未就绪帧 → 视觉闪烁
- 【中置信】"有的视频闪"取决于：分辨率/帧率（上传带宽）、播放器渲染方式（独立视频层/直扫路径是否触发）、窗口缩放/全屏切换状态
- 【待验证】mutter 49 对缺失 acquire point 的容忍行为（宽松→不崩，但可能产生时序性闪烁）

### 2.3 实验矩阵（按序执行）

| # | 实验 | 方法 | 判读 |
|---|---|---|---|
| E1 | 基线复现 | 默认设置播闪烁样本（先建"样本清单"：URL/编码/分辨率/全屏/显示器组合），录屏 | 锁定可复现样本集 |
| E2 | **X11 对照（先做）** | `GDK_BACKEND=x11 shift-browser` 播同样本 | X11 不闪→Wayland explicit sync 路线；仍闪→WebKit 内部渲染路线 |
| E3 | 关 renderer 复验 | `WEBKIT_DISABLE_DMABUF_RENDERER=1`，干净 profile 仅视频站点 | 09-25 结论在当前补丁版是否仍成立 |
| E4 | 关加速合成 | `WEBKIT_DISABLE_COMPOSITING_MODE=1` | 定位是否合成路径（性能代价大，仅诊断） |
| E5 | 版本对照 | 2.54+（Flatpak/备用环境） | 上游是否已改善 |

### 2.4 建议动作
1. **先跑 E2**：成本最低、鉴别力最强，结果直接决定后续路线
2. 若锁定 Wayland explicit sync：跟踪 WebKitGTK bug #280210 + mutter 上游；短期维持现设置项策略（文案注明"修闪烁、但重合成站点可能崩溃"）
3. **复核 09-26 崩溃结论**：在 2.52.3 的 Ubuntu 补丁迭代版上重测"禁用 dmabuf + 微博"是否仍崩——若空指针已被上游修，则默认策略可重估
4. 实验录像 + 样本清单存入 `docs/test-reports/` 作为回归资产

---

## 三、证据与复现

- **实测原始数据**：`/tmp/opencode/chrome-peet.json`（真 Chrome 指纹回显全文）；shift 实测为本次会话执行记录（tls.peet.ws / httpbin.org）
- **代码依据**：`src-tauri/src/app/env_setup.rs`（渲染/解码开关）、`src-tauri/src/useragent.rs`（UA 策略）、`src-tauri/src/compat_chrome.js`（指纹补齐）、`src-tauri/src/feed.rs`（pinned reqwest 出口）、`src-tauri/src/api/handlers/page/open_read.rs`（可达验证）、`src-tauri/src/extensions/adblock.rs`（Client Hints brands 注入）
- **上游检索**：webkit2gtk_nvidia_quirk（docs.rs）/ WebKitGTK bug #280210 / WebKitGTK 2.45.4 release notes / Cloudflare JA4 官方文档与博客 / Handy issue #2166（2.54 行为变化）
- **复现命令**：
  ```bash
  # shift 实测（经浏览器 MCP）：open_and_read tls.peet.ws/api/all、httpbin.org/headers
  # Chrome 对照：
  google-chrome --headless=new --disable-gpu --user-data-dir=/tmp/opencode/chrome-peet-profile \
    "https://tls.peet.ws/api/all" --dump-dom
  ```

## 四、局限与未决
- Cloudflare **实站挑战未端到端复现**（缺"过不去"的具体站点样本清单——建议高哥提供 2-3 个，我们跑受控复测）
- 视频闪烁**未能在本环境录屏**（需图形会话人工操作，样本清单待建）
- "换网络栈"（TLS 层根治）未评估：架构级决策，另行立项

---

*本文档由王二狗基于实测与代码原文整理；引用数字均可复现。发现与建议区分【观测】/【推断】已逐条标注。*
