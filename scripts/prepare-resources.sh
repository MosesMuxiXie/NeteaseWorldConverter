#!/usr/bin/env bash
# prepare-resources.sh — 填充 backends / runtime 目录（macOS/Linux）。
# macOS 优先：若同机有原版便携包则复用；否则自动从 je2be-core 源码构建 b2j，
# 并用系统 jlink 生成 runtime（需先装 JDK 17+）。
# 用法：npm run prepare:unix [原版包目录]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="${1:-$(dirname "$ROOT")/NeteaseWorldConverter}"
BACKENDS="$ROOT/src-tauri/backends"
RUNTIME="$ROOT/src-tauri/runtime"
mkdir -p "$BACKENDS" "$RUNTIME"

if [ -d "$SOURCE" ]; then
  echo ">> 从原版便携包复制：$SOURCE"
  cp -f "$SOURCE/app/chunker-cli.jar" "$BACKENDS/" 2>/dev/null || true
  cp -f "$SOURCE/app/native/b2j.exe" "$BACKENDS/" 2>/dev/null || true
  cp -f "$SOURCE/app/native/libc++.dll" "$BACKENDS/" 2>/dev/null || true
  cp -f "$SOURCE/app/native/libunwind.dll" "$BACKENDS/" 2>/dev/null || true
  if [ -d "$SOURCE/runtime" ]; then
    cp -a "$SOURCE/runtime/." "$RUNTIME/"
  fi
fi

# Chunker（缺失时从官方 Release 拉取，MIT 许可）
if [ ! -f "$BACKENDS/chunker-cli.jar" ]; then
  echo ">> 下载 chunker-cli.jar"
  curl -L "https://github.com/HiveGamesOSS/Chunker/releases/download/1.19.1/chunker-cli.zip" -o /tmp/chunker-cli.zip
  unzip -o /tmp/chunker-cli.zip -d /tmp/chunker
  find /tmp/chunker -name 'chunker-cli.jar' -exec cp {} "$BACKENDS/" \;
fi

# b2j：macOS 需要本机构建（je2be-core 的 CMake b2j 目标）
if [ "$(uname -s)" = "Darwin" ] && [ ! -f "$BACKENDS/b2j" ] && [ ! -f "$BACKENDS/b2j.exe" ]; then
  echo ">> 构建 b2j（je2be-core）"
  bash "$ROOT/scripts/build-b2j-macos.sh"
fi

# runtime：系统 JDK 生成 jlink 镜像（Chunker 子进程使用）
if [ ! -f "$RUNTIME/bin/java" ] && [ ! -f "$RUNTIME/bin/java.exe" ] && command -v jlink >/dev/null 2>&1; then
  echo ">> 生成 jlink runtime"
  rm -rf "$RUNTIME"   # checkout 自带 README.md，jlink 要求输出目录为空
  mkdir -p "$RUNTIME"
  jlink --add-modules java.base,java.desktop,java.logging,java.management,java.naming,java.sql,java.xml,jdk.unsupported,jdk.crypto.ec \
        --no-header-files --no-man-pages --compress=zip-9 \
        --output "$RUNTIME"
fi

if [ ! -f "$BACKENDS/b2j" ] && [ ! -f "$BACKENDS/b2j.exe" ]; then
  echo "注意：未找到 b2j。请运行 bash scripts/build-b2j-macos.sh 或从 je2be-core Release 获取。" >&2
fi

ls -la "$BACKENDS"
echo "runtime: $(du -sh "$RUNTIME" 2>/dev/null | cut -f1)"
echo "OK"
