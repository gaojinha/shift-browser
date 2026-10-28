#!/usr/bin/env bash
# 一键收集 ShiftBrowser 运行日志（排查崩溃/异常用）
#
# 用法：bash scripts/collect-logs.sh
# 产出：/tmp/shift-logs-<时间>.tar.gz —— 直接把这个文件发给二狗
set -uo pipefail

APP_DIR="$HOME/.local/share/com.shiftbrowser.app"
STAMP=$(date +%Y%m%d-%H%M%S)
OUT="/tmp/shift-logs-$STAMP.tar.gz"
TMP=$(mktemp -d)

{
  echo "=== 收集时间 ==="; date -Is
  echo; echo "=== 系统 ==="; uname -a; uptime
  echo; echo "=== 内存 ==="; free -h
  echo; echo "=== 进程（ShiftBrowser）==="
  ps -eo pid,ppid,%cpu,%mem,rss,etime,cmd | grep -i "shift-browser" | grep -v grep
  echo; echo "=== 安装位置 ==="
  ls -l /opt/shift-browser-cef/shift-browser /usr/bin/shift-browser 2>/dev/null
  echo; echo "=== CEF 资源 ==="; ls /opt/shift-browser-cef 2>/dev/null | head -12
  echo; echo "=== 版本 ==="; grep -m1 '^version' "$(dirname "$0")/../src-tauri/Cargo.toml" 2>/dev/null || echo "未知"
} > "$TMP/system.txt" 2>&1

mkdir -p "$TMP/app"
# 日志：tauri-plugin-log 主日志 + 轮转、stdio 重定向、panic、崩溃信号
for f in "$APP_DIR"/logs/*.log "$APP_DIR"/logs/*.log.* \
         "$APP_DIR"/panic.log "$APP_DIR"/crash.log "$APP_DIR"/crash-signal.log \
         "$APP_DIR"/errors.log "$APP_DIR"/diagnostics.txt; do
  [ -f "$f" ] && cp "$f" "$TMP/app/" 2>/dev/null
done

# 每个日志只留最后 3000 行，避免打包过大（完整内容仍在原文件）
for f in "$TMP/app"/*; do
  [ -f "$f" ] || continue
  if [ "$(wc -l < "$f" 2>/dev/null || echo 0)" -gt 3000 ]; then
    tail -3000 "$f" > "$f.tail" && mv "$f.tail" "$f"
  fi
done

# 内核消息（可能没权限，尽力而为）
dmesg 2>/dev/null | tail -80 > "$TMP/dmesg-tail.txt" 2>/dev/null \
  || echo "无权限读 dmesg（可 sudo dmesg | tail -80 手动补）" > "$TMP/dmesg-tail.txt"

# 系统崩溃目录（有 core dump / crash 文件时一并带上）
ls -lt /var/crash 2>/dev/null | head -20 > "$TMP/var-crash.txt" 2>&1

tar -czf "$OUT" -C "$TMP" . 2>/dev/null
rm -rf "$TMP"

echo "已生成：$OUT"
ls -lh "$OUT"
echo
echo "把这个文件发给二狗即可。"
echo "提示："
echo "  · 排查时用 SHIFT_LOG_LEVEL=debug shift-browser 启动，日志会详细得多"
echo "  · 菜单「诊断信息」会生成 diagnostics.txt（错误计数 + 最近 30 条）"
echo "  · errors.log 是结构化错误流水（module=... detail=... ctx{...}）"
