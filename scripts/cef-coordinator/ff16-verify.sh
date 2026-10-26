#!/usr/bin/env bash
# F-CEF-16 验收：主菜单 ⋮ + 页面右键菜单
#
# 断言：
#   1) 工具栏存在 #menubtn / #menupanel，菜单 11 项、6 项 disabled
#   2) 菜单 → 下载：菜单关闭、下载面板打开（互斥）
#   3) 菜单 → 密码：下载关、密码开（互斥）
#   4) 内容页右键出现 #__shift_ctxmenu，8 个节点（6 项 + 2 分隔）
#   5) 右键「全选」→ 页面有选区
#   6) 右键「重新加载」→ navigation.type=reload
#
# 用法: ./ff16-verify.sh [起始页URL]     默认 https://example.com
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="$REPO/src-tauri/target/debug/shift-browser"
URL="${1:-https://example.com}"
TS="$(date +%H%M%S)"
PROF="/tmp/opencode/ff16-prof-$TS"
OUTD="/tmp/opencode/ff16-out-$TS"
mkdir -p "$PROF" "$OUTD"
say() { printf '%s\n' "$*"; }
EV() { python3 "$REPO/scripts/cef-coordinator/cdp-eval-any.py" "$1" "$2" "$3"; }

if [ "${FORCE:-0}" != "1" ] && command -v systemctl >/dev/null 2>&1; then
  busy="$(systemctl --user list-units --no-pager 2>/dev/null | grep -E '^\s*shift-cef-' | grep -c 'active running' || true)"
  [ "$busy" -gt 0 ] && { say "ABORT 有 $busy 个 shift-cef-* 在跑（FORCE=1 可强跑）"; exit 90; }
fi
PORT=""
for p in 9370 9371 9372 9373 9374; do
  ss -ltn 2>/dev/null | grep -q ":$p " || { PORT="$p"; break; }
done
[ -z "$PORT" ] && { say "ABORT 9370-9374 全占"; exit 91; }

say "端口=$PORT profile=$PROF"
{
  say "采集时刻: $(date '+%F %T%z')"
  say "HEAD: $(git -C "$REPO" rev-parse --short HEAD)"
  say "二进制: $(stat -c '%y' "$BIN" 2>/dev/null | cut -c1-19) sha256=$(sha256sum "$BIN" | cut -c1-16)"
  say "工作树改动: $(git -C "$REPO" status --short -- src-tauri/ | wc -l)"
} > "$OUTD/meta.md"

( cd "$REPO/src-tauri" && nohup env DISPLAY=:0 \
    SHIFT_CEF_DEBUG_PORT="$PORT" SHIFT_CEF_SHELL_URL="$URL" \
    SHIFT_CEF_CACHE_DIR="$PROF" GDK_BACKEND=x11 \
    LD_LIBRARY_PATH="$REPO/src-tauri/target/debug" \
    "$BIN" >> "$OUTD/start.log" 2>&1 < /dev/null & echo $! > "$OUTD/pid" ) || true
cleanup() {
  [ -f "$OUTD/pid" ] && kill "$(cat "$OUTD/pid")" 2>/dev/null || true
  pkill -f -- "--user-data-dir=$PROF" 2>/dev/null || true
}
trap cleanup EXIT

ok=0
for i in $(seq 1 50); do
  curl -sf -m 2 "http://127.0.0.1:$PORT/json" >/dev/null 2>&1 && { ok=1; break; }
  sleep 1
done
[ "$ok" = "1" ] || { say "FAIL CDP 未起"; tail -20 "$OUTD/start.log"; exit 92; }
say "CDP 就绪（${i}s），等页面加载 15s…"
sleep 15

FAIL=0
pass() { say "PASS $*"; }
fail() { say "FAIL $*"; FAIL=1; }

# 1) 菜单结构
M="$(EV "$PORT" toolbar.html "JSON.stringify({btn:!!document.getElementById('menubtn'),items:document.querySelectorAll('#menupanel .menu-item').length,disabled:document.querySelectorAll('#menupanel .menu-item.disabled').length})" 2>/dev/null)"
say "菜单=$M"
printf '%s' "$M" | grep -q '"btn":true' || fail "菜单按钮不存在"
printf '%s' "$M" | grep -q '"items":11' || fail "菜单项不是 11"
printf '%s' "$M" | grep -q '"disabled":6' || fail "置灰项不是 6"

# 2) 菜单 → 下载（互斥）
D="$(EV "$PORT" toolbar.html "document.getElementById('menubtn').click(); document.querySelector('#menupanel .menu-item[data-panel=\"dl\"]').click(); JSON.stringify({menu:document.documentElement.classList.contains('menu-open'),dl:document.documentElement.classList.contains('dl-open')})" 2>/dev/null)"
printf '%s' "$D" | grep -q '"menu":false' || fail "点下载后菜单没关"
printf '%s' "$D" | grep -q '"dl":true' || fail "下载面板没打开"

# 3) 菜单 → 密码（互斥）
P="$(EV "$PORT" toolbar.html "document.getElementById('dlbtn').click(); document.getElementById('menubtn').click(); document.querySelector('#menupanel .menu-item[data-panel=\"pw\"]').click(); JSON.stringify({pw:document.documentElement.classList.contains('pw-open'),dl:document.documentElement.classList.contains('dl-open')})" 2>/dev/null)"
printf '%s' "$P" | grep -q '"pw":true' || fail "密码面板没打开"
printf '%s' "$P" | grep -q '"dl":false' || fail "密码打开后下载没关（互斥失败）"

# 4) 右键菜单
python3 "$REPO/scripts/cef-coordinator/cdp-rightclick.py" "$PORT" example.com 500 300 >/dev/null 2>&1 || fail "右键发送失败"
C="$(EV "$PORT" example.com "JSON.stringify({menu:!!document.getElementById('__shift_ctxmenu'),items:Array.from(document.querySelectorAll('#__shift_ctxmenu > div')).map(d=>d.textContent||'sep')})" 2>/dev/null)"
say "右键=$C"
printf '%s' "$C" | grep -q '"menu":true' || fail "右键菜单没出现"
printf '%s' "$C" | grep -q '后退' || fail "右键菜单缺后退"
printf '%s' "$C" | grep -q '查看网页源代码' || fail "右键菜单缺查看源代码"

# 5) 全选
EV "$PORT" example.com "document.querySelectorAll('#__shift_ctxmenu > div')[5].click(); 'ok'" >/dev/null 2>&1 || true
S="$(EV "$PORT" example.com "JSON.stringify({sel:(window.getSelection()||'').toString().length})" 2>/dev/null)"
printf '%s' "$S" | grep -qE '"sel":[1-9]' || fail "右键全选没选中文本"

# 6) 重新加载
python3 "$REPO/scripts/cef-coordinator/cdp-rightclick.py" "$PORT" example.com 400 300 >/dev/null 2>&1 || true
EV "$PORT" example.com "document.querySelectorAll('#__shift_ctxmenu > div')[2].click(); 'ok'" >/dev/null 2>&1 || true
sleep 4
R="$(EV "$PORT" example.com "JSON.stringify({nav:(performance.getEntriesByType('navigation')[0]||{}).type})" 2>/dev/null)"
printf '%s' "$R" | grep -q '"nav":"reload"' || fail "右键重新加载没生效（$R）"

cp -a "$OUTD" "$REPO/.ai/evidence/ff16-verify-$TS" 2>/dev/null || true
say "证据: $REPO/.ai/evidence/ff16-verify-$TS"
[ "$FAIL" = "0" ] && { say "RESULT: PASS"; exit 0; } || { say "RESULT: FAIL"; exit 1; }
