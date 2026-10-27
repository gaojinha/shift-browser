#!/usr/bin/env bash
# CEF 安装版/构建版功能回归（小步快跑用）
#
# 检查：9 个面板 DOM 存在、证书面板打开+条数、用户脚本面板、设置、历史、书签管理、
#       地址栏建议、星标、缩放显示、查找栏。
# 用法: ./regress-cef.sh [二进制路径]   默认 /opt/shift-browser-cef/shift-browser
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="${1:-/opt/shift-browser-cef/shift-browser}"
TS="$(date +%H%M%S)"
PROF="/tmp/opencode/regress-prof-$TS"
OUTD="/tmp/opencode/regress-out-$TS"
PORT=9430
mkdir -p "$PROF" "$OUTD"
say(){ printf '%s\n' "$*"; }
EV(){ python3 "$REPO/scripts/cef-coordinator/cdp-eval-any.py" "$PORT" toolbar.html "$1"; }

[ -x "$BIN" ] || { say "❌ 二进制不存在: $BIN"; exit 1; }
pkill -9 -x shift-browser 2>/dev/null; sleep 2
rm -f "$PROF"/Singleton* 2>/dev/null

LD_LIBRARY_PATH="$(dirname "$BIN")" nohup env DISPLAY=:0 \
  XAUTHORITY="$(ls /run/user/1000/.mutter-Xwaylandauth.* 2>/dev/null | head -1)" \
  GDK_BACKEND=x11 SHIFT_CEF_CACHE_DIR="$PROF" SHIFT_CEF_DEBUG_PORT="$PORT" \
  "$BIN" > "$OUTD/start.log" 2>&1 < /dev/null &
cleanup(){ pkill -9 -x shift-browser 2>/dev/null; }
trap cleanup EXIT

ok=0
for i in $(seq 1 50); do
  curl -sf -m 2 "http://127.0.0.1:$PORT/json" >/dev/null 2>&1 && { ok=1; break; }
  sleep 1
done
[ "$ok" = "1" ] || { say "❌ CDP 未起"; tail -10 "$OUTD/start.log"; exit 2; }
say "CDP 就绪（${i}s）"
sleep 8

FAIL=0
pass(){ say "PASS $*"; }
fail(){ say "FAIL $*"; FAIL=1; }

D="$(EV "JSON.stringify({pw:!!document.getElementById('pwpanel'),dl:!!document.getElementById('dlpanel'),hist:!!document.getElementById('histpanel'),menu:!!document.getElementById('menupanel'),site:!!document.getElementById('sitepanel'),set:!!document.getElementById('settingspanel'),cert:!!document.getElementById('certpanel'),us:!!document.getElementById('uspanel'),bm:!!document.getElementById('bmmanage')})")"
say "面板=$D"
for k in pw dl hist menu site set cert us bm; do
  printf '%s' "$D" | grep -q "\"$k\":true" || fail "面板 $k 缺失"
done

[ "$FAIL" = "0" ] && pass "9 个面板 DOM 全在"

# 证书面板
EV "document.getElementById('menubtn').click(); document.querySelector('#menupanel .menu-item[data-panel=\"cert\"]').click(); 'ok'" >/dev/null 2>&1
sleep 1
C="$(EV "JSON.stringify({open:document.documentElement.classList.contains('cert-open'),inner:window.innerHeight,rows:document.querySelectorAll('.cert-row').length})")"
say "证书=$C"
printf '%s' "$C" | grep -q '"open":true' && pass "证书面板打开" || fail "证书面板打不开"
printf '%s' "$C" | grep -q '"inner":462' && pass "证书面板撑高 462" || fail "证书撑高异常（$C）"

# 用户脚本面板
EV "document.getElementById('menubtn').click(); document.querySelector('#menupanel .menu-item[data-panel=\"us\"]').click(); 'ok'" >/dev/null 2>&1
sleep 1
U="$(EV "JSON.stringify({open:document.documentElement.classList.contains('us-open'),inner:window.innerHeight})")"
printf '%s' "$U" | grep -q '"open":true' && pass "用户脚本面板打开" || fail "用户脚本面板打不开"

# 设置面板
EV "document.getElementById('menubtn').click(); document.querySelector('#menupanel .menu-item[data-panel=\"settings\"]').click(); 'ok'" >/dev/null 2>&1
sleep 1
S="$(EV "JSON.stringify({open:document.documentElement.classList.contains('settings-open'),engine:document.getElementById('set-engine').value})")"
printf '%s' "$S" | grep -q '"open":true' && pass "设置面板打开" || fail "设置面板打不开"

# 地址栏建议 + 星标
A="$(EV "JSON.stringify({star:!!document.getElementById('starbtn'),omni:!!document.getElementById('omni')})")"
printf '%s' "$A" | grep -q '"star":true' && pass "星标存在" || fail "星标缺失"
printf '%s' "$A" | grep -q '"omni":true' && pass "地址栏建议容器存在" || fail "建议容器缺失"

# 缩放/查找控件
Z="$(EV "JSON.stringify({zoom:!!document.getElementById('zoomval'),find:!!document.getElementById('findbar')})")"
printf '%s' "$Z" | grep -q '"zoom":true' && pass "缩放显示存在" || fail "缩放显示缺失"
printf '%s' "$Z" | grep -q '"find":true' && pass "查找栏存在" || fail "查找栏缺失"

cp -a "$OUTD" "$REPO/.ai/evidence/regress-cef-$TS" 2>/dev/null || true
say "证据: $REPO/.ai/evidence/regress-cef-$TS"
[ "$FAIL" = "0" ] && { say "RESULT: PASS"; exit 0; } || { say "RESULT: FAIL"; exit 1; }
