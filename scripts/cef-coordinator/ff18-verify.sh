#!/usr/bin/env bash
# F-CEF-18 验收：缩放 + 页内查找
#
# 断言：
#   1) act('zoomin') → #zoomval 从 100% → 110%，内容页 devicePixelRatio 同步变大
#   2) act('zoomreset') → #zoomval 回 100%
#   3) act('findopen') → #findbar 可见
#   4) 输入关键词 → #findcount 出现 “n/m”（CEF 回调真值，不是页面自己数 DOM）
#   5) act('findclose') → #findbar 隐藏
#
# 用法: ./ff18-verify.sh [起始页URL]     默认 https://example.com
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="$REPO/src-tauri/target/debug/shift-browser"
URL="${1:-https://example.com}"
TS="$(date +%H%M%S)"
PROF="/tmp/opencode/ff18-prof-$TS"
OUTD="/tmp/opencode/ff18-out-$TS"
mkdir -p "$PROF" "$OUTD"
say() { printf '%s\n' "$*"; }
EV() { python3 "$REPO/scripts/cef-coordinator/cdp-eval-any.py" "$1" "$2" "$3"; }

if [ "${FORCE:-0}" != "1" ] && command -v systemctl >/dev/null 2>&1; then
  busy="$(systemctl --user list-units --no-pager 2>/dev/null | grep -E '^\s*shift-cef-' | grep -c 'active running' || true)"
  [ "$busy" -gt 0 ] && { say "ABORT 有 $busy 个 shift-cef-* 在跑（FORCE=1 可强跑）"; exit 90; }
fi
PORT=""
for p in 9380 9381 9382 9383 9384; do
  ss -ltn 2>/dev/null | grep -q ":$p " || { PORT="$p"; break; }
done
[ -z "$PORT" ] && { say "ABORT 9380-9384 全占"; exit 91; }

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

# 1) 缩放：zoomin
Z0="$(EV "$PORT" toolbar.html "document.getElementById('zoomval')?document.getElementById('zoomval').textContent:'MISSING'" 2>/dev/null)"
DPR0="$(EV "$PORT" example.com "String(window.devicePixelRatio)" 2>/dev/null)"
EV "$PORT" toolbar.html "act('zoomin'); 'ok'" >/dev/null 2>&1 || true
sleep 2
Z1="$(EV "$PORT" toolbar.html "document.getElementById('zoomval')?document.getElementById('zoomval').textContent:'MISSING'" 2>/dev/null)"
DPR1="$(EV "$PORT" example.com "String(window.devicePixelRatio)" 2>/dev/null)"
say "缩放: $Z0 → $Z1  dpr: $DPR0 → $DPR1"
[ "$Z0" != "MISSING" ] || fail "缺少 #zoomval"
[ "$Z0" != "$Z1" ] && pass "百分比变化" || fail "百分比没变（$Z1）"
[ "$DPR0" != "$DPR1" ] && pass "devicePixelRatio 同步变化（CEF zoom 真生效）" || fail "dpr 没变，可能只改了文本（$DPR1）"

# 2) 重置
EV "$PORT" toolbar.html "act('zoomreset'); 'ok'" >/dev/null 2>&1 || true
sleep 2
Z2="$(EV "$PORT" toolbar.html "document.getElementById('zoomval')?document.getElementById('zoomval').textContent:''" 2>/dev/null)"
printf '%s' "$Z2" | grep -q '100' && pass "zoomreset 回 100%" || fail "重置后不是 100%（$Z2）"

# 3) 查找栏打开
EV "$PORT" toolbar.html "act('findopen'); 'ok'" >/dev/null 2>&1 || true
sleep 1
F0="$(EV "$PORT" toolbar.html "JSON.stringify({bar:!!document.getElementById('findbar'),visible:(function(){const e=document.getElementById('findbar');return e?e.offsetHeight>0:false})()})" 2>/dev/null)"
say "查找栏=$F0"
printf '%s' "$F0" | grep -q '"bar":true' || fail "缺少 #findbar"
printf '%s' "$F0" | grep -q '"visible":true' || fail "查找栏没显示"

# 4) 输入关键词 → 计数
EV "$PORT" toolbar.html "(function(){const i=document.getElementById('findinput');if(!i)return 'no';i.focus();i.value='Example';i.dispatchEvent(new Event('input'));return 'typed';})()" >/dev/null 2>&1 || true
sleep 2
C="$(EV "$PORT" toolbar.html "document.getElementById('findcount')?document.getElementById('findcount').textContent:'MISSING'" 2>/dev/null)"
say "计数=$C"
[ "$C" != "MISSING" ] || fail "缺少 #findcount"
printf '%s' "$C" | grep -qE '[0-9]+/[0-9]+' && pass "计数是 n/m 形式" || fail "计数格式不对（$C）"

# 5) 关闭
EV "$PORT" toolbar.html "act('findclose'); 'ok'" >/dev/null 2>&1 || true
sleep 1
F1="$(EV "$PORT" toolbar.html "document.getElementById('findbar')?document.getElementById('findbar').offsetHeight:0" 2>/dev/null)"
[ "${F1:-0}" -eq 0 ] && pass "findclose 隐藏查找栏" || fail "查找栏仍高 $F1"

cp -a "$OUTD" "$REPO/.ai/evidence/ff18-verify-$TS" 2>/dev/null || true
say "证据: $REPO/.ai/evidence/ff18-verify-$TS"
[ "$FAIL" = "0" ] && { say "RESULT: PASS"; exit 0; } || { say "RESULT: FAIL"; exit 1; }
