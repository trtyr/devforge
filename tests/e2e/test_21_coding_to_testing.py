#!/usr/bin/env python3
"""test_21 · P007 编码→测试交接（预置小 plan tree，绕开规划与大体量编码）。

test_19 的三段全链受编码段耗时拖累；本脚本把「规划」换成预置的小 plan tree，
保留**真 coding SOP → 真 testing SOP** 的跨 SOP 交接，验证 P007 最关键的衔接面：

1. 编码段：Coding Agent 读小 plan tree → 分阶段开发（代码+文档伴写+单测）→ Settled
2. 交接：`start {"sop":"testing"}` 在同任务上切换到测试阶段
3. 测试段：Test Agent 只读文档写集成/e2e 测试 → 自起自跑 → Test Review → Settled

断言：两段 run 均 Settled、任务 Completed、黑板两侧产物齐备、工作区有代码+测试+commit。
窗口：编码 180 分钟 / 测试 180 分钟（两段各 10800s；小体量 fixture，但测试窗口按 D011「最慢阶段」给足）。
"""

import json
import re
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

EIGHT = (
    "plan-agent", "plan-review", "coding-agent", "coding-review",
    "test-agent", "test-review", "doc-fix", "doc-review",
)
CODE_TIMEOUT = 10800.0  # 180 分钟（与 test_19 同口径：编码阶段生产口径 ≥10800s）
TEST_TIMEOUT = 10800.0  # 180 分钟（D011 最慢阶段；test_21 实测 3600s 不够——真实编码产物文档量大）

PLAN_README = """# wordcount-lite · 产品说明（预置 plan tree）

目标：实现一个零依赖的 Python CLI `wc_lite.py`，统计文本的行数与词数。

## 需求

1. 子命令 `lines`：输出行数；`words`：输出词数（空白分隔）。
2. 输入：文件路径参数；省略时读 stdin。
3. 输出：单个整数到 stdout，退出码 0。
4. 错误：文件不存在 → stderr `error: no such file`，退出码 2。
5. 空输入：行数 0、词数 0，退出码 0。
6. 零运行时依赖，Python 3.8+，只用标准库。

## 交付要求

- 实现 + 单元测试（coding 阶段）+ 伴写文档（coding/docs/）
- 文档须含 Runbook：环境要求、运行命令与预期输出、接口契约、测试运行方式
"""

PLAN_TASK = """# T001 · 实现 wordcount-lite CLI

按 `README.md` 的需求 1-6 实现 `wc_lite.py`，含单元测试与伴写文档（coding/docs/）。
"""


def newest_session_bytes(server: Server, kind: str) -> int:
    sd = server.home / ".devforge" / "sessions" / kind
    files = list(sd.glob("**/*.jsonl")) if sd.is_dir() else []
    return max((f.stat().st_size for f in files), default=0)


def wait_run(server: Server, project: str, task: str, exclude: set[str], window: float, sse, timeline):
    t0 = time.time()
    growth: list[tuple[int, int]] = []
    while time.time() - t0 < window:
        _, body = server.req("GET", f"/projects/{project}/tasks/{task}/runs")
        runs = body.get("runs", [])
        new = [x for x in runs if x["id"] not in exclude]
        if new and new[-1]["status"] in ("Settled", "Failed"):
            return new[-1]["status"], new[-1]["id"], time.time() - t0, growth
        timeline.extend(sse.events[len(timeline):])
        growth.append((round(time.time() - t0), newest_session_bytes(server, "coding") + newest_session_bytes(server, "testing")))
        time.sleep(10.0)
    return None, None, time.time() - t0, growth


