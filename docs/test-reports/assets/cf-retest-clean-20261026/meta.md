# cf-retest-clean-20261026 采集 meta（干净复测）

## 为什么重测
上一轮（`cf-retest-20261026`）被协调器核验降级：并发污染（atomcode 实例在窗口内启动）、
CEF 用应用真实 profile（可能带历史 `cf_clearance`）、挑战框探针只看顶层 document。
本轮把三条全部堵住。

## 采集条件
- 时间窗：2026-10-26 11:17:31 起，共 **15.5 分钟**（≤30 分钟）
- 出口：mihomo 代理 → 美国 LAX，`warp=off`（海外口径，不代表国内）
- 被测：
  - CEF `Chrome/154.0.8037.98`，**临时 profile** `/tmp/cef-clean-1116`（`SHIFT_CEF_CACHE_DIR`）
  - Chrome `154.0.8037.57` 有头，**临时 profile** `/tmp/chrome-clean-1116`
  - 两侧均 CDP 驱动，端口 9336 / 9337
- 隔离：两个协作 AI 均已退出、无其他 shift-browser 实例，**环境静默**
- 每次访问前：`Network.clearBrowserCookies` + `Network.clearBrowserCache` + 先导航 about:blank
- 顺序：**严格交错**，每轮（run）交换先后手
- 判定：load 后固定等 8s 取状态，仍像挑战则再等 12s 复查

## 探针（本轮修正）
- 挑战框：顶层 `querySelector` **不再作为唯一依据**，改用 `Page.getFrameTree` 递归收集所有 frame URL，
  匹配 `challenges.cloudflare.com` → `has_cf_frame` / `cf_frames`
- 标签：严格三态 `pass` / `turnstile_or_challenge_shown` / `hard_block_or_stuck`
- **HTTP 4xx/5xx 必判拦截**（本轮修正：上一版会因标题"看起来正常"把 403 判成 pass，
  实测踩到 indeed `Security Check` 页）

## 结果（verdict_final）
| 站点 | CEF | Chrome |
|---|---|---|
| nodeseek.com | 3/3 pass | 3/3 pass |
| chatgpt.com | 3/3 pass | 3/3 pass |
| nowsecure.nl | 3/3 pass | 3/3 pass |
| indeed.com | 2 pass / 1 硬拦 | 1 pass / 2 硬拦 |
| linux.do（n=6） | 4 pass / 2 硬拦（403、429） | 3 pass / 3 硬拦（均 403） |
| **合计** | **15/18** | **13/18** |

订正记录：2 处（`chrome indeed#2`、`cef indeed#3`）原判 pass → `hard_block_or_stuck`，
依据是文档响应 403；订正方向**不偏袒任何一方**（前者对 Chrome 不利、后者对 CEF 不利）。

## 局限（必须一起看）
1. **海外 LAX 出口**，国内网络口径未取得
2. n 小（每站 3、linux.do 6），差异不能当统计显著
3. **全程 0 次出现 Turnstile 挑战框**（FrameTree 探针有能力，页面里确实没有）；
   「挑战框可点过」这一档本轮 0 观察
4. CEF 与 Chrome 内核小版本不同（154.0.8037.98 vs .57）
5. 两侧均 CDP 驱动，`navigator.webdriver` 状态一致，但都不是纯人工点击
6. 临时 profile 为全新创建（120M vs 应用真实 profile 125M），无历史 cookie
