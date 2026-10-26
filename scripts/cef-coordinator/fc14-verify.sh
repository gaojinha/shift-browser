#!/usr/bin/env bash
# F-CEF-14 下载闭环真机验收（脚本化，可复现）
#
# 验什么：
#   1) 导航到 Content-Disposition: attachment 的 URL → 文件落盘 >300KB
#   2) 原页面保留（不是白屏/about:blank）
#   3) 无 .crdownload 残留
#   4) 数据库有对应下载记录（completed）
#
# 用法: ./fc14-verify.sh [起始页URL] [下载URL]
# 默认: 起始页 https://example.com，下载 https://github.com/opencode-ai/opencode/archive/refs/heads/main.zip
#
# 让行铁律：有他人 CEF 实例在跑或端口被占 → 退出 90/91（FORCE=1 可跳过实例检查）
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="$REPO/src-tauri/target/debug/shift-browser"
START_URL="${1:-https://example.com}"
DL_URL="${2:-https://github.com/opencode-ai/opencode/archive/refs/heads/main.zip}"
TS="$(date +%H%M%S)"
UNIT="shift-cef-fc14-$TS"
PROF="/tmp/opencode/fc14-prof-$TS"
OUTD="/tmp/opencode/fc14-out-$TS"
mkdir -p "$PROF" "$OUTD"
say() { printf '%s\n' "$*"; }

# ── 0. 让行 + 端口 ─────────────────────────────────────────────
if [ "${FORCE:-0}" != "1" ] && command -v systemctl >/dev/null 2>&1; then
  busy="$(systemctl --user list-units --no-pager 2>/dev/null | grep -E '^\s*shift-cef-' | grep -c 'active running' || true)"
  if [ "$busy" -gt 0 ]; then
    say "ABORT 有 $busy 个 shift-cef-* 实例在跑，按排他铁律让行（FORCE=1 可强跑）"
    exit 90
  fi
fi
PORT=""
for p in 9343 9344 9345 9346 9347; do
  if ! ss -ltn 2>/dev/null | grep -q ":$p "; then PORT="$p"; break; fi
done
[ -z "$PORT" ] && { say "ABORT 9343-9347 全被占"; exit 91; }

# 下载目录：xdg-user-dir → ~/下载 → ~/Downloads（与 engine::platform::resolve_download_dir 同优先级）
DL_DIR="$(xdg-user-dir DOWNLOAD 2>/dev/null || true)"
[ -z "$DL_DIR" ] && [ -d "$HOME/下载" ] && DL_DIR="$HOME/下载"
[ -z "$DL_DIR" ] && DL_DIR="$HOME/Downloads"
mkdir -p "$DL_DIR"

# 清掉旧目标，免得把上次的文件当成本次成果
rm -f "$DL_DIR"/opencode-main*.zip "$DL_DIR"/opencode-main*.zip.crdownload 2>/dev/null || true

say "端口=$PORT  profile=$PROF  下载目录=$DL_DIR  单元=$UNIT"

# ── 1. 取证 ───────────────────────────────────────────────────
{
  say "采集时刻: $(date '+%F %T%z')"
  say "起始页: $START_URL"
  say "下载 URL: $DL_URL"
  say "二进制: $BIN  mtime=$(stat -c '%y' "$BIN" 2>/dev/null | cut -c1-19)  sha256=$(sha256sum "$BIN" | cut -c1-16)"
  say "HEAD: $(git -C "$REPO" rev-parse --short HEAD)"
  say "工作树: $(git -C "$REPO" status --short -- src-tauri/ | wc -l) 个改动"
  say "启动命令:"
  say "  LD_LIBRARY_PATH=$REPO/src-tauri/target/debug GDK_BACKEND=x11 \\"
  say "  SHIFT_CEF_CACHE_DIR=$PROF SHIFT_CEF_SHELL_URL=$START_URL SHIFT_CEF_DEBUG_PORT=$PORT $BIN"
} > "$OUTD/meta.md"

# ── 2. 起实例（优先 systemd-run 脱离 cgroup；失败回退 nohup）────
XAUTH="$(ls -1 /run/user/1000/.mutter-Xwaylandauth.* 2>/dev/null | head -1)"
STARTED=0
if command -v systemd-run >/dev/null 2>&1; then
  systemd-run --user --unit="$UNIT" --collect \
    --setenv=DISPLAY=:0 --setenv=XAUTHORITY="$XAUTH" \
    --setenv=SHIFT_CEF_DEBUG_PORT="$PORT" --setenv=SHIFT_CEF_SHELL_URL="$START_URL" \
    --setenv=SHIFT_CEF_CACHE_DIR="$PROF" \
    --setenv=GDK_BACKEND=x11 --setenv="LD_LIBRARY_PATH=$REPO/src-tauri/target/debug" \
    "$BIN" > "$OUTD/start.log" 2>&1 && STARTED=1
