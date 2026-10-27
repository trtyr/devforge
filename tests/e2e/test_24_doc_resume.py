#!/usr/bin/env python3
"""test_24 · doc-from-zero 续跑实证（plan 010 · T008 / M4）。

第一轮：两个包的 fixture 跑 doc-from-zero，**第一个单元 done 后立刻 cancel**；
第二轮：同一条 start 再发（不带 fresh），验证：
- start 回显 `resumed=true`（断点被认账）；
- 第一单元产物 mtime+size 逐字节不变（不重写）；
- 第二单元补完、run Settled；
- 第二轮新增的 doc-agent 会话里**不出现**第一单元的 kickoff（010 T008 验收原文）。
"""

import json
import os
import shutil
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

DOC_TIMEOUT = float(os.environ.get("DOC_TIMEOUT", "7200"))
WAIT_FIRST_UNIT = float(os.environ.get("WAIT_FIRST_UNIT", "1800"))

ALPHA_PY = '''"""alpha 包：求和。"""


def alpha_sum(xs: list[int]) -> int:
    """返回列表元素之和（空列表返回 0）。"""
    return sum(xs)
'''

BETA_PY = '''"""beta 包：计数。"""


def beta_count(xs: list[int]) -> int:
    """返回列表长度（不看元素内容）。"""
    return len(xs)
'''


def state_path(server: Server, project: str, task: str) -> Path:
    return (
        server.home
        / ".devforge"
        / "plans"
        / project
        / task
        / "doc-from-zero"
        / "state.json"
    )


def load_state(server: Server, project: str, task: str) -> dict:
    p = state_path(server, project, task)
    if not p.exists():
        return {}
    return json.loads(p.read_text(encoding="utf-8"))


def unit_files(repo: Path, state: dict, unit: str) -> dict[str, tuple[int, int]]:
    files = (state.get("data") or {}).get("files") or {}
    snap: dict[str, tuple[int, int]] = {}
    for rel in files.get(unit) or []:
        p = repo / "docs" / "contexts" / rel
        if p.exists():
            st = p.stat()
            snap[rel] = (st.st_mtime_ns, st.st_size)
    return snap


def session_inventory(server: Server) -> dict[str, int]:
    """会话文件 → 当前大小（观测第二轮新增/增长）。

    2026-10-27 对齐 run-id 隔离布局：key = 相对 sessions 根的路径
    （第二轮是新 run-id 目录 → 整目录计入「新增」，正是验收要观测的）。
    """
    root = server.home / ".devforge" / "sessions"
    if not root.is_dir():
        return {}
    return {
        str(p.relative_to(root)): p.stat().st_size for p in root.glob("*/doc/**/*.jsonl")
    }


def wait_run_final(server: Server, project: str, task: str, timeout: float) -> tuple[str | None, float]:
    t0 = time.time()
    while time.time() - t0 < timeout:
        _, body = server.req("GET", f"/projects/{project}/tasks/{task}/runs")
        runs = body.get("runs", [])
        if runs and runs[-1]["status"] in ("Settled", "Failed"):
            return runs[-1]["status"], time.time() - t0
        time.sleep(5.0)
    return None, time.time() - t0


