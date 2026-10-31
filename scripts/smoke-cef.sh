#!/usr/bin/env bash
# CEF 单栈构建冒烟测试（规划 M2 验收项 `scripts/smoke-cef.sh`，2026-10-31 落地）
#
# 用法：
#   scripts/smoke-cef.sh                # 完整冒烟：重启 CEF 构建 → 逐项检查
#   scripts/smoke-cef.sh --no-restart   # 只检查当前运行实例
#   APP_BIN=/path/to/shift-browser scripts/smoke-cef.sh   # 指定二进制
#
# 与 scripts/smoke.sh 的区别：那份检查 WebKitGTK 路径专有关键字（GtkOverlay/reparent），
# CEF 构建下永远不会出现，直接跑必红。本脚本只认 CEF 侧日志与 X11 事实。
#
# 日志位置：CEF 构建启动时 redirect_stdio_to_file 把 stdout/stderr 落到
#   ~/.local/share/com.shiftbrowser.app/logs/stdio.log（追加 + 5MB 轮转），
# 本脚本直接读那个文件；每次完整冒烟前 truncate 一次，--no-restart 时截尾 200 行。
# 前提：同机无其他 ShiftBrowser 实例在跑（脚本会 pkill 掉 CEF 构建实例）。
#
# 退出码：0=全过（SKIP 不算失败），1=任一关键项失败，2=用法错误
#
# 诚实边界（别当成万能验收）：
# - 标签 AfterCreated/BeforeClose 目前只走 `cef_poll_events` IPC，不落日志 → 本脚本
#   标 SKIP，不用"日志里有也算过"糊弄；要真验需接 CDP（scripts/cef-coordinator/ 有 ad-hoc 脚本）。
# - 分屏双标签 resize、翻译、AI 快照在 CEF 侧尚未交付（见
#   docs/工程全面分析_2026-10-31.md P1-5），这里标 SKIP 而不是 FAIL。

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_BIN="${APP_BIN:-$REPO_ROOT/src-tauri/target/release/shift-browser}"
APP_DATA="${HOME}/.local/share/com.shiftbrowser.app"
LOG="$APP_DATA/logs/stdio.log"
RESTART=1
BOOT_TIMEOUT="${BOOT_TIMEOUT:-60}"

if [ "${1:-}" = "--no-restart" ]; then
  RESTART=0
elif [ -n "${1:-}" ]; then
  echo "用法: $0 [--no-restart]" >&2
  exit 2
fi

PASS=0
FAIL=0
SKIP=0

pass() { PASS=$((PASS + 1)); echo "  ✅ $1"; }
fail() { FAIL=$((FAIL + 1)); echo "  ❌ $1"; }
skip() { SKIP=$((SKIP + 1)); echo "  ⏭️  SKIP $1"; }

check_log() {
  local label="$1" pattern="$2"
  if grep -qE "$pattern" "$LOG" 2>/dev/null; then pass "$label"; else fail "$label（日志未命中: $pattern）"; fi
}

check_no_match() {
  local label="$1" pattern="$2"
  if grep -qiE "$pattern" "$LOG" 2>/dev/null; then fail "$label（命中: $pattern）"; else pass "$label"; fi
}

echo "=============================================="
echo " ShiftBrowser CEF 冒烟测试"
echo " 时间: $(date '+%F %T')"
echo " 二进制: $APP_BIN"
echo "=============================================="

if [ ! -x "$APP_BIN" ]; then
  echo "❌ 找不到可执行文件: $APP_BIN" >&2
  echo "   先构建：cd src-tauri && cargo build --release --features cef-engine" >&2
  exit 2
fi

