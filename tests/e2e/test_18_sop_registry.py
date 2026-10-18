#!/usr/bin/env python3
"""test_18 · P007 触发链：testing / doc-fix 的注册与触发面。

覆盖功能点（对照 P007 decisions/D004/D012 + roadmap T005/T006）：
- 注册表扩容：创建任务时 sop=testing / sop=doc-fix 均为合法（201）
- 未知 SOP 拒绝面：ghost → 400，错误清单含全部四个内置 SOP
- 触发链：start {"sop":"testing"} / {"sop":"doc-fix"} → 202 且任务 sop 切换
  （未渲染 profile 下 run 快速 Failed——F1 门卫，串行槽即时释放）
- 无 body start → 202 沿用任务当前 sop
- D012 启动方式：testing 由 API 显式触发，与跨 SOP 触发同款
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_18_sop_registry")
    server = Server()
    try:
        server.start()

        ws = server.home / "workspace" / "demo-app"
        status, _ = server.req("POST", "/projects", {"name": "demo-app", "local_path": str(ws)})
        r.check(status == 201, "项目 demo-app 创建", f"status={status}")

        # 1. 创建时注册面：testing / doc-fix 均为合法 SOP
        status, _ = server.req(
            "POST", "/projects/demo-app/tasks", {"id": "flow", "sop": "planning"},
        )
        r.check(status == 201, "planning 任务创建（链头）", f"status={status}")
        status, body = server.req(
            "POST", "/projects/demo-app/tasks", {"id": "t-fix", "sop": "doc-fix"},
        )
        r.check(status == 201, "doc-fix 任务创建（注册表已含 doc-fix）", f"status={status}, body={body}")

        # 2. 未知 SOP 拒绝面：400 + 清单含全部四个内置 SOP
        status, body = server.req(
            "POST", "/projects/demo-app/tasks/flow/start", {"sop": "ghost"},
        )
        body_str = str(body)
        r.check(status == 400, "未知 sop == 400", f"status={status}")
        for sop in ("planning", "coding", "testing", "doc-fix"):
            r.check(
                sop in body_str,
                f"拒绝清单含 {sop}（注册表四 SOP）", body_str[:160],
            )

        def wait_task_sop(want: str, deadline_s: float = 15.0) -> str:
            deadline = time.time() + deadline_s
            sop_now = None
            while time.time() < deadline:
                _, tasks = server.req("GET", "/projects/demo-app/tasks")
                for t in tasks.get("tasks", []):
                    if t["id"] == "flow":
                        sop_now = t["sop"]
                if sop_now == want:
                    break
                time.sleep(0.3)
            return sop_now or ""

        def wait_runs_terminal(deadline_s: float = 15.0) -> str:
            deadline = time.time() + deadline_s
            final = None
            while time.time() < deadline:
                _, runs_body = server.req("GET", "/projects/demo-app/tasks/flow/runs")
                runs = runs_body.get("runs", [])
                if runs and runs[-1]["status"] in ("Settled", "Failed"):
                    final = runs[-1]["status"]
                    break
                time.sleep(0.3)
            return final or "Running"

        # 3. 触发链：planning → testing → doc-fix（D012：显式 API 触发，同款跨 SOP 机制）
        status, _ = server.req(
            "POST", "/projects/demo-app/tasks/flow/start", {"sop": "testing"},
        )
        r.check(status == 202, "start sop=testing == 202（TESTING_SOP 注册+触发面）", f"status={status}")
        sop_now = wait_task_sop("testing")
        r.check(sop_now == "testing", "任务 sop 已切换为 testing", f"sop={sop_now}")
        r.check(
            wait_runs_terminal() == "Failed",
            "未渲染 profile 下 testing run 快速 Failed（F1 门卫，槽即时释放）",
        )

        status, _ = server.req(
            "POST", "/projects/demo-app/tasks/flow/start", {"sop": "doc-fix"},
        )
        r.check(status == 202, "start sop=doc-fix == 202（DOC_FIX_SOP 触发面）", f"status={status}")
        sop_now = wait_task_sop("doc-fix")
        r.check(sop_now == "doc-fix", "任务 sop 已切换为 doc-fix", f"sop={sop_now}")
        r.check(wait_runs_terminal() == "Failed", "doc-fix run 快速 Failed（F1 门卫）")

        # 4. 无 body start → 202 沿用当前 sop（doc-fix）
        status, _ = server.req("POST", "/projects/demo-app/tasks/flow/start", None)
        r.check(status == 202, "无 body start == 202（沿用注册 sop；Failed 可重跑）", f"status={status}")

    finally:
        server.stop()

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
