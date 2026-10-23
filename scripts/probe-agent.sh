#!/usr/bin/env bash
# DR-6 L1 冒烟：agent spawn/凭据层探针（引擎无关，秒级）。
# 用法：scripts/probe-agent.sh <profile 目录> [prompt]
# 例：scripts/probe-agent.sh ~/.devforge/profiles/doc-agent
# 判定：60s 内收到 success=true 的 response（模型调用真的发起）= PASS；
#       success=false（如 No API key）或超时无响应 = FAIL（DR-1 型故障）。
set -u
PROFILE="${1:?用法: probe-agent.sh <profile 目录> [prompt]}"
PROMPT="${2:-只回复两个字：收到}"
SESSION="$(mktemp -d)"
OUT="$SESSION/stdout.log"

cleanup() { kill "$PI_PID" 2>/dev/null; rm -rf "$SESSION"; }
trap cleanup EXIT

( echo "{\"id\":\"probe-1\",\"type\":\"prompt\",\"message\":\"$PROMPT\"}"; sleep 60 ) \
  | PI_CODING_AGENT_DIR="$PROFILE" DEVFORGE_MANAGED=probe-$$ DEVFORGE_ROLE=probe \
    timeout 60 pi --mode rpc --session-dir "$SESSION/sessions" > "$OUT" 2>"$SESSION/stderr.log" &
PI_PID=$!

for _ in $(seq 1 60); do
  if grep -q '"type":"response"' "$OUT" 2>/dev/null; then
    if grep -q '"success":true' "$OUT"; then
      echo "PASS: $PROFILE 模型调用已发起（response success=true）"
      exit 0
    else
      echo "FAIL: $PROFILE 命令失败："
      grep '"type":"response"' "$OUT" | head -1 | cut -c1-300
      exit 1
    fi
  fi
  sleep 1
done
echo "FAIL: $PROFILE 60s 无任何 response（挂死——DR-1 型）"
exit 1
