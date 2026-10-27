#!/usr/bin/env python3
"""test_19 · P007 全链黑盒测试闭环（真 pi + 真模型 + AI 替用户应答）。

三段跨 SOP 全链（P007 主场景，D012 启动方式）：
创建项目/任务 → 规划 run（Settled+Completed）→ start sop=coding（编码 Settled+Completed）
→ start sop=testing（Test Agent 不看代码只看文档：自划分阶段→写集成/e2e 测试→
   自起被测系统→自跑自收→Test Review 三判据审查）
→ doc-side 判定时编排层自动触发 doc-fix 子 SOP → 过审后同阶段重审（D004 闭环）
→ finish → run Settled + 任务 Completed。

取证对照（P007 设计承诺）：
1. 黑板 testing/ 产物：stages.md + todo.md + findings/ + coverage-matrix.md + reports/review-*.md
2. 工作区留痕：tests/integration 与 tests/e2e 的 Python 测试（D010 落仓库）+ git commit（D009）
3. doc-fix 移交证据：reports/doc-review-*.md（若触发；未触发属 agent 自主选择，记录观察）
4. 交互代理：ui.request 全部由 AI 应答
5. 测试会话 toolCall 命名空间取证（todo/ask-user 插件可见性）

窗口（D011：测试阶段是全流水线最慢阶段，慷慨预算；只收割证据不无限等待）：
PLAN 2400s / CODE 10800s / TEST 10800s。
"""

import json
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

PLAN_TIMEOUT = 2400.0     # 规划段：40 分钟（沿 test_08/test_14 口径）
CODE_TIMEOUT = 21600.0    # 编码段：360 分钟（2026-10-19 两次 3h 窗口均未收尾的实际教训：真模型慢时需要 6h）
TEST_TIMEOUT = 10800.0    # 测试段：180 分钟（D011 最慢阶段；写测+起系统+跑测+审查多轮）

EIGHT = (
    "plan-agent",
    "plan-review",
    "coding-agent",
    "coding-review",
    "test-agent",
    "test-review",
    "doc-fix",
    "doc-agent",
    "doc-review",
)


def newest_session_bytes(server: Server, kind: str) -> int:
    # 2026-10-27 对齐 run-id 隔离布局（旧 sessions/<kind> 平铺已不存在）
    sd = server.home / ".devforge" / "sessions"
    files = list(sd.glob(f"*/{kind}/*.jsonl"))
    return max((f.stat().st_size for f in files), default=0)


