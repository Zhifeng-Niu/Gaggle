#!/usr/bin/env bash
# ──────────────────────────────────────────────────
# Gaggle 端到端集成测试
#
# 验证：创建 Space → 邀请 Provider → 多轮 Proposal → 接受/拒绝 → Close
#
# 用法:
#   bash scripts/test-e2e.sh                          # 本地 localhost:8080
#   bash scripts/test-e2e.sh --server 106.15.228.101  # 远程 ECS
#   GAGGLE_SERVER=106.15.228.101 bash scripts/test-e2e.sh
#
# 前置条件:
#   - Gaggle 服务已部署并运行（本地 cargo run 或远程 ECS）
#   - Python 3.10+ 已安装
#   - pip 依赖已安装: requests, websocket-client
# ──────────────────────────────────────────────────

set -euo pipefail

# ─── 颜色 ──────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${BLUE}[INFO]${NC}  $*"; }
ok()    { echo -e "${GREEN}[PASS]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
fail()  { echo -e "${RED}[FAIL]${NC}  $*"; }
step()  { echo -e "${CYAN}[STEP]${NC}  $*"; }

# ─── 配置 ──────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SERVER="${GAGGLE_SERVER:-localhost:8080}"
TIMEOUT="${E2E_TIMEOUT:-180}"    # 总超时秒数
PROVIDER_COUNT=2

# 解析命令行参数
while [[ $# -gt 0 ]]; do
    case "$1" in
        --server)   SERVER="$2"; shift 2 ;;
        --timeout)  TIMEOUT="$2"; shift 2 ;;
        --providers) PROVIDER_COUNT="$2"; shift 2 ;;
        *) warn "未知参数: $1"; shift ;;
    esac
done

# 构造 URL
if [[ "$SERVER" == http* ]]; then
    HTTP_URL="$SERVER"
else
    HTTP_URL="http://${SERVER}"
fi

# ─── 清理 ──────────────────────────────────────────
PIDS=()
cleanup() {
    info "清理进程..."
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
    done
    wait 2>/dev/null || true
    info "清理完成"
}
trap cleanup EXIT

# ─── 辅助函数 ──────────────────────────────────────

check_health() {
    local url="${HTTP_URL}/health"
    # 先试 /health，再试根路径（向后兼容）
    curl -sf "${url}" >/dev/null 2>&1 && return 0
    curl -sf "${HTTP_URL}/" >/dev/null 2>&1 && return 0
    return 1
}

wait_for_server() {
    local attempts="${1:-12}"
    local delay="${2:-5}"
    info "等待服务器 ${HTTP_URL} 上线..."
    for i in $(seq 1 "$attempts"); do
        if check_health; then
            ok "服务器在线 (attempt $i)"
            return 0
        fi
        info "  等待中... ($i/$attempts)"
        sleep "$delay"
    done
    return 1
}

# 使用 Python 单行脚本调 REST API
api_call() {
    local method="$1" path="$2" body="${3:-}"
    if [[ -n "$body" ]]; then
        python3 -c "
import requests, sys, json
try:
    r = requests.${method}('${HTTP_URL}${path}', json=json.loads('''${body}'''), timeout=10)
    print(json.dumps({'status': r.status_code, 'body': r.json() if r.text else {}}))
except Exception as e:
    print(json.dumps({'status': 0, 'body': {'error': str(e)}}))
"
    else
        python3 -c "
import requests, sys, json
try:
    r = requests.${method}('${HTTP_URL}${path}', timeout=10)
    print(json.dumps({'status': r.status_code, 'body': r.json() if r.text else {}}))
except Exception as e:
    print(json.dumps({'status': 0, 'body': {'error': str(e)}}))
"
    fi
}

# ─── 测试开始 ──────────────────────────────────────

echo ""
echo "╔════════════════════════════════════════════════╗"
echo "║   Gaggle 端到端集成测试                        ║"
echo "║   Server: ${HTTP_URL}"
echo "║   Timeout: ${TIMEOUT}s"
echo "╚════════════════════════════════════════════════╝"
echo ""

