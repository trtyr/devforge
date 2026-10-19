#!/usr/bin/env python3
"""run_all · 按序跑全部 e2e 脚本并生成 TEST_REPORT.md。

用法: python3 tests/e2e/run_all.py [--quick]   # --quick 跳过 07/08 长链路
先 cargo build 确保二进制新鲜。
"""

import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROJECT_ROOT = HERE.parents[1]
RESULTS = HERE / "results"

# (脚本, 超时秒, 说明)
SCRIPTS = [
    ("test_00_server.py", 120, "服务器生命周期与基础通道"),
    ("test_01_projects.py", 120, "项目管理 API"),
    ("test_02_tasks.py", 120, "任务管理 API"),
    ("test_03_config.py", 180, "配置管理与 profile 渲染"),
    ("test_04_auth.py", 120, "鉴权"),
    ("test_05_events_sse.py", 120, "SSE 事件流"),
    ("test_06_artifacts_graph.py", 120, "产物归集与运行图"),
    ("test_07_rerun_serial.py", 900, "串行守卫与 cancel 收场/重跑留档（真 pi + 真模型）"),
    ("test_08_e2e_planning.py", 2700, "端到端规划流水线（真 pi + 真模型；内部 RUN_TIMEOUT=2400s）"),
    ("test_09_run_control.py", 300, "运行控制面：cancel/spawn 工作目录/stderr 观测"),
    ("test_10_timeout_async_render.py", 300, "可选超时与异步渲染 SSE 通知"),
    ("test_11_coding_config.py", 600, "P005 编码配置面：四 profile 渲染/mcp.json/插件/四语言 skills"),
    ("test_12_sop_override.py", 300, "P005 跨 SOP 触发：start 可选 sop 覆盖/未知 400/无 body 沿用"),
    ("test_13_coding_preflight.py", 300, "P005 引擎预检：工作区无索引→init+黑板骨架"),
    ("test_14_coding_full_chain.py", 3600, "P005 编码全链（真 pi + 真模型；内部超时更宽）"),
    ("test_15_coding_run_control.py", 600, "P005 编码运行控制面：cancel→Failed/终态 404"),
    ("test_16_run_liveness.py", 600, "006 运行活性面：失活收割（IDLE_MS opt-in）/默认不杀/cancel 恢复"),
    ("test_17_testing_config.py", 300, "P007 测试配置面：四新 profile 供给(D007)/黑盒纪律锚点/mcp 有无矩阵/skills 矩阵"),
    ("test_18_sop_registry.py", 300, "P007 触发链：testing/doc-fix 注册与触发/未知 SOP 拒绝面"),
    ("test_19_testing_full_chain.py", 43200, "P007 全链黑盒闭环（真 pi + 真模型；规划→编码→testing 三段跨 SOP；编码窗口 6h/doc-fix 移交若触发则验证；最慢 10h 量级）"),
    ("test_20_testing_fixture.py", 7200, "P007 测试阶段本体（fixture 路线）：预置被测系统+文档，验 stages/矩阵/发现/tests 落仓/审查闭环"),
    ("test_21_coding_to_testing.py", 25200, "P007 编码→测试交接（预置小 plan tree + 真 coding + 真 testing；两段各 10800s 窗口）"),
]


def main() -> int:
    quick = "--quick" in sys.argv
    print("== cargo build ==")
    subprocess.run(["cargo", "build"], cwd=PROJECT_ROOT, check=True)

    runs = []
    for script, timeout, desc in SCRIPTS:
        if quick and script in (
            "test_07_rerun_serial.py",
            "test_08_e2e_planning.py",
            "test_14_coding_full_chain.py",
        ):
            runs.append({"script": script, "desc": desc, "skipped": True})
            print(f"[SKIP] {script} (--quick)")
            continue
        print(f"\n===== {script} · {desc} =====")
        t0 = time.time()
        try:
            proc = subprocess.run(
                [sys.executable, str(HERE / script)],
                cwd=str(HERE), capture_output=True, text=True, timeout=timeout,
            )
            tail = "\n".join(proc.stdout.strip().splitlines()[-6:])
            runs.append({
                "script": script, "desc": desc, "exit": proc.returncode,
                "elapsed_s": round(time.time() - t0, 1), "tail": tail,
            })
            print(tail)
        except subprocess.TimeoutExpired:
            runs.append({"script": script, "desc": desc, "exit": "TIMEOUT",
                         "elapsed_s": timeout})
            print(f"[TIMEOUT] {script} 超 {timeout}s")

    generate_report(runs)
    return 0


def generate_report(runs: list[dict]) -> None:
    # 汇总 results/*.json
    all_issues: dict[str, dict] = {}
    rows: list[str] = []
    total_pass = total_fail = 0
    for run in runs:
        script = run["script"]
        res_file = RESULTS / f"{script.replace('.py', '')}.json"
        if run.get("skipped"):
            rows.append(f"| {script} | {run['desc']} | SKIP | — | — |")
            continue
        if res_file.exists():
            data = json.loads(res_file.read_text())
            p, f = data.get("passed", 0), data.get("failed", 0)
            total_pass += p
            total_fail += f
            for iss in data.get("issues", []):
                iid = iss["id"]
                if iid not in all_issues:
                    all_issues[iid] = iss
            rows.append(f"| {script} | {run['desc']} | {p} PASS / {f} FAIL | {len(data.get('issues', []))} | {run.get('elapsed_s','?')}s |")
        else:
            rows.append(f"| {script} | {run['desc']} | ❌ 脚本异常 | — | {run.get('elapsed_s','?')}s |")

    sev_rank = {"high": 0, "medium": 1, "low": 2, "info": 3}
    issues = sorted(all_issues.values(), key=lambda i: sev_rank.get(i["severity"], 9))

    lines = [
        "# devforge 端到端测试报告",
        "",
        f"- 生成时间：{time.strftime('%Y-%m-%d %H:%M:%S')}",
        f"- 被测版本：`{git_rev()}`（target/debug，隔离 HOME 运行）",
        f"- 结果总览：**{total_pass} PASS / {total_fail} FAIL**，问题 {len(issues)} 条（去重后）",
        "",
        "## 逐脚本结果",
        "",
        "| 脚本 | 说明 | 单测结果 | 关联 issues | 耗时 |",
        "|------|------|----------|-------------|------|",
        *rows,
        "",
        "## 发现问题清单（只记录，不修）",
        "",
    ]
    if not issues:
        lines.append("（无）")
    for i in issues:
        lines.append(f"### {i['id']} · {i['severity']}")
        lines.append(f"- {i['desc']}")
        if i.get("evidence"):
            lines.append(f"- 证据：{i['evidence']}")
        lines.append("")

    report = RESULTS / "RUN_ALL_SUMMARY.md"
    report.write_text("\n".join(lines))
    print(f"\n== 汇总已生成: {report}")


def git_rev() -> str:
    try:
        return subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"], cwd=str(PROJECT_ROOT),
            capture_output=True, text=True,
        ).stdout.strip()
    except Exception:
        return "unknown"


if __name__ == "__main__":
    sys.exit(main())
