#!/usr/bin/env bash
# Docker 多阶段编译 Gaggle (推荐方案)
#
# 在本地 Docker 中编译 Linux x86_64 二进制，避免交叉编译问题
#
# 前置条件:
#   docker (with buildx support)
#
# 用法:
#   bash scripts/build-in-docker.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUTPUT_DIR="$PROJECT_ROOT/build-output"
BINARY_NAME="gaggle"

echo "=== Gaggle Docker 编译 (推荐方案) ==="

# 检查 Docker
if ! command -v docker &>/dev/null; then
    echo "错误: 未找到 docker"
    echo "请先安装 Docker Desktop"
    exit 1
fi

# 检查 buildx
if ! docker buildx version &>/dev/null; then
    echo "错误: docker buildx 不可用"
    echo "请确保 Docker Desktop 已更新"
    exit 1
fi

cd "$PROJECT_ROOT"

# 清理旧构建
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

echo "开始 Docker 编译 (linux/amd64)..."
echo "首次编译较慢（下载依赖），后续利用缓存层加速"

docker buildx build \
    --platform linux/amd64 \
    -f Dockerfile.cross \
    -t gaggle-build:latest \
    --load \
    .

# 从容器中提取二进制
CONTAINER_ID=$(docker create gaggle-build:latest)
docker cp "$CONTAINER_ID:/gaggle" "$OUTPUT_DIR/$BINARY_NAME"
docker rm "$CONTAINER_ID" >/dev/null

chmod +x "$OUTPUT_DIR/$BINARY_NAME"

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