fi
if [ "$STARTED" != "1" ]; then
  ( cd "$REPO/src-tauri" && \
    nohup env DISPLAY=:0 XAUTHORITY="$XAUTH" \
      SHIFT_CEF_DEBUG_PORT="$PORT" SHIFT_CEF_SHELL_URL="$START_URL" \
      SHIFT_CEF_CACHE_DIR="$PROF" GDK_BACKEND=x11 \
      LD_LIBRARY_PATH="$REPO/src-tauri/target/debug" \
      "$BIN" >> "$OUTD/start.log" 2>&1 < /dev/null & echo $! > "$OUTD/pid" ) || true
  STARTED=2
fi
say "启动方式=$([ "$STARTED" = "1" ] && echo systemd-run || echo nohup)"

cleanup() {
  [ "$STARTED" = "1" ] && systemctl --user stop "$UNIT" 2>/dev/null || true
  [ -f "$OUTD/pid" ] && kill "$(cat "$OUTD/pid")" 2>/dev/null || true
  # CEF 子进程命令行带 --user-data-dir=<profile>，按它收尾（主进程 cmdline 不含 env）
  pkill -f -- "--user-data-dir=$PROF" 2>/dev/null || true
}
trap cleanup EXIT

# ── 3. 等 CDP 就绪（最多 50s）──────────────────────────────────
ok=0
for i in $(seq 1 50); do
  if curl -sf -m 2 "http://127.0.0.1:$PORT/json" > /dev/null 2>&1; then ok=1; break; fi
  sleep 1
done
say "CDP 就绪=$ok（等了 ${i}s）"
if [ "$ok" != "1" ]; then
  say "FAIL CDP 未起"; tail -20 "$OUTD/start.log" | sed 's/^/  /'
  exit 92
fi

# ── 4. 等页面完全加载（交接单：≥15s，否则 act() 的 console.log 收不到）──
say "等待起始页加载 15s…"
sleep 15

# ── 5. 在内容页执行导航（用仓库内 cef-eval.py，选 http 页面）────
cat > "$OUTD/nav.js" <<EOF
location.href = ${DL_URL@Q};
'nav-sent';
EOF
python3 "$REPO/scripts/cef-coordinator/cef-eval.py" "$PORT" "$OUTD/nav.json" "$OUTD/nav.js" \
  > "$OUTD/nav.log" 2>&1 || true
say "导航已发（见 $OUTD/nav.log）"

# ── 6. 等下载（最多 60s）───────────────────────────────────────
FOUND=""
for i in $(seq 1 60); do
  FOUND="$(find "$DL_DIR" -maxdepth 1 -name 'opencode-main*.zip' -size +300k -print -quit 2>/dev/null)"
  [ -n "$FOUND" ] && break
  sleep 1
done

# ── 7. 断言 ───────────────────────────────────────────────────
FAIL=0
if [ -n "$FOUND" ]; then
  SIZE="$(stat -c '%s' "$FOUND")"
  say "PASS 文件落盘: $FOUND（$SIZE 字节）"
else
  say "FAIL 未找到 >300KB 的 opencode-main*.zip（等了 ${i}s）"
  FAIL=1
fi

CR="$(find "$DL_DIR" -maxdepth 1 -name '*.crdownload' -print -quit 2>/dev/null)"
if [ -z "$CR" ]; then say "PASS 无 .crdownload 残留"; else say "FAIL 残留: $CR"; FAIL=1; fi

# 原页保留：CDP targets 里内容页应仍是起始页 host
CUR="$(curl -sf -m 5 "http://127.0.0.1:$PORT/json" | python3 -c "
import json,sys
try:
    d=json.load(sys.stdin)
except Exception:
    print(''); raise SystemExit
for t in d:
    if 'toolbar.html' in t.get('url',''): continue
    print(t.get('url','')); break
" 2>/dev/null)"
if printf '%s' "$CUR" | grep -q "example.com"; then
  say "PASS 原页保留: $CUR"
else
  say "FAIL 原页不是起始页: $CUR"; FAIL=1
fi

# 数据库记录（只查最近一条 opencode-main）
DB="$HOME/.local/share/com.shiftbrowser.app/shift-browser.db"
if [ -f "$DB" ] && python3 - "$DB" <<'PY'
import sqlite3, sys
try:
    c = sqlite3.connect(sys.argv[1])
    row = c.execute("SELECT filename,state,received_bytes FROM downloads WHERE filename LIKE 'opencode-main%' ORDER BY id DESC LIMIT 1").fetchone()
except Exception:
    row = None
raise SystemExit(0 if row and row[1] == 'completed' and row[2] > 300*1024 else 1)
PY
then
  say "PASS 数据库记录 completed 且字节数 >300KB"
else
  say "FAIL 数据库无 completed 记录（或字节数不足）"; FAIL=1
fi

cp -a "$OUTD" "$REPO/.ai/evidence/fc14-verify-$TS" 2>/dev/null || true
say "证据目录: $REPO/.ai/evidence/fc14-verify-$TS"

if [ "$FAIL" = "0" ]; then
  say "RESULT: PASS"
  exit 0
else
  say "RESULT: FAIL"
  exit 1
fi
