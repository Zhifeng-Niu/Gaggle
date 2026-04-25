#!/usr/bin/env bash
# 本地交叉编译 Linux x86_64 二进制 (musl 静态链接)
#
# 前置条件:
#   brew install filosottile/musl-cross/musl-cross
#   rustup target add x86_64-unknown-linux-musl
#
# 注意：如果 solana 依赖链编译失败，请改用 scripts/build-in-docker.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY_NAME="gaggle"
TARGET="x86_64-unknown-linux-musl"
OUTPUT_DIR="$PROJECT_ROOT/target/$TARGET/release"

echo "=== Gaggle 交叉编译 (macOS -> Linux x86_64) ==="

# 检查交叉编译工具链
if ! command -v x86_64-linux-musl-gcc &>/dev/null; then
    echo "错误: 未找到 x86_64-linux-musl-gcc"
    echo "请运行: brew install filosottile/musl-cross/musl-cross"
    exit 1
fi

# 检查 rustup target
if ! rustup target list --installed | grep -q "$TARGET"; then
    echo "安装 Rust target: $TARGET"
    rustup target add "$TARGET"
fi

cd "$PROJECT_ROOT"

echo "开始编译 release binary..."
cargo build --release --target "$TARGET"

BINARY_PATH="$OUTPUT_DIR/$BINARY_NAME"
if [ -f "$BINARY_PATH" ]; then
    BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
    echo ""
    echo "编译成功!"
    echo "  二进制: $BINARY_PATH"
    echo "  大小: $BINARY_SIZE"
    echo ""
    echo "下一步: bash scripts/deploy-ecs.sh"
else
    echo "错误: 编译产物未找到"
    exit 1
fi
