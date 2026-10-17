#!/usr/bin/env python3
"""test_08 · 端到端规划流水线（真 pi + 真模型 + AI 替用户应答）。

全链路：创建项目/任务 → start → spawn 真 pi → Plan Agent 对话（UI 中继：SSE 收
ui.request → POST answer，AI 替用户应答「完成」）→ 完成标记 → Review 循环 →
verdict PASS → 任务 Completed → plan tree 落盘取证。

取证对照（联调校准点，逐项记录）：
1. UI 应答格式（extension_ui_response）是否被 pi 接受（以 Plan Agent 是否继续推进为证）
2. 模型 id `DeepSeek flash` 是否可跑（以 run 是否进入真实对话为证）
3. plan tree 实际落点 vs decisions/002 约定（plans/<项目>/<任务>/ vs 服务器 cwd）
4. Review 循环轮次与 verdict（从事件时间线/最终状态观测）
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

RUN_TIMEOUT = 1200.0  # 审计补偿路径：换 grok-4.7（非 reasoning、秒回）+ 去 pi-blackhole（排除 setStatus 干扰）


def session_bytes(server: Server) -> int:
    """最新 plan 会话 JSONL 的字节数（增长探针）。"""
    sd = server.home / ".devforge" / "sessions" / "plan"
    files = list(sd.glob("*.jsonl")) if sd.is_dir() else []
    return max((f.stat().st_size for f in files), default=0)


def session_bytes(server: Server) -> int:
    """最新 plan 会话 JSONL 的字节数（增长探针）。"""
    sd = server.home / ".devforge" / "sessions" / "plan"
    files = list(sd.glob("*.jsonl")) if sd.is_dir() else []
    return max((f.stat().st_size for f in files), default=0)


def dump_session_tail(server: Server, out: Path, last_n: int = 12) -> None:
    """审计补偿诊断：dump 会话尾部 N 条（type/role/内容预览）到文件，定性 pi 卡在哪。"""
    sd = server.home / ".devforge" / "sessions" / "plan"
    files = sorted(sd.glob("*.jsonl")) if sd.is_dir() else []
    if not files:
        out.write_text("(无会话文件)")
        return
    lines = [l for l in files[-1].read_text(errors="replace").splitlines() if l.strip()]
    picks = lines[-last_n:]
    rows = []
    for l in picks:
        try:
            d = json.loads(l)
        except json.JSONDecodeError:
            continue
        m = d.get("message", {})
        role = m.get("role", "") if isinstance(m, dict) else ""
        content = m.get("content", "") if isinstance(m, dict) else ""
        if isinstance(content, list):
            content = " ".join(
                str(x.get("text", x.get("name", "")))[:200]
                for x in content if isinstance(x, dict)
            )
        rows.append(f"- {d.get('type','?')} {role}: {str(content)[:300]}")
    out.write_text(
        f"session={files[-1].name}\nentries={len(lines)}\n\n" + "\n".join(rows)
    )


def main() -> int:
    r = Reporter("test_08_e2e_planning")
    server = Server()
    timeline: list[dict] = []
    try:
        server.start()

        # ---- 前置：manifest 注入真实 key，渲染 profile（长超时，同步装包已知） ----
        # 模型池与固定分配已在内置清单（用户拍板 2026-10-17）：审查→grok-4.7，规划→deepseek-flash
        key = real_api_key("newapi")
        status, body = server.req("GET", "/config/manifest")
        manifest = body
        manifest["api_key"] = key
        t0 = time.time()
        try:
            status, _ = server.req("PUT", "/config/manifest", manifest, timeout=600.0)
            r.check(status == 200, f"manifest 就绪（耗时 {time.time()-t0:.0f}s）")
        except TimeoutError:
            r.issue("P8-ISSUE-00", "high", "PUT manifest 装包超 600s，全链路无法继续")
            return r.finish()

        # ---- 补偿：配置全量复刻（models.json/auth.json/settings 运行时键/插件配置原封） ----
        compensated = compensate_profile(server)
        r.check(
            set(["models.json", "auth.json"]).issubset(set(compensated)),
            "测试侧补偿完成（配置全量复刻）",
            f"{compensated}",
        )

        # ---- 建项目/任务（对应用户真实路径：建项目 → 立任务 → 跑规划） ----
        project_dir = server.home / "workspace" / "demo-app"
        status, _ = server.req(
            "POST", "/projects", {"name": "demo-app", "local_path": str(project_dir)},
        )
        r.check(status == 201, "项目 demo-app 创建", f"status={status}")
        status, _ = server.req(
            "POST", "/projects/demo-app/tasks",
            {"id": "planning-run", "sop": "planning"},
        )
        r.check(status == 201, "任务 planning-run 创建", f"status={status}")

        # ---- SSE + 自动应答 + 启动 ----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="完成")
            autopilot.start()

            status, _ = server.req("POST", "/projects/demo-app/tasks/planning-run/start")
            r.check(status == 202, "start == 202", f"status={status}")

            # ---- 等待结束（Completed/Failed），期间持续收集事件时间线 ----
            deadline = time.time() + RUN_TIMEOUT
            final = None
            growth: list[tuple[float, int]] = []  # (elapsed, session_bytes) 会话增长探针
            while time.time() < deadline:
                for ev in sse.events:
                    if ev.get("type") == "run.finished":
                        final = ev
                        break
                if final:
                    break
                timeline.extend(sse.events[len(timeline):])
                el = RUN_TIMEOUT - (deadline - time.time())
                growth.append((round(el), session_bytes(server)))
                time.sleep(15.0)
            timeline.extend(sse.events[len(timeline):])
            autopilot.stop()

            # 会话增长分析：持续增长=模型慢生成；长期静止=流挂起
            if growth:
                sizes = [g[1] for g in growth]
                r.check(
                    len(set(sizes)) > 2,
                    "会话文件持续增长（模型在慢生成而非死流）",
                    f"增长曲线: {sizes[:8]}{'...' if len(sizes) > 8 else ''}",
                )
                if len(set(sizes)) <= 2:
                    r.issue(
                        "P8-ISSUE-06", "high",
                        "pi 会话文件长期静止——模型请求流挂起（非慢生成），"
                        "且系统无 run 超时/取消机制，编排器只能无限等",
                        f"增长曲线: {sizes}",
                    )
            # 审计补偿诊断：会话尾部落盘，定性 pi 卡在哪
            dump_session_tail(
                server,
                Path(__file__).resolve().parent / "results" / "test_08_session_tail.txt",
            )

            if final is None:
                r.issue(
                    "P8-ISSUE-01", "high",
                    f"规划 run 超过 {RUN_TIMEOUT}s 未结束（事件时间线已保留 {len(timeline)} 条）",
                    "可能：模型不可用 / pi 卡死 / UI 应答格式未被接受导致 Plan Agent 停在等待",
                )
                r.check(False, "run 在超时内结束")
            else:
                r.check(
                    final.get("ok") is True,
                    "run.finished ok=True（规划 SOP 通过）",
                    f"{final}",
                )

        # ---- 事件时间线取证 ----
        types = [e.get("type") for e in timeline]
        r.check(
            "run.started" in types, "时间线含 run.started", f"{types[:6]}...",
        )
        r.check(
            "ui.request" in types,
            "时间线含 ui.request（Plan Agent 发起完成确认）",
            f"ui 事件数={types.count('ui.request')}",
        )
        ui_count = types.count("ui.request")
        r.check(
            autopilot.answered and len(autopilot.answered) == ui_count,
            "AI 应答全部送达（数量对齐）",
            f"answered={len(autopilot.answered)}, requested={ui_count}",
        )
        if ui_count == 0:
            r.issue(
                "P8-ISSUE-02", "high",
                "全程未收到 ui.request——Plan Agent 未发起完成确认（提示词/ask-user-question 包未生效），"
                "完成确认机制未真正运转",
                f"types={types}",
            )
        r.check(
            "run.finished" in types, "时间线含 run.finished", f"{types[-4:]}",
        )

        # ---- 任务终态 ----
        status, runs_body = server.req("GET", "/projects/demo-app/tasks/planning-run/runs")
        r.check(status == 200, "任务 runs 可查", f"status={status}")
        if runs_body and runs_body.get("runs"):
            final_status = runs_body["runs"][0]["status"]
            r.check(
                final_status == "Settled",
                f"run 终态 Settled（实际 {final_status}）",
            )
            task_status = runs_body  # current_run 已在
        status, graph = server.req("GET", "/projects/demo-app/tasks/planning-run/graph")
        if status == 200:
            r.check(
                graph.get("status") in ("Completed", "Failed"),
                f"任务终态 {graph.get('status')}（期望 Completed；Failed 记链路问题）",
            )
            if graph.get("status") == "Failed":
                r.issue(
                    "P8-ISSUE-03", "high",
                    "规划 run 以 Failed 终态结束——模型调用/认证/提示词链路存在真实故障，"
                    "详见服务器日志与时间线",
                    f"graph={graph.get('status')}",
                )

        # ---- plan tree 落点取证（decisions/002 vs 实际） ----
        expected = server.home / ".devforge" / "plans" / "demo-app" / "planning-run"
        expected_files = list(expected.glob("**/*")) if expected.is_dir() else []
        r.check(
            len(expected_files) > 0,
            f"plan tree 落在约定路径（decisions/002）：{expected}",
            f"文件数={len(expected_files)}",
        )
        if not expected_files:
            # 观察实际落点：服务器 cwd
            cwd_files = [
                p for p in server.cwd.glob("**/*")
                if p.is_file() and p.suffix in (".md", ".json") and "server-stderr" not in p.name
            ]
            if cwd_files:
                sample = [str(p.relative_to(server.cwd)) for p in cwd_files[:8]]
                r.issue(
                    "P8-ISSUE-04", "high",
                    "plan tree 落在服务器进程 cwd 而非 plans/<项目>/<任务>/——"
                    "decisions/002 被违反（SpawnSpec 未设置子进程工作目录）",
                    f"server-cwd 下发现: {sample}",
                )
            else:
                r.issue(
                    "P8-ISSUE-05", "high",
                    "未找到任何 plan tree 产物（约定路径与服务器 cwd 均无）——"
                    "Plan Agent 可能未产出或产出在未知位置",
                    f"已搜索 {expected} 与 {server.cwd}",
                )

        # ---- 时间线落盘（报告生成器消费） ----
        out = Path(__file__).resolve().parent / "results" / "test_08_timeline.json"
        out.write_text(json.dumps(timeline, ensure_ascii=False, indent=2))

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
