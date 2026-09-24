#!/usr/bin/env bash
# ShiftBrowser 供应链安全审计（HANDOFF P2-4）
# 用法：scripts/security-audit.sh
#
# 检查：
#   1. cargo audit —— RustSec 公告（忽略清单见 src-tauri/audit.toml，需逐条评审）
#   2. npm audit   —— npm 公告（high+ 失败；国内镜像不支持 audit 时自动走官方 registry）
#
# 退出码：0=无未处置漏洞；非 0=发现需处理的漏洞或工具缺失
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fail=0

echo "== [1/2] cargo audit =="
if ! cargo audit --version >/dev/null 2>&1; then
  echo "cargo-audit 未安装，执行：cargo install cargo-audit --locked" >&2
  exit 2
fi
(
  cd "$ROOT/src-tauri"
  cargo audit --color never
) || fail=1

echo
echo "== [2/2] npm audit (high+) =="
cd "$ROOT"
# npm audit 走当前配置 registry；npmmirror 等镜像未实现 audit 端点时回退官方源
if ! npm audit --audit-level=high; then
  echo "当前 registry 不支持 audit 或发现漏洞，改用 https://registry.npmjs.org 重试…"
  npm audit --audit-level=high --registry=https://registry.npmjs.org || fail=1
fi

echo
if [ "$fail" -eq 0 ]; then
  echo "供应链审计通过：无未处置漏洞（忽略项见 src-tauri/audit.toml）"
else
  echo "供应链审计未通过：请升级依赖或在评审后登记 audit.toml / audit-level" >&2
fi
exit "$fail"