TESTS_PASSED=0
TESTS_FAILED=0

assert_pass() {
    local name="$1" result="$2"
    if echo "$result" | python3 -c "
import sys, json
d = json.load(sys.stdin)
s = d.get('status', 0)
sys.exit(0 if 200 <= s < 300 else 1)
" 2>/dev/null; then
        ok "$name"
        ((TESTS_PASSED++))
    else
        fail "$name — response: $result"
        ((TESTS_FAILED++))
    fi
}

# ─── Step 1: 健康检查 ──────────────────────────────
step "1/7: 健康检查"
if wait_for_server 6 5; then
    ((TESTS_PASSED++))
else
    fail "服务器 ${HTTP_URL} 不可达"
    echo ""
    echo "请确认 Gaggle 服务已启动："
    echo "  本地: cargo run"
    echo "  远程: bash scripts/deploy-ecs.sh"
    exit 1
fi

# ─── Step 2: 启动 Provider Agent ──────────────────
step "2/7: 启动 ${PROVIDER_COUNT} 个 Provider Agent"

PROVIDER_NAMES=("专业设计服务商" "创意工作室")
PROVIDER_IDS=()

for i in $(seq 0 $((PROVIDER_COUNT - 1))); do
    name="${PROVIDER_NAMES[$i]}"
    info "启动 Provider: ${name}"

    python3 "${SCRIPT_DIR}/provider_agent.py" \
        --name "$name" \
        --server "$SERVER" \
        > "/tmp/gaggle_provider_${i}.log" 2>&1 &
    PIDS+=($!)
    info "  PID=$! 日志=/tmp/gaggle_provider_${i}.log"
done

# 等待 Provider 注册完成
info "等待 Provider 注册..."
sleep 5

# ─── Step 3: 验证 Provider 已注册 ─────────────────
step "3/7: 验证 Provider 已注册"

