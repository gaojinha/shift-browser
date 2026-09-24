#!/usr/bin/env bash
# PostToolUse hook: auto-format frontend files with Prettier.
# 读取工具输入 JSON（stdin），若 file_path 是前端文件（.ts/.tsx/.js/.vue）则跑 prettier --write。
# 静默执行，失败不阻塞（CI 的 lint/build 仍是最终兜底）。
set -u

input=$(cat)
path=$(printf '%s' "$input" | grep -oE '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')

[ -n "$path" ] && [ -f "$path" ] || exit 0

case "$path" in
  *.ts|*.tsx|*.js|*.jsx|*.vue)
    npx prettier --write "$path" >/dev/null 2>&1 || true
    ;;
esac

exit 0
