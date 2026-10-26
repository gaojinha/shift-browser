#!/usr/bin/env bash
# F-CEF-09 补测：copilot 13:00 验收回执里「没做成」的那步——手动 resize 后工具栏高度是否跟随。
# 它的推断：apply_toolbar_height() 只在 toggle_bookmarks / switch_tab 调用，窗口 resize 无回调，
#            BoxLayout 重排可能把收起态(88)顶回缓存的 122 → 底部 34px 空白。
# 判据（窗口高 = 工具栏高 + 内容页 innerHeight，已验证 1:1 无缩放）：
#   收起态 resize 到 H：内容 innerHeight == H-88  → 跟随正确，copilot 推断不成立
#                      == H-122 → 回落，它的推断成立，本卡有条件通过转为不过
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="$REPO/src-tauri/target/debug/shift-browser"
TS="$(date +%H%M%S)"
UNIT="shift-cef-rz-$TS"
PROF="/tmp/opencode/rz-prof-$TS"; OUTD="/tmp/opencode/rz-out-$TS"
mkdir -p "$PROF" "$OUTD"
export DISPLAY=:0
export XAUTHORITY="$(ls -1 /run/user/1000/.mutter-Xwaylandauth.* 2>/dev/null | head -1)"
say() { printf '%s\n' "$*"; }

# ── 0. 双查排他（进程级 + 端口级，只看单元名会漏）────────────────
# 他人实例在跑时是「等」不是「抢」：FORCE=1 也不能硬起，因为 single_instance 会把
# 第二实例的参数转发给活着的实例（等于污染别人的验收现场），转发后自退、CDP 永远不起来。
if [ "${FORCE:-0}" != "1" ]; then
  for i in $(seq 1 ${WAITMAX:-90}); do
    p="$(pgrep -f "^$BIN$" | head -1 || true)"
    [ -z "$p" ] && break
    say "$(date '+%T') 他人主进程 $p 在跑，等待…（第 ${i}/${WAITMAX:-90} 轮）"; sleep 10
  done
  p="$(pgrep -f "^$BIN$" | head -1 || true)"
  [ -n "$p" ] && { say "ABORT 等待超时，主进程 $p 仍在（别抢，会污染其现场）"; exit 90; }
fi
[ -z "$XAUTHORITY" ] && { say "ABORT 拿不到 XAUTHORITY（无显示权限，别硬跑）"; exit 93; }
PORT=""; for x in 9342 9343 9344; do ss -ltn 2>/dev/null | grep -q ":$x " || { PORT="$x"; break; }; done
[ -z "$PORT" ] && { say "ABORT 无空闲端口"; exit 91; }

# ── 1. 被测身份：跑之前记死 ────────────────────────────────────
{ say "采集: $(date '+%F %T%z')  目的: F-CEF-09 resize 压测（copilot 缺条件那步）"
  say "单元 $UNIT  端口 $PORT  profile $PROF"
  say "二进制 mtime=$(stat -c '%y' "$BIN"|cut -c1-19)  sha256_12=$(sha256sum "$BIN"|cut -c1-12)"
  say "HEAD=$(git -C "$REPO" rev-parse --short HEAD)  脏: $(git -C "$REPO" status --short -- src-tauri/ | tr '\n' ' ')"
} | tee "$OUTD/meta.md"

systemd-run --user --unit="$UNIT" --collect \
  --setenv=DISPLAY=:0 --setenv=XAUTHORITY="$XAUTHORITY" \
  --setenv=SHIFT_CEF_DEBUG_PORT="$PORT" --setenv=SHIFT_CEF_SHELL_URL=https://example.com \
  --setenv=SHIFT_CEF_CACHE_DIR="$PROF" --setenv=GDK_BACKEND=x11 \
  --setenv="LD_LIBRARY_PATH=$REPO/src-tauri/target/debug" \
  "$BIN" > "$OUTD/systemd-run.log" 2>&1
ok=0
for i in $(seq 1 45); do
  curl -sf -m 2 "http://127.0.0.1:$PORT/json" >/dev/null 2>&1 && { ok=1; break; }
  journalctl --user --since "-1 min" --no-pager 2>/dev/null | grep -qa "已有实例收到新实例参数" && { say "FAIL 被 single_instance 拦"; break; }
  sleep 1
done
say "CDP 就绪=$ok（等 ${i}s）"
[ "$ok" != 1 ] && { systemctl --user stop "$UNIT" 2>/dev/null; rm -rf "$PROF"; exit 92; }
sleep 4
cd "$REPO"

