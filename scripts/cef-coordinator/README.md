# 协调器自采工具（F-CEF-05 同轮对照）

**为什么进仓库**：这三个脚本原先只在 `/tmp/opencode/cef05/` 下，而 `/tmp` 是 **tmpfs（重启即失）**——
`.ai/evidence/20261026-0138-…同轮对照.md` §8 的复现命令引用的就是它们。不落库，那条"可复现"就是空话。

| 文件 | 作用 |
|---|---|
| `cef-eval.py` | 连 CDP 端口跑一段 JS，结果连同采集 meta（时刻/HEAD/引擎开关/出口）一起落盘。**注意：直连 page 的 WebSocket 即该 target 会话，再传 `sessionId` 会报 `-32001`** |
| `probe3.js` | 浏览器层自检项。**同一段 JS 必须打两侧**才叫同口径（现有 `chrome-probe.py fp` 的自检不采 WebGL / deviceMemory / screen / brands，照它采会漏判） |
| `compare2.py` | 严格对照表：SAME / DIFF / 探针假象（`webdriver`）/ GREASE 随机（不计判据）四类分好 |

```bash
# 采前必跑出口校验（详见 .ai/evidence/20261026-0820-出口取证-*.md）
scripts/net-egress-check.sh                      # 默认路径
scripts/net-egress-check.sh http://127.0.0.1:7897

# CEF（须 systemd-run 脱离调用方 cgroup，否则调用方一退实例陪葬）
systemd-run --user --unit=cef-recheck --collect \
  --setenv=DISPLAY=:0 --setenv=XAUTHORITY=/run/user/1000/.mutter-Xwaylandauth.XXXXXX \
  --setenv=SHIFT_CEF_DEBUG_PORT=9333 \
  --setenv=SHIFT_CEF_TEST_URL=https://tls.peet.ws/api/all \
  --setenv=LD_LIBRARY_PATH=$PWD/src-tauri/target/debug \
  ./src-tauri/target/debug/shift-browser
python3 scripts/cef-coordinator/cef-eval.py 9333 out-cef.json scripts/cef-coordinator/probe3.js

# 真 Chrome 有头对照（独立临时 profile）
systemd-run --user --unit=chrome-recheck --collect \
  --setenv=DISPLAY=:0 --setenv=XAUTHORITY=/run/user/1000/.mutter-Xwaylandauth.XXXXXX \
  /usr/bin/google-chrome --user-data-dir=/tmp/chrome-prof --remote-debugging-port=9334 \
  --no-first-run --ozone-platform=x11 https://tls.peet.ws/api/all
python3 scripts/cef-coordinator/cef-eval.py 9334 out-chrome.json scripts/cef-coordinator/probe3.js
python3 scripts/cef-coordinator/compare2.py      # 表里路径按需改
```
