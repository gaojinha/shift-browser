#!/usr/bin/env bash
# F-CEF-09 书签栏开关：协调器独立复现（不采信回执表格，自己按键、自己读数、自己落盘）
# 一石二鸟：同一台实例顺手把 F-CEF-11 的注入探针也跑了（da5c2be 已提交，代码在同一产物里）。
# 排他口径升级（本轮教训）：只看 systemd 单元会漏（atomcode 那台实例单元名没匹配上），
#   改成 ① pgrep 主进程 ② ss 查端口 双查；且启动后若 journal 出现 single-instance 转发 → 立刻判定被拦。
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="$REPO/src-tauri/target/debug/shift-browser"
TS="$(date +%H%M%S)"
UNIT="shift-cef-erGou-$TS"
PROF="/tmp/opencode/ergou-prof-$TS"
OUTD="/tmp/opencode/ergou-out-$TS"
mkdir -p "$PROF" "$OUTD"
say() { printf '%s\n' "$*"; }

# ── 0. 双查排他 ────────────────────────────────────────────
if [ "${FORCE:-0}" != "1" ]; then
  for i in $(seq 1 ${WAITMAX:-60}); do
    p="$(pgrep -f "^$BIN$" 2>/dev/null | head -1 || true)"
    if [ -z "$p" ]; then break; fi
    say "$(date '+%T') 他人主进程 $p 在跑，等待…"; sleep 10
  done
  p="$(pgrep -f "^$BIN$" 2>/dev/null | head -1 || true)"
  [ -n "$p" ] && { say "ABORT 等待超时，主进程 $p 仍在"; exit 90; }
fi
PORT=""
for x in 9338 9339 9340 9341; do
  ss -ltn 2>/dev/null | grep -q ":$x " || { PORT="$x"; break; }
done
[ -z "$PORT" ] && { say "ABORT 无空闲端口"; exit 91; }
XAUTH="$(ls -1 /run/user/1000/.mutter-Xwaylandauth.* 2>/dev/null | head -1)"

# ── 1. 被测身份（跑之前记死，别再犯"身份写错"）─────────────
{ say "采集: $(date '+%F %T%z')"
  say "单元: $UNIT  端口: $PORT  profile: $PROF"
  say "二进制 mtime=$(stat -c '%y' "$BIN" | cut -c1-19)  sha256_12=$(sha256sum "$BIN" | cut -c1-12)"
  say "HEAD=$(git -C "$REPO" rev-parse --short HEAD)  工作树脏: $(git -C "$REPO" status --short -- src-tauri/ | tr '\n' ' ')"
  say "源文件: shell.rs=$(stat -c '%y' "$REPO/src-tauri/src/engine/cef/shell.rs" | cut -c12-19) toolbar.rs=$(stat -c '%y' "$REPO/src-tauri/src/engine/cef/toolbar.rs" | cut -c12-19)"
  say "启动: systemd-run --user --unit=$UNIT --setenv=SHIFT_CEF_DEBUG_PORT=$PORT --setenv=SHIFT_CEF_SHELL_URL=https://example.com --setenv=SHIFT_CEF_CACHE_DIR=$PROF $BIN"
} > "$OUTD/meta.md"

# ── 2. 起实例 ─────────────────────────────────────────────
systemd-run --user --unit="$UNIT" --collect \
  --setenv=DISPLAY=:0 --setenv=XAUTHORITY="$XAUTH" \
  --setenv=SHIFT_CEF_DEBUG_PORT="$PORT" --setenv=SHIFT_CEF_SHELL_URL=https://example.com \
  --setenv=SHIFT_CEF_CACHE_DIR="$PROF" \
  --setenv=GDK_BACKEND=x11 --setenv="LD_LIBRARY_PATH=$REPO/src-tauri/target/debug" \
  "$BIN" > "$OUTD/systemd-run.log" 2>&1
