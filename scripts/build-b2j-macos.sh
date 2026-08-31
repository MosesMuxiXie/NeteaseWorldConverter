#!/usr/bin/env bash
# build-b2j-macos.sh — 在 macOS 上从 je2be-core 源码用 CMake 构建 b2j，
# 并把产物（b2j 及其依赖 dylib）放入 src-tauri/backends。
# 用法：bash scripts/build-b2j-macos.sh [je2be-core 版本号，默认 web-4.3.0]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${1:-web-4.3.0}"
BACKENDS="$ROOT/src-tauri/backends"
BUILD_DIR="$(mktemp -d)"
trap 'rm -rf "$BUILD_DIR"' EXIT

echo ">> 下载 je2be-core $VERSION 源码"
curl -L "https://github.com/kbinani/je2be-core/archive/refs/tags/$VERSION.tar.gz" -o "$BUILD_DIR/je2be.tar.gz"
tar -xzf "$BUILD_DIR/je2be.tar.gz" -C "$BUILD_DIR"
SRC="$BUILD_DIR/je2be-core-$VERSION"

echo ">> CMake 配置（目标架构：${ARCH:-本机 $(uname -m)}）"
CMAKE_ARGS=(-S "$SRC" -B "$BUILD_DIR/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5)
if [ -n "${ARCH:-}" ]; then
  CMAKE_ARGS+=(-DCMAKE_OSX_ARCHITECTURES="$ARCH")
fi
cmake "${CMAKE_ARGS[@]}"

echo ">> 构建 b2j 目标"
cmake --build "$BUILD_DIR/build" --target b2j -j"$(sysctl -n hw.ncpu)"

echo ">> 安装到 $BACKENDS"
mkdir -p "$BACKENDS"
find "$BUILD_DIR/build" -type f -name 'b2j' -perm -u+x | head -n1 | xargs -I{} cp {} "$BACKENDS/b2j"
# b2j 静态链接了大部分依赖；若产物带动态库依赖一并复制
find "$BUILD_DIR/build" -type f -name 'libje2be*.dylib' | while read -r lib; do
  cp "$lib" "$BACKENDS/" 2>/dev/null || true
done
chmod +x "$BACKENDS/b2j"
echo "OK: b2j -> $BACKENDS/b2j"
