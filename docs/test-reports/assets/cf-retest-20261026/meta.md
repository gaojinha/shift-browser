# cf-retest-20261026 采集 meta
- 时间窗：2026-10-26 10:21:16–10:33（共 12.1 分钟，≤30 分钟）；脚本 `scripts/cef-coordinator/cf-interleave.py`
- 出口：mihomo 代理 → 美国 LAX，`warp=off`（net-egress-check.sh 与两浏览器内 cdn-cgi/trace 确认），IP=<exit-ip>。**海外口径，不代表国内网络**。
- 被测：CEF=`src-tauri/target/debug/shift-browser`（00:51 构建，含 Vulkan，HEAD 4269ea5）；Chrome 154.0.8037.57 有头、独立临时 profile；两侧均 CDP 驱动
- 每次访问前清 cookie+缓存；两浏览器严格交错，奇偶轮交换先后手
- 判定：load 后固定等 8s 取状态；仍像挑战页再等 12s 复查（20s）。字段含 title/body_head100/has_challenge_platform/has_cf_iframe
- `has_challenge_platform=1` 在 nodeseek/nowsecure/linux.do/indeed 的正常页上也为 1（站点嵌 CF 脚本），不可单独作拦截依据
- 局限：本脚本的 verdict 为 pass / turnstile_or_challenge_shown / hard_block_or_stuck；本轮无任何一次出现 Turnstile iframe，故「可点过」中档为 0 次观察
