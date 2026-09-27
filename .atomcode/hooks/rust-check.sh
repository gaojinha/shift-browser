#!/usr/bin/env bash
# PostToolUse hook: 编辑 src-tauri 下 .rs 后自动 cargo check（带防抖）。
# 目的：前端有 format-front.sh，后端编辑此前无任何即时反馈（webzh.rs E0063
# 2026-09-27 实证漏网到手动 clippy 才暴露）。本钩子把类型错误提前到编辑当时。
# 行为：热编译增量下通常 10–40s；用 timeout 90 兜底，超时/失败只提示不阻塞，
# 最终关口仍是 deliver skill / CI。
set -u

input=$(cat)
path=$(printf '%s' "$input" | grep -oE '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')

[ -n "$path" ] && [ -f "$path" ] || exit 0
case "$path" in
  *src-tauri/*.rs) ;;
  *) exit 0 ;;
esac

cd src-tauri || exit 0

# 防抖：60s 内重复编辑只查一次，避免连续编辑时排队跑多个 cargo
MARK=".atomcode-rustcheck.last"
now=$(date +%s)
last=$(cat "$MARK" 2>/dev/null || echo 0)
if [ $((now - last)) -lt 60 ]; then
  exit 0
fi
date +%s > "$MARK"

out=$(timeout 90 cargo check --message-format=short 2>&1)
rc=$?
if [ $rc -eq 124 ]; then
  echo "rust-check: 冷编译超时（90s），请手动跑 cargo check" >&2
elif [ $rc -ne 0 ]; then
  echo "rust-check: cargo check 失败，错误尾部：" >&2
  printf '%s\n' "$out" | grep -E "^error|error\[" | head -10 >&2
fi
exit 0
