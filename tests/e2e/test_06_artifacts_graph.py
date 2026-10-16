#!/usr/bin/env python3
"""test_06 · 产物归集与运行图：runs/plan_tree_dir/graph 端点（启动前静态形态）。"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_06_artifacts_graph")
    server = Server()
    try:
        server.start()
        server.req("POST", "/projects", {"name": "p", "local_path": "/tmp/p"})
        server.req("POST", "/projects/p/tasks", {"id": "t", "sop": "planning"})

        # 1. 产物归集端点：初始空 runs + plan_tree_dir 指向两层级约定路径
        status, body = server.req("GET", "/projects/p/tasks/t/runs")
        r.check(status == 200, "GET runs == 200", f"status={status}")
        r.check(body.get("runs") == [], "初始 runs 为空", f"{body.get('runs')}")
        r.check(body.get("current_run") is None, "初始无 current_run", f"{body.get('current_run')}")

        expected_dir = str(server.home / ".devforge" / "plans" / "p" / "t")
        actual_dir = body.get("plan_tree_dir", "")
        r.check(
            actual_dir == expected_dir,
            "plan_tree_dir 符合两层级约定（decisions/002）",
            f"actual={actual_dir}, expected={expected_dir}",
        )

        # 2. 运行图端点：Pending 态
        status, graph = server.req("GET", "/projects/p/tasks/t/graph")
        r.check(status == 200, "GET graph == 200", f"status={status}")
        r.check(graph.get("status") == "Pending", "图状态 Pending", f"{graph.get('status')}")
        r.check(graph.get("sop") == "planning", "图含 SOP 标识", f"{graph.get('sop')}")
        r.check(graph.get("nodes") == [], "Pending 无节点", f"{graph.get('nodes')}")

        # 3. 幽灵 404
        status, _ = server.req("GET", "/projects/p/tasks/ghost/runs")
        r.check(status == 404, "幽灵任务 runs == 404", f"status={status}")
        status, _ = server.req("GET", "/projects/p/tasks/ghost/graph")
        r.check(status == 404, "幽灵任务 graph == 404", f"status={status}")

        # 4. findings 预埋：服务器 cwd 围栏目录（供 e2e 对照 plan tree 实际落点）
        r.check(
            server.cwd.is_dir(), "服务器 cwd 围栏就绪（e2e 落点对照用）", str(server.cwd),
        )

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
