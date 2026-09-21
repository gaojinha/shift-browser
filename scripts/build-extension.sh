#!/usr/bin/env bash
# 构建 Web 进程扩展（阶段 3.2 真扩展体系）并拷到「web extensions 目录」。
# 该目录 = 主程序 exe 所在目录（lib.rs 里 set_web_extensions_directory 指定的位置），
# WebKit 启动时从该目录加载所有扩展 .so。
#
# 用法：scripts/build-extension.sh
# 注意：改完扩展后需重启浏览器才生效（扩展在 Web 进程启动时加载）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/src-tauri/web-extension"
DEST="$ROOT/src-tauri/target/release"

cargo build --release --manifest-path "$SRC/Cargo.toml"
cp "$SRC/target/release/libshift_web_extension.so" "$DEST/"
echo "✅ 扩展已构建并拷到 $DEST/libshift_web_extension.so"
echo "   重启浏览器后生效。"
