#!/usr/bin/env bash
# Gaggle ECS 一键部署脚本
#
# 功能：编译 -> 上传 -> 远程初始化 -> 健康检查
#
# 用法:
#   bash scripts/deploy-ecs.sh                    # 交互式（需确认）
#   bash scripts/deploy-ecs.sh --yes              # 跳过确认
#   SKIP_BUILD=1 bash scripts/deploy-ecs.sh       # 跳过编译（仅部署）
#
# 环境变量:
#   ECS_HOST      - ECS 公网 IP (默认: 106.15.228.101)
#   ECS_USER      - SSH 用户名 (默认: root)
#   ECS_PORT      - SSH 端口 (默认: 22)
#   BUILD_METHOD  - 编译方式: docker | cross (默认: docker)
#   SKIP_BUILD    - 设为 1 跳过编译步骤

set -euo pipefail

# ─── 配置 ───
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY_NAME="gaggle"
REMOTE_DIR="/opt/gaggle"

ECS_HOST="${ECS_HOST:-106.15.228.101}"
ECS_USER="${ECS_USER:-root}"
ECS_PORT="${ECS_PORT:-22}"
BUILD_METHOD="${BUILD_METHOD:-docker}"
SKIP_BUILD="${SKIP_BUILD:-0}"

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -p $ECS_PORT"
SCP_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -P $ECS_PORT"

# ─── 颜色输出 ───
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[OK]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
fail()  { echo -e "${RED}[FAIL]${NC}  $*"; exit 1; }

# ─── 确认提示 ───
if [[ "${1:-}" != "--yes" ]]; then
    echo "=== Gaggle ECS 部署 ==="
    echo "目标: $ECS_USER@$ECS_HOST:$ECS_PORT"
    echo "编译方式: $BUILD_METHOD"
    echo ""
    read -rp "确认部署? [y/N] " confirm
    [[ "$confirm" =~ ^[Yy]$ ]] || { echo "已取消"; exit 0; }
fi

# ─── 步骤 1: 编译 ───
if [[ "$SKIP_BUILD" != "1" ]]; then
    info "步骤 1/5: 编译 Linux x86_64 二进制 (方式: $BUILD_METHOD)"

    cd "$PROJECT_ROOT"

    case "$BUILD_METHOD" in
        docker)
            if ! command -v docker &>/dev/null; then
                fail "未找到 docker，请安装 Docker Desktop 或改用 BUILD_METHOD=cross"
            fi
            bash scripts/build-in-docker.sh
            BINARY_PATH="$PROJECT_ROOT/build-output/$BINARY_NAME"
            ;;
        cross)
            bash scripts/build-linux.sh
            BINARY_PATH="$PROJECT_ROOT/target/x86_64-unknown-linux-musl/release/$BINARY_NAME"
            ;;
        *)
            fail "未知编译方式: $BUILD_METHOD (支持: docker, cross)"
            ;;
    esac

    if [[ ! -f "$BINARY_PATH" ]]; then
        fail "编译产物未找到: $BINARY_PATH"
    fi

    BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
    ok "编译完成 ($BINARY_SIZE)"
else
    # 查找已编译的二进制
    if [[ -f "$PROJECT_ROOT/build-output/$BINARY_NAME" ]]; then
        BINARY_PATH="$PROJECT_ROOT/build-output/$BINARY_NAME"
    elif [[ -f "$PROJECT_ROOT/target/x86_64-unknown-linux-musl/release/$BINARY_NAME" ]]; then
        BINARY_PATH="$PROJECT_ROOT/target/x86_64-unknown-linux-musl/release/$BINARY_NAME"
    else
        fail "未找到已编译的二进制，请先运行编译或去掉 SKIP_BUILD=1"
    fi
    ok "使用已有二进制: $BINARY_PATH"
fi

# ─── 步骤 2: 上传文件 ───
info "步骤 2/5: 上传文件到 ECS"

# 检查 .env 文件
ENV_FILE="$PROJECT_ROOT/.env"
if [[ ! -f "$ENV_FILE" ]]; then
    warn "未找到 .env 文件，将使用 .env.example 作为模板"
    ENV_FILE="$PROJECT_ROOT/.env.example"
fi

# 上传二进制
info "  上传二进制 ($BINARY_SIZE)..."
scp $SCP_OPTS "$BINARY_PATH" "$ECS_USER@$ECS_HOST:/tmp/$BINARY_NAME"

# 上传 .env
info "  上传 .env 配置..."
scp $SCP_OPTS "$ENV_FILE" "$ECS_USER@$ECS_HOST:/tmp/gaggle.env"

# 上传 nginx 配置
info "  上传 nginx 配置..."
scp $SCP_OPTS "$PROJECT_ROOT/deploy/nginx/gaggle.conf" "$ECS_USER@$ECS_HOST:/tmp/gaggle-nginx.conf"

