# 全球浏览器隐私与指纹防护调研报告（2026 年版）

> 调研时间：2026-09-18 · 面向 ShiftBrowser 的技术选型参考
> 数据来源：PrivacyTests.org、EFF Cover Your Tracks、PCMag、Proton、WebTraceCheck、UCSD 论文等

---

## 一、2026 年的三个重大变化（先看懂大势）

### 1. Chrome Privacy Sandbox 已死
- 2025 年 10 月 Google 正式宣布**退役 Privacy Sandbox 核心 API**：Topics API、Attribution Reporting、Protected Audience、Fenced Frames、Shared Storage 等**均不再复活**。
- **第三方 Cookie 保留**：Google 2024 年反转后不再弃用，2026 年**没有任何弃用时间表**，改由用户自行决定是否屏蔽。
- 影响：Chrome 的"隐私叙事"崩塌，广告追踪能力实际上**回到 cookie + 指纹双轨**。

### 2. 指纹追踪成为主流追踪手段
- 2026 年**超过 1 万家顶级网站**部署指纹追踪，唯一识别率达 **80–90%**。
- Cookie 可以删、可以拦，**指纹删不掉**——成为跨设备、跨会话追踪的核心。

### 3. GPC（Global Privacy Control）成为法规刚需
- **加州 Opt Me Out Act**：要求 Chrome、Safari、Edge 在 **2027 年 1 月前**必须提供 GPC 能力；目前美国 12 个州已立法。
- 现状：**Firefox、Brave、DuckDuckGo 已原生支持**；Chrome / Edge **尚未支持**（只能用扩展，如 EFF Privacy Badger）。
- 技术实现极简：请求头 `Sec-GPC: 1`，网站可用 `/.well-known/gpc.json` 声明遵守。

---

## 二、主流浏览器隐私能力对比（2026）

### 综合排名（实用主义视角）

| 排名 | 浏览器 | 引擎 | 反指纹策略 | 强度 | 定位 |
|---|---|---|---|---|---|
| 1 | **Brave** | Chromium | **随机化**（farbling） | 强 | 日常实用首选 |
| 2 | **Mullvad Browser** | Gecko | **统一化**（全员一致） | 最强 | 不挂 Tor 的最强防护 |
| 3 | **Tor Browser** | Gecko | 统一化 + Tor 路由 | 最强 | 极限匿名 |
| 4 | **Firefox**（加固） | Gecko | RFP / FPP 可调 | 中→强 | 可配置性之王 |
| 5 | **LibreWolf** | Gecko | 默认开 RFP | 强 | Firefox 纯净分支 |
| 6 | **Safari** | WebKit | farbling（Safari 26 加噪） | 中 | 苹果生态内够用 |
| 7 | **Vivaldi** | Chromium | 无遥测（崩溃报告除外） | 中 | 功能党 |
| 8 | **Edge** | Chromium | 弱 | 弱 | 微软遥测 |
| 9 | **Chrome** | Chromium | 最弱 | 弱 | 广告模式冲突 |

### 指纹防护强度细节

| 浏览器 | Canvas | WebGL | AudioContext | 时区/语言 | 屏幕归一化 | 方法 |
|---|---|---|---|---|---|---|
| Tor Browser | 统一 | 统一 | 统一 | 统一 UTC/EN | letterboxing | 全员完全相同 |
| Mullvad | 统一 | 统一 | 统一 | 统一 | 是 | 同 Tor 方案 |
| Brave | 加噪 | 加噪 | 加噪 | 不统一 | 否 | 每站点每会话随机 |
| Firefox RFP | 加噪 | 加噪 | 加噪 | 统一 | letterboxing | 统一化 |
| Firefox FPP | 部分加噪 | 部分 | 部分 | 不统一 | 否 | 轻量默认保护 |
| Safari | 加噪 | 加噪 | 加噪 | 不统一 | 否 | farbling |
| Chrome | 无 | 无 | 无 | 不统一 | 否 | 基本裸奔 |

---

## 三、指纹技术全景：2026 年什么还有效

### 检测栈分层（从上到下，越靠前越难防）

| 层 | 检测内容 | 时机 | 可否被 JS 伪造 |
|---|---|---|---|
| **传输层** | TLS 指纹（JA4）、HTTP/2 行为、密码套件 | 页面加载前 | ❌ 不能（最强信号） |
| **浏览器信号** | Client Hints、User-Agent、请求头顺序 | 请求时 | ⚠️ 部分 |
| **JS API 层** | Canvas、WebGL、WebGPU、AudioContext、字体 | 页面加载后 | ✅ 可加噪/统一 |
| **行为层** | 鼠标轨迹、键入节奏、会话模式 | 全程 | ❌ 难伪造 |

### 仍有效的信号（按熵值排序）

