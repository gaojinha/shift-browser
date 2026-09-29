#!/usr/bin/env bash
# ShiftBrowser E2E 冒烟测试
# 用法：
#   scripts/smoke.sh            # 完整冒烟：重启浏览器 → 等启动 → 逐项检查
#   scripts/smoke.sh --no-restart  # 只检查当前运行实例（不重启）
#
# 目的：把架构解耦/引擎抽象各批次的关键链路钉死，防止回归。
# 检查项对应自检 18 项 + 解耦关键链路（布局/reparent/注入/快捷键）。
#
# 退出码：0=全过，1=任一关键项失败，2=脚本用法错误

set -euo pipefail

APP_BIN="/home/gao/shift-browser-tauri/src-tauri/target/release/shift-browser"
LOG="/tmp/opencode/shift-browser.log"
DB="${HOME}/.local/share/com.shiftbrowser.app/shift-browser.db"
RESTART=1

if [ "${1:-}" = "--no-restart" ]; then
  RESTART=0
elif [ -n "${1:-}" ]; then
  echo "用法: $0 [--no-restart]" >&2
  exit 2
fi

PASS=0
FAIL=0

pass() { PASS=$((PASS+1)); echo "  ✅ $1"; }
fail() { FAIL=$((FAIL+1)); echo "  ❌ $1"; }

check_log() {
  local label="$1" pattern="$2"
  if grep -qE "$pattern" "$LOG"; then pass "$label"; else fail "$label"; fi
}

check_no_panic() {
  if grep -qiE "panic|thread 'main' panicked" "$LOG"; then
    fail "无 panic（发现 panic 痕迹）"
  else
    pass "无 panic"
  fi
}

echo "=============================================="
echo " ShiftBrowser E2E 冒烟测试"
echo " 时间: $(date '+%F %T')"
echo "=============================================="

# ---------- 0. 准备 ----------
if [ "$RESTART" = "1" ]; then
  echo "[0/3] 重启浏览器..."
  pkill -x shift-browser 2>/dev/null || true
  sleep 2
  rm -f "$LOG"
  setsid "$APP_BIN" > "$LOG" 2>&1 < /dev/null &
  # 最多等 45 秒（首次启动可能编译注入脚本）
  for i in $(seq 1 45); do
    sleep 1
    if pgrep -x shift-browser >/dev/null 2>&1 && grep -q "selfcheck" "$LOG" 2>/dev/null; then
      break
    fi
  done
  # 再给注入/探针一点时间
  sleep 3
else
  echo "[0/3] 检查当前实例（不重启）..."
fi

# ---------- 1. 进程 ----------
echo "[1/3] 进程与数据库..."
if pgrep -x shift-browser >/dev/null 2>&1; then
  pass "进程存活 (PID $(pgrep -x shift-browser))"
else
  fail "进程存活"
fi

if [ -f "$DB" ]; then
  pass "数据库存在: $DB"
else
  fail "数据库存在"
fi

# ---------- 2. 自检与环境 ----------
echo "[2/3] 自检与环境..."
check_log "自检健康 (健康=true)" '\[selfcheck\] 健康=true'
check_log "环境快照 (版本/引擎/系统/数据目录)" '\[selfcheck\] 环境: ShiftBrowser='
check_log "WebKitGTK 引擎版本" 'WebKitGTK=[0-9.]+'
check_log "全局快捷键挂载 (F12/Ctrl+Shift+I)" '全局快捷键已挂载'

# ---------- 3. 解耦关键链路 ----------
echo "[3/3] 解耦关键链路..."
check_log "GtkOverlay 布局初始化" 'GtkOverlay 布局初始化完成'
check_log "child webview reparent 到 Fixed" 'reparent 到 Fixed'
check_log "隐私防护注入" '隐私防护已注入'
check_log "翻译菜单注入探针 (__shiftTranslateMenu)" '__shiftTranslateMenu=boolean'
check_log "扩展即时注入成功" '页面加载后即时注入成功'
check_no_panic

# ---------- 汇总 ----------
echo "=============================================="
echo " 结果: $PASS 通过 / $FAIL 失败"
echo "=============================================="

if [ "$FAIL" -gt 0 ]; then
  echo "失败项详情见日志: $LOG"
  exit 1
fi
echo "✅ 冒烟测试全部通过"
exit 0
