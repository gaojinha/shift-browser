#!/usr/bin/env bash
# 首屏体积卡（T-011 · 2026-10-14）
#
# vite build 后运行：入口 chunk（dist/assets/index-*.js）raw 体积超过限额即失败。
# - 默认硬卡 90 kB（R4-5 目标口径）
# - 豁免清单 scripts/size-exemptions.json（仿 LINES-EXEMPT）：
#     { "exemptions": [ { "file": "index", "limit": 200, "until": "R4-5-full", "reason": "…" } ] }
#   file 匹配 chunk 名首个 '-' 前的 token（index-CE3fpNlY.js → "index"）
# - 失败路径自测（两步）：
#     SIZE_EXEMPT_BYPASS=1 SIZE_LIMIT_KB=1 bash scripts/check-bundle-size.sh   # → exit=1
#   （SIZE_LIMIT_KB=1 单独改不触发拦截：豁免条目 limit 优先生效，属预期行为）
#
# 用法：npm run build && bash scripts/check-bundle-size.sh（CI frontend job 挂载）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
WHITELIST="$ROOT/scripts/size-exemptions.json"
DEFAULT_LIMIT_KB="${SIZE_LIMIT_KB:-90}"
# 置 1 时忽略豁免清单（CI/本地验证失败路径用，日常跑不要开）
BYPASS="${SIZE_EXEMPT_BYPASS:-0}"

if [ ! -d "$DIST" ]; then
  echo "❌ 未找到 $DIST —— 先跑 npm run build 再执行本检查" >&2
  exit 1
fi

entry_chunks=$(find "$DIST/assets" -maxdepth 1 -name 'index-*.js' 2>/dev/null || true)
if [ -z "$entry_chunks" ]; then
  echo "❌ $DIST/assets 下未找到入口 chunk（index-*.js）——构建产物结构变了？检查脚本匹配规则" >&2
  exit 1
fi

offenders=()
while IFS= read -r f; do
  bytes=$(wc -c < "$f" | tr -d '[:space:]')
  kb=$(( (bytes + 1023) / 1024 ))
  base=$(basename "$f")
  key="${base%%-*}"

  limit=$DEFAULT_LIMIT_KB
  note=""
  if [ "$BYPASS" = "1" ]; then
    note="豁免已绕过（SIZE_EXEMPT_BYPASS=1）"
  elif [ -f "$WHITELIST" ]; then
    # 豁免查询交给 node（frontend job 必有 node；本地亦然）
    limit_and_reason=$(node -e '
      const fs = require("fs");
      const key = process.argv[1], def = process.argv[2];
      const j = JSON.parse(fs.readFileSync(process.argv[3], "utf-8"));
      const e = (j.exemptions || []).find((x) => x.file === key);
      if (e) console.log(e.limit + "|" + (e.reason || "") + " (until:" + (e.until || "?") + ")");
      else console.log(def + "|");
    ' "$key" "$DEFAULT_LIMIT_KB" "$WHITELIST")
    limit="${limit_and_reason%%|*}"
    note="${limit_and_reason#*|}"
  fi

  if [ "$kb" -gt "$limit" ]; then
    offenders+=("$base  ${kb} kB > 限额 ${limit} kB")
  else
    echo "  ${base}: ${kb} kB ≤ ${limit} kB${note:+   [$note]}"
  fi
done < <(echo "$entry_chunks")

if [ "${#offenders[@]}" -gt 0 ]; then
  echo "❌ 以下入口 chunk 超过首屏体积限额：" >&2
  printf '   %s\n' "${offenders[@]}" >&2
  echo "   处理：优先瘦身（拆依赖/懒加载）；确需放宽则在 scripts/size-exemptions.json 加条目" >&2
  echo "   （file/limit/until/reason 四字段都要填），并在提交说明里写清理由。" >&2
  exit 1
fi

echo "✅ 首屏体积检查通过（默认硬卡 ${DEFAULT_LIMIT_KB} kB + 豁免清单）"
