#!/usr/bin/env python3
"""test_12 · P005 跨 SOP 触发：start 可选 sop 覆盖（同任务先规划后编码）。

覆盖功能点（对照 P005 decisions/001 + topics/orchestration.md）：
- start 带 {"sop": "coding"} → 202 且任务 sop 已切换（跨 SOP 链路的编排入口）
- 未知 sop → 400（注册表校验，先于 run 状态检查——无竞态）
- 无 body start → 202 且沿用任务注册 sop
- 任务创建时 workspace 快照项目 local_path（decisions/001 代码归项目层）
- 未渲染 profile 时编码 run 快速失败（006 T001 F1 修复回归，commit 5d51e29：
  spawn 前门卫拦截，不再真起 pi 挂 Running 占串行槽）
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, start_tolerant  # noqa: E402


def main() -> int:
    r = Reporter("test_12_sop_override")
    server = Server()
    try:
        server.start()

        ws = server.home / "workspace" / "demo-app"
        status, _ = server.req("POST", "/projects", {"name": "demo-app", "local_path": str(ws)})
        r.check(status == 201, "项目 demo-app 创建", f"status={status}")
        status, body = server.req(
            "POST", "/projects/demo-app/tasks", {"id": "flow", "sop": "planning"},
        )
        r.check(status == 201, "规划任务创建", f"status={status}")
        r.check(
            isinstance(body, dict) and str(body.get("workspace", "")).endswith("workspace/demo-app"),
            "任务 workspace 快照项目 local_path（decisions/001）", f"{body}",
        )

        # 1. 未知 sop → 400（先于 run 状态检查，确定性）
        status, body = server.req(
            "POST", "/projects/demo-app/tasks/flow/start", {"sop": "ghost"},
        )
        r.check(status == 400, "未知 sop 覆盖 == 400", f"status={status}, body={body}")

        # 2. sop 覆盖为 coding → 202 且任务 sop 已切换
        status, _ = server.req(
            "POST", "/projects/demo-app/tasks/flow/start", {"sop": "coding"},
        )
        r.check(status == 202, "start sop=coding == 202", f"status={status}")
        deadline = time.time() + 15
        sop_now = None
        while time.time() < deadline:
            _, tasks = server.req("GET", "/projects/demo-app/tasks")
            sop_now = tasks["tasks"][0]["sop"]
            if sop_now == "coding":
                break
            time.sleep(0.5)
        r.check(sop_now == "coding", "任务 sop 已切换为 coding（跨 SOP 链路入口）", f"sop={sop_now}")

        # 3. F1 修复回归（006 T001，commit 5d51e29）：未渲染 profile 的编码 run
        # spawn 前门卫拦截 → run 秒级 Failed，不再真起 pi 挂 Running 占串行槽
        t0 = time.time()
        status, _ = server.req("POST", "/projects/demo-app/tasks/flow/start", {"sop": "coding"})
        r.check(status == 202, "未渲染 profile 下 start 仍受理 == 202", f"status={status}")
        deadline = t0 + 15
        final = None
        while time.time() < deadline:
            _, runs_body = server.req("GET", "/projects/demo-app/tasks/flow/runs")
            runs = runs_body.get("runs", [])
            if runs and runs[-1]["status"] in ("Settled", "Failed"):
                final = runs[-1]["status"]
                break
            time.sleep(0.3)
        elapsed = time.time() - t0
        r.check(
            final == "Failed",
            "未渲染 profile 的编码 run 快速 Failed（F1 修复回归）",
            f"final={final}, elapsed={elapsed:.1f}s",
        )
        r.check(
            elapsed < 10,
            "秒级失败（无真 pi 挂 Running 占串行槽）",
            f"elapsed={elapsed:.1f}s",
        )
        _, tasks_body = server.req("GET", "/projects/demo-app/tasks")
        r.check(
            tasks_body["tasks"][0]["status"] == "Failed",
            "任务终态同步 Failed",
            f"status={tasks_body['tasks'][0]['status']}",
        )

        status, _ = start_tolerant(server, "demo-app", "flow", None)
        r.check(status == 202, "无 body start == 202（沿用注册 sop；Failed 可重跑）", f"status={status}")

    finally:
        server.stop()

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
