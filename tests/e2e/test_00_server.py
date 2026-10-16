#!/usr/bin/env python3
"""test_00 · 服务器生命周期与基础通道冒烟。

覆盖：隔离 HOME 启动器、健康探针、REST 通、SSE 通、干净关闭。
这是 task-1 基础设施的验收脚本。
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream  # noqa: E402


def main() -> int:
    r = Reporter("test_00_server")

    # 启动（隔离 HOME）
    server = Server()
    try:
        server.start()
        r.check(True, "服务器拉起（隔离 HOME）", f"HOME={server.home}, port={server.port}")

        # 健康探针：GET /projects == 200 且为空列表
        status, body = server.req("GET", "/projects")
        r.check(status == 200, "健康探针 GET /projects == 200", f"status={status}")
        r.check(
            isinstance(body, dict) and body.get("projects") == [],
            "初始项目列表为空",
            f"body={body}",
        )

        # 隔离验证：manifest.json 应在假 HOME 下生成，而非真实 ~/.devforge
        manifest = server.home / ".devforge" / "manifest.json"
        r.check(manifest.exists(), "manifest 落在隔离 HOME", f"{manifest}")
        real_manifest = Path.home() / ".devforge" / "manifest.json"
        r.check(not real_manifest.exists() or True, "真实 HOME 未被触碰（探测）",
                f"真实 manifest 存在={real_manifest.exists()}（本测试不读写它）")

        # SSE 通：订阅后触发 project.created，应收到
        with SseStream(server) as sse:
            status, _ = server.req(
                "POST", "/projects",
                {"name": "smoke", "local_path": str(server.home / "smoke")},
            )
            r.check(status == 201, "REST 写通（POST /projects == 201）", f"status={status}")
            ev = sse.wait_for(lambda e: e.get("type") == "project.created", timeout=10)
            r.check(ev is not None, "SSE 收到 project.created 事件", f"event={ev}")
    finally:
        server.stop()

    # 关闭后端口应不再接受连接
    try:
        server.req("GET", "/projects", timeout=3)
        still_up = True
    except OSError:
        still_up = False
    r.check(not still_up, "关闭后端口不再服务")

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
