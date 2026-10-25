# CEF 指纹实测报告（F-CEF-05 · M3）— 准备阶段版

> 状态：**准备项 1-4 已完成；2026-10-25 20:00 补：有头基线（§1.1）、Cloudflare 真 Chrome 实测（§4）、DNT/GPC 实测结论（§7）。CEF 构建实测栏位待 F-CEF-02（M2 标签接入）交付后填写，标「待测」的格子均无数据，不得当作结果引用。**
> 取数时间：2026-10-25 19:18–19:20（`date` 实跑）。采集脚本：`scripts/fingerprint-collect.py`。

## 1. 真 Chrome 154 基线（本机实测）

环境：本机 `google-chrome` = `154.0.8037.57`，`--headless=new --no-sandbox`，访问 `https://tls.peet.ws/api/all` 与 `https://httpbin.org/headers`。

| 项 | 真 Chrome 154（本次实测） | 2026-10-14 诊断报告所记 Chrome 154 | ShiftBrowser WebKit（诊断报告 10-14） | CEF 构建 |
|---|---|---|---|---|
| JA4 | `t13d1517h2_8daaf6152771_cb7bf5808d99` | 同 | `t13d2913h2_723694b0fccc_5671b5df5029` | 待测 |
| JA3 hash | `c1b4c3933042b9ef29473a406fbeac3b` | `ec6b4c18…`（不同） | `500acc5c…` | 待测 |
| peetprint hash | `fc97c1cdfb1409c9a9326c1b726d1dee` | — | — | 待测 |
| Akamai H2 | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` | 同 | `4:6291456;1:65536;2:0\|15663105\|0\|m,s,a,p` | 待测 |
| 密码套件数（去 GREASE） | 15 | 15 | 29 | 待测 |
| 扩展数 | 19 | 17（JA4 口径） | 13 | 待测 |

**解读（对比口径）**：
- JA3 hash 两次采集 Chrome 都不同，原因是 Chrome 自 110 起随机打乱扩展顺序；**JA3 不能作一致性判据，以 JA4（对扩展排序后哈希）+ Akamai H2 为准**。这里 JA4 与诊断报告完全一致，可作稳定基线。
- 扩展数 19（peet 原始数组，含 GREASE/padding）与 JA4 里的 `17` 口径不同，不要直接比；对比时两边都用同一脚本输出。

真 Chrome 实读请求头（httpbin，headless，去 `Host`/`X-Amzn-Trace-Id`）：
`Accept`（长模板）· `Accept-Encoding: gzip, deflate, br, zstd` · `Accept-Language: zh-CN,zh;q=0.9` · `Priority: u=0, i` · `Sec-Ch-Ua: "Chromium";v="154", "Google Chrome";v="154", "Not A(Brand";v="99"` · `Sec-Ch-Ua-Mobile: ?0` · `Sec-Ch-Ua-Platform: "Linux"` · `Sec-Fetch-Dest/Mode/Site/User` · `Upgrade-Insecure-Requests: 1` · `User-Agent`。**无 `Dnt`**。

**基线局限（诚实声明）**：headless 的 UA 是 `HeadlessChrome/154.0.0.0`，且 headless 不一定完全等于有头 Chrome；正式对比前应补一份**有头** Chrome 154 采集（需 X11 会话）。TLS/H2 层 headless 与有头同一网络栈，预期一致，但这是预期不是实测。

### 1.1 有头 vs headless（2026-10-25 19:50 实测，同机同网络，`scripts/chrome-probe.py fp`）

有头：`--ozone-platform=x11`（XWayland）真窗口；headless：`--headless=new`。均为独立临时 profile、CDP 驱动。出口 IP 为本机代理出口（洛杉矶 LAX，证据中已脱敏为 `<exit-ip>`）。

| 项 | headless | 有头 | 判定 |
|---|---|---|---|
| JA4 | `t13d1517h2_8daaf6152771_cb7bf5808d99` | 同 | 一致 |
| peetprint hash | `fc97c1cdfb1409c9a9326c1b726d1dee` | 同 | 一致 |
| Akamai H2 | `1:65536;2:0;4:6291456;6:262144\|15663105\|0\|m,a,s,p` | 同 | 一致 |
| 套件数 / 扩展数 | 15 / 19 | 15 / 19 | 一致 |
| JA3 hash | `1e104bb0…` | `042e2884…` | 差异（扩展乱序，预期，本文已定不作判据） |
| 请求头（Accept/Encoding/Language/Priority/Sec-CH-UA 三项/Sec-Fetch 四项/Upgrade-Insecure-Requests） | — | — | 全部一致 |
| User-Agent | `…HeadlessChrome/154.0.0.0…` | `…Chrome/154.0.0.0…` | **差异，仅此一处** |

**结论**：TLS/H2/请求头层 headless 与有头逐项一致（§1 里"预期一致"现已实测证实），可互用作基线；仅 UA 里的 `Headless` 字样不同，**对比 CEF 时以有头为准**。有头 Chrome 154 的 `navigator.userAgentData`：brands `Chromium 154 / Google Chrome 154 / Not A(Brand 99`，fullVersionList `154.0.8037.57`，platform `Linux`（原始值见 `assets/cef-fp-2026-10-25/headed-meta.json`）。

**探针局限**：CDP 驱动下 `navigator.webdriver === true`（有头无头皆然），真用户浏览器是 `false`；加 `--disable-blink-features=AutomationControlled` 后为 `false`（已实测）。这影响 §4 的 Cloudflare 结果，见该节。

## 2. 采集流程（可复现）

```bash
# 基线
google-chrome --headless=new --no-sandbox --virtual-time-budget=8000 --dump-dom https://tls.peet.ws/api/all > chrome-peet.html
google-chrome --headless=new --no-sandbox --virtual-time-budget=8000 --dump-dom https://httpbin.org/headers > chrome-headers.html
python3 scripts/fingerprint-collect.py summarize chrome-peet.html chrome-headers.html > base.json
# CEF 构建：应用内打开同两个 URL，把页面文本存成文件（peet 的 JSON 直接存即可；脚本兼容纯 JSON 与 <pre> 包裹）
python3 scripts/fingerprint-collect.py summarize cef-peet.json cef-headers.json > cef.json
python3 scripts/fingerprint-collect.py compare base.json cef.json   # 输出逐项差异表，贴入第 5 节
```

CEF 构建若 UA 仍是应用层设置的 142，`user_agent` 与 `Sec-Ch-Ua` 行会显示差异——这正是第 3 节要对齐的。

## 3. UA 跟版对齐清单（现状 → 154）

`useragent.rs::DEFAULT_UA` 是 Chrome/142。以下是代码里**所有**写死 142 的位置（grep 实查）：

| # | 位置 | 内容 | 类别 |
|---|---|---|---|
| 1 | `src-tauri/src/useragent.rs:20` `DEFAULT_UA`；`:28` `chrome_major` 的 `unwrap_or(142)`；测试 `:229/:239-242/:261` | 主 UA 与解析兜底 | **UA 本体（同步点 1）** |
| 2 | `src-tauri/src/compat_chrome.js:41-42,46-47,62` | `userAgentData.brands`/`fullVersionList`/`uaFullVersion` 写死 142 | **userAgentData（同步点 2）** |
| 3 | `src-tauri/src/extensions/adblock.rs:159` 经 `brands_json(ua_for(""))` 动态注入 | 已随 UA 动态生成（privacy.js 消费） | 已动态，仅依赖点 1 |
| 4 | Client Hints **请求头** `Sec-CH-UA*` | WebKit 下无（诊断报告第 3 层）；CEF 下由 Chromium 自带，版本取自 CEF 内核而非应用设置 | **Client Hints（同步点 3）** |
| 4b | `src/utils/compat_fp_dom.test.ts:43` `expect(he['uaFullVersion']).toContain('142')` | 前端 vitest 真断言，跟版到 154 会红 | **测试断言（随同步点 2 一起改）** |
| 5 | `reader/fetch.rs:54`、`newtab/wallpaper.rs:21`、`translate.rs:18`、`api/handlers/page/open_read.rs:14`（MOBILE_UA）、前端 `PrivacyDataCard.vue:50,53`（预设 UA） | 各处独立复制的 142 UA 常量 | 散落副本（违反「同类只一份」守则，建议收口到 `useragent.rs`） |

**CEF 下的关键判断（待实测验证，现仅为推论）**：真 Chromium 自己发 UA 与 Client Hints，**应用层不应再覆盖**为 142，否则出现「JA4 是 Chrome 154、UA 是 142」的矛盾，恰是诊断报告 1.2 节第 1 层所说的「JA4 + UA 声明不符」规则。建议 CEF 构建默认不传 UA 覆盖（用内核原生 UA），仅在用户/站点显式覆盖时才改；`compat_chrome.js` 的 brands 伪装在 CEF 下也应关闭（`compat_chrome_fp` 设置项，`contracts.rs:65`），因为原生 `navigator.userAgentData` 已真实。**该推论需 M2 交付后在 CEF 构建上验证。**

**季度跟版机制（提案）**：
1. 单一常量：版本号从 `useragent.rs` 一处派生，散落副本（#5）改为引用；JS 侧 brands 通过注入参数取值而非硬编码。
2. 每季度复核（与 `strategy-2026.md` 阶段 0 引擎治理 / 标准跟踪同节奏，下次 2026-12-20）：对照 `cef` crate 当前 Chromium 主版本（现 154.5 → Chromium 154）与稳定版 Chrome 主版本，差距 >2 个大版本即升级。
3. 加一条单测：`DEFAULT_UA` 的主版本 == 常量 `CHROME_MAJOR`，防止只改一处。
4. WebKit 构建仍需伪装（保持现行机制）；CEF 构建走原生。

## 4. Cloudflare 实站验证集（真 Chrome 154 有头实测；CEF/WebKit 列待测）

**选站方法**：先 `curl -I` 看响应头 `server: cloudflare` + `cf-ray` 确认确实走 Cloudflare（zhihu = `BLB`、hostloc = `Quantum`，不是 CF，已排除）；再用真 Chrome 实测，**不凭记忆断言**哪个站会弹验证。5 站：中文 `linux.do`、`nodeseek.com`；国际 `chatgpt.com`、`indeed.com`、`nowsecure.nl`（bot 检测测试站）。

**方法**：`scripts/chrome-probe.py cf`，有头 Chrome 154.0.8037.57，独立临时 profile，每次访问前清 cookie+缓存；记主文档 HTTP 状态、`cf-mitigated` 响应头、8 秒后页面标题/正文长度，仍在挑战页再等 12 秒复查。同出口 IP（LAX）、同时段（20:00–20:13）。原始数据：`assets/cef-fp-2026-10-25/cf-*/cf-raw.json`。

### 4.1 默认 CDP 驱动（`navigator.webdriver=true`），每站 3 次，间隔 3 秒

| 站点 | 放行 | 停在挑战页 | 备注 |
|---|---|---|---|
| nodeseek.com | 3/3 | 0 | 主文档 200，无 `cf-mitigated` |
| chatgpt.com | 3/3 | 0 | 同上 |
| nowsecure.nl | 3/3 | 0 | 页面含 Turnstile 脚本但正常出内容 |
| linux.do | 1/3 | 2/3 | 第 2、3 次主文档 **403 + `cf-mitigated: challenge`**，标题"请稍候…"，正文长度 0，20 秒后仍未放行 |
| indeed.com | 2/3 | 1/3 | 第 1 次 403 challenge 且 8 秒后正文长度 0；第 2、3 次出内容 |

### 4.2 linux.do 加测

| 条件 | 结果 |
|---|---|
| 间隔 60 秒、默认 CDP，3 次 | **0/3**（403、403、**429**，均停在"请稍候…"） |
| 加 `--disable-blink-features=AutomationControlled`（`webdriver=false`），间隔 20 秒，3 次 | 3 次首包均 403 challenge，**其中 2 次随后自动通过**出内容（run1 出的是站点的反 AI 提示页，不算正常内容；run2 出正常首页），1 次停在挑战页 |
| 同上条件的 indeed.com，3 次 | 首包均 403 challenge；run1 落到 Cloudflare 排障页"需要进行其他验证"（硬拦），run2/3 自动通过出内容 |

### 4.3 能得出与不能得出的结论

- **可以确认**：从本机这个出口 IP，**真 Chrome 154 有头也会被 linux.do、indeed.com 触发 Cloudflare challenge**；nodeseek/chatgpt/nowsecure 不触发。"验证集"里有 2 站是真会拦的、3 站是对照放行的，口径可用。
- **可以确认**：`webdriver=true` 的自动化标记有影响方向（linux.do 默认 CDP 共 1/6 放行，去掉标记后 1/3 出正常内容、另 1/3 过了挑战但落在站点反 AI 提示页），**因此 CEF 对照时必须用同口径**——CEF 用人工点开或同样的 CDP 方式，而不是拿 CDP 驱动的 CEF 去比人工的 Chrome。
- **不能确认**：规划关口"CEF > WebKit 且轻中度 ≥ 90%"——**真 Chrome 本身在 linux.do 都做不到 90%**，该关口对强风控站点可能不合理，需要高哥/二狗决定是否改成"CEF ≥ 真 Chrome 同口径通过率 − X"。这是实测暴露的口径问题，不是我调低标准。
- **样本局限**：每格 n=3，各条件顺序执行未交错（IP 信誉随时间漂移会混入），出口是海外代理 IP，**与高哥国内直连网络结果可能完全不同**。只作方向。
- **未核实**：是否出现需要人手点击的 Turnstile 勾选框——我保存了截图但无法读取图像内容，因此**没有"需点击"的观察结论**，上表只是"自动通过 / 停在挑战页 / 排障页"三态，来自 DOM 文本与响应头。

### 4.4 CEF / WebKit 列

| 站点 | 真 Chrome 默认 CDP | 真 Chrome 去自动化标记 | CEF 构建 | WebKit 构建 |
|---|---|---|---|---|
| linux.do | 1/6（间隔 3s 与 60s 合计） | 1/3 正常内容（另 1/3 过挑战但是反 AI 提示页） | 待测 | 待测 |
| indeed.com | 2/3 | 2/3 | 待测 | 待测 |
| nodeseek.com | 3/3 | — | 待测 | 待测 |
| chatgpt.com | 3/3 | — | 待测 | 待测 |
| nowsecure.nl | 3/3 | — | 待测 | 待测 |

## 5. 指纹面板对比方案（CreepJS / Cover Your Tracks）

- 对象：真 Chrome 154（有头）· WebKit 构建 · CEF 构建，三方同机同会话。
- CreepJS（`abrahamjuliot.github.io/creepjs`）：记录 trust score / lies 数 / headless 与 stealth 检测项；截图留档到 `docs/test-reports/assets/`。
- Cover Your Tracks（`coveryourtracks.eff.org`）：记录 fingerprinting 唯一性比特数与「strong protection」判定。
- 判据：CEF 与真 Chrome 在 lies / 一致性项上无新增差异；有差异则列表并标注是否由应用层注入脚本（`compat_chrome.js`、`privacy.js`）造成。CEF 下注入的伪装脚本是最可能引入 lies 的来源，建议先在「全关注入」与「现状注入」两种配置各跑一次。
- 这两个站点均为第三方服务，访问仅发送浏览器自身指纹，不上传项目代码或数据。

## 6. 待办与阻塞

- [ ] 阻塞：F-CEF-02 交付后，在 CEF 构建上补第 1 节「CEF 构建」列、第 5 节结果、第 3 节推论验证。
- [x] 关口口径已裁定（2026-10-25 20:20 高哥）：**CEF ≥ 同口径真 Chrome**，原「轻中度 ≥ 90%」作废。
- [x] 补有头 Chrome 154 基线（§1.1，2026-10-25 19:50）。
- [ ] 请求头补齐（规划 M3 任务 2）：仅当 CEF 实测仍缺项才做；DNT/GPC 已给结论（§7），待高哥拍板。
- [x] Cloudflare 样本站点：已自选 5 站并实测真 Chrome（§4）；CEF/WebKit 列待 F-CEF-02。
- [ ] 关口口径："通过率 ≥ 90%"对强风控站点真 Chrome 自己都达不到（§4.3），请高哥/二狗裁定。

## 7. DNT / GPC 结论（原则：不破坏与真 Chrome 指纹一致性优先）

### 7.1 实测证据（真 Chrome 154.0.8037.57，本地回显服务器抓请求头，`chrome-probe.py hdr`，原始：`assets/cef-fp-2026-10-25/hdr-headed.json`）

| 条件 | 请求头 `DNT` | 请求头 `Sec-GPC` | `navigator.doNotTrack` | `navigator.globalPrivacyControl` |
|---|---|---|---|---|
| 全新 profile 默认 | **无** | **无** | `null` | `undefined` |
| 预置 Preferences `enable_do_not_track=true`（等同用户在设置里开） | `1` | **无** | `"1"` | `undefined` |

即：真 Chrome 154 **默认两个都不发**；用户主动开 DNT 才发 `DNT: 1`；**这个版本的 Chrome 无 GPC 实现**（开了 DNT 也没有 `Sec-GPC`，`navigator.globalPrivacyControl` 不存在）。

### 7.2 本项目现状（grep 实查）

- `contracts.rs:89` / `src/constants.ts:68`：`gpc_header` **默认 `true`**；`db/settings.rs:290-299` 无设置时视为开，写入 `Sec-GPC: 1` 到每个出网请求。
- `extensions/privacy.rs:167` / `privacy.js`：注入 `navigator.globalPrivacyControl = true`。
- 结果：**WebKit 构建当前对每个请求发 `Sec-GPC: 1` 且暴露 `globalPrivacyControl`——两处都是真 Chrome 154 没有的特征**。诊断报告（10-14）所说的 `Dnt: 1` 现在代码里已不是默认发（只剩用户自定义请求头框里的示例）。

### 7.3 结论

| 项 | 结论 | 依据 |
|---|---|---|
| **DNT** | **不建议默认发**。仅作为用户显式开关（对齐 Chrome 设置项行为，开了才发 `DNT: 1` 并让 `navigator.doNotTrack="1"`） | §7.1 实测：真 Chrome 默认不发；开启后行为已摸清可复刻 |
| **GPC（请求头 + JS 属性）** | **不建议在 CEF 构建默认发**；保持为独立、默认关的隐私增强开关 | §7.1 实测：真 Chrome 154 既无头也无属性，默认发就是比真 Chrome 多出的可识别特征 |
| 例外 | 无 | 没有实测证据表明发这两个头能提高通过率或降低风险 |

**诚实声明**：
1. **我没有做"带/不带 Sec-GPC 对 Cloudflare 通过率"的 A/B**——§4 显示 linux.do 在同条件下通过率本身波动很大（0/3～2/3），n=3 的 A/B 分不出信号。因此"发 GPC 会触发 CF"**没有实测证据**，我的"不建议"依据的是**一致性原则**（与真 Chrome 不同），不是"已证明会被拦"。
2. GPC 有**法规层面**价值（strategy-2026.md:103 记加州 2027-01 强制）。这是产品/合规决策，不在指纹范畴，超出我的证据范围，需高哥拍板：若要保留，建议只在 WebKit 构建保留现状，CEF 构建默认关并在设置页独立开关，且开关处注明"开启后与普通 Chrome 行为不同，部分风控站点更易识别"。
3. 只测了 Chrome 154 的一个构建，不代表以后版本（Chrome 若后续加 GPC 支持，需在季度跟版时复测）。

**最终裁定（2026-10-25 20:20 高哥拍板）**：
- **GPC：CEF 构建默认关**（用内核原生行为，不发 `Sec-GPC`、不注入 `navigator.globalPrivacyControl`）；
  **WebKit 构建保持现状**（合规表态保留）；设置页给独立开关并注明「开启后与普通 Chrome 行为不同」。
- **DNT：不默认发**，仅作为用户显式开关。
- 实现归入 M3 任务 2，见 `docs/CEF换核规划_2026-10-25.md`。