def main() -> int:
    r = Reporter("test_24_doc_resume")
    server = Server()
    try:
        host_manifest = Path.home() / ".devforge" / "manifest.json"
        if host_manifest.exists():
            dest = server.home / ".devforge" / "manifest.json"
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(host_manifest, dest)
        server.start()
        key = real_api_key("newapi")
        _, manifest = server.req("GET", "/config/manifest")
        manifest["api_key"] = key
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        with SseStream(server) as sse:
            status, _ = server.req("PUT", "/config/manifest", manifest, timeout=300.0)
            r.check(status == 202, "PUT 剥包 manifest == 202", f"status={status}")
            ev = sse.wait_for(
                lambda e: e.get("type") == "config.rendered" and int(e.get("rendered") or 0) >= 9,
                timeout=300.0,
            )
            r.check(ev is not None, "profile 渲染完成", f"{ev}")
        compensate_profile(server)

        repo = server.home / "workspace" / "resume-app"
        (repo / "alpha").mkdir(parents=True, exist_ok=True)
        (repo / "beta").mkdir(parents=True, exist_ok=True)
        (repo / "alpha" / "__init__.py").write_text("", encoding="utf-8")
        (repo / "beta" / "__init__.py").write_text("", encoding="utf-8")
        (repo / "alpha" / "a.py").write_text(ALPHA_PY, encoding="utf-8")
        (repo / "beta" / "b.py").write_text(BETA_PY, encoding="utf-8")

        project = "resume-app"
        status, _ = server.req("POST", "/projects", {"name": project, "local_path": str(repo)})
        r.check(status == 201, "项目创建", f"status={status}")
        status, _ = server.req(
            "POST", f"/projects/{project}/tasks", {"id": "d1", "sop": "doc-from-zero"}
        )
        r.check(status == 201, "任务创建", f"status={status}")

        # ---- 第 1 轮：跑到第一个单元 done 就 cancel ----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="继续")
            autopilot.start()
            status, _ = server.req("POST", f"/projects/{project}/tasks/d1/start", None)
            r.check(status == 202, "第 1 轮 start == 202", f"status={status}")

            t0 = time.time()
            first_unit = None
            while time.time() - t0 < WAIT_FIRST_UNIT:
                st = load_state(server, project, "d1")
                done = (st.get("data") or {}).get("done") or []
                if done:
                    first_unit = done[0]
                    break
                time.sleep(5.0)
            r.check(first_unit is not None, f"第一个单元 done（{first_unit}）")
            if first_unit is None:
                return r.finish()

            snap1 = unit_files(repo, load_state(server, project, "d1"), first_unit)
            r.check(bool(snap1), f"第一单元产物已登记（{len(snap1)} 个文件）", f"{sorted(snap1)[:4]}")
            sessions_before = session_inventory(server)

            status, _ = server.req("POST", f"/projects/{project}/tasks/d1/cancel", None)
            r.check(status in (200, 202), "cancel 受理", f"status={status}")
            final1, took1 = wait_run_final(server, project, "d1", 300)
            r.check(final1 == "Failed", f"第 1 轮以取消终态收场（{final1}，{took1:.0f}s）", "cancel 语义 = Failed")
            autopilot.stop()

        # ---- 第 2 轮：同一条 start（无 fresh）----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="继续")
            autopilot.start()
            status, body = server.req("POST", f"/projects/{project}/tasks/d1/start", None)
            r.check(status == 202, "第 2 轮 start == 202", f"status={status}")
            r.check(
                isinstance(body, dict) and body.get("resumed") is True,
                "start 回显 resumed=true（断点认账）",
                f"{body}",
            )
            final2, took2 = wait_run_final(server, project, "d1", DOC_TIMEOUT)
            r.check(final2 == "Settled", f"第 2 轮 Settled（{took2:.0f}s）", f"final={final2}")
            autopilot.stop()
            if final2 != "Settled":
                return r.finish()

        st2 = load_state(server, project, "d1")
        done2 = (st2.get("data") or {}).get("done") or []
        r.check(len(done2) >= 2, f"两个单元都完成（{done2}）")

        snap2 = unit_files(repo, st2, first_unit)
        r.check(
            bool(snap2) and snap1 == snap2,
            "第一单元产物未被重写（mtime+size 逐字节不变）",
            f"before={snap1} after={snap2}",
        )

        # 第二轮新增/增长的会话里不得出现第一单元 kickoff（验收原文）
        sessions_after = session_inventory(server)
        needle = f"本单元 `{first_unit}`"
        leaks = []
        for name, size in sessions_after.items():
            old = sessions_before.get(name, 0)
            if size <= old:
                continue
            p = server.home / ".devforge" / "sessions" / name
            text = p.read_text(encoding="utf-8", errors="ignore")[old:]
            if needle in text:
                leaks.append(name)
        r.check(
            not leaks,
            "第二轮会话无第一单元 kickoff（没重跑已完成的单元）",
            f"leaks={leaks[:5]}",
        )

        print(f"== 数字：第 1 轮到首单元 done 后 cancel；第 2 轮续跑 {took2:.0f}s 补完")
        return r.finish()
    finally:
        server.stop()


if __name__ == "__main__":
    sys.exit(main())
