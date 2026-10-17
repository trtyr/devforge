#!/usr/bin/env python3
"""test_07 · 串行守卫与重跑留档（真 pi 运行）。

覆盖：
1. Running 期间再 start（同任务/他任务）→ 409（系统级串行守卫，decisions/003 实现口径）
2. P8-ISSUE-01 修复面：cancel 端点收场（守卫验证后立即取消，不等完整规划——
   全链路对话与审查由 test_08 专责）
3. 重跑留档（Q004）：取消后再次 start → 新 run，旧 run 留档，runs 递增

运行前校准：
- PUT manifest 注入真实 api_key（异步渲染就绪后开跑）
- compensate_profile 交叉校验（渲染器已原生覆盖配置供给，此处验证产物一致性）
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, compensate_profile, real_api_key, wait_run_status  # noqa: E402

CANCEL_WAIT = 60.0  # cancel 端点轮询上限（control 注册前后 404→200）


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
        # 模型池与固定分配已在内置清单（用户拍板 2026-10-17）：审查→grok-4.7，规划→deepseek-flash
        t0 = time.time()
        try:
            # P7-ISSUE-03 已修：PUT 异步化（202 即回，后台渲染+安装，config.rendered SSE 通知）
            status, resp = server.req("PUT", "/config/manifest", manifest, timeout=600.0)
            elapsed = time.time() - t0
            r.check(status == 202, "manifest 替换受理（异步渲染）", f"status={status}, 耗时={elapsed:.1f}s")
            # 等 profile 就绪：两 profile 的 npm/package.json（安装器收尾产物）+ 3s 宽限
            prof = server.home / ".devforge" / "profiles"
            for _ in range(600):
                marks = [
                    (prof / a / "npm" / "package.json").exists()
                    for a in ("plan-agent", "plan-review")
                ]
                if all(marks):
                    break
                time.sleep(1.0)
            time.sleep(3.0)
        except TimeoutError:
            r.issue(
                "P7-ISSUE-03", "medium",
                "PUT manifest 受理超过 600s 未完成——异步化已修后仍超时属异常",
                "PUT timeout=600s 超时",
            )
            r.check(False, "manifest PUT 完成")
            return r.finish()

        # ---- 交叉校验：渲染器已原生覆盖配置供给（P2-ISSUE-01/03/04/05 修复面） ----
        compensated = compensate_profile(server)
        r.check(
            {"models.json", "auth.json"} <= set(compensated),
            "测试侧交叉校验执行（含 models.json/auth.json）",
            f"{compensated}",
        )
        # 原生分发断言：渲染产物应自带 models.json（新 api provider + 预算），不再依赖补偿
        prof = server.home / ".devforge" / "profiles"
        for agent in ("plan-agent", "plan-review"):
            mj = prof / agent / "models.json"
            ok = False
            detail = f"{mj} 不存在"
            if mj.exists():
                try:
                    cfg = json.loads(mj.read_text())
                    models = cfg.get("providers", {}).get("newapi", {}).get("models", [])
                    ok = any(
                        m.get("contextWindow") == 1_000_000 and m.get("maxTokens") == 65_536
                        for m in models
                    )
                    detail = f"models={[(m.get('id'), m.get('contextWindow'), m.get('maxTokens')) for m in models]}"
                except (OSError, json.JSONDecodeError) as e:
                    detail = f"解析失败: {e}"
            r.check(ok, f"{agent} 渲染原生分发 models.json（1M/64K 预算）", detail)
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

        # ---- P8-ISSUE-01 修复面：cancel 端点收场（守卫验证完即取消，不等完整规划） ----
        t0 = time.time()
        cancel_status = None
        while time.time() - t0 < 60:
            status, _ = server.req("POST", "/projects/p/tasks/t1/cancel", timeout=10.0)
            if status == 200:
                cancel_status = 200
                break
            time.sleep(1.0)
        r.check(cancel_status == 200, "cancel 端点受理 200", f"last={cancel_status}")

        final, runs_body = wait_run_status(server, "p", "t1", {"Failed"}, 90.0)
        elapsed = time.time() - t0
        if final is None:
            r.issue(
                "P7-ISSUE-01", "high",
                f"cancel 后 run 超过 90s 未进 Failed（取消链路失效）",
                f"runs={runs_body}",
            )
            r.check(False, "cancel 后 run 进 Failed", f"elapsed={elapsed:.0f}s")
            return r.finish()
        r.check(True, f"cancel 收场：run 进 Failed（{elapsed:.0f}s 内）")

        # 串行守卫释放：取消后他任务可启动（202）——立即再取消避免占用
        status, _ = server.req("POST", "/projects/p/tasks/t2/start")
        r.check(status == 202, "取消后他任务可启动（串行已释放）", f"status={status}")
        cancel2 = None
        t0 = time.time()
        while time.time() - t0 < 60:
            status, _ = server.req("POST", "/projects/p/tasks/t2/cancel", timeout=10.0)
            if status == 200:
                cancel2 = 200
                break
            time.sleep(1.0)
        r.check(cancel2 == 200, "t2 取消受理", f"last={cancel2}")
        wait_run_status(server, "p", "t2", {"Failed"}, 90.0)

        # ---- 重跑留档（Q004）：终态后可再跑，runs 递增且旧 run 留档 ----
        run1_id = runs_body["runs"][0]["id"]
        run1_status = runs_body["runs"][0]["status"]

        status, _ = server.req("POST", "/projects/p/tasks/t1/start")
        r.check(status == 202, "终态后重跑 start == 202", f"status={status}")
        t0 = time.time()
        while time.time() - t0 < 60:
            status, _ = server.req("POST", "/projects/p/tasks/t1/cancel", timeout=10.0)
            if status == 200:
                break
            time.sleep(1.0)
        final2, runs_body2 = wait_run_status(server, "p", "t1", {"Failed"}, 90.0)
        r.check(final2 == "Failed", "第二次 run 取消收场", f"{runs_body2}")
        runs = runs_body2["runs"]
        r.check(len(runs) == 2, "重跑后 runs 留档 2 条", f"{len(runs)}")
        r.check(
            runs[0]["id"] == run1_id and runs[0]["status"] == run1_status,
            "旧 run 状态留档不变", f"run1={run1_status}",
        )
        r.check(
            runs_body2.get("current_run") == runs[1]["id"],
            "current_run 指向最新 run", f"{runs_body2.get('current_run')}",
        )

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
