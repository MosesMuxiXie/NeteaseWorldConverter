#!/usr/bin/env bash
# build-b2j-windows-arm64.sh — 在 Windows ARM64 运行器上尝试从 je2be-core 源码
# 用 CMake(MSVC, ARM64) 构建原生 arm64 b2j，产物放入 src-tauri/backends/b2j-arm64-native.exe。
# Best effort：该环境下的 MSVC/ARM64 组合未经长期验证（x64 runner 曾稳定崩溃），
# 任何失败都由调用方（release.yml）回退到 vendored x64 产物。
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

echo ">> CMake 配置（Windows ARM64）"
cmake -S "$SRC" -B "$BUILD_DIR/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -A ARM64

echo ">> 构建 b2j 目标"
cmake --build "$BUILD_DIR/build" --config Release --target b2j -j"${NUMBER_OF_PROCESSORS:-4}"

echo ">> 定位产物"
B2J="$(find "$BUILD_DIR/build" -type f -name 'b2j.exe' | head -n1)"
if [ -z "$B2J" ]; then
  echo "未找到 b2j.exe" >&2
  exit 1
fi

mkdir -p "$BACKENDS"
cp "$B2J" "$BACKENDS/b2j-arm64-native.exe"
echo "OK: $BACKENDS/b2j-arm64-native.exe"