def main() -> int:
    r = Reporter("test_21_coding_to_testing")
    server = Server()
    timeline: list[dict] = []
    try:
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
                lambda e: e.get("type") == "config.rendered" and e.get("rendered") == 8,
                timeout=300.0,
            )
            r.check(ev is not None, "八 profile 渲染完成（剥包快路径）", f"{ev}")
        compensated = compensate_profile(server)
        r.check(
            set(["models.json", "auth.json"]).issubset(set(compensated)),
            "测试侧补偿（配置全量复刻）", f"{compensated}",
        )

        # ---- 预置 plan tree + 空工作区 ----
        ws = server.home / "workspace" / "wordcount-lite"
        ws.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "init", "-q"], cwd=ws, check=False)
        status, _ = server.req("POST", "/projects", {"name": "wc-lite", "local_path": str(ws)})
        r.check(status == 201, "项目创建（local_path=工作区）", f"status={status}")
        status, _ = server.req("POST", "/projects/wc-lite/tasks", {"id": "t1", "sop": "planning"})
        r.check(status == 201, "任务 t1 创建", f"status={status}")

        board = server.home / ".devforge" / "plans" / "wc-lite" / "t1"
        board.mkdir(parents=True, exist_ok=True)
        (board / "README.md").write_text(PLAN_README, encoding="utf-8")
        (board / "tasks").mkdir(exist_ok=True)
        (board / "tasks" / "T001.md").write_text(PLAN_TASK, encoding="utf-8")
        r.check((board / "README.md").exists(), "预置小 plan tree 就位")

        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="完成")
            autopilot.start()

            # ---- 段一：真 coding ----
            status, _ = server.req("POST", "/projects/wc-lite/tasks/t1/start", {"sop": "coding"})
            r.check(status == 202, "start sop=coding == 202", f"status={status}")
            code_final, code_run, code_elapsed, growth = wait_run(
                server, "wc-lite", "t1", set(), CODE_TIMEOUT, sse, timeline
            )
            if code_final is None:
                r.issue("C21-CODE-TIMEOUT", "high", f"编码段 {CODE_TIMEOUT}s 未结束（增长 {growth[-5:]}）")
                r.check(False, "编码 run 在窗口内结束")
            else:
                r.check(code_final == "Settled", f"编码 run Settled（实际 {code_final}，{code_elapsed:.0f}s）")

            # 编码段产物（证据不依赖终态）
            cb = board / "coding"
            r.check((cb / "stages.md").exists(), "编码段 stages.md 存在", f"{cb}")
            docs = list((cb / "docs").glob("**/*.md")) if (cb / "docs").is_dir() else []
            r.check(len(docs) >= 1, f"伴写文档 {len(docs)} 篇（D004）", f"{[p.name for p in docs][:5]}")

            # ---- 段二：跨 SOP 交接到 testing ----
            if code_final == "Settled":
                status, _ = server.req("POST", "/projects/wc-lite/tasks/t1/start", {"sop": "testing"})
                r.check(status == 202, "start sop=testing == 202（编码→测试交接）", f"status={status}")
                exclude = {code_run} if code_run else set()
                test_final, _, test_elapsed, tgrowth = wait_run(
                    server, "wc-lite", "t1", exclude, TEST_TIMEOUT, sse, timeline
                )
                if test_final is None:
                    r.issue("C21-TEST-TIMEOUT", "high", f"测试段 {TEST_TIMEOUT}s 未结束（增长 {tgrowth[-5:]}）")
                    r.check(False, "testing run 在窗口内结束")
                else:
                    r.check(test_final == "Settled", f"testing run Settled（实际 {test_final}，{test_elapsed:.0f}s）")
                    _, graph = server.req("GET", "/projects/wc-lite/tasks/t1/graph")
                    r.check(
                        graph.get("status") == "Completed",
                        f"任务终态 Completed（实际 {graph.get('status')}）",
                    )
            else:
                r.issue("C21-SKIP-TESTING", "high", "编码段未 Settled，testing 段跳过")
                r.check(False, "编码段收尾，具备交接条件")

            autopilot.stop()

        # ---- 测试段产物 ----
        tb = board / "testing"
        r.check((tb / "stages.md").exists(), "testing/stages.md 存在", f"{tb}")
        r.check((tb / "coverage-matrix.md").exists(), "coverage-matrix.md 存在", f"{tb}")
        reports = list((tb / "reports").glob("*.md")) if (tb / "reports").is_dir() else []
        r.check(len(reports) >= 1, "testing/reports 至少一份", f"{[p.name for p in reports]}")

        # ---- 工作区：代码 + 测试 + commit ----
        code_files = [p for p in ws.rglob("*.py") if ".git" not in p.parts and "tests" not in p.parts]
        r.check(len(code_files) >= 1, f"工作区有实现代码（{len(code_files)} 个 .py）", f"{[p.name for p in code_files][:5]}")
        tests_dir = ws / "tests"
        py_tests = list(tests_dir.rglob("test_*.py")) if tests_dir.is_dir() else []
        r.check(len(py_tests) >= 1, f"工作区有测试（{len(py_tests)} 个 test_*.py）", f"{[str(p.relative_to(ws)) for p in py_tests][:6]}")
        log = subprocess.run(["git", "-C", str(ws), "log", "--oneline"], capture_output=True, text=True)
        r.check(len([l for l in log.stdout.splitlines() if l.strip()]) >= 1, "工作区有 commit 留痕（D009）")

        # 黑盒纪律旁证
        names: set[str] = set()
        sd = server.home / ".devforge" / "sessions" / "testing"
        for jf in sd.glob("**/*.jsonl") if sd.is_dir() else []:
            try:
                for line in jf.read_text(errors="replace").splitlines():
                    for m in re.finditer(r'"name"\s*:\s*"([^"]+)"', line):
                        names.add(m.group(1))
            except OSError:
                pass
        banned = sorted(n for n in names if n.startswith("mcp__codegraph") or "lens" in n.lower())
        r.check(not banned, "测试会话未越界调用 codegraph/pi-lens（D007）", f"banned={banned}")

        out = Path(__file__).resolve().parent / "results" / "test_21_timeline.json"
        out.write_text(json.dumps(timeline, ensure_ascii=False, indent=2))

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