1. **TLS 指纹（JA4）** —— **2026 年最可靠的信号**，JS 无法伪造，只有换浏览器/改指纹库才能变
2. **WebGPU 指纹** —— 2026 年**新兴高熵信号**，多数浏览器尚无防护
3. **Canvas 指纹** —— 仍是主力（GPU + 字体渲染差异 → 像素哈希）
4. **WebGL 指纹** —— GPU 型号、驱动、扩展列表
5. **AudioContext 指纹** —— 音频处理浮点差异
6. **字体列表** —— 系统已装字体指纹（国内用户字体组合很独特）
7. **屏幕参数** —— 分辨率、色深、devicePixelRatio、窗口尺寸
8. **时区 / 语言 / Accept-Language**
9. **硬件规格** —— `hardwareConcurrency`（CPU 核数）、`deviceMemory`
10. **网络信息** —— WebRTC 泄露真实 IP（VPN 用户的大坑）

### 已被削弱/阉割的信号

- `navigator.plugins` / `mimeTypes` —— Chrome 已基本清空
- `User-Agent` —— 被 Client Hints 取代，可伪造性上升

---

## 四、两条反指纹路线（技术选型关键）

### 路线 A：随机化（Randomization）
**代表**：Brave、Safari、Firefox canvas noise

- **做法**：Canvas/WebGL/Audio 的输出每次会话、每个站点加入**不同的微小噪声** → 同一台设备产生不同哈希。
- **优点**：网站功能不受影响，易用性好；扩展兼容性正常。
- **难点**（关键工程细节）：
  - 噪声必须 **"同站点内一致、跨站点/跨会话不同"**，否则追踪方可识别出"这人在加噪"（UCSD IMC25 论文有专门分析）。
  - 噪声过小 → 可被取平均值还原；过大 → 网站渲染异常。
  - 需要维护 **per-site seed**（每个源一个种子的确定性伪随机）。
- **Brave 实测**：CreepJS 跨会话测试确实每次指纹哈希都不同。

### 路线 B：统一化（Uniformity）
**代表**：Tor Browser、Mullvad Browser、Firefox RFP（含 letterboxing）

- **做法**：把所有用户的浏览器**伪装成一模一样**——统一 UA、统一时区（UTC）、统一语言（en-US）、统一屏幕尺寸（letterboxing 加黑边凑标准尺寸）、统一 canvas 输出。
- **优点**：**最抗指纹**（混在人群里，不是"独特"而是"大众脸"）。
- **缺点**：
  - **功能破坏大**（时区错乱、语言不符、页面有黑边、部分网站打不开）。
  - 需要大量"标准化池"，池子太小反而更独特。
- **Firefox 的双轨**：RFP（强、破坏大）vs **FPP**（`privacy.fingerprintingProtection`，轻量默认保护，LibreWolf 正在讨论从 RFP 转向 FPP）。

### 选型建议
| 场景 | 推荐路线 |
|---|---|
| 面向普通用户的自研浏览器 | **随机化**（不破坏体验，Brave 模式） |
| 面向高敏感用户/记者 | 统一化 + 可选 Tor 接入 |

---

## 五、国内浏览器隐私现状

### 现状扫描
- **QQ 浏览器 / 360 / 搜狗**：多伦多大学公民实验室（Citizen Lab）曾发布报告指出其数据传输存在**未加密/弱加密**问题；国内浏览器普遍与安全卫士、输入法、账号体系深度绑定（数据打通）。
- **合规压力**：中国《个人信息保护法》(PIPL) 实施后，中国网络空间安全协会推动 **62 款 App（含浏览器类）完成个人信息合规整改**（超范围收集、过度索权、注销不便等）。
- **国产"指纹浏览器"是反向赛道**：AdsPower、比特浏览器、Multilogin、GoLogin、NestBrowser 等，专门为**跨境电商养号、多账号隔离**服务——**主动伪造/隔离指纹**，与隐私保护方向相反，但技术可借鉴（它们对指纹面理解最深）。

### 对国内产品的启示
- 账号体系 + 云同步 = 天然的跨设备追踪能力，**自研浏览器若要打隐私牌，必须做到零账号关联、零云端指纹上传**。
- PIPL 要求"去标识化/加密"，可对标 GPC 做合规叙事。

---

## 六、对 ShiftBrowser 的落地建议（重点）

> ShiftBrowser 技术栈：Tauri 2 + WebKitGTK 4.1（Linux）+ 自研 newtab/标签管理

### 6.1 WebKitGTK 可用的控制面

| 层面 | 可控手段 |
|---|---|
| WebView Settings | UA、JS 开关、WebRTC、LocalStorage、图片自动加载、媒体播放策略 |
| UserScript 注入 | **改写 JS API**（Canvas/WebGL/Audio/时区/硬件信息）—— 最强手段 |
| 自定义协议响应头 | CSP、权限策略（Permissions-Policy）、`Sec-GPC` |
| 请求拦截 | 广告/追踪器拦截（已有内置词表）、URL 参数清洗 |

### 6.2 推荐实施清单（按性价比排序）

