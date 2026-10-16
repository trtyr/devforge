#!/usr/bin/env python3
"""test_07 · 串行守卫与重跑留档（真 pi 运行）。

覆盖：
1. Running 期间再 start（同任务/他任务）→ 409（系统级串行守卫，decisions/003 实现口径）
2. 真 pi 规划 run 的完整生命周期（含 AI 替用户应答 ui.request）
3. 重跑留档（Q004）：完成后再次 start → 新 run，旧 run 留档，runs 递增

运行前补偿（测试侧，非代码修复，报告中标注）：
- PUT manifest 注入真实 api_key 并渲染 profile
- 以 pi 期望的形状（provider 键控）修正渲染出的 auth.json（渲染器形状缺陷的对照补偿）
"""

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key, wait_run_status  # noqa: E402

RUN_TIMEOUT = 240.0  # 真模型会话给足时间；超时本身记录为发现


def main() -> int:
    r = Reporter("test_07_rerun_serial")
    server = Server()
    try:
        server.start()

        # ---- 前置：PUT manifest 注入真实 key + 正确模型 id，并渲染 profile ----
        # 校准结论（P001 T005 联调校准点）：清单照录的显示名「DeepSeek flash」不可用，
        # 真实模型 id = mokeywork/deepseek-flash（newapi.trtyr.top 自建中转）
        key = real_api_key("newapi")
        if key is None:
            r.issue("P7-ISSUE-00", "high", "无法读取真实 auth.json 的 newapi key，模型调用必败",
                    "~/.pi/agent/auth.json 不可读")
        status, body = server.req("GET", "/config/manifest")
        manifest = body
        manifest["api_key"] = key
        manifest["model_pool"][0]["model_id"] = "mokeywork/deepseek-flash"
        t0 = time.time()
        try:
            status, resp = server.req("PUT", "/config/manifest", manifest, timeout=600.0)
            elapsed = time.time() - t0
            r.check(status == 200, "manifest 注入 key 并渲染 profile", f"status={status}, 耗时={elapsed:.0f}s")
            if elapsed > 30:
                r.issue(
                    "P7-ISSUE-03", "medium",
                    f"PUT manifest 同步执行包安装阻塞 {elapsed:.0f}s（四件套含 git clone），"
                    "客户端常规超时必炸——应异步化或返回任务句柄（发现于 test_07 首跑 10s 超时崩溃）",
                    f"PUT 耗时 {elapsed:.0f}s",
                )
        except TimeoutError:
            r.issue(
                "P7-ISSUE-03", "medium",
                "PUT manifest 同步装包超过 600s 未完成——同步阻塞设计在真实环境不可用",
                "PUT timeout=600s 超时",
            )
            r.check(False, "manifest PUT 完成")
            autopilot_never = None
            return r.finish()

        # ---- 补偿：models.json 分发 + auth.json 形状（渲染器缺陷的测试侧对照，非代码修复） ----
        compensated = compensate_profile(server)
        r.check(
            sorted(compensated) == ["auth.json", "models.json"],
            "测试侧补偿完成（models.json + auth.json）",
            f"{compensated}",
        )
        r.issue(
            "P2-ISSUE-03", "high",
            "渲染器不分发 models.json（provider 的 baseUrl/模型定义缺失）——"
            "pi 无法知道 newapi 的地址，模型调用必然失败。测试侧从真实 pi 配置复制补偿",
            "对比真实 ~/.pi/agent/models.json 与渲染产物（无此文件）",
        )
        r.issue(
            "P1-CAL-01", "info",
            "联调校准点结论：清单 model_id 照录显示名「DeepSeek flash」不可用，"
            "真实 id = mokeywork/deepseek-flash；provider baseUrl = https://newapi.trtyr.top/v1（自建中转，直连可达）",
            "curl chat/completions 验证 200",
        )

        # ---- 前置：项目 + 两个任务 ----
        server.req("POST", "/projects", {"name": "p", "local_path": "/tmp/p"})
        server.req("POST", "/projects/p/tasks", {"id": "t1", "sop": "planning"})
        server.req("POST", "/projects/p/tasks", {"id": "t2", "sop": "planning"})

        # ---- 串行守卫：t1 Running 期间，t1/t2 再 start 都 409 ----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="完成")
            autopilot.start()

            status, _ = server.req("POST", "/projects/p/tasks/t1/start")
            r.check(status == 202, "t1 启动 == 202", f"status={status}")

            # 轮询到 run 真正 Running（后台线程 begin），再试守卫
            started = False
            for _ in range(50):
                st, body = server.req("GET", "/projects/p/tasks/t1/runs")
                if st == 200 and body.get("runs"):
                    started = True
                    break
                time.sleep(0.2)
            r.check(started, "t1 run 已开始")

            status, body = server.req("POST", "/projects/p/tasks/t1/start")
            r.check(status == 409, "Running 中同任务再 start == 409", f"status={status}, body={body}")
            status, body = server.req("POST", "/projects/p/tasks/t2/start")
            r.check(status == 409, "Running 中他任务再 start == 409（系统级串行）", f"status={status}, body={body}")

            # ---- 等 t1 第一次 run 结束（真模型会话）----
            t0 = time.time()
            final, runs_body = wait_run_status(server, "p", "t1", {"Settled", "Failed"}, RUN_TIMEOUT)
            elapsed = time.time() - t0
            if final is None:
                r.issue(
                    "P7-ISSUE-01", "high",
                    f"run 超过 {RUN_TIMEOUT}s 未结束（可能 pi 卡死/模型不可用/无取消端点无法干预）",
                    f"runs={runs_body}",
                )
                r.check(False, "t1 第一次 run 在超时内结束", f"elapsed={elapsed:.0f}s")
                autopilot.stop()
                return r.finish()
            r.check(True, f"t1 第一次 run 结束（{final}，耗时 {elapsed:.0f}s）")
            if final == "Failed":
                r.issue(
                    "P7-ISSUE-02", "high",
                    "第一次规划 run 以 Failed 结束（模型/认证/提示词链路存在问题，详见服务端日志）",
                    f"runs={runs_body}",
                )
            autopilot.stop()

        # ---- 重跑留档（Q004）：Failed/Completed 后可再跑，runs 递增且旧 run 留档 ----
        run1_id = runs_body["runs"][0]["id"]
        run1_status = runs_body["runs"][0]["status"]

        status, _ = server.req("POST", "/projects/p/tasks/t1/start")
        r.check(status == 202, "结束后重跑 start == 202", f"status={status}")

        with SseStream(server) as sse2:
            autopilot2 = UiAutoPilot(server, sse2, default_answer="完成")
            autopilot2.start()
            final2, runs_body2 = wait_run_status(server, "p", "t1", {"Settled", "Failed"}, RUN_TIMEOUT)
            autopilot2.stop()

        r.check(final2 is not None, "第二次 run 在超时内结束")
        runs = runs_body2["runs"]
        r.check(len(runs) == 2, "重跑后 runs 留档 2 条", f"{len(runs)}")
        r.check(
            runs[0]["id"] == run1_id and runs[0]["status"] == run1_status,
            "旧 run 状态留档不变", f"run1={run1_status}",
        )
        r.check(
            runs_body2["current_run"] == runs[1]["id"],
            "current_run 指向最新 run", f"{runs_body2.get('current_run')}",
        )

    finally:
        server.stop()
    return r.finish()


def json_dumps(obj) -> str:
    import json
    return json.dumps(obj)


if __name__ == "__main__":
    sys.exit(main())