# ---------- 0. 启动 ----------
# CEF 构建的启动脚本会把 stdio 重定向到 $LOG（redirect_stdio_to_file），
# 该文件跨启动追加——用启动前行数 + 窗口内的 [cef] 关键字判断本次启动。
if [ "$RESTART" = "1" ]; then
  echo "[0/3] 重启 CEF 构建..."
  pkill -x shift-browser 2>/dev/null || true
  sleep 2
  : >"$LOG" 2>/dev/null || true
  setsid "$APP_BIN" </dev/null &
  started=0
  for _ in $(seq 1 "$BOOT_TIMEOUT"); do
    sleep 1
    if grep -qE "\[cef\] 初始化完成" "$LOG" 2>/dev/null; then
      started=1
      break
    fi
    if ! pgrep -x shift-browser >/dev/null 2>&1; then
      break
    fi
  done
  if [ "$started" != "1" ]; then
    echo "❌ CEF 未在 ${BOOT_TIMEOUT}s 内完成初始化，日志尾部：" >&2
    tail -30 "$LOG" >&2 || true
    exit 1
  fi
  echo "  已启动，等待窗口映射..."
  sleep 3
else
  echo "[0/3] 跳过重启（--no-restart）；按已有 $LOG 尾部 200 行检查"
  tail -200 "$LOG" >"$LOG.tmp" && cat "$LOG.tmp" >"$LOG" && rm -f "$LOG.tmp"
fi

echo ""
echo "[1/3] 启动链路"
check_log "CEF 初始化完成" "\[cef\] 初始化完成"
check_log "单栈窗口已创建" "\[cef\] 单栈窗口已创建"
check_log "force_show 已调用" "\[cef\] force_show 被调用"
check_log "X11 窗口映射 + 去装饰" "\[cef\] XSetClassHint"
check_log "CEF 拿到有效 XID" "window_handle=0x[0-9a-f]+"
check_no_match "无 panic" "panicked at|thread 'main' panicked"

echo ""
echo "[2/3] 进程与窗口事实"
if pgrep -x shift-browser >/dev/null 2>&1; then
  pass "主进程存活"
else
  fail "主进程存活（进程已退出）"
fi

# CEF 是多进程架构：主进程 + zygote + renderer + gpu 至少 2 个
cef_procs=$(pgrep -f "$APP_BIN" 2>/dev/null | wc -l | tr -d ' ')
if [ "$cef_procs" -ge 2 ]; then
  pass "CEF 子进程树存在（$cef_procs 个进程）"
else
  fail "CEF 子进程树存在（只见到 $cef_procs 个，可能退化到单进程）"
fi

if command -v xwininfo >/dev/null 2>&1; then
  # 窗口标题/类名由 shell.rs 设置（class = shift-browser）
  if xwininfo -root -tree 2>/dev/null | grep -qiE "shift-browser|Shift Browser"; then
    pass "X11 顶层窗口存在"
  else
    fail "X11 顶层窗口存在（xwininfo 未见）"
  fi
else
  skip "X11 顶层窗口存在（未安装 xwininfo）"
fi

check_no_match "无断言失败（CHECK failed）" "CHECK failed"
check_no_match "无致命信号（SIGTRAP/段错误）" "SIGTRAP|SIGSEGV|code=dumped"

echo ""
echo "[3/3] 未交付项（SKIP，不计失败）"
skip "标签 AfterCreated/BeforeClose 计数（事件只走 IPC，未落日志；需接 CDP）"
skip "分屏双标签 resize（CEF 侧未交付）"
skip "翻译 / AI 快照 / Agent 动作（CEF 侧未交付，见分析报告 P1-5）"

echo ""
echo "=============================================="
echo " 结果: $PASS 通过 / $FAIL 失败 / $SKIP 跳过"
echo "=============================================="

if [ "$RESTART" = "1" ]; then
  echo "清理：关闭测试实例..."
  pkill -x shift-browser 2>/dev/null || true
  sleep 2
  if pgrep -x shift-browser >/dev/null 2>&1; then
    echo "  ⚠️  仍有残留进程（可能未正常退出）"
  else
    echo "  已退出，进程归零"
  fi
fi

if [ "$FAIL" -gt 0 ]; then
  echo "❌ 冒烟未通过，日志: $LOG"
  exit 1
fi

echo "✅ CEF 冒烟通过，日志: $LOG"
exit 0
