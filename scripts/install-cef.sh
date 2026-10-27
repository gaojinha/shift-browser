#!/usr/bin/env bash
# 把 release CEF 构建安装到系统（/opt + /usr/bin wrapper）
#
# 用法：sudo bash scripts/install-cef.sh
# 说明：旧 WebKit 版 /usr/bin/shift-browser 会备份为 shift-browser.webkit.bak；
#       desktop 文件 Exec=shift-browser %U 继续兼容，不用改。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/src-tauri/target/release"
DST="/opt/shift-browser-cef"

[ -x "$SRC/shift-browser" ] || { echo "❌ release 二进制不存在：$SRC/shift-browser"; exit 1; }
[ -f "$SRC/libcef.so" ] || { echo "❌ 缺 libcef.so，release 资源不全"; exit 1; }

echo "==> 安装到 $DST"
install -d -m 755 "$DST"
install -m 755 "$SRC/shift-browser" "$DST/shift-browser"
for f in libcef.so icudtl.dat v8_context_snapshot.bin \
         chrome_100_percent.pak chrome_200_percent.pak resources.pak; do
  [ -e "$SRC/$f" ] && install -m 644 "$SRC/$f" "$DST/$f"
done
[ -d "$SRC/locales" ] && cp -a "$SRC/locales" "$DST/"
[ -d "$SRC/resources" ] && cp -a "$SRC/resources" "$DST/"

echo "==> 替换 /usr/bin/shift-browser（旧版备份为 .webkit.bak）"
if [ -f /usr/bin/shift-browser ] && ! grep -q "opt/shift-browser-cef" /usr/bin/shift-browser 2>/dev/null; then
  cp -a /usr/bin/shift-browser /usr/bin/shift-browser.webkit.bak
  echo "    已备份：/usr/bin/shift-browser.webkit.bak"
fi
cat > /usr/bin/shift-browser <<'WRAP'
#!/bin/sh
# CEF 构建启动器：资源与二进制都在 /opt/shift-browser-cef
export LD_LIBRARY_PATH=/opt/shift-browser-cef${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
exec /opt/shift-browser-cef/shift-browser "$@"
WRAP
chmod 755 /usr/bin/shift-browser

echo "==> 校验"
ls -lh "$DST/shift-browser" "$DST/libcef.so" | sed 's/^/    /'
echo "    /usr/bin/shift-browser -> $(head -3 /usr/bin/shift-browser | tail -1)"
echo "✅ 安装完成。启动：shift-browser"
