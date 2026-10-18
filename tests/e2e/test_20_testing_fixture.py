#!/usr/bin/env python3
"""test_20 · P007 测试阶段本身的黑盒验证（fixture 路线，不依赖编码段）。

为什么需要它：test_19 的三段全链受编码段耗时拖累（真模型慢时 >3h 未收尾），
testing 段永远轮不到。本脚本用「预置被测系统 + 预置文档」直接验证 testing SOP：
Test Agent 是否真的读文档、写集成/e2e 测试、自起自跑、落追溯矩阵、过 Test Review。

fixture：
- workspace：`calc.py`（极简 CLI，含错误码契约）+ 已 git 化
- 黑板 coding/docs/：完整规格 + Runbook（环境/运行方式/接口契约/错误码）
覆盖功能点（对照 P007 D001/D005/D006/D009/D010）：
- start {"sop":"testing"} → 202；run 在窗口内结束
- 黑板 testing/：stages.md / todo.md / coverage-matrix.md / reports/review-*.md
- 工作区 tests/：Python 测试文件（D010 落仓库）+ git commit 留痕（D009）
- 测试会话 toolCall 取证：核心工具真实调用；未越界调用 codegraph/pi-lens（D007）
- 交互全代理（ask-user-question 由 AI 应答）
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
    "plan-agent",
    "plan-review",
    "coding-agent",
    "coding-review",
    "test-agent",
    "test-review",
    "doc-fix",
    "doc-review",
)
TEST_TIMEOUT = 5400.0  # 90 分钟：fixture 体量小，只验机制

CALC_PY = '''#!/usr/bin/env python3
"""极简计算器 CLI（fixture 被测系统）。

契约见 coding/docs/01-calc-spec.md：
- 子命令：add / sub / mul / div
- 正常：打印整数结果，退出码 0
- 参数错误（命令未知/缺参数/非整数）：stderr 打印 "error: invalid arguments"，退出码 2
- 除零：stderr 打印 "error: division by zero"，退出码 3
"""
import sys


def compute(op: str, a: int, b: int) -> int:
    if op == "add":
        return a + b
    if op == "sub":
        return a - b
    if op == "mul":
        return a * b
    if op == "div":
        if b == 0:
            raise ZeroDivisionError
        return int(a / b) if False else a // b
    raise ValueError(op)


def main(argv):
    if len(argv) != 4:
        print("error: invalid arguments", file=sys.stderr)
        return 2
    op, a_s, b_s = argv[1], argv[2], argv[3]
    if op not in ("add", "sub", "mul", "div"):
        print("error: invalid arguments", file=sys.stderr)
        return 2
    try:
        a, b = int(a_s), int(b_s)
    except ValueError:
        print("error: invalid arguments", file=sys.stderr)
        return 2
    try:
        print(compute(op, a, b))
    except ZeroDivisionError:
        print("error: division by zero", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
'''

SPEC_DOC = """# 01 · calc 计算器 CLI 规格与 Runbook

## 1. 环境要求

- Python 3.8+（只用标准库，无第三方依赖）
- 工作目录：仓库根

## 2. 运行方式（Runbook）

```bash
python3 calc.py add 2 3     # 期望输出 5，退出码 0
python3 calc.py div 7 2     # 期望输出 3（整除），退出码 0
```

## 3. 接口契约（命令行）

| 子命令 | 参数 | 正常输出（stdout） | 退出码 |
|--------|------|--------------------|--------|
| add | 两个整数 | 两数之和 | 0 |
| sub | 两个整数 | 两数之差 | 0 |
| mul | 两个整数 | 两数之积 | 0 |
| div | 两个整数 | 整除结果（向下取整） | 0 |

## 4. 错误路径

| 场景 | stderr 内容 | 退出码 |
|------|-------------|--------|
| 参数个数不对（缺/多） | `error: invalid arguments` | 2 |
| 未知子命令 | `error: invalid arguments` | 2 |
| 参数不是整数 | `error: invalid arguments` | 2 |
| 除数为 0 | `error: division by zero` | 3 |

