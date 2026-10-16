#!/usr/bin/env python3
"""test_05 · SSE 事件流：领域事件订阅、payload 字段、顺序保持。"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream  # noqa: E402


def main() -> int:
    r = Reporter("test_05_events_sse")
    server = Server()
    try:
        server.start()

        with SseStream(server) as sse:
            # 触发两个领域事件
            server.req("POST", "/projects", {"name": "evp", "local_path": "/tmp/evp"})
            server.req("POST", "/projects/evp/tasks", {"id": "evt", "sop": "planning"})

            # 1. project.created 到达且 payload 正确
            ev1 = sse.wait_for(lambda e: e.get("type") == "project.created", timeout=10)
            r.check(ev1 is not None, "收到 project.created", f"{ev1}")
            r.check(
                ev1 and ev1.get("project") == "evp",
                "project.created payload 含项目名", f"{ev1}",
            )

            # 2. task.created 到达且 payload 含项目+任务
            ev2 = sse.wait_for(lambda e: e.get("type") == "task.created", timeout=10)
            r.check(ev2 is not None, "收到 task.created", f"{ev2}")
            r.check(
                ev2 and ev2.get("project") == "evp" and ev2.get("task") == "evt",
                "task.created payload 完整", f"{ev2}",
            )

            # 3. 顺序保持：project.created 先于 task.created
            types = [e.get("type") for e in sse.events]
            idx_p = types.index("project.created") if "project.created" in types else -1
            idx_t = types.index("task.created") if "task.created" in types else -1
            r.check(
                0 <= idx_p < idx_t, "事件顺序与触发顺序一致", f"{types}",
            )

            # 4. 无关动作不产生伪事件：GET 请求后无新事件
            n_before = len(sse.events)
            server.req("GET", "/projects")
            import time
            time.sleep(1.0)
            r.check(
                len(sse.events) == n_before, "只读请求不产生事件", f"{len(sse.events)} vs {n_before}",
            )

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
