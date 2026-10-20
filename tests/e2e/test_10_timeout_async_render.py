#!/usr/bin/env python3
"""test_10 · 超时与异步渲染（修复面）：可选 run 超时 + PUT 异步渲染的 SSE 通知。

覆盖修复项：
- P8-ISSUE-01  可选超时：DEVFORGE_RUN_TIMEOUT_MS 显式开启（默认不限时——零硬限制哲学），
  到点 watchdog 清场 → run/task 进 Failed
- P7-ISSUE-03  PUT manifest 异步化：202 即回，后台渲染，完成经 SSE config.rendered
  通知（失败走 config.render_failed）
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, real_api_key, wait_run_status  # noqa: E402

RUN_TIMEOUT_MS = 20000  # 20s：足够 pi 起跑并被超时收割


def main() -> int:
    r = Reporter("test_10_timeout_async_render")
    server = Server(env_extra={"DEVFORGE_RUN_TIMEOUT_MS": str(RUN_TIMEOUT_MS)})
    try:
        server.start()

        # 1. P7-ISSUE-03：订阅 SSE 后 PUT（空包免网络安装），202 即回 + config.rendered 通知
        key = real_api_key("newapi")
        _, manifest = server.req("GET", "/config/manifest")
        if key:
            manifest["api_key"] = key
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        with SseStream(server) as sse:
            t0 = time.time()
            status, resp = server.req("PUT", "/config/manifest", manifest, timeout=30.0)
            elapsed = time.time() - t0
            r.check(status == 202, "PUT manifest == 202（即回不等渲染）", f"status={status}, {elapsed:.1f}s")
            r.check(
                isinstance(resp, dict) and resp.get("async") is True,
                "受理体标记 async", f"{resp}",
            )
            ev = sse.wait_for(lambda e: e.get("type") == "config.rendered", timeout=120)
            r.check(ev is not None, "收到 config.rendered SSE 通知", f"{ev}")
            if ev is not None:
                r.check(
                    ev.get("rendered") == 15,
                    "通知含渲染 profile 数（P008 后九 Agent）", f"{ev}",
                )

        # 2. profile 就绪（渲染已在后台完成）
        prof = server.home / ".devforge" / "profiles"
        for _ in range(30):
            if all((prof / a / "settings.json").exists() for a in ("plan-agent", "plan-review")):
                break
            time.sleep(1.0)

        # 3. P8-ISSUE-01：启动 run，等待超时收割（deadline 20s + watchdog 轮询 300ms + 余量）
        server.req("POST", "/projects", {"name": "to", "local_path": "/tmp/to-e2e"})
        server.req("POST", "/projects/to/tasks", {"id": "timeout-me", "sop": "planning"})
        t0 = time.time()
        status, _ = server.req("POST", "/projects/to/tasks/timeout-me/start")
        r.check(status == 202, "任务启动受理 202", f"status={status}")

        final, runs_body = wait_run_status(server, "to", "timeout-me", {"Failed"}, timeout=90.0)
        took = time.time() - t0
        r.check(final == "Failed", "超时后 run 进 Failed", f"{runs_body}")
        if final == "Failed":
            r.check(
                took < 60.0,
                f"超时收割及时（{took:.0f}s < 60s，deadline={RUN_TIMEOUT_MS / 1000:.0f}s）",
                f"took={took:.0f}s",
            )
        status, body = server.req("GET", "/projects/to/tasks")
        tasks = body.get("tasks", []) if isinstance(body, dict) else []
        me = next((t for t in tasks if t.get("id") == "timeout-me"), {})
        r.check(
            status == 200 and me.get("status") == "Failed",
            "任务状态同步 Failed", f"status={status}, task={me}",
        )

        # 4. run.error / SSE 侧可观测（超时事件若经 SSE 广播，记录证据）
        # （引擎错误详情现在有 stderr 日志与 run 状态双通道；此处只验证 run 列表可见终态）

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