ok=0
for i in $(seq 1 45); do
  curl -sf -m 2 "http://127.0.0.1:$PORT/json" >/dev/null 2>&1 && { ok=1; break; }
  # 被单实例插件拦掉就早点认，别白等 45s
  if journalctl --user --since "-1 min" --no-pager 2>/dev/null | grep -qa "single-instance] 已有实例收到新实例参数"; then
    say "FAIL 被 single_instance 拦截（有他人实例未退）"; break
  fi
  sleep 1
done
say "CDP 就绪=$ok（等了 ${i}s）"
if [ "$ok" != "1" ]; then
  { say "journal 尾部:"; journalctl --user -u "$UNIT" --no-pager 2>/dev/null | tail -15; } >> "$OUTD/meta.md"
  systemctl --user stop "$UNIT" 2>/dev/null; rm -rf "$PROF"; exit 92
fi
sleep 4   # 等页面 load + 注入完成

cd "$REPO"
# ── 3. 基线（展开态）──────────────────────────────────────
say "── ① 基线（默认展开）──"
python3 scripts/cef-coordinator/cef-eval.py "$PORT" "$OUTD/01-baseline.json" scripts/cef-coordinator/ff09-probe.js 2>&1 | grep -vE "^\s*$" | head -14

# ── 4. xdotool 真按键（不是 JS 模拟）───────────────────────
MAINPID="$(pgrep -f "^$BIN$" | head -1)"
XID="$(xdotool search --pid "$MAINPID" 2>/dev/null | while read -r w; do
         nm="$(xdotool getwindowname "$w" 2>/dev/null)"
         geo="$(xdotool getwindowgeometry "$w" 2>/dev/null | grep -c 'Position\|Geometry')"
         [ -n "$nm" ] && echo "$w"; done | head -1)"
say "主进程=$MAINPID  窗口 XID=$XID"
say "窗口列表: $(xdotool search --pid "$MAINPID" 2>/dev/null | tr '\n' ' ')"
[ -n "$XID" ] && xdotool windowactivate --sync "$XID" sleep 0.5 key --clearmodifiers ctrl+shift+b
say "已发 Ctrl+Shift+B  退出码=$?"
sleep 2

# ── 5. 收起态 ─────────────────────────────────────────────
say "── ② 收起态 ──"
python3 scripts/cef-coordinator/cef-eval.py "$PORT" "$OUTD/02-collapsed.json" scripts/cef-coordinator/ff09-probe.js 2>&1 | grep -vE "^\s*$" | head -14

# ── 6. 再切回 ─────────────────────────────────────────────
[ -n "$XID" ] && xdotool windowactivate --sync "$XID" sleep 0.5 key --clearmodifiers ctrl+shift+b
sleep 2
say "── ③ 再切回展开 ──"
python3 scripts/cef-coordinator/cef-eval.py "$PORT" "$OUTD/03-restored.json" scripts/cef-coordinator/ff09-probe.js 2>&1 | grep -vE "^\s*$" | head -14

# ── 7. 同台实例补跑 F-CEF-11 注入取证 ─────────────────────
say "── ④ F-CEF-11 注入探针（同台实例）──"
python3 scripts/cef-coordinator/cef-eval.py "$PORT" "$OUTD/04-inject.json" scripts/cef-coordinator/ff11-probe.js 2>&1 | grep -vE "^\s*$" | head -20

# ── 8. 截图 + 后端日志 ────────────────────────────────────
DISPLAY=:0 XAUTHORITY="$XAUTH" import -window "$XID" "$OUTD/shot-final.png" 2>/dev/null && say "截图: $OUTD/shot-final.png"
journalctl --user -u "$UNIT" --no-pager 2>/dev/null | grep -aiE "cef-inject|注入通道|single-instance" > "$OUTD/inject.log"
say "注入日志 $(wc -l < "$OUTD/inject.log") 行:"; sed 's/^/  /' "$OUTD/inject.log" | cut -c1-150 | head -8

systemctl --user stop "$UNIT" 2>/dev/null; sleep 2; rm -rf "$PROF"
say "已收尾：单元停 + profile 删"
say "OUTDIR=$OUTD"
