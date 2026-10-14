#!/usr/bin/env bash
# 大文件红线卡（R1 纪律，2026-10-14 落地）
#
# 任意 src-tauri/src 下的 .rs 文件一旦超过 400 行，必须在文件内显式标注豁免：
#     // LINES-EXEMPT: <理由>
# 目的：防 page.rs 式"治理后反弹"无感知发生——新越线文件不声明理由就进不了 CI。
# 越线且无标记 → 本脚本非零退出，CI 红。
#
# 用法：bash scripts/check-rs-file-size.sh   （CI static-checks job 挂载）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC_DIR="$ROOT/src-tauri/src"
LIMIT="${LINES_LIMIT:-400}"   # 本地自测失败路径：LINES_LIMIT=1 bash scripts/check-rs-file-size.sh
MARKER="LINES-EXEMPT"

if [ ! -d "$SRC_DIR" ]; then
  echo "❌ 找不到源码目录: $SRC_DIR" >&2
  exit 1
fi

offenders=()
total_over=0
while IFS= read -r f; do
  lines=$(wc -l < "$f" | tr -d '[:space:]')
  if [ "$lines" -gt "$LIMIT" ]; then
    total_over=$((total_over + 1))
    if ! grep -q "$MARKER" "$f"; then
      offenders+=("$f  (${lines} 行)")
    fi
  fi
done < <(find "$SRC_DIR" -name '*.rs' -type f | sort)

if [ "${#offenders[@]}" -gt 0 ]; then
  echo "❌ 以下 .rs 文件 > ${LIMIT} 行且缺少 '// ${MARKER}: <理由>' 豁免标记：" >&2
  printf '   %s\n' "${offenders[@]}" >&2
  echo "   处理：优先拆分；确需保留则在文件头加 '// ${MARKER}: <理由>' 后重跑。" >&2
  exit 1
fi

echo "✅ 大文件红线检查通过：${total_over} 个 >${LIMIT} 行文件均带 ${MARKER} 豁免标记"