## 5. 边界与语义

- 支持负数参数：`python3 calc.py add -3 5` → `2`，退出码 0
- 整除语义：`div` 向下取整（Python `//`），不支持小数结果
- stdout 只输出结果一行；错误信息只走 stderr

## 6. 测试运行方式

```bash
python3 -m unittest discover -s tests -v
```

（测试仅用标准库 unittest，避免外部依赖。）
"""


def newest_session_bytes(server: Server, kind: str) -> int:
    sd = server.home / ".devforge" / "sessions" / kind
    files = list(sd.glob("**/*.jsonl")) if sd.is_dir() else []
    return max((f.stat().st_size for f in files), default=0)


def main() -> int:
    r = Reporter("test_20_testing_fixture")
    server = Server()
    timeline: list[dict] = []
    try:
        server.start()
        # 剥包快渲染（本脚本验的是测试阶段机制，不是装包）
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
        prof = server.home / ".devforge" / "profiles"
        compensated = compensate_profile(server)
        r.check(
            set(["models.json", "auth.json"]).issubset(set(compensated)),
            "测试侧补偿（配置全量复刻）", f"{compensated}",
        )

        # ---- fixture：被测系统 + 文档 ----
        project_dir = server.home / "workspace" / "calc-app"
        project_dir.mkdir(parents=True, exist_ok=True)
        (project_dir / "calc.py").write_text(CALC_PY, encoding="utf-8")
        subprocess.run(["git", "init", "-q"], cwd=project_dir, check=False)
        subprocess.run(["git", "add", "-A"], cwd=project_dir, check=False)
        subprocess.run(
            ["git", "-c", "user.email=e2e@devforge", "-c", "user.name=e2e", "commit", "-qm", "fixture: calc CLI"],
            cwd=project_dir,
            check=False,
        )
        status, _ = server.req(
            "POST", "/projects", {"name": "calc-app", "local_path": str(project_dir)},
        )
        r.check(status == 201, "项目 calc-app 创建（local_path=fixture 工作区）", f"status={status}")
        status, body = server.req(
            "POST", "/projects/calc-app/tasks", {"id": "t1", "sop": "planning"},
        )
        r.check(status == 201, "任务 t1 创建", f"status={status}, body={body}")

        # 黑板文档：测试 Agent 的唯一输入（不看代码，只看文档）
        blackboard = server.home / ".devforge" / "plans" / "calc-app" / "t1"
        docs = blackboard / "coding" / "docs"
        docs.mkdir(parents=True, exist_ok=True)
        (docs / "01-calc-spec.md").write_text(SPEC_DOC, encoding="utf-8")
        r.check((docs / "01-calc-spec.md").exists(), "预置文档就位（coding/docs/01-calc-spec.md）")

        # ---- 直接触发 testing SOP（D012） ----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="完成")
            autopilot.start()
            status, _ = server.req(
                "POST", "/projects/calc-app/tasks/t1/start", {"sop": "testing"},
            )
            r.check(status == 202, "start sop=testing == 202（testing 阶段点火）", f"status={status}")

            t0 = time.time()
            growth: list[tuple[int, int]] = []
            final = None
            while time.time() - t0 < TEST_TIMEOUT:
                _, runs_body = server.req("GET", "/projects/calc-app/tasks/t1/runs")
                runs = runs_body.get("runs", [])
                if runs and runs[-1]["status"] in ("Settled", "Failed"):
                    final = runs[-1]["status"]
                    break
                timeline.extend(sse.events[len(timeline):])
                growth.append((round(time.time() - t0), newest_session_bytes(server, "testing")))
                time.sleep(10.0)
            timeline.extend(sse.events[len(timeline):])
            autopilot.stop()
            elapsed = time.time() - t0

            if final is None:
                r.issue(
                    "C20-TEST-TIMEOUT", "high",
                    f"testing run {TEST_TIMEOUT}s 未结束（增长曲线 {growth[-6:]}）",
                )
                r.check(False, "testing run 在窗口内结束")
            else:
                r.check(
                    final == "Settled",
                    f"testing run 终态 Settled（实际 {final}，耗时 {elapsed:.0f}s）",
                )
                _, graph = server.req("GET", "/projects/calc-app/tasks/t1/graph")
                r.check(
                    graph.get("status") == "Completed",
                    f"任务终态 Completed（实际 {graph.get('status')}）",
                )

        # ---- 黑板产物 ----
        tb = blackboard / "testing"
        r.check((tb / "stages.md").exists(), "testing/stages.md 存在（阶段自划分）", f"{tb}")
        r.check((tb / "todo.md").exists(), "testing/todo.md 存在", f"{tb}")
        r.check((tb / "coverage-matrix.md").exists(), "coverage-matrix.md 追溯矩阵存在（Q007）", f"{tb}")
        reports = list((tb / "reports").glob("*.md")) if (tb / "reports").is_dir() else []
        r.check(len(reports) >= 1, "testing/reports/ 审查报告至少一份", f"{[p.name for p in reports]}")
        findings = list((tb / "findings").glob("*.md")) if (tb / "findings").is_dir() else []
        r.check(True, f"findings 条数={len(findings)}（0 合法：实现符合文档）", "")

        # ---- 工作区：测试落仓库（D010）+ 提交留痕（D009） ----
        tests_dir = project_dir / "tests"
        py_tests = list(tests_dir.rglob("test_*.py")) if tests_dir.is_dir() else []
        r.check(
            len(py_tests) >= 1,
            "工作区 tests/ 有 Python 测试文件（D010 黑盒套件进仓库）",
            f"{[str(p.relative_to(project_dir)) for p in py_tests]}",
        )
        layers = {d.name for d in tests_dir.iterdir() if d.is_dir()} if tests_dir.is_dir() else set()
        r.check(True, f"tests/ 分层目录={sorted(layers) or '（未分层，观察项）'}", "")
        log = subprocess.run(
            ["git", "-C", str(project_dir), "log", "--oneline"],
            capture_output=True, text=True,
        )
        commits = [l for l in log.stdout.splitlines() if l.strip()]
        r.check(
            len(commits) >= 2,
            f"工作区新增测试提交（D009；含 fixture 基线 ≥2 条）",
            f"commits={commits[:6]}",
        )

        # ---- 测试会话工具取证（D007：黑盒纪律 + 供给边界） ----
        tool_names: set[str] = set()
        sd = server.home / ".devforge" / "sessions" / "testing"
        for jf in sd.glob("**/*.jsonl") if sd.is_dir() else []:
            try:
                for line in jf.read_text(errors="replace").splitlines():
                    for m in re.finditer(r'"name"\s*:\s*"([^"]+)"', line):
                        tool_names.add(m.group(1))
            except OSError:
                pass
        r.check(bool(tool_names), f"测试会话工具取证: {sorted(tool_names)[:15]}")
        r.check(
            any(n in tool_names for n in ("read", "write", "edit", "bash")),
            "测试 Agent 核心工具真实调用", f"{sorted(tool_names)[:10]}",
        )
        banned = sorted(n for n in tool_names if n.startswith("mcp__codegraph") or "lens" in n.lower())
        r.check(not banned, "未越界调用 codegraph/pi-lens（D007 供给未配）", f"banned={banned}")

        types = [e.get("type") for e in timeline]
        r.check(
            not autopilot.answered or len(autopilot.answered) >= types.count("ui.request"),
            "AI 应答与 ui.request 对齐（全代理）",
            f"answered={len(autopilot.answered)}, requested={types.count('ui.request')}",
        )

        out = Path(__file__).resolve().parent / "results" / "test_20_timeline.json"
        out.write_text(json.dumps(timeline, ensure_ascii=False, indent=2))

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