def main() -> int:
    r = Reporter("test_19_testing_full_chain")
    server = Server()
    timeline: list[dict] = []
    try:
        server.start()

        # ---- manifest：真 key；全供给真装（八 profile；测试链需要真插件） ----
        key = real_api_key("newapi")
        if key is None:
            # 2026-10-27 自审 major：缺凭据是环境故障，不是产品 run 失败——
            # 走 ENV_ERROR 出口，不再把 None 写进 manifest 让故障伪装成产品红。
            print("ENV_ERROR：real_api_key 返回 None（缺 NEWAPI 凭据），跳过本脚本")
            return 2
        _, manifest = server.req("GET", "/config/manifest")
        manifest["api_key"] = key
        t0 = time.time()
        status, _ = server.req("PUT", "/config/manifest", manifest, timeout=900.0)
        r.check(status == 202, f"manifest 受理（{time.time()-t0:.1f}s，八 profile 全供给）", f"status={status}")
        prof = server.home / ".devforge" / "profiles"
        deadline = time.time() + 1800  # 今晚装包慢 2-3 倍实测：八 profile 全供给预算翻倍
        while time.time() < deadline:
            if all((prof / n / "settings.json").exists() for n in EIGHT):
                break
            time.sleep(2)
        r.check(
            all((prof / n / "AGENTS.md").exists() for n in EIGHT),
            "八 profile 渲染齐（全供给）", f"{prof}",
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

        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="完成")
            autopilot.start()

            # ---- 段一：规划 run ----
            status, _ = server.req("POST", "/projects/demo-app/tasks/planning-run/start")
            r.check(status == 202, "规划 start == 202", f"status={status}")
            fin = None
            plan_deadline = time.time() + PLAN_TIMEOUT
            while time.time() < plan_deadline:
                for ev in sse.events:
                    if ev.get("type") == "run.finished":
                        fin = ev
                        break
                if fin:
                    break
                time.sleep(5.0)
            timeline.extend(sse.events[len(timeline):])
            if fin is None:
                r.issue("C19-PLAN-TIMEOUT", "high",
                        f"规划段 {PLAN_TIMEOUT}s 未结束（时间线 {len(timeline)} 条已留档）")
                r.check(False, "规划 run 在超时内结束")
            else:
                r.check(fin.get("ok") is True, "规划 run.finished ok=True", f"{fin}")

            _, runs_body = server.req("GET", "/projects/demo-app/tasks/planning-run/runs")
            runs = runs_body.get("runs", [])
            plan_settled = bool(runs) and runs[0]["status"] == "Settled"
            r.check(plan_settled, "规划 run Settled", f"{runs[0].get('status') if runs else None}")
            tree = server.home / ".devforge" / "plans" / "demo-app" / "planning-run"
            r.check(tree.is_dir() and any(tree.iterdir()),
                    "plan tree 已落黑板（编码段的输入）", f"{tree}")
            plan_run_id = runs[0]["id"] if runs else ""

            # ---- 段二：跨 SOP 触发 coding（runs 数组时间正序，编码 run = id 不同的那条） ----
            status, _ = server.req(
                "POST", "/projects/demo-app/tasks/planning-run/start", {"sop": "coding"},
            )
            r.check(status == 202, "start sop=coding == 202（跨 SOP 触发）", f"status={status}")

            code_start = time.time()
            code_deadline = code_start + CODE_TIMEOUT
            growth: list[tuple[float, int]] = []
            coding_run_id = None
            code_final = None
            while time.time() < code_deadline:
                _, runs_body = server.req("GET", "/projects/demo-app/tasks/planning-run/runs")
                coding = next(
                    (x for x in runs_body.get("runs", []) if x["id"] != plan_run_id), None
                )
                if coding and coding["status"] in ("Settled", "Failed"):
                    coding_run_id = coding["id"]
                    code_final = coding["status"]
                    break
                timeline.extend(sse.events[len(timeline):])
                el = time.time() - code_start
                growth.append((round(el), newest_session_bytes(server, "coding")))
                time.sleep(15.0)
            timeline.extend(sse.events[len(timeline):])

            if code_final is None:
                r.issue("C19-CODE-TIMEOUT", "high",
                        f"编码段 {CODE_TIMEOUT}s 未结束（增长曲线 {growth[-6:]}）")
                r.check(False, "编码 run 在超时内结束")
            else:
                r.check(
                    code_final == "Settled",
                    f"编码 run 终态 Settled（实际 {code_final}）",
                )
            _, graph = server.req("GET", "/projects/demo-app/tasks/planning-run/graph")
            coding_completed = graph.get("status") == "Completed"
            r.check(
                coding_completed,
                f"编码段后任务终态 Completed（实际 {graph.get('status')}）",
            )

            # ---- 段三：跨 SOP 触发 testing（D012；测试段最慢，窗口慷慨） ----
            if not coding_completed:
                r.issue("C19-SKIP-TESTING", "high",
                        "编码段未 Completed，testing 段跳过（前置产物不成立）")
                r.check(False, "testing 段前置（编码 Completed）成立")
            else:
                status, _ = server.req(
                    "POST", "/projects/demo-app/tasks/planning-run/start", {"sop": "testing"},
                )
                r.check(status == 202, "start sop=testing == 202（第三段跨 SOP 触发，D012）",
                        f"status={status}")

                test_start = time.time()
                test_deadline = test_start + TEST_TIMEOUT
                t_growth: list[tuple[float, int]] = []
                testing_final = None
                while time.time() < test_deadline:
                    _, runs_body = server.req("GET", "/projects/demo-app/tasks/planning-run/runs")
                    testing = next(
                        (x for x in runs_body.get("runs", [])
                         if x["id"] not in (plan_run_id, coding_run_id)),
                        None,
                    )
                    if testing and testing["status"] in ("Settled", "Failed"):
                        testing_final = testing["status"]
                        break
                    timeline.extend(sse.events[len(timeline):])
                    el = time.time() - test_start
                    t_growth.append((round(el), newest_session_bytes(server, "testing")))
                    time.sleep(15.0)
                timeline.extend(sse.events[len(timeline):])

                if testing_final is None:
                    r.issue("C19-TEST-TIMEOUT", "high",
                            f"测试段 {TEST_TIMEOUT}s 未结束（增长曲线 {t_growth[-6:]}，D011 最慢阶段窗口用尽）")
                    r.check(False, "testing run 在超时内结束")
                else:
                    r.check(
                        testing_final == "Settled",
                        f"testing run 终态 Settled（实际 {testing_final}）",
                    )
                    _, graph = server.req("GET", "/projects/demo-app/tasks/planning-run/graph")
                    r.check(
                        graph.get("status") == "Completed",
                        f"全链收官：任务终态 Completed（实际 {graph.get('status')}）",
                    )

            autopilot.stop()

        # ---- testing 黑板产物取证（无论终态如何，落盘即是证据） ----
        tb = server.home / ".devforge" / "plans" / "demo-app" / "planning-run" / "testing"
        r.check((tb / "stages.md").exists(), "黑板 testing/stages.md 存在（测试阶段自划分）", f"{tb}")
        r.check((tb / "todo.md").exists(), "黑板 testing/todo.md 存在", f"{tb / 'todo.md'}")
        findings = list((tb / "findings").glob("*.md")) if (tb / "findings").is_dir() else []
        r.check(True, f"findings/ 台账条数={len(findings)}（0 亦合法：文档够好或未到此）",
                f"{[p.name for p in findings]}")
        matrix = tb / "coverage-matrix.md"
        r.check(matrix.exists(), "追溯矩阵 coverage-matrix.md 存在（Q007）", f"{matrix}")
        t_reports = list((tb / "reports").glob("*.md")) if (tb / "reports").is_dir() else []
        r.check(len(t_reports) >= 1, "testing/reports/ 审查报告至少一份", f"{[p.name for p in t_reports]}")

        # doc-fix 移交证据（D004：若触发，doc-review 报告落盘）
        doc_reviews = [p for p in t_reports if p.name.startswith("doc-review-")]
        if doc_reviews:
            r.check(True, f"doc-fix 移交闭环实证：doc-review 报告 {len(doc_reviews)} 份（D004 编排闭环触发）",
                    f"{[p.name for p in doc_reviews]}")
        else:
            r.check(True, "doc-fix 移交未触发（审查未判 doc-side——agent 自主选择，观察项不计 FAIL）", "")

        # ---- 工作区取证：tests/ 落仓库（D010）+ git commit（D009） ----
        tests_dir = project_dir / "tests"
        py_tests = list(tests_dir.rglob("test_*.py")) if tests_dir.is_dir() else []
        r.check(
            len(py_tests) >= 1,
            "工作区 tests/ 有 Python 测试（D010 黑盒套件落仓库）",
            f"test 文件数={len(py_tests)}",
        )
        layer_dirs = {d.name for d in tests_dir.iterdir() if d.is_dir()} if tests_dir.is_dir() else set()
        r.check(True, f"tests/ 分层目录={sorted(layer_dirs) or '（未分层，观察项）'}", "")
        log = subprocess.run(
            ["git", "-C", str(project_dir), "log", "--oneline"],
            capture_output=True, text=True,
        )
        commits = [l for l in log.stdout.splitlines() if l.strip()]
        r.check(len(commits) >= 1, "工作区 git commit 留痕（D009 全链延续）", f"commits={commits[:6]}")

        # ---- 测试会话 toolCall 命名空间取证（插件可见性） ----
        tool_names: set[str] = set()
        # 2026-10-27 对齐 run-id 隔离布局：testing 角色 dir 只存在于跑过 testing SOP 的 run
        sessions_dir = server.home / ".devforge" / "sessions"
        for jf in sessions_dir.glob("*/testing/*.jsonl"):
            try:
                for line in jf.read_text(errors="replace").splitlines():
                    for m in re.finditer(r'"name"\s*:\s*"([^"]+)"', line):
                        tool_names.add(m.group(1))
            except OSError:
                pass
        r.check(True, f"测试会话工具命名空间取证: {sorted(tool_names)[:20]}", "")
        has_core = any(n in tool_names for n in ("read", "write", "edit", "bash"))
        r.check(has_core, "测试 Agent 核心工具真实调用（会话 toolCall 实证）",
                f"namespaces={sorted(tool_names)[:10]}")
        banned = sorted(n for n in tool_names if n.startswith("mcp__codegraph") or "lens" in n.lower())
        r.check(
            not banned,
            "黑盒纪律旁证：测试会话未调用 codegraph/pi-lens（D007 供给未配，会话亦未越界）",
            f"banned={banned}",
        )

        # 交互代理对齐
        types = [e.get("type") for e in timeline]
        ui_count = types.count("ui.request")
        r.check(
            not autopilot.answered or len(autopilot.answered) >= ui_count,
            "AI 应答与 ui.request 对齐（全代理）",
            f"answered={len(autopilot.answered)}, requested={ui_count}",
        )

        # 时间线落盘
        out = Path(__file__).resolve().parent / "results" / "test_19_timeline.json"
        out.write_text(json.dumps(timeline, ensure_ascii=False, indent=2))

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
