#!/usr/bin/env python3
"""test_14 · P005 编码 SOP 全链（真 pi + 真模型 + AI 替用户应答）。

跨 SOP 全链路（P005 主场景）：同一任务先规划后编码——
创建项目/任务 → 规划 run（真 pi 产出 plan tree，Settled+Completed）
→ start 带 sop=coding 覆盖 → Coding Agent 读树/自定 N 阶段/逐阶段开发
（代码+文档伴写+单测，工作区=项目 local_path）→ Coding Review 循环
→ finish → run Settled + 任务 Completed → 黑板与工作区产物取证。

取证对照（P005 设计承诺）：
1. 跨 SOP 触发：同任务两次 run，sop 切换生效（decisions/001 黑板交接）
2. 黑板产物：coding/stages.md + todo.md + docs/ + reports/review-*.md
3. 工作区留痕：真代码 + git commit（D009 阶段 PASS 即 commit）
4. 交互代理：ui.request 全部由 AI 应答（含偏差协议的 ask-user-question）

已知降级（F2，记录不修）：pi-lens/rpiv-todo 安装失败 → 编码 Agent 无诊断/todo 插件，
todo 纪律退化为 todo.md 纯文件路径（P005 预案）。
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

PLAN_TIMEOUT = 2400.0    # 规划段：40 分钟（沿 test_08 口径）
CODE_TIMEOUT = 7200.0    # 编码段：120 分钟（v2 实测 60min 内 stage1+2 双 commit 已落但未及收尾；窗口翻倍拿 Settled+Completed 实证）


def newest_session_bytes(server: Server, kind: str) -> int:
    sd = server.home / ".devforge" / "sessions" / kind
    files = list(sd.glob("*.jsonl")) if sd.is_dir() else []
    return max((f.stat().st_size for f in files), default=0)


def wait_run_finished(sse, deadline_ts: float):
    while time.time() < deadline_ts:
        for ev in sse.events:
            if ev.get("type") == "run.finished":
                return ev
        time.sleep(5.0)
    return None


def main() -> int:
    r = Reporter("test_14_coding_full_chain")
    server = Server()
    timeline: list[dict] = []
    try:
        server.start()

        # ---- manifest：真 key；F2 规避剥全部安装包（渲染面完整，插件降级已注明） ----
        key = real_api_key("newapi")
        _, manifest = server.req("GET", "/config/manifest")
        manifest["api_key"] = key
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        t0 = time.time()
        status, _ = server.req("PUT", "/config/manifest", manifest, timeout=600.0)
        r.check(status == 202, f"manifest 受理（{time.time()-t0:.1f}s）", f"status={status}")
        prof = server.home / ".devforge" / "profiles"
        deadline = time.time() + 300
        while time.time() < deadline:
            if all((prof / n / "settings.json").exists()
                   for n in ("plan-agent", "plan-review", "coding-agent", "coding-review")):
                break
            time.sleep(1)
        r.check(
            all((prof / n / "AGENTS.md").exists()
                for n in ("plan-agent", "plan-review", "coding-agent", "coding-review")),
            "四 profile 渲染齐", f"{prof}",
        )
        compensated = compensate_profile(server)
        r.check(
            set(["models.json", "auth.json"]).issubset(set(compensated)),
            "测试侧补偿（配置全量复刻）", f"{compensated}",
        )

        # ---- 项目 + 规划任务 ----
        project_dir = server.home / "workspace" / "demo-app"
        status, _ = server.req(
            "POST", "/projects", {"name": "demo-app", "local_path": str(project_dir)},
        )
        r.check(status == 201, "项目 demo-app 创建（local_path=工作区）", f"status={status}")
        status, _ = server.req(
            "POST", "/projects/demo-app/tasks", {"id": "planning-run", "sop": "planning"},
        )
        r.check(status == 201, "任务 planning-run 创建（sop=planning）", f"status={status}")

        # ---- 段一：规划 run（真 pi 产出 plan tree） ----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="完成")
            autopilot.start()
            status, _ = server.req("POST", "/projects/demo-app/tasks/planning-run/start")
            r.check(status == 202, "规划 start == 202", f"status={status}")
            fin = wait_run_finished(sse, time.time() + PLAN_TIMEOUT)
            timeline.extend(sse.events[len(timeline):])
            if fin is None:
                r.issue("C14-PLAN-TIMEOUT", "high",
                        f"规划段 {PLAN_TIMEOUT}s 未结束（时间线 {len(timeline)} 条已留档）")
                r.check(False, "规划 run 在超时内结束")
            else:
                r.check(fin.get("ok") is True, "规划 run.finished ok=True", f"{fin}")

            # 规划段轻断言（深断言归 test_08）
            status, runs_body = server.req("GET", "/projects/demo-app/tasks/planning-run/runs")
            plan_settled = bool(runs_body.get("runs")) and runs_body["runs"][0]["status"] == "Settled"
            r.check(plan_settled, "规划 run Settled", f"{runs_body.get('runs', [{}])[0].get('status') if runs_body else None}")
            tree = server.home / ".devforge" / "plans" / "demo-app" / "planning-run"
            r.check(tree.is_dir() and any(tree.iterdir()),
                    "plan tree 已落黑板（编码段的输入）", f"{tree}")

            # ---- 段二：跨 SOP 触发——同任务 sop 覆盖为 coding ----
            status, _ = server.req(
                "POST", "/projects/demo-app/tasks/planning-run/start", {"sop": "coding"},
            )
            r.check(status == 202, "start sop=coding == 202（跨 SOP 触发）", f"status={status}")

            # 记住规划 run 的 id（runs 数组按时间正序，runs[0] 是规划）——
            # 编码 run = id 不同的那条（首跑脚本 bug：错把 runs[0] 当编码 run，覆盖后立刻误判收尾）
            _, runs_body = server.req("GET", "/projects/demo-app/tasks/planning-run/runs")
            plan_run_id = runs_body["runs"][0]["id"]

            code_start = time.time()
            code_deadline = code_start + CODE_TIMEOUT
            growth: list[tuple[float, int]] = []
            fin2 = None
            while time.time() < code_deadline:
                _, runs_body = server.req("GET", "/projects/demo-app/tasks/planning-run/runs")
                coding = next(
                    (x for x in runs_body.get("runs", []) if x["id"] != plan_run_id), None
                )
                if coding and coding["status"] in ("Settled", "Failed"):
                    fin2 = {"status": coding["status"], "run_id": coding["id"]}
                    break
                timeline.extend(sse.events[len(timeline):])
                el = time.time() - code_start
                growth.append((round(el), newest_session_bytes(server, "coding")))
                time.sleep(15.0)
            timeline.extend(sse.events[len(timeline):])
            autopilot.stop()

            if fin2 is None:
                r.issue("C14-CODE-TIMEOUT", "high",
                        f"编码段 {CODE_TIMEOUT}s 未结束（增长曲线 {growth[-6:]}，时间线 {len(timeline)} 条已留档）")
                r.check(False, "编码 run 在超时内结束")
            else:
                r.check(
                    fin2["status"] == "Settled",
                    f"编码 run 终态 Settled（实际 {fin2['status']}）",
                )

            # 任务终态（graph 端点）
            status, graph = server.req("GET", "/projects/demo-app/tasks/planning-run/graph")
            if status == 200:
                r.check(
                    graph.get("status") == "Completed",
                    f"任务终态 Completed（实际 {graph.get('status')}）",
                )

        # ---- 编码段产物取证 ----
        # 黑板：coding 子树（stages/todo/docs/reports）
        bb = server.home / ".devforge" / "plans" / "demo-app" / "planning-run" / "coding"
        r.check((bb / "stages.md").exists(), "黑板 coding/stages.md 存在（N 阶段划分）", f"{bb}")
        r.check((bb / "todo.md").exists(), "黑板 coding/todo.md 存在", f"{bb / 'todo.md'}")
        docs = list((bb / "docs").glob("**/*")) if (bb / "docs").is_dir() else []
        r.check(any(f.is_file() for f in docs), "伴写文档 coding/docs/ 非空（D004）", f"文件数={len(docs)}")
        reports = list((bb / "reports").glob("review-*.md")) if (bb / "reports").is_dir() else []
        r.check(len(reports) >= 1, "Review Report 至少一份", f"{[p.name for p in reports]}")
        blockers = list((bb / "blockers").glob("*")) if (bb / "blockers").is_dir() else []
        r.check(True, f"blockers/ 留痕数={len(blockers)}（偏差协议未触发则应为 0）", "")

        # 工作区：真代码 + git commit（D009）
        git_dir = project_dir / ".git"
        r.check(git_dir.exists(), "工作区已 git 化（D009 commit 锚点前提）", f"{project_dir}")
        code_files = [
            p for p in project_dir.rglob("*")
            if p.is_file() and ".git" not in p.parts and p.suffix in (".rs", ".py", ".go", ".ts", ".js", ".html", ".css", ".json", ".md", ".toml")
        ]
        r.check(len(code_files) >= 3, "工作区有真实代码产物", f"文件数={len(code_files)}")
        import subprocess
        log = subprocess.run(
            ["git", "-C", str(project_dir), "log", "--oneline"],
            capture_output=True, text=True,
        )
        commits = [l for l in log.stdout.splitlines() if l.strip()]
        r.check(len(commits) >= 1, "工作区有 git commit 留痕（D009）",
                f"commits={commits[:5]}")

        # 交互代理对齐
        types = [e.get("type") for e in timeline]
        ui_count = types.count("ui.request")
        r.check(
            not autopilot.answered or len(autopilot.answered) >= ui_count - 0,
            "AI 应答与 ui.request 对齐（全代理）",
            f"answered={len(autopilot.answered)}, requested={ui_count}",
        )

        # 时间线落盘
        out = Path(__file__).resolve().parent / "results" / "test_14_timeline.json"
        out.write_text(json.dumps(timeline, ensure_ascii=False, indent=2))

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