# 上传 systemd unit
info "  上传 systemd unit..."
scp $SCP_OPTS "$PROJECT_ROOT/deploy/gaggle.service" "$ECS_USER@$ECS_HOST:/tmp/gaggle.service"

# 上传 frontend
info "  上传前端资源..."
scp -r $SCP_OPTS "$PROJECT_ROOT/frontend" "$ECS_USER@$ECS_HOST:/tmp/frontend"

ok "文件上传完成"

# ─── 步骤 3: 远程初始化 ───
info "步骤 3/5: 远程初始化 ECS"

ssh $SSH_OPTS "$ECS_USER@$ECS_HOST" bash -s <<'REMOTE_SCRIPT'
set -euo pipefail

echo "  [1/9] 创建部署目录..."
mkdir -p /opt/gaggle

echo "  [2/9] 配置 Swap (2GB)..."
if ! swapon --show | grep -q '/swapfile'; then
    if [[ ! -f /swapfile ]]; then
        fallocate -l 2G /swapfile
        chmod 600 /swapfile
        mkswap /swapfile
    fi
    swapon /swapfile
    # 持久化 swap
    if ! grep -q '/swapfile' /etc/fstab; then
        echo '/swapfile none swap sw 0 0' >> /etc/fstab
    fi
    echo "    Swap 已启用: $(free -h | awk '/Swap/{print $2}')"
else
    echo "    Swap 已存在，跳过"
fi

echo "  [3/9] 部署二进制..."
mv /tmp/gaggle /opt/gaggle/gaggle
chmod +x /opt/gaggle/gaggle

echo "  [4/9] 部署 .env 配置..."
if [[ ! -f /opt/gaggle/.env ]]; then
    mv /tmp/gaggle.env /opt/gaggle/.env
    echo "    .env 已部署 (首次)"
else
    # 保留已有的 .env，只在首次部署时写入
    echo "    .env 已存在，保留当前配置"
    echo "    如需更新，请手动编辑: vi /opt/gaggle/.env"
    rm -f /tmp/gaggle.env
fi

echo "  [5/9] 部署前端资源..."
rm -rf /opt/gaggle/frontend
mv /tmp/frontend /opt/gaggle/frontend

echo "  [6/9] 安装配置 Nginx..."
if ! command -v nginx &>/dev/null; then
    apt-get update -qq
    apt-get install -y -qq nginx
fi

# 部署 nginx 配置
cp /tmp/gaggle-nginx.conf /etc/nginx/sites-available/gaggle.conf

# 启用站点，禁用默认站点
ln -sf /etc/nginx/sites-available/gaggle.conf /etc/nginx/sites-enabled/gaggle.conf
rm -f /etc/nginx/sites-enabled/default

# 验证 nginx 配置
nginx -t

echo "  [7/9] 部署 systemd unit..."
cp /tmp/gaggle.service /etc/systemd/system/gaggle.service
systemctl daemon-reload

echo "  [8/9] 启动 Gaggle 服务..."
systemctl enable gaggle
systemctl restart gaggle

echo "  [9/9] 启动 Nginx..."
systemctl enable nginx
systemctl restart nginx

echo "  远程初始化完成"
REMOTE_SCRIPT

ok "远程初始化完成"

# ─── 步骤 4: 健康检查 ───
info "步骤 4/5: 健康检查"

HEALTH_OK=false

# 等待服务启动
for i in $(seq 1 12); do
    if ssh $SSH_OPTS "$ECS_USER@$ECS_HOST" "curl -sf http://localhost:8080/health >/dev/null 2>&1 || curl -sf http://localhost:8080/ >/dev/null 2>&1"; then
        HEALTH_OK=true
        break
    fi
    info "  等待服务启动... ($i/12)"
    sleep 5
done

if $HEALTH_OK; then
    ok "健康检查通过"
else
    warn "健康检查未通过，检查服务状态:"
    ssh $SSH_OPTS "$ECS_USER@$ECS_HOST" "systemctl status gaggle --no-pager -l" || true
    warn "可能原因：首次启动需要更多时间，或 .env 配置有误"
fi

# ─── 步骤 5: 输出信息 ───
info "步骤 5/5: 部署完成"

echo ""
echo "======================================"
echo "  Gaggle 部署完成!"
echo "======================================"
echo ""
echo "  服务地址:   http://$ECS_HOST"
echo "  API 端口:   80 (nginx) -> 8080 (gaggle)"
echo "  WebSocket:  ws://$ECS_HOST/ws/"
echo ""
echo "  管理命令:"
echo "    systemctl status gaggle       # 查看服务状态"
echo "    journalctl -u gaggle -f       # 实时日志"
echo "    systemctl restart gaggle      # 重启服务"
echo "    vi /opt/gaggle/.env           # 修改配置"
echo "    nginx -t && systemctl reload nginx  # 重载 nginx"
echo ""
echo "  重新部署:"
echo "    bash scripts/deploy-ecs.sh"
echo ""
