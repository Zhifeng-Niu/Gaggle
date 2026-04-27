#!/bin/bash
# Gaggle HTTPS 设置脚本
# 在 ECS 服务器上运行此脚本来安装 Let's Encrypt 证书
#
# 使用方法:
#   sudo GAGGLE_DOMAIN=your-domain.com ./setup-https.sh

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 日志函数
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# 检查是否以 root 权限运行
if [ "$EUID" -ne 0 ]; then
    log_error "请使用 sudo 运行此脚本"
    exit 1
fi

# 检查域名变量
if [ -z "$GAGGLE_DOMAIN" ]; then
    log_error "请设置 GAGGLE_DOMAIN 环境变量"
    echo "使用方法: sudo GAGGLE_DOMAIN=your-domain.com ./setup-https.sh"
    exit 1
fi

log_info "开始为 $GAGGLE_DOMAIN 设置 HTTPS..."

# 更新包管理器
log_info "更新包管理器..."
apt-get update -qq

# 安装 certbot 和 nginx 插件
log_info "安装 certbot 和 nginx 插件..."
apt-get install -y certbot python3-certbot-nginx

# 备份现有 nginx 配置
NGINX_CONF="/etc/nginx/sites-available/gaggle.conf"
if [ -f "$NGINX_CONF" ]; then
    log_info "备份现有 nginx 配置..."
    cp "$NGINX_CONF" "${NGINX_CONF}.backup.$(date +%Y%m%d_%H%M%S)"
fi

# 创建临时 nginx 配置用于 HTTP 验证
log_info "创建临时 HTTP-01 验证配置..."
cat > /etc/nginx/sites-available/gaggle-http01.conf << EOF
server {
    listen 80;
    server_name $GAGGLE_DOMAIN;

    location / {
        return 200 'OK';
        add_header Content-Type text/plain;
    }

    location /.well-known/acme-challenge/ {
        root /var/www/html;
    }
}
EOF

# 启用临时配置
ln -sf /etc/nginx/sites-available/gaggle-http01.conf /etc/nginx/sites-enabled/gaggle-http01.conf

# 测试 nginx 配置
log_info "测试 nginx 配置..."
nginx -t

# 重启 nginx
log_info "重启 nginx..."
systemctl restart nginx

# 获取证书
log_info "使用 certbot 获取 SSL 证书..."
certbot certonly --webroot \
    -w /var/www/html \
    -d "$GAGGLE_DOMAIN" \
    --email admin@"$GAGGLE_DOMAIN" \
    --agree-tos \
    --no-eff-email \
    --non-interactive

# 检查证书是否成功获取
if [ ! -f "/etc/letsencrypt/live/$GAGGLE_DOMAIN/fullchain.pem" ]; then
    log_error "证书获取失败，请检查域名解析和 certbot 日志"
    exit 1
fi

log_info "证书获取成功！"

# 设置自动续期
log_info "配置证书自动续期..."
(crontab -l 2>/dev/null | grep -F "certbot renew"; echo "0 0 * * * certbot renew --quiet --post-hook 'systemctl reload nginx'") | crontab -

# 移除临时配置
rm -f /etc/nginx/sites-enabled/gaggle-http01.conf
rm -f /etc/nginx/sites-available/gaggle-http01.conf

# 更新 nginx 配置中的域名占位符
log_info "更新 nginx 配置..."
if [ -f "$NGINX_CONF" ]; then
    # 使用 sed 替换 $GAGGLE_DOMAIN 占位符
    sed -i "s|\\\$GAGGLE_DOMAIN|$GAGGLE_DOMAIN|g" "$NGINX_CONF"
fi

# 测试最终配置
log_info "测试最终 nginx 配置..."
nginx -t

# 重启 nginx
log_info "重启 nginx..."
systemctl restart nginx

log_info "HTTPS 设置完成！"
log_info "证书文件位置:"
echo "  - 证书: /etc/letsencrypt/live/$GAGGLE_DOMAIN/fullchain.pem"
echo "  - 私钥: /etc/letsencrypt/live/$GAGGLE_DOMAIN/privkey.pem"
echo "  - 链: /etc/letsencrypt/live/$GAGGLE_DOMAIN/chain.pem"
log_info "自动续期已配置为每天 00:00 运行"
