#!/usr/bin/env bash
# Build the Linux b2j backend from the same je2be-core tag as macOS.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${1:-web-4.3.0}"
BUILD_DIR="$(mktemp -d)"
trap 'rm -rf "$BUILD_DIR"' EXIT

curl -fL "https://github.com/kbinani/je2be-core/archive/refs/tags/$VERSION.tar.gz" -o "$BUILD_DIR/je2be.tar.gz"
tar -xzf "$BUILD_DIR/je2be.tar.gz" -C "$BUILD_DIR"
SRC="$BUILD_DIR/je2be-core-$VERSION"
cmake -S "$SRC" -B "$BUILD_DIR/build" -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5
cmake --build "$BUILD_DIR/build" --target b2j --parallel 2

B2J="$(find "$BUILD_DIR/build" -type f -name b2j -perm -u+x -print -quit)"
if [ -z "$B2J" ]; then
  echo "Linux b2j build produced no executable" >&2
  exit 1
fi
mkdir -p "$ROOT/src-tauri/backends"
cp "$B2J" "$ROOT/src-tauri/backends/b2j"
chmod +x "$ROOT/src-tauri/backends/b2j"
echo "Built Linux b2j from je2be-core $VERSION"
