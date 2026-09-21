#!/usr/bin/env bash
# 一键打包 deb（第八十四章固化，替代手敲 tauri build 长命令）。
#
# 用法：bash scripts/package-deb.sh
# 产出：src-tauri/target/release/bundle/deb/Shift Browser_<版本>_amd64.deb
#
# 说明：本机缺 rpmbuild / linuxdeploy / appimagetool，targets:"all" 会因缺工具
# 失败，所以显式 --bundles deb 锁死（见 HANDOFF 第八十四章）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TAURI_BIN="$ROOT/node_modules/.bin/tauri"
SUBVER=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['version'])" 2>/dev/null || echo "?.?.?")
DEB="src-tauri/target/release/bundle/deb/Shift Browser_${SUBVER}_amd64.deb"

echo "==> ① 校验工具链（缺了先装，别白等编译）"
for t in dpkg-deb fakeroot ar; do
  command -v "$t" >/dev/null || { echo "❌ 缺 $t，先 apt install $t"; exit 1; }
done
echo "    工具齐活（dpkg-deb/fakeroot/ar）"

echo "==> ② 扩展库 + 前端产物"
bash scripts/build-extension.sh >/dev/null 2>&1 && echo "    扩展已构建（libshift_web_extension.so）"
[ -d dist ] || { echo "❌ dist 缺失"; exit 1; }

echo "==> ③ tauri release 打包（--bundles deb，编译约 5-8 分钟）"
"$TAURI_BIN" build --bundles deb

echo "==> ④ 产物"
[ -f "$DEB" ] && echo "    ✅ $DEB" || { echo "❌ 打包产物未找到"; exit 1; }
echo ""
echo "安装：sudo dpkg -i '$DEB'"
