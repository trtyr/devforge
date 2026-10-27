#!/usr/bin/env python3
"""test_02 · 任务管理 API：SOP 校验/重复/幽灵项目/列表/跨项目同名隔离。"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_02_tasks")
    server = Server()
    try:
        server.start()

        # 两个项目（跨项目同名任务隔离用）
        for name in ("p1", "p2"):
            status, _ = server.req("POST", "/projects", {"name": name, "local_path": f"/tmp/e2e-{name}"})
            r.check(status == 201, f"前置项目 {name} 创建", f"status={status}")

        # 1. 正常创建任务（内置 planning SOP）
        status, body = server.req(
            "POST", "/projects/p1/tasks", {"id": "mvp", "sop": "planning"},
        )
        r.check(status == 201, "创建任务 == 201", f"status={status}, body={body}")
        r.check(
            isinstance(body, dict) and body.get("status") == "pending",
            "新任务状态 Pending", f"{body}",
        )

        # 2. 未知 SOP → 400（P005 后 coding 已入注册表，未知例改用 ghost；coding 自身应 201）
        status, body = server.req(
            "POST", "/projects/p1/tasks", {"id": "t2", "sop": "coding"},
        )
        r.check(status == 201, "coding SOP 任务 == 201（P005 注册表扩容）", f"status={status}, body={body}")
        status, body = server.req(
            "POST", "/projects/p1/tasks", {"id": "t-ghost", "sop": "ghost"},
        )
        r.check(status == 400, "未知 SOP == 400", f"status={status}, body={body}")

        # 3. 同项目重复任务 → 409
        status, _ = server.req(
            "POST", "/projects/p1/tasks", {"id": "mvp", "sop": "planning"},
        )
        r.check(status == 409, "同项目重复任务 == 409", f"status={status}")

        # 4. 幽灵项目 → 404
        status, _ = server.req(
            "POST", "/projects/ghost/tasks", {"id": "t", "sop": "planning"},
        )
        r.check(status == 404, "幽灵项目 == 404", f"status={status}")

        # 5. 跨项目同名任务：p2 下同 id 应成功（隔离性）
        status, _ = server.req(
            "POST", "/projects/p2/tasks", {"id": "mvp", "sop": "planning"},
        )
        r.check(status == 201, "跨项目同名任务可创建", f"status={status}")

        # 6. 列表按项目隔离（校准：步骤 2 已在 p1 建 t2(coding)，列表应为两单）
        status, body = server.req("GET", "/projects/p1/tasks")
        ids = [t["id"] for t in body.get("tasks", [])]
        r.check(status == 200 and ids == ["mvp", "t2"], "p1 任务列表", f"{ids}")

        # 7. 列表访问幽灵项目 → 404
        status, _ = server.req("GET", "/projects/ghost/tasks")
        r.check(status == 404, "幽灵项目任务列表 == 404", f"status={status}")

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
