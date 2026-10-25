# CEF 指纹实测报告（F-CEF-05 · M3）— 准备阶段版

> 状态：**准备项 1-4 已完成；CEF 构建实测栏位待 F-CEF-02（M2 标签接入）交付后填写，本文件中标「待测」的格子均无数据，不得当作结果引用。**
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
| 5 | `reader/fetch.rs:54`、`newtab/wallpaper.rs:21`、`translate.rs:18`、`api/handlers/page/open_read.rs:14`（MOBILE_UA）、前端 `PrivacyDataCard.vue:50,53`（预设 UA） | 各处独立复制的 142 UA 常量 | 散落副本（违反「同类只一份」守则，建议收口到 `useragent.rs`） |

**CEF 下的关键判断（待实测验证，现仅为推论）**：真 Chromium 自己发 UA 与 Client Hints，**应用层不应再覆盖**为 142，否则出现「JA4 是 Chrome 154、UA 是 142」的矛盾，恰是诊断报告 1.2 节第 1 层所说的「JA4 + UA 声明不符」规则。建议 CEF 构建默认不传 UA 覆盖（用内核原生 UA），仅在用户/站点显式覆盖时才改；`compat_chrome.js` 的 brands 伪装在 CEF 下也应关闭（`compat_chrome_fp` 设置项，`contracts.rs:65`），因为原生 `navigator.userAgentData` 已真实。**该推论需 M2 交付后在 CEF 构建上验证。**

**季度跟版机制（提案）**：
1. 单一常量：版本号从 `useragent.rs` 一处派生，散落副本（#5）改为引用；JS 侧 brands 通过注入参数取值而非硬编码。
2. 每季度复核（与 `strategy-2026.md` 阶段 0 引擎治理 / 标准跟踪同节奏，下次 2026-12-20）：对照 `cef` crate 当前 Chromium 主版本（现 154.5 → Chromium 154）与稳定版 Chrome 主版本，差距 >2 个大版本即升级。
3. 加一条单测：`DEFAULT_UA` 的主版本 == 常量 `CHROME_MAJOR`，防止只改一处。
4. WebKit 构建仍需伪装（保持现行机制）；CEF 构建走原生。

## 4. Cloudflare 实站验证集（待选定，需高哥/二狗确认）

我无法凭记忆断言哪些站点「已知会弹真人验证」——这类名单随时间和 IP 变化，**不编**。建议按下列分级征集样本，由高哥提供 3-5 个实际遇到过验证的站点，我在准备阶段只定口径：

| 级别 | 说明 | 样本 |
|---|---|---|
| 轻 | 仅 JS challenge / Turnstile 受管模式 | 待高哥提供 |
| 中 | 开启 Bot Fight Mode / 基于 JA3/JA4 评分 | 待高哥提供 |
| 激进 | 「只看 TLS 指纹」类 | 待高哥提供 |

对照口径：每站点 × {WebKit 构建, CEF 构建} × 各 5 次（间隔 ≥ 1 分钟，同 IP、同时段、清 cookie），记录「直接放行 / 出现 Turnstile 勾选 / 硬拦截（1020/403）」三态；通过率 = 直接放行 + 一次点击通过 / 总次数。验收门槛取自规划：CEF > WebKit，轻中度 ≥ 90%。样本量小，结论只作方向，不作统计显著。

## 5. 指纹面板对比方案（CreepJS / Cover Your Tracks）

- 对象：真 Chrome 154（有头）· WebKit 构建 · CEF 构建，三方同机同会话。
- CreepJS（`abrahamjuliot.github.io/creepjs`）：记录 trust score / lies 数 / headless 与 stealth 检测项；截图留档到 `docs/test-reports/assets/`。
- Cover Your Tracks（`coveryourtracks.eff.org`）：记录 fingerprinting 唯一性比特数与「strong protection」判定。
- 判据：CEF 与真 Chrome 在 lies / 一致性项上无新增差异；有差异则列表并标注是否由应用层注入脚本（`compat_chrome.js`、`privacy.js`）造成。CEF 下注入的伪装脚本是最可能引入 lies 的来源，建议先在「全关注入」与「现状注入」两种配置各跑一次。
- 这两个站点均为第三方服务，访问仅发送浏览器自身指纹，不上传项目代码或数据。

## 6. 待办与阻塞

- [ ] 阻塞：F-CEF-02 交付后，在 CEF 构建上补第 1 节「CEF 构建」列、第 5 节结果、第 3 节推论验证。
- [ ] 补有头 Chrome 154 基线。
- [ ] 请求头补齐（规划 M3 任务 2）：仅当 CEF 实测仍缺项才做；`Dnt` 取舍仍待高哥拍板（诊断报告 1.3 P0 决策）。
- [ ] 高哥提供 Cloudflare 样本站点。
