#!/usr/bin/env python3
"""test_15 · P005 编码运行控制面：可选超时收割 + 终态 cancel 404。

覆盖功能点（对照 P005 decisions/003 + P001 P8-ISSUE-01 机制在编码路径的复用）：
- DEVFORGE_RUN_TIMEOUT_MS 小窗口：真 pi 编码 run 超时 → watchdog 按 run-id 清场 → Failed
- 终态后 cancel → 404（无活动 run 可取消）
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, real_api_key  # noqa: E402

RUN_TIMEOUT_MS = 20000


def main() -> int:
    r = Reporter("test_15_coding_run_control")
    server = Server(env_extra={"DEVFORGE_RUN_TIMEOUT_MS": str(RUN_TIMEOUT_MS)})
    try:
        server.start()

        # 剥必装四件套（免 git clone），保留 key（真模型调用才有时长的意义）
        _, manifest = server.req("GET", "/config/manifest")
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        if (key := real_api_key("newapi")):
            manifest["api_key"] = key
        status, _ = server.req("PUT", "/config/manifest", manifest, timeout=180.0)
        r.check(status == 202, "PUT manifest == 202", f"status={status}")
        # 等 profile 就绪（settings.json 出现即可，spawn 只需目录与配置）
        prof = server.home / ".devforge" / "profiles" / "coding-agent"
        deadline = time.time() + 60
        while time.time() < deadline and not (prof / "settings.json").exists():
            time.sleep(1)
        r.check((prof / "settings.json").exists(), "coding-agent profile 就绪", f"{prof}")

        ws = server.home / "workspace" / "rc-demo"
        status, _ = server.req("POST", "/projects", {"name": "rc-demo", "local_path": str(ws)})
        r.check(status == 201, "项目创建", f"status={status}")
        status, _ = server.req("POST", "/projects/rc-demo/tasks", {"id": "timeout-me", "sop": "coding"})
        r.check(status == 201, "编码任务创建", f"status={status}")

        status, _ = server.req("POST", "/projects/rc-demo/tasks/timeout-me/start")
        r.check(status == 202, "start == 202", f"status={status}")

        # 超时收割：deadline(20s) 后 watchdog 清场 → Failed；容 60s 观测窗
        deadline = time.time() + 60
        final = None
        while time.time() < deadline:
            _, runs_body = server.req("GET", "/projects/rc-demo/tasks/timeout-me/runs")
            runs = runs_body.get("runs", [])
            if runs and runs[0]["status"] in ("settled", "failed"):
                final = runs[0]["status"]
                break
            time.sleep(1)
        r.check(final == "failed", f"超时后编码 run 进 Failed（窗口 {RUN_TIMEOUT_MS}ms）", f"final={final}")

        # 任务状态同步
        _, tasks = server.req("GET", "/projects/rc-demo/tasks")
        r.check(tasks["tasks"][0]["status"] == "failed", "任务状态同步 Failed", f"{tasks['tasks'][0]['status']}")

        # 终态后 cancel → 404
        status, _ = server.req("POST", "/projects/rc-demo/tasks/timeout-me/cancel")
        r.check(status == 404, "终态后 cancel == 404", f"status={status}")

    finally:
        server.stop()

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
