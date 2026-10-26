#!/usr/bin/env bash
# F-CEF-17 验收：地址栏下拉建议 + 书签星标
#
# 断言：
#   1) 输入关键词 → #omni 打开且至少 1 条
#   2) ↓ 选中第一项；Enter 打开后内容页 URL 变成建议项
#   3) 星标：点一次 → #starbtn.on + bookmarks 表新增；再点 → 取消 + 表内 0 条
#
# 用法: ./ff17-verify.sh [起始页URL]     默认 https://example.com
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="$REPO/src-tauri/target/debug/shift-browser"
URL="${1:-https://example.com}"
TS="$(date +%H%M%S)"
PROF="/tmp/opencode/ff17-prof-$TS"
OUTD="/tmp/opencode/ff17-out-$TS"
DB="$HOME/.local/share/com.shiftbrowser.app/shift-browser.db"
mkdir -p "$PROF" "$OUTD"
say() { printf '%s\n' "$*"; }
EV() { python3 "$REPO/scripts/cef-coordinator/cdp-eval-any.py" "$1" "$2" "$3"; }
DBQ() { python3 - "$DB" "$1" <<'PY'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1])
print(c.execute(sys.argv[2]).fetchone()[0])
PY
}

if [ "${FORCE:-0}" != "1" ] && command -v systemctl >/dev/null 2>&1; then
  busy="$(systemctl --user list-units --no-pager 2>/dev/null | grep -E '^\s*shift-cef-' | grep -c 'active running' || true)"
  [ "$busy" -gt 0 ] && { say "ABORT 有 $busy 个 shift-cef-* 在跑（FORCE=1 可强跑）"; exit 90; }
fi
PORT=""
for p in 9375 9376 9377 9378 9379; do
  ss -ltn 2>/dev/null | grep -q ":$p " || { PORT="$p"; break; }
done
[ -z "$PORT" ] && { say "ABORT 9375-9379 全占"; exit 91; }

XAUTH="$(ls -1 /run/user/1000/.mutter-Xwaylandauth.* 2>/dev/null | head -1)"
say "端口=$PORT profile=$PROF XAUTH=$XAUTH"
{
  say "采集时刻: $(date '+%F %T%z')"
  say "HEAD: $(git -C "$REPO" rev-parse --short HEAD)"
  say "二进制: $(stat -c '%y' "$BIN" 2>/dev/null | cut -c1-19) sha256=$(sha256sum "$BIN" | cut -c1-16)"
} > "$OUTD/meta.md"

( cd "$REPO/src-tauri" && nohup env DISPLAY=:0 XAUTHORITY="$XAUTH" \
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

# 1) 输入关键词 → 下拉
O="$(EV "$PORT" toolbar.html "(function(){const a=document.getElementById('addr');a.focus();a.value='example';a.dispatchEvent(new Event('input'));return 'typed';})()" 2>/dev/null)"
sleep 1
O="$(EV "$PORT" toolbar.html "JSON.stringify({open:document.getElementById('omni').classList.contains('open'),n:document.querySelectorAll('.omni-item').length})" 2>/dev/null)"
say "下拉=$O"
printf '%s' "$O" | grep -q '"open":true' || fail "下拉没打开"
printf '%s' "$O" | grep -qE '"n":[1-9]' || fail "下拉没有建议项"

# 2) ↓ + Enter → 打开第一项
EV "$PORT" toolbar.html "(function(){const a=document.getElementById('addr');a.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true}));return 'down';})()" >/dev/null 2>&1 || true
FIRST="$(EV "$PORT" toolbar.html "(function(){const r=document.querySelector('.omni-item');return r?r.querySelector('.omni-url').textContent:'';})()" 2>/dev/null)"
EV "$PORT" toolbar.html "(function(){const a=document.getElementById('addr');a.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return 'enter';})()" >/dev/null 2>&1 || true
sleep 5
CUR="$(curl -sf -m 5 "http://127.0.0.1:$PORT/json" | python3 -c "
import json,sys
try: d=json.load(sys.stdin)
except Exception: print(''); raise SystemExit
for t in d:
    if 'toolbar.html' in t.get('url',''): continue
    print(t.get('url','')); break
" 2>/dev/null)"
say "建议首项=$FIRST  当前内容页=$CUR"
[ -n "$FIRST" ] && printf '%s' "$CUR" | grep -Fq "$FIRST" && pass "Enter 打开了建议项" || fail "Enter 没打开建议项"

# 3) 星标收藏
ON0="$(EV "$PORT" toolbar.html "document.getElementById('starbtn').classList.contains('on')" 2>/dev/null)"
if [ "$ON0" = "true" ]; then
  # 起始页本来就已收藏：先取消，保证从「未收藏」开始
  EV "$PORT" toolbar.html "document.getElementById('starbtn').click(); 'off'" >/dev/null 2>&1 || true
  sleep 2
fi
EV "$PORT" toolbar.html "document.getElementById('starbtn').click(); 'on'" >/dev/null 2>&1 || true
sleep 2
ON="$(EV "$PORT" toolbar.html "document.getElementById('starbtn').classList.contains('on')" 2>/dev/null)"
CURURL="$(EV "$PORT" toolbar.html "document.getElementById('addr').value" 2>/dev/null)"
CNT="$(DBQ "SELECT COUNT(*) FROM bookmarks WHERE url = '$CURURL'" 2>/dev/null)"
say "星标 on=$ON url=$CURURL db条数=$CNT"
[ "$ON" = "true" ] && pass "星标点亮" || fail "星标没点亮"
[ "${CNT:-0}" -ge 1 ] && pass "数据库已收藏" || fail "数据库没有收藏记录"

# 取消，恢复现场
EV "$PORT" toolbar.html "document.getElementById('starbtn').click(); 'off'" >/dev/null 2>&1 || true
sleep 2
CNT2="$(DBQ "SELECT COUNT(*) FROM bookmarks WHERE url = '$CURURL'" 2>/dev/null)"
[ "${CNT2:-0}" -eq 0 ] && pass "取消后数据库已清理" || fail "取消后仍有 $CNT2 条记录"

cp -a "$OUTD" "$REPO/.ai/evidence/ff17-verify-$TS" 2>/dev/null || true
say "证据: $REPO/.ai/evidence/ff17-verify-$TS"
[ "$FAIL" = "0" ] && { say "RESULT: PASS"; exit 0; } || { say "RESULT: FAIL"; exit 1; }