SEARCH_RESULT=$(api_call GET "/api/v1/providers/search")
PROVIDER_COUNT_FOUND=$(echo "$SEARCH_RESULT" | python3 -c "
import sys, json
d = json.load(sys.stdin)
providers = d.get('body', [])
if isinstance(providers, list):
    print(len(providers))
else:
    print(0)
" 2>/dev/null || echo "0")

info "发现 ${PROVIDER_COUNT_FOUND} 个 Provider"

if [[ "$PROVIDER_COUNT_FOUND" -ge "$PROVIDER_COUNT" ]]; then
    ok "至少 ${PROVIDER_COUNT} 个 Provider 已注册"
    ((TESTS_PASSED++))
else
    warn "注册的 Provider 数量不足 (${PROVIDER_COUNT_FOUND}/${PROVIDER_COUNT})，继续测试..."
fi

# 提取 Provider IDs
for i in $(seq 0 $((PROVIDER_COUNT - 1))); do
    id_file="/tmp/gaggle_provider_id_${PROVIDER_NAMES[$i]}.txt"
    if [[ -f "$id_file" ]]; then
        pid_val=$(cat "$id_file")
        PROVIDER_IDS+=("$pid_val")
        info "Provider ${PROVIDER_NAMES[$i]}: ${pid_val}"
    else
        warn "未找到 ${PROVIDER_NAMES[$i]} 的 ID 文件"
    fi
done

# ─── Step 4: 启动 Consumer (Hermes RFP) ───────────
step "4/7: 启动 Hermes Consumer 发起 RFP"

python3 "${SCRIPT_DIR}/hermes_consumer.py" \
    --server "$SERVER" \
    --rfp-name "E2E测试-RFP" \
    --rfp-description "端到端测试RFP：验证多轮自主谈判" \
    > "/tmp/gaggle_hermes.log" 2>&1 &
HERMES_PID=$!
PIDS+=($HERMES_PID)
info "Hermes PID=$HERMES_PID 日志=/tmp/gaggle_hermes.log"

# ─── Step 5: 等待谈判完成 ─────────────────────────
step "5/7: 等待谈判完成 (最多 ${TIMEOUT}s)"

ELAPSED=0
INTERVAL=5
NEGOTIATION_DONE=false

while [[ $ELAPSED -lt $TIMEOUT ]]; do
    # 检查 Hermes 进程
    if ! kill -0 "$HERMES_PID" 2>/dev/null; then
        NEGOTIATION_DONE=true
        break
    fi

    # 检查日志中是否有完成标志
    if grep -q "Negotiation complete\|RFP Negotiation complete\|Space.*closed.*concluded" \
        "/tmp/gaggle_hermes.log" 2>/dev/null; then
        NEGOTIATION_DONE=true
        break
    fi

    sleep "$INTERVAL"
    ELAPSED=$((ELAPSED + INTERVAL))
    info "  等待中... (${ELAPSED}/${TIMEOUT}s)"
done

if $NEGOTIATION_DONE; then
    ok "谈判已完成 (耗时 ~${ELAPSED}s)"
    ((TESTS_PASSED++))
else
    fail "谈判超时 (${TIMEOUT}s)"
    ((TESTS_FAILED++))
fi

# ─── Step 6: 输出谈判日志 ─────────────────────────
step "6/7: 谈判日志摘要"

echo ""
echo "── Hermes Consumer 日志 ─────────────────────"
if [[ -f /tmp/gaggle_hermes.log ]]; then
    head -50 /tmp/gaggle_hermes.log
    echo "  ... (完整日志: /tmp/gaggle_hermes.log)"
fi

echo ""
echo "── Provider 日志 ────────────────────────────"
for i in $(seq 0 $((PROVIDER_COUNT - 1))); do
    echo "[Provider ${i}: ${PROVIDER_NAMES[$i]}]"
    if [[ -f "/tmp/gaggle_provider_${i}.log" ]]; then
        head -30 "/tmp/gaggle_provider_${i}.log"
        echo "  ... (完整日志: /tmp/gaggle_provider_${i}.log)"
    fi
    echo ""
done

# ─── Step 7: 验证结果 ─────────────────────────────
step "7/7: 验证谈判结果"

# 检查 Hermes 日志中的关键事件
HERMES_LOG="/tmp/gaggle_hermes.log"

if [[ -f "$HERMES_LOG" ]]; then
    # 检查 RFP 创建
    if grep -q "RFP Created\|Creating RFP" "$HERMES_LOG" 2>/dev/null; then
        ok "RFP 已创建"
        ((TESTS_PASSED++))
    else
        warn "未检测到 RFP 创建事件"
        ((TESTS_FAILED++))
    fi

    # 检查提案提交
    if grep -q "proposal\|Proposal" "$HERMES_LOG" 2>/dev/null; then
        ok "收到提案"
        ((TESTS_PASSED++))
    else
        warn "未检测到提案事件"
        ((TESTS_FAILED++))
    fi

    # 检查谈判完成
    if grep -q "complete\|concluded\|accepted" "$HERMES_LOG" 2>/dev/null; then
        ok "谈判已完成 (concluded/accepted)"
        ((TESTS_PASSED++))
    else
        warn "未检测到谈判完成标志"
        ((TESTS_FAILED++))
    fi
else
    fail "Hermes 日志文件不存在"
    ((TESTS_FAILED+=3))
fi

# ─── 结果摘要 ──────────────────────────────────────
echo ""
echo "╔════════════════════════════════════════════════╗"
echo "║   E2E 测试结果                                 ║"
echo "║   通过: ${TESTS_PASSED}  失败: ${TESTS_FAILED}"
echo "╚════════════════════════════════════════════════╝"
echo ""

if [[ $TESTS_FAILED -eq 0 ]]; then
    ok "所有测试通过！"
    exit 0
else
    warn "部分测试失败，请检查日志"
    exit 1
fi
