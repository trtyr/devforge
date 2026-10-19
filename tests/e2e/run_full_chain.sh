#!/usr/bin/env bash
# 全链路测试包装器：隔离 HOME + dialog 应答器（/proc 自动发现端口）+ test_08。
# 用法: bash run_full_chain.sh   （在 tests/e2e/ 下执行）
set -u
cd "$(dirname "$0")"

# /tmp 是 tmpfs（size ≈ 半内存 ≈ 3.9G），全供给跑会顶爆——隔离 HOME 落 /var/tmp（006 decisions/001）
CHAIN_HOME="$(mktemp -d /var/tmp/devforge-e2e-chain-XXXXXXXX)"
export DEVFORGE_E2E_HOME="$CHAIN_HOME"
echo "[chain] HOME=$CHAIN_HOME"

# 应答器先起：自动发现端口（服务器未起时持续轮询）
python3 dialog_responder.py "$CHAIN_HOME" auto > "$CHAIN_HOME/responder.log" 2>&1 &
RESPONDER_PID=$!
trap 'kill $RESPONDER_PID 2>/dev/null' EXIT

# 全链路测试（PUT 装包 + 真实规划 + 多轮审查，实测 15-25 分钟）
python3 test_08_e2e_planning.py
RC=$?

echo "[chain] test_08 rc=$RC, responder log:"
kill $RESPONDER_PID 2>/dev/null
[ -f "$CHAIN_HOME/responder.log" ] && cat "$CHAIN_HOME/responder.log"
exit $RC
