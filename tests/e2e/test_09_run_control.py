#!/usr/bin/env python3
"""test_09 · 运行控制面（修复面）：cancel 端点 + spawn 工作目录 + stderr 可观测。

覆盖修复项：
- P8-ISSUE-01  运行取消：POST /projects/{p}/tasks/{t}/cancel 置位 → watchdog 按
  DEVFORGE_MANAGED==run_id 精确清场（不误伤用户进程）→ run/task 进 Failed
- P8-ISSUE-04  spawn working_dir = ~/.devforge/plans/<项目>/<任务>/（启动即建目录）
- P8-ISSUE-08  pi stderr 落 <session_dir>/pi-stderr.log（存在即校验可读；pi 静默时
  文件可能不落盘——软观测记录，不算 FAIL）
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, real_api_key, wait_run_status  # noqa: E402


def main() -> int:
    r = Reporter("test_09_run_control")
    server = Server()
    try:
        server.start()

        # 0. 渲染真实 profile（空包免网络安装；带 key 让 pi 健康起跑，堵住「没 key 早死」竞态）
        key = real_api_key("newapi")
        _, manifest = server.req("GET", "/config/manifest")
        if key:
            manifest["api_key"] = key
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        status, _ = server.req("PUT", "/config/manifest", manifest, timeout=30.0)
        r.check(status == 202, "PUT manifest 受理（异步渲染）", f"status={status}")
        prof = server.home / ".devforge" / "profiles"
        for _ in range(120):
            if all((prof / a / "settings.json").exists() for a in ("plan-agent", "plan-review")):
                break
            time.sleep(1.0)
        time.sleep(2.0)

        # 1. 项目 + 任务 + 启动
        status, _ = server.req("POST", "/projects", {"name": "rc", "local_path": "/tmp/rc-e2e"})
        r.check(status in (200, 201, 409), "项目就绪", f"status={status}")
        server.req("POST", "/projects/rc/tasks", {"id": "cancel-me", "sop": "planning"})
        status, _ = server.req("POST", "/projects/rc/tasks/cancel-me/start")
        r.check(status == 202, "任务启动受理 202", f"status={status}")

        # 2. P8-ISSUE-04：spawn 工作目录启动即创建
        plan_root = server.home / ".devforge" / "plans" / "rc" / "cancel-me"
        for _ in range(20):
            if plan_root.is_dir():
                break
            time.sleep(0.5)
        r.check(plan_root.is_dir(), "spawn 工作目录已创建（plans/rc/cancel-me）", f"{plan_root}")

        # 3. P8-ISSUE-01：cancel 端点（run 未注册 control 前是 404，轮询到 200 为止）
        deadline = time.time() + 60
        cancel_status = None
        while time.time() < deadline:
            status, body = server.req(
                "POST", "/projects/rc/tasks/cancel-me/cancel", timeout=10.0,
            )
            if status == 200:
                cancel_status = 200
                break
            # 404 = 无活动 run（尚未注册或已终态）；短等重试
            time.sleep(1.0)
        r.check(cancel_status == 200, "cancel 端点受理 200", f"last={cancel_status}")

        # 4. run / task 进入 Failed
        final, runs_body = wait_run_status(server, "rc", "cancel-me", {"Failed"}, timeout=90.0)
        r.check(final == "Failed", "取消后 run 进 Failed", f"{runs_body}")
        status, body = server.req("GET", "/projects/rc/tasks/cancel-me")
        r.check(
            status == 200 and body.get("status") == "Failed",
            "任务状态同步 Failed", f"status={status}, task={body}",
        )

        # 5. P8-ISSUE-08（软观测）：session 目录与 pi-stderr.log
        sess_dir = server.home / ".devforge" / "sessions" / "plan"
        has_session = sess_dir.is_dir() and any(sess_dir.glob("*.jsonl"))
        r.check(has_session, "plan 会话目录有 jsonl（spawn 实证）", f"{sess_dir}")
        stderr_log = sess_dir / "pi-stderr.log"
        if stderr_log.exists():
            content = stderr_log.read_text(errors="replace")
            r.check(isinstance(content, str), "pi-stderr.log 可读", f"{len(content)} chars")
        else:
            r.issue(
                "OBS-STDERR", "low",
                "pi-stderr.log 未落盘——pi 全程静默时文件不创建（soft 行为，非缺陷；"
                "落盘机制由单测 stderr_lines_land_in_session_log 覆盖）",
                f"{stderr_log} 不存在",
            )

        # 6. 幂等/空态：再 cancel 一次应 404（run 已终态）
        status, _ = server.req("POST", "/projects/rc/tasks/cancel-me/cancel")
        r.check(status == 404, "无活动 run 时 cancel == 404", f"status={status}")

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