# ── 2. 定位主窗口：按「几何自洽」选，不是按「先匹配」选 ──────────
# 教训（12:58 那轮全废）：同一 pid 常有 3 个窗（10x10 宿主、"Shift Browser"、"Shift"），
# 取第一个 ≥600x300 的会挑到「Shift Browser」那个 837 高的窗，而 837 ≠ 122+738（内容页实高）
# → 挑错窗，键发到不收 CEF 快捷键的窗上，offsetHeight 全程 34，整批数据作废。
# 正解：先 CDP 读一次内容页 innerHeight，再挑 |窗高 − (工具栏 + 内容)| 最小的那个窗。
PID="$(pgrep -f "^$BIN$" | head -1)"
echo '({innerHeight: window.innerHeight, innerWidth: window.innerWidth, href: location.href})' > /tmp/opencode/h.js
python3 scripts/cef-coordinator/cdp-eval-target.py "$PORT" example.com "$OUTD/pre-page.json" /tmp/opencode/h.js >/dev/null 2>&1
CONTENT_H="$(python3 -c "import json;print(json.load(open('$OUTD/pre-page.json'))['eval']['innerHeight'])" 2>/dev/null || echo 0)"
say "内容页实高=$CONTENT_H（工具栏展开 122 → 期望主窗高≈$((CONTENT_H+122))）"
CANDS=""; XID=""; BESTDIFF=99999
for w in $(xdotool search --name "" 2>/dev/null | head -300); do
  [ "$(xdotool getwindowpid "$w" 2>/dev/null)" = "$PID" ] || continue
  geo="$(xdotool getwindowgeometry "$w" 2>/dev/null | sed -n 's/.*Geometry: \([0-9]*\)x\([0-9]*\).*/\1 \2/p')"
  W="${geo%% *}"; H="${geo##* }"
  [ "${W:-0}" -ge 600 ] && [ "${H:-0}" -ge 300 ] || continue
  NAME="$(xdotool getwindowname "$w" 2>/dev/null)"
  DIFF=$(( H - CONTENT_H - 122 )); [ "$DIFF" -lt 0 ] && DIFF=$(( -DIFF ))
  CANDS="$CANDS  XID=$w 名='$NAME' 几何=${W}x${H} 与期望差=$DIFF\n"
  if [ "$DIFF" -lt "$BESTDIFF" ]; then BESTDIFF="$DIFF"; XID="$w"; GEO0="${W}x${H}"; fi
done
[ -z "$XID" ] && { say "FAIL 找不到主窗口（XID 空）→ 按键/resize 全无效，硬失败退出"; printf "$CANDS"; systemctl --user stop "$UNIT" 2>/dev/null; rm -rf "$PROF"; exit 94; }
say "主窗口 XID=$XID  初始几何=$GEO0  与期望差=$BESTDIFF（差>40 说明挑的窗可能不对，慎用）"
printf "$CANDS" | sed 's/^/  /'

TB() { python3 scripts/cef-coordinator/cdp-eval-target.py "$PORT" toolbar.html "$1" scripts/cef-coordinator/ff09-probe.js >/dev/null 2>&1; }
PG() { echo '({innerHeight: window.innerHeight, innerWidth: window.innerWidth, href: location.href})' > /tmp/opencode/h.js
       python3 scripts/cef-coordinator/cdp-eval-target.py "$PORT" example.com "$1" /tmp/opencode/h.js >/dev/null 2>&1; }
