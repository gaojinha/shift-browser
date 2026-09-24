#!/usr/bin/env bash
# PreToolUse hook: block direct edits to lock files.
# 锁文件只能由包管理器（npm / cargo）变更，AI 直接编辑是常见事故源。
# 读取工具输入 JSON（stdin），若 file_path 命中锁文件则 exit 2 拦截。
set -u

input=$(cat)
path=$(printf '%s' "$input" | grep -oE '"file_path"[[:space:]]*:[[:space:]]*"[^"]*"' | head -1 | sed 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')

case "$path" in
  *package-lock.json|*Cargo.lock)
    echo "已拦截：$path 是锁文件，必须通过 npm install / cargo add|update 等包管理器命令变更，不能直接编辑。" >&2
    exit 2
    ;;
esac

exit 0