#### P0：低成本高收益
1. **发送 `Sec-GPC: 1` 请求头** —— 一行代码，法规趋势（2027 前后成强制），且无需处理网站适配。
2. **WebRTC 泄露防护** —— WebKitGTK settings 关闭/限制 `enable-webrtc`，或强制走代理。**VPN 场景下的第一号坑**。
3. **URL 参数清洗** —— 剥离 `utm_*`、`fbclid`、`gclid`、`msclkid` 等追踪参数（ShiftBrowser 可做，Brave 已有此功能）。
4. **第三方 Cookie 默认阻断** —— WebKitGTK 自带 ITP（Intelligent Tracking Prevention），确认开启并显式配置。
5. **UA 统一** —— 当前已设置 Chrome 120 完整 UA；建议**固定一个主流版本号**（别带罕见组合），避免成为稀缺指纹。

#### P1：核心反指纹（随机化路线）
6. **Canvas 加噪**（UserScript 注入）：
   - 挂 `HTMLCanvasElement.prototype.toDataURL` / `getImageData`，按 **per-origin seed** 加确定性微小噪声。
   - 关键：**同源同会话稳定、跨源/跨会话变化**（否则被识别为加噪用户）。
7. **WebGL 参数归一化**：`getParameter(UNMASKED_RENDERER_WEBGL)` 等返回通用值（如 "Apple M1" 之类大众型号），或加噪。
8. **AudioContext 加噪**：`AnalyserNode.getFloatFrequencyData` 输出加微噪。
9. **`hardwareConcurrency` / `deviceMemory` 归一化**：统一返回常见值（如 8 / 8GB），别上报真实 4 核或 32 核。
10. **屏幕尺寸归一化**（可选）：窗口尺寸取整到常见分辨率；如走统一化路线再考虑 letterboxing。

#### P2：进阶
11. **字体探针防护**：拦截 `document.fonts.check` 与 CSS 字体测量，返回固定字体集。
12. **时区/语言策略**：随机化路线下保持真实；统一化路线下归一化。
13. **WebGPU 指纹防护**（前瞻）：WebGPU 是 2026 新信号，提前拦截 `navigator.gpu` 的 adapter 信息读取。
14. **TLS 指纹（JA4）**：**最难处理**——需定制 TLS 栈（如 BoringSSL/rustls 的密码套件顺序），让 ShiftBrowser 的 TLS 指纹混入 Chrome/Firefox 主流群体，而不是"独一份的 WebKitGTK 指纹"。

### 6.3 自测方案（必做）

| 工具 | 测什么 |
|---|---|
| `coveryourtracks.eff.org` | 综合指纹唯一性 + 追踪器拦截（Bits of Identifying Information 越低越好） |
| `privacytests.org` | 跨站隔离、Cookie 分区、HTTPS 升级、参数清洗等逐项对比 |
| `browserleaks.com` | Canvas / WebGL / WebRTC / 字体 / 时区逐项细节 |
| `creepjs.org` | 指纹熵值 + 反检测能力（最严格） |

**验收标准建议**：
- Cover Your Tracks：至少 "strong protection against Web tracking"
- 多次会话访问 CreepJS：指纹哈希**不应完全一致**（走上随机化路线时）
- WebRTC 测试：不泄露真实内网/公网 IP

### 6.4 架构决策提醒
- **别做"孤本指纹"**：如果 ShiftBrowser 的指纹是全网独一份（WebKitGTK + 自定义特征），**比用 Chrome 更危险**——追踪方一眼就能锁定"这个人用的是 ShiftBrowser"。
- **首选策略**：**基础特征模仿主流 Chrome（UA/Client Hints/屏幕常见值）→ 高熵特征（Canvas/WebGL/Audio）加噪**，即"大众脸 + 每站可变"。
- **统一化的代价**：若要对标 Tor，需做 letterboxing（页面黑边）+ 统一时区语言，用户体验损失大，建议做成**开关**而非默认。

---

## 七、一页速查：各浏览器"该抄什么"

| 想学什么 | 抄谁 |
|---|---|
| 随机化加噪的工程细节（per-site seed） | **Brave**（开源，farbling 源码可读） |
| 统一化 + letterboxing 的标准化池设计 | **Tor Browser / Mullvad**（开源） |
| 细粒度反指纹开关（RFP/FPP 双轨） | **Firefox**（about:config 文档全） |
| 追踪参数清洗 + 分域存储 | **Brave / Safari** |
| 内置拦截词表 + 不破坏网站 | **Brave Shields** |
| 反检测（指纹伪造/隔离）的极致理解 | **AdsPower / Multilogin**（商业，思路可借鉴） |

---

## 八、结论

1. **2026 年隐私赛道的分水岭**：Chrome 的 Privacy Sandbox 死亡 = 浏览器厂商的隐私承诺重新洗牌，**"默认私密"成为差异化卖点**。
2. **反指纹已无"完美解"**：随机化（易用）与统一化（安全）必须二选一或分档提供；**孤本指纹比不防护更糟**。
3. **对 ShiftBrowser 最实际的路**：**模仿 Chrome 的基础特征 + Canvas/WebGL/Audio 随机化加噪 + Sec-GPC + WebRTC 防护 + 参数清洗**，用 CreepJS/Cover Your Tracks 做回归测试。
4. **法规红利**：GPC 2027 强制化在即，**现在就支持 GPC 等于提前合规**，且实现成本极低。

---

*报告整理：王二狗 · 存档于 ShiftBrowser 项目 docs/*
