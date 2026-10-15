#!/usr/bin/env bash
# 发布物归档（B-06 风险 R4 · 归档纪律）——详见 docs/发布物调试文件归档规范_2026-10-15.md
#
# 用途：在任何剥离（strip="symbols"）决策落地之前，把 4 个 release 产物按
#       “版本+BuildID” 归档，使日后 panic.log 裸地址可用 addr2line /
#       llvm-symbolizer 反解到函数名。
# 纯增量：不改 package-deb.sh / Cargo.toml，不参与现有打包流程；发布时手工调用即可。
#
# 用法：bash scripts/archive-release-artifacts.sh [归档根目录]
#   归档根目录默认 release-artifacts（相对仓库根，自动写入 .gitignore）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ARCHIVE_ROOT="${1:-release-artifacts}"
RELEASE="src-tauri/target/release"

fail() { echo "❌ $*" >&2; exit 1; }

# BuildID：readelf 读 ELF 的 .note.gnu.build-id，输出 16 进制（无则空）
build_id() { readelf -n "$1" 2>/dev/null | awk '/Build ID:/{print $3; exit}'; }
sha256_of() { sha256sum "$1" 2>/dev/null | awk '{print $1}'; }

echo "==> ① 定位 release 产物"
P_MAIN="$RELEASE/shift-browser"
P_MCP="$RELEASE/shift-browser-mcp"
P_LIB="$RELEASE/libshift_browser_lib.so"
[ -f "$P_MAIN" ] || fail "$P_MAIN 缺失（先跑 tauri build --release 或 scripts/package-deb.sh）"
[ -f "$P_MCP" ]  || fail "$P_MCP 缺失"
[ -f "$P_LIB" ]  || fail "$P_LIB 缺失"
# 扩展 .so：优先 release 非 bundle 位置，否则取 deb 包内副本
P_EXT="$(find "$RELEASE" -name libshift_web_extension.so -type f 2>/dev/null | grep -v '/bundle/' | head -1 || true)"
[ -n "$P_EXT" ] || P_EXT="$(find "$RELEASE/bundle" -name libshift_web_extension.so -type f 2>/dev/null | head -1 || true)"
[ -n "$P_EXT" ] || fail "libshift_web_extension.so 未找到（先跑 bash scripts/build-extension.sh）"

NAMES=(shift-browser shift-browser-mcp libshift_browser_lib.so libshift_web_extension.so)
SRCS=("$P_MAIN" "$P_MCP" "$P_LIB" "$P_EXT")
for i in "${!NAMES[@]}"; do
  printf "    ✅ %-28s %s\n" "${NAMES[$i]}" "$(du -h "${SRCS[$i]}" | awk '{print $1}')"
done

VERSION="$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])" 2>/dev/null || echo unknown)"
MAIN_BID="$(build_id "$P_MAIN")"
[ -n "$MAIN_BID" ] || fail "主程序 BuildID 读取失败（缺 readelf？）"

# 归档根：绝对路径直接用，相对路径相对仓库根
case "$ARCHIVE_ROOT" in
  /*) DEST="$ARCHIVE_ROOT/${VERSION}-${MAIN_BID}" ;;
  *)  DEST="$ROOT/$ARCHIVE_ROOT/${VERSION}-${MAIN_BID}" ;;
esac
echo "==> ② 归档 → $DEST"
mkdir -p "$DEST"

GIT_COMMIT="$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo n/a)"
GIT_DATE="$(git -C "$ROOT" log -1 --format=%cd --date=short 2>/dev/null || echo n/a)"
{
  echo "# Shift Browser 发布物归档清单（B-06 R4 · 归档纪律）"
  echo "version:        $VERSION"
  echo "main_build_id:  $MAIN_BID"
  echo "git_commit:     $GIT_COMMIT"
  echo "git_date:       $GIT_DATE"
  echo "archived_at:    $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "rustc:          $(rustc --version 2>/dev/null || echo n/a)"
  echo "cargo:          $(cargo --version 2>/dev/null || echo n/a)"
  echo "strip_profile:  debuginfo（DWARF 已剥、.symtab 保留 → 函数名可解，行号不可）"
  echo ""
  echo "# artifact  build_id  bytes  sha256"
} > "$DEST/MANIFEST.txt"

for i in "${!NAMES[@]}"; do
  n="${NAMES[$i]}"; s="${SRCS[$i]}"
  cp -f "$s" "$DEST/$n"
  printf "%-28s %-16s %10s  %s\n" "$n" "$(build_id "$s")" "$(stat -c%s "$s")" "$(sha256_of "$s")" >> "$DEST/MANIFEST.txt"
done
echo "    📦 4 产物已复制 + MANIFEST.txt"

# .gitignore：仅默认仓库内位置自动维护（自定义根目录交由使用者自行处理）
if [ "$ARCHIVE_ROOT" = "release-artifacts" ]; then
  if ! grep -qxF "release-artifacts/" .gitignore 2>/dev/null; then
    printf '\n# 发布物归档（B-06 R4 · 符号/调试文件，不进仓库）\nrelease-artifacts/\n' >> .gitignore
    echo "    ✅ 已把 release-artifacts/ 写入 .gitignore"
  else
    echo "    ✅ .gitignore 已含 release-artifacts/"
  fi
fi

echo ""
echo "✅ 归档完成：$DEST  （$(du -sh "$DEST" | awk '{print $1}')）"
echo ""
echo "离线符号化示例（panic.log 含裸地址时）："
echo "   addr2line -e '$DEST/shift-browser' -f -C <地址>"
echo "   llvm-symbolizer --obj='$DEST/shift-browser' <地址>"
