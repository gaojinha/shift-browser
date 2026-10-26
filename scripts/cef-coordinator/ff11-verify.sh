#!/usr/bin/env bash
# F-CEF-11 真机验收编排（协调器代跑）
# 为什么要有这个脚本：/tmp 是 tmpfs，"跑过了"如果只活在记忆里就不可复现；
# 且上一轮干净复测暴露过断点——journal 不记 env，事后无法铁证「临时 profile 真用上没」。
# 本脚本把 启动命令行 / env / profile 路径 / 被测二进制与源文件的 mtime+sha256 / HEAD 全写进 meta。
#
# 用法: ./ff11-verify.sh [url]     # 默认 https://example.com
# 让行铁律: 有他人 shift-cef* 单元在跑 或 端口被占 → 直接退出（FORCE=1 可跳过检查）
set -uo pipefail
REPO="/home/gao/shift-browser-cef-poc"
BIN="$REPO/src-tauri/target/debug/shift-browser"
URL="${1:-https://example.com}"
TS="$(date +%H%M%S)"
UNIT="shift-cef-ff11-$TS"
PROF="/tmp/opencode/ff11-prof-$TS"
OUTD="/tmp/opencode/ff11-out-$TS"
mkdir -p "$PROF" "$OUTD"

say() { printf '%s\n' "$*"; }

# ── 0. 让行检查 + 挑空闲端口 ───────────────────────────────────
# WAIT=1：排队等他人 CEF 实例收工（不是抢，是等）。WAITMAX 秒后放弃。
if [ "${FORCE:-0}" != "1" ]; then
  if [ "${WAIT:-0}" = "1" ]; then
    deadline=$(( $(date +%s) + ${WAITMAX:-900} ))
    while :; do
      busy="$(systemctl --user list-units --no-pager 2>/dev/null | grep -E '^\s*shift-cef-' | grep -c 'active running' || true)"
      [ "$busy" -eq 0 ] && break
      [ "$(date +%s)" -ge "$deadline" ] && { say "ABORT 等待他人实例超时（仍有 $busy 个在跑）"; exit 90; }
      say "$(date '+%T') 有 $busy 个他人 CEF 实例在跑，排队等待…"; sleep 15
    done
    say "$(date '+%T') 窗口空出，起跑"
  else
    busy="$(systemctl --user list-units --no-pager 2>/dev/null | grep -E '^\s*shift-cef-' | grep -c 'active running' || true)"
    if [ "$busy" -gt 0 ]; then
      say "ABORT 有 $busy 个他人 CEF 实例在跑，按排他铁律让行。确认无人后可 FORCE=1 强跑。"
      exit 90
    fi
  fi
fi
PORT=""
for p in 9338 9339 9340 9341 9342; do
  if ! ss -ltn 2>/dev/null | grep -q ":$p "; then PORT="$p"; break; fi
done
[ -z "$PORT" ] && { say "ABORT 9338-9342 全被占"; exit 91; }
XAUTH="$(ls -1 /run/user/1000/.mutter-Xwaylandauth.* 2>/dev/null | head -1)"
say "端口=$PORT  profile=$PROF  单元=$UNIT  XAUTH=$XAUTH"

# ── 1. 取证被测对象（跑之前先记死）────────────────────────────
{
  say "采集时刻: $(date '+%F %T%z')"
  say "被测 URL: $URL"
  say "二进制: $BIN  mtime=$(stat -c '%y' "$BIN" 2>/dev/null | cut -c1-19)  sha256=$(sha256sum "$BIN" | cut -c1-16)"
  say "HEAD: $(git -C "$REPO" rev-parse --short HEAD)"
  say "工作树脏文件（本次被测含在途未提交改动）:"
  git -C "$REPO" status --short -- src-tauri/ | sed 's/^/  /'
  say "F-CEF-11 四个源文件 mtime+sha256:"
  for f in src-tauri/src/extensions/cef_inject.rs src-tauri/src/extensions.rs src-tauri/src/engine/cef/browser.rs src-tauri/src/cef_cmd.rs; do
    say "  $f  $(stat -c '%y' "$REPO/$f" 2>/dev/null | cut -c1-19)  sha256=$(sha256sum "$REPO/$f" 2>/dev/null | cut -c1-16)"
  done
  say "启动命令（真实 env 全集，journal 不记这个，故此处落盘）:"
  say "  systemd-run --user --unit=$UNIT --collect \\
    --setenv=DISPLAY=:0 --setenv=XAUTHORITY=$XAUTH \\
    --setenv=SHIFT_CEF_DEBUG_PORT=$PORT --setenv=SHIFT_CEF_SHELL_URL=$URL \\
    --setenv=SHIFT_CEF_CACHE_DIR=$PROF --setenv=GDK_BACKEND=x11 --setenv=LD_LIBRARY_PATH=$REPO/src-tauri/target/debug \\
    $BIN"
  say "CDP 探针: scripts/cef-coordinator/ff11-probe.js   求值工具: scripts/cef-coordinator/cef-eval.py"
} > "$OUTD/meta.md"

# ── 2. 起实例（独立 systemd 单元，脱离调用方 cgroup）──────────
systemd-run --user --unit="$UNIT" --collect \
  --setenv=DISPLAY=:0 --setenv=XAUTHORITY="$XAUTH" \
  --setenv=SHIFT_CEF_DEBUG_PORT="$PORT" --setenv=SHIFT_CEF_SHELL_URL="$URL" \
  --setenv=SHIFT_CEF_CACHE_DIR="$PROF" \
  --setenv=GDK_BACKEND=x11 --setenv="LD_LIBRARY_PATH=$REPO/src-tauri/target/debug" \
  "$BIN" > "$OUTD/systemd-run.log" 2>&1
say "systemd-run 退出码=$? $(tail -1 "$OUTD/systemd-run.log" 2>/dev/null)"

# ── 3. 等 CDP 就绪（最多 40s）─────────────────────────────────
ok=0
for i in $(seq 1 40); do
  if curl -sf -m 2 "http://127.0.0.1:$PORT/json" > /dev/null 2>&1; then ok=1; break; fi
  sleep 1
done
say "CDP 就绪=$ok （等了 ${i}s）"
if [ "$ok" != "1" ]; then
  say "FAIL CDP 未起，实例日志尾部:"; journalctl --user -u "$UNIT" --no-pager 2>/dev/null | tail -20 | sed 's/^/  /'
  systemctl --user stop "$UNIT" 2>/dev/null; rm -rf "$PROF"
  exit 92
fi

# ── 4. 求值 ───────────────────────────────────────────────────
cd "$REPO"
python3 scripts/cef-coordinator/cef-eval.py "$PORT" "$OUTD/probe.json" scripts/cef-coordinator/ff11-probe.js
say "cef-eval 退出码=$?"

# ── 5. 抓后端注入日志（预计算是否真跑、几项、多少字节）────────
journalctl --user -u "$UNIT" --no-pager 2>/dev/null | grep -iE "cef-inject|\[cef\] 注入通道|预计算" > "$OUTD/inject.log"
say "注入相关日志 $(wc -l < "$OUTD/inject.log") 行:"
sed 's/^/  /' "$OUTD/inject.log" | head -12

# ── 6. 清理 ──────────────────────────────────────────────────
systemctl --user stop "$UNIT" 2>/dev/null; say "单元已停(退出码 $?)"
sleep 2
rm -rf "$PROF"; say "临时 profile 已删: $PROF"
say "产物目录: $OUTD"
say "OUTDIR=$OUTD"
