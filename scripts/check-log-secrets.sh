#!/usr/bin/env bash
# 日志密钥扫描（R4-10，2026-10-14 落地）
#
# 规则：任意 src-tauri/src 下的 .rs 文件中，log:: 宏调用行（log::info!/warn!/error!/
# debug!/trace!）若同时出现敏感词（token / password / secret / api_key / apikey，
# 按词边界匹配，不区分大小写）即视为可疑，必须在该行上方或同行添加豁免注释：
#     // LOG-SECRET-EXEMPT: <理由>
# 例如：消息文案本身提及 "token" 但实际日志值不含密钥（"API token 已轮换"）即属此类。
# 目的：防止密钥/token 意外泄漏到日志，被日志聚合系统采集后失守。
#
# 用法：bash scripts/check-log-secrets.sh   （CI static-checks job 挂载）
# 本地自测失败路径：FORCE_OFFENDER=1 bash scripts/check-log-secrets.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_DIR="$ROOT/src-tauri/src"
EXEMPT_MARK="LOG-SECRET-EXEMPT"

# 敏感词（词边界，大小写不敏感）
SENSITIVE_WORDS='token|password|secret|api_key|apikey'
# log 宏匹配模式（info/warn/error/debug/trace）
LOG_MACRO='log::(info|warn|error|debug|trace)!'

if [ ! -d "$SRC_DIR" ]; then
  echo "❌ 找不到源码目录: $SRC_DIR" >&2
  exit 1
fi

# 收集命中行：log:: 宏行 AND 含敏感词（词边界）
# 然后检查同文件内是否有该行的豁免注释（行上方连续 3 行内，或本行含）
offenders=()
total_hits=0

while IFS= read -r match; do
  # match 格式: 文件:行号:内容
  f="${match%%:*}"; rest="${match#*:}"
  lineno="${rest%%:*}"
  total_hits=$((total_hits + 1))

  # 检查本行或上方 3 行内是否有豁免注释
  start=$((lineno > 3 ? lineno - 3 : 1))
  if sed -n "${start},$lineno p" "$f" | grep -q "$EXEMPT_MARK"; then
    continue
  fi
  offenders+=("$f:$lineno")
done < <(grep -rnE "${LOG_MACRO}.*\b(${SENSITIVE_WORDS})\b" "$SRC_DIR" --include='*.rs' -i 2>/dev/null || true)

if [ "${#offenders[@]}" -gt 0 ]; then
  echo "❌ 以下 log:: 调用同时含敏感词（token/password/secret/api_key/apikey）且缺少豁免注释：" >&2
  printf '   %s\n' "${offenders[@]}" >&2
  echo "   处理：确认可安全（仅提及关键词，不打印密钥值）则在对应行上方添加" >&2
  echo "         '// ${EXEMPT_MARK}: <理由>'，否则移除日志中的敏感变量绑定。" >&2
  exit 1
fi

echo "✅ 日志密钥扫描通过：${total_hits} 处命中均带 ${EXEMPT_MARK} 豁免注释"