R() { python3 -c "
import json;d=json.load(open('$1'));e=d.get('eval',{})
print('   ', json.dumps({k:e.get(k) for k in ('bookmarks_offsetHeight','html_class','innerHeight','innerWidth')},ensure_ascii=False))"; }
GEO() { xdotool getwindowgeometry "$XID" 2>/dev/null | sed -n 's/.*Geometry: \([0-9]*x[0-9]*\).*/\1/p'; }

press() {
  # 12:52 那轮能成，靠的是「先 windowactivate --sync 再 key」；12:58 只发 key 就全废。
  # 三条都发一遍：activate 拿焦点 → --window 定向发 → 裸 key 发给焦点窗兜底。
  xdotool windowactivate --sync "$XID" 2>/dev/null; sleep 0.6
  xdotool key --window "$XID" --clearmodifiers ctrl+shift+b 2>/dev/null
  sleep 0.3
  xdotool key --clearmodifiers ctrl+shift+b 2>/dev/null
}
die_if_not_collapsed() {  # 按键后必须校验"真收起"了，否则后面全是在测展开态
  local f="$1" oh
  oh="$(python3 -c "import json;print(json.load(open('$f'))['eval'].get('bookmarks_offsetHeight'))" 2>/dev/null || echo ERR)"
  if [ "$oh" != "0" ]; then
    say "FAIL 按键后 offsetHeight=${oh}（应为 0）→ 键没走到 toggle_bookmarks，后续 resize 数据全部无效"
    say "  换窗再试：候选清单见上；也可手动 XID=<正确窗> 重跑本脚本"
    timeout 20 import -window "$XID" "$OUTD/shot-die.png" 2>/dev/null && say "  已截图 $OUTD/shot-die.png"
    systemctl --user stop "$UNIT" 2>/dev/null; sleep 2; rm -rf "$PROF"
    exit 95
  fi
  say "  ✔ 校验通过：offsetHeight=0，真进收起态"
}

say "── ① 基线（展开态）──"
TB "$OUTD/a-toolbar.json"; R "$OUTD/a-toolbar.json"; PG "$OUTD/a-page.json"; say "    内容: $(python3 -c "import json;print(json.load(open('$OUTD/a-page.json'))['eval']['innerHeight'])")"; say "    窗口: $(GEO)"

say "── ② 收起（真按键）──"; press; sleep 1.8
TB "$OUTD/b-toolbar.json"; R "$OUTD/b-toolbar.json"; die_if_not_collapsed "$OUTD/b-toolbar.json"
PG "$OUTD/b-page.json"; say "    内容: $(python3 -c "import json;print(json.load(open('$OUTD/b-page.json'))['eval']['innerHeight'])")"; say "    窗口: $(GEO)"

say "── ③ 收起态 resize 到 1000x700（copilot 猜的翻车点）──"
xdotool windowsize "$XID" 1000 700; sleep 2.0
TB "$OUTD/c-toolbar.json"; R "$OUTD/c-toolbar.json"; PG "$OUTD/c-page.json"
CH=$(python3 -c "import json;print(json.load(open('$OUTD/c-page.json'))['eval']['innerHeight'])")
G=$(GEO); WH=$(echo "$G" | awk -Fx '{print $2}')
say "    窗口 ${G}（resize 生效判据：应含 700）→ 内容 innerHeight=${CH}   判据: 跟随88应=$((WH-88))；回落122则=$((WH-122))"
say "    工具栏实况: "; R "$OUTD/c-toolbar.json"

say "── ④ 收起态 resize 到 1600x900（换个尺寸复验）──"
xdotool windowsize "$XID" 1600 900; sleep 2.0
TB "$OUTD/d-toolbar.json"; R "$OUTD/d-toolbar.json"; PG "$OUTD/d-page.json"
DH=$(python3 -c "import json;print(json.load(open('$OUTD/d-page.json'))['eval']['innerHeight'])")
G=$(GEO); WH=$(echo "$G" | awk -Fx '{print $2}')
say "    窗口 ${G} → 内容 innerHeight=${DH}   判据: 跟随88应=$((WH-88))；回落122则=$((WH-122))"

say "── ⑤ 切回展开 + resize（对照组，应恒为 H-122）──"
press; sleep 1.5; xdotool windowsize "$XID" 1100 800; sleep 2.0
TB "$OUTD/e-toolbar.json"; R "$OUTD/e-toolbar.json"; PG "$OUTD/e-page.json"
EH=$(python3 -c "import json;print(json.load(open('$OUTD/e-page.json'))['eval']['innerHeight'])")
G=$(GEO); WH=$(echo "$G" | awk -Fx '{print $2}')
say "    窗口 ${G} → 内容 innerHeight=${EH}   判据: 应=$((WH-122))   （offsetHeight 应=34，实为 $(python3 -c "import json;print(json.load(open('$OUTD/e-toolbar.json'))['eval'].get('bookmarks_offsetHeight'))" 2>/dev/null)）"

timeout 25 import -window "$XID" "$OUTD/shot-resize.png" 2>/dev/null && say "截图 ok" || say "截图跳过（import 不可用/超时）"
systemctl --user stop "$UNIT" 2>/dev/null; sleep 2
LEFT="$(pgrep -f "^$BIN$" | head -1 || true)"
say "收尾：单元已停；残留主进程=${LEFT:-无}"
rm -rf "$PROF"; say "OUTDIR=$OUTD"
