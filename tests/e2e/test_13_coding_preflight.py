#!/usr/bin/env python3
"""test_13 · P005 引擎预检与黑板骨架：编码 run 启动后的确定性产物。

覆盖功能点（对照 P005 decisions/013 + topics/shared-blackboard.md）：
- 工作区（项目 local_path）无 .codegraph → 引擎预检 init（agent 不自行 init）
- 黑板骨架：coding/ docs/ reports/ blockers/ 四目录由引擎创建（stages/todo 由 Agent 自建）
- 任务 workspace 快照：run 后 .codegraph 落在项目登记地址，不是服务器 cwd/黑板

注：剥离 api_key 的 manifest → 编码 Agent 的模型调用会失败，run 终态多为 Failed——
本脚本只验预检与骨架这两件确定性产物，终态不作为断言；run 结束后清理。
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_13_coding_preflight")
    server = Server()
    try:
        server.start()

        # 剥包 + 剥 key：渲染零网络、模型调用必败——只验确定性产物
        _, manifest = server.req("GET", "/config/manifest")
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        manifest["api_key"] = None
        status, _ = server.req("PUT", "/config/manifest", manifest, timeout=120.0)
        r.check(status == 202, "PUT 剥离清单 == 202", f"status={status}")

        # 工作区：全新目录，无 .codegraph
        ws = server.home / "workspace" / "pf-demo"
        status, _ = server.req("POST", "/projects", {"name": "pf-demo", "local_path": str(ws)})
        r.check(status == 201 and not (ws / ".codegraph").exists(),
                "项目创建且工作区无 .codegraph（前置）", f"status={status}, ws={ws}")

        status, _ = server.req("POST", "/projects/pf-demo/tasks", {"id": "pf", "sop": "coding"})
        r.check(status == 201, "编码任务创建", f"status={status}")

        with __import__("common").SseStream(server) as sse:
            status, _ = server.req("POST", "/projects/pf-demo/tasks/pf/start")
            r.check(status == 202, "start == 202", f"status={status}")

            # 轮询预检产物：工作区 .codegraph + 黑板四骨架
            blackboard = server.home / ".devforge" / "plans" / "pf-demo" / "pf"
            deadline = time.time() + 60
            pre = False
            skeleton = False
            while time.time() < deadline:
                pre = (ws / ".codegraph").exists()
                skeleton = all(
                    (blackboard / d).is_dir()
                    for d in ("coding", "coding/docs", "coding/reports", "coding/blockers")
                )
                if pre and skeleton:
                    break
                time.sleep(1)
            r.check(pre, "引擎预检 init：工作区出现 .codegraph（D013）", f"ws={ws}")
            r.check(skeleton, "黑板骨架 coding/docs/reports/blockers 就位（黑板=任务目录）", f"{blackboard}")

            # 收场：等终态或 cancel（模型无 key，run 会自行失败；超时则 cancel 解锁）
            final = None
            deadline = time.time() + 60
            while time.time() < deadline:
                _, runs_body = server.req("GET", "/projects/pf-demo/tasks/pf/runs")
                runs = runs_body.get("runs", [])
                if runs and runs[0]["status"] in ("Settled", "Failed"):
                    final = runs[0]["status"]
                    break
                time.sleep(1)
            if final is None:
                server.req("POST", "/projects/pf-demo/tasks/pf/cancel")
                deadline = time.time() + 30
                while time.time() < deadline:
                    _, runs_body = server.req("GET", "/projects/pf-demo/tasks/pf/runs")
                    runs = runs_body.get("runs", [])
                    if runs and runs[0]["status"] in ("Settled", "Failed"):
                        final = runs[0]["status"]
                        break
                    time.sleep(1)
            r.check(final in ("Settled", "Failed"), "run 收场（终态或 cancel 解锁）", f"final={final}")

    finally:
        server.stop()

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
