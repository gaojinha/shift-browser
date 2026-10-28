#!/usr/bin/env bash
# F-CEF-24d 真机回归：验证 multi_threaded_message_loop=1 后的关键链路。
#
# 用法：scripts/cef-coordinator/regress-24d.sh [release|debug]
# 前提：已 cargo build --features cef-engine（或 --release）
# 退出码：0 全过；1 有失败项
set -uo pipefail

PROFILE="${1:-release}"
ROOT="/home/gao/shift-browser-cef-poc"
BIN="$ROOT/src-tauri/target/$PROFILE/shift-browser"
PORT="${SHIFT_CEF_DEBUG_PORT:-9441}"
LOG="/tmp/regress-24d-$PROFILE.log"
CACHE="/tmp/regress-24d-cache"
FAIL=0

ok()   { echo "  ✅ $1"; }
bad()  { echo "  ❌ $1"; FAIL=1; }

[ -x "$BIN" ] || { echo "二进制不存在：$BIN（先 cargo build）"; exit 1; }

pkill -9 -x shift-browser 2>/dev/null
sleep 2
rm -rf "$CACHE"

nohup env DISPLAY=:0 \
  XAUTHORITY="$(ls /run/user/1000/.mutter-Xwaylandauth.* 2>/dev/null | head -1)" \
  GDK_BACKEND=x11 SHIFT_CEF_CACHE_DIR="$CACHE" SHIFT_CEF_DEBUG_PORT="$PORT" \
  LD_LIBRARY_PATH="$(dirname "$BIN")" setsid "$BIN" > "$LOG" 2>&1 < /dev/null &
disown

echo "启动 $PROFILE，等 22s…"
sleep 22

echo "1) 窗口与进程"
[ "$(wmctrl -l | grep -ci Shift)" -ge 1 ] && ok "窗口存在" || bad "窗口不存在"
PN=$(pgrep -x shift-browser | wc -l)
[ "$PN" -ge 5 ] && ok "进程数 $PN" || bad "进程数 $PN（异常）"

echo "2) 无 panic"
PC=$(awk '/\[panic\]/{n++} END{print n+0}' "$LOG" 2>/dev/null)
[ "$PC" -eq 0 ] && ok "0 panic" || bad "$PC 处 panic（看 $LOG）"

EV() { python3 "$ROOT/scripts/cef-coordinator/cdp-eval-any.py" "$PORT" toolbar.html "$1" 2>/dev/null | tail -1; }

echo "3) 工具栏基线"
R=$(EV "JSON.stringify({tabs:document.querySelectorAll('#tabbar .tab').length,fn:typeof window.__shiftTabs})")
echo "     $R"
echo "$R" | grep -q '"fn":"function"' && ok "__shiftTabs 就绪" || bad "工具栏脚本未就绪"

echo "4) 新标签 / 切换 / 关闭"
EV "document.getElementById('tabnew').click(); 'ok'" >/dev/null
sleep 3
T2=$(EV "document.querySelectorAll('#tabbar .tab').length")
[ "$T2" = "2" ] && ok "新建标签 1→2" || bad "新建标签失败（$T2）"
EV "document.querySelectorAll('#tabbar .tab')[0]?.click(); 'sw'" >/dev/null
sleep 2
EV "document.querySelectorAll('#tabbar .tab')[1]?.querySelector('.t-close')?.click(); 'cl'" >/dev/null
sleep 2
T1=$(EV "document.querySelectorAll('#tabbar .tab').length")
[ "$T1" = "1" ] && ok "关闭标签 2→1" || bad "关闭标签失败（$T1）"

echo "5) 面板开合"
for panel in dl hist groups sess cert us settings; do
  R=$(EV "document.getElementById('menubtn').click(); document.querySelector('#menupanel .menu-item[data-panel=\"$panel\"]')?.click(); JSON.stringify([...document.documentElement.classList].filter(c=>c.endsWith('-open')))")
  echo "$R" | grep -q "$panel-open" && ok "面板 $panel" || bad "面板 $panel（$R）"
  EV "document.getElementById('menubtn').click();" >/dev/null
done

echo "6) 主进程 CPU（瞬时，3 次取最小）"
PID=$(pgrep -x shift-browser | head -1)
MIN=999
for _ in 1 2 3; do
  C=$(top -b -n 2 -d 1 -p "$PID" 2>/dev/null | tail -1 | awk '{print $9}' | cut -d. -f1)
  [ -n "$C" ] && [ "$C" -lt "$MIN" ] && MIN=$C
done
[ "$MIN" -le 10 ] && ok "主进程 CPU ${MIN}%（目标 ≤10%）" || bad "主进程 CPU ${MIN}%（>10%，未达标）"

echo "7) 命令面板（F-CEF-29）"
EV "setPalettePanel(true); 'ok'" >/dev/null
sleep 1
PL=$(EV "JSON.stringify({open:document.documentElement.classList.contains('palette-open'),h:window.innerHeight,items:document.querySelectorAll('.pl-item').length})")
echo "     $PL"
echo "$PL" | grep -q '"open":true' && ok "面板打开" || bad "面板未打开（$PL）"
echo "$PL" | grep -qE '"h":4[0-9][0-9]' && ok "高度已撑高" || bad "高度未撑高（$PL）"
echo "$PL" | grep -qE '"items":[1-9]' && ok "候选已渲染" || bad "候选为空（$PL）"
EV "setPalettePanel(false); 'ok'" >/dev/null

echo "8) 关窗退出"
W=$(wmctrl -l | grep -i Shift | head -1 | awk '{print $1}')
wmctrl -ic "$W"
sleep 4
P2=$(pgrep -x shift-browser | wc -l)
[ "$P2" -eq 0 ] && ok "关窗进程归零" || bad "关窗后仍有 $P2 个进程"

echo
if [ "$FAIL" -eq 0 ]; then echo "✅ 24d 回归全过（日志 $LOG）"; else echo "❌ 24d 回归有失败项（日志 $LOG）"; fi
exit "$FAIL"
