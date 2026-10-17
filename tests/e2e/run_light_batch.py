#!/usr/bin/env python3
"""轻量批跑器：顺序执行 test_00..test_06（无真实模型会话），逐脚本留日志与退出码。"""
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPTS = [
    "test_00_server.py",
    "test_01_projects.py",
    "test_02_tasks.py",
    "test_03_config.py",
    "test_04_auth.py",
    "test_05_events_sse.py",
    "test_06_artifacts_graph.py",
]
PER_SCRIPT_TIMEOUT = 300  # 每脚本上限 5 分钟

results = []
for name in SCRIPTS:
    t0 = time.time()
    log_path = HERE / "results" / f"{name.replace('.py', '')}.log"
    log_path.parent.mkdir(exist_ok=True)
    with open(log_path, "wb") as log:
        p = subprocess.Popen(
            [sys.executable, str(HERE / name)],
            stdout=log, stderr=subprocess.STDOUT, cwd=str(HERE),
        )
        try:
            rc = p.wait(timeout=PER_SCRIPT_TIMEOUT)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()
            rc = 124  # timeout
    took = time.time() - t0
    results.append((name, rc, took))
    print(f"[batch] {name}: rc={rc} ({took:.0f}s)", flush=True)

print("\n[batch] ===== 汇总 =====", flush=True)
fails = [r for r in results if r[1] not in (0, 1)]
for name, rc, took in results:
    print(f"[batch] {name}: rc={rc} ({took:.0f}s)", flush=True)
print(f"[batch] 非环境退出码脚本数: {len(fails)}", flush=True)
