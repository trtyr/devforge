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

供给升级（006）：pi-lens/rpiv-todo 真装不剥（F2 已根因定论非上游缺陷），
链尾扫描编码会话 toolCall 命名空间作为插件/MCP 工具可见性证据。
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

PLAN_TIMEOUT = 2400.0    # 规划段：40 分钟（沿 test_08 口径）
CODE_TIMEOUT = 10800.0   # 编码段：180 分钟（v4 实证 7200s 不足：晚向慢模型 5 阶段自划分 2h 未竟被收割，C14 同窗预算问题重演；v2→v3 先例：加窗即通非缺陷，故 180min 拿全供给 Settled+Completed 见证）


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

        # ---- manifest：真 key；全供给真装（006 供给升级：不剥包，F2 已定论非上游缺陷） ----
        key = real_api_key("newapi")
        _, manifest = server.req("GET", "/config/manifest")
        manifest["api_key"] = key
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
            plan_settled = bool(runs_body.get("runs")) and runs_body["runs"][0]["status"] == "settled"
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
                if coding and coding["status"] in ("settled", "failed"):
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
                    fin2["status"] == "settled",
                    f"编码 run 终态 Settled（实际 {fin2['status']}）",
                )

            # 任务终态（graph 端点）
            status, graph = server.req("GET", "/projects/demo-app/tasks/planning-run/graph")
            if status == 200:
                r.check(
                    graph.get("status") == "completed",
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

        # ---- 供给与工具可见性取证（006 供给升级） ----
        # 1) 供给件真落盘：pi-lens 的 @ast-grep/cli 二进制在 coding-agent profile
        astgrep = server.home / ".devforge" / "profiles" / "coding-agent" / "npm" / "node_modules" / "@ast-grep" / "cli" / "ast-grep"
        r.check(astgrep.exists(), "pi-lens 供给真落盘（全供给跑）", f"{astgrep}")
        # 2) 会话 toolCall 命名空间扫描（插件/MCP 工具可见性证据）
        import re
        tool_names: set[str] = set()
        sessions_dir = server.home / ".devforge" / "sessions" / "coding"
        for jf in sessions_dir.glob("*.jsonl"):
            try:
                for line in jf.read_text(errors="replace").splitlines():
                    for m in re.finditer(r'"name"\s*:\s*"([^"]+)"', line):
                        tool_names.add(m.group(1))
            except OSError:
                pass
        r.check(
            True,
            f"编码会话工具命名空间取证: {sorted(tool_names)[:20]}",
            "",
        )
        has_core = any(n in tool_names for n in ("read", "write", "edit", "bash"))
        r.check(
            has_core,
            "编码 Agent 核心工具真实调用（会话 toolCall 实证）",
            f"namespaces={sorted(tool_names)[:10]}",
        )
        mcp_called = sorted(n for n in tool_names if n.startswith("mcp__"))
        plugin_called = sorted(
            n for n in tool_names
            if not n.startswith("mcp__") and ("lens" in n.lower() or "todo" in n.lower())
        )
        r.check(
            True,
            f"MCP 工具调用={mcp_called or '无'} / 插件工具调用={plugin_called or '无'}（agent 自主选择，计数不计 FAIL）",
            "",
        )

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
