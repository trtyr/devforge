#!/usr/bin/env python3
"""test_16 · 运行活性面：失活收割（DEVFORGE_RUN_IDLE_MS，opt-in）+ 默认不杀语义。

覆盖功能点（对照 006 decisions/002 超时机制重设计）：
- 失活收割：会话指纹平线超过闲置阈值 → watchdog 置 stalled 收割 → run Failed
  （确定性构造：外部 SIGKILL 自家 pi 进程，会话停写即平线——等价于进程真死）
- 默认不杀语义：未配置 IDLE_MS 时，即使 pi 被杀、会话平线，run 也不被自动收割
  （零硬限制哲学；恢复手段 = cancel 端点）
- 显式墙钟预算语义回归由 test_10/test_15 覆盖（本轮未动）
"""
import os
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, compensate_profile, real_api_key  # noqa: E402

IDLE_MS = 6000


def find_run_pi(run_id: str, timeout: float = 30.0) -> int | None:
    """扫 /proc 找 DEVFORGE_MANAGED == run_id 的 pi 进程（与产品 scan_and_kill_run 同口径）。"""
    deadline = time.time() + timeout
    while time.time() < deadline:
        for name in os.listdir("/proc"):
            if not name.isdigit():
                continue
            try:
                raw = Path(f"/proc/{name}/environ").read_bytes()
            except OSError:
                continue
            for kv in raw.split(b"\0"):
                if kv.startswith(b"DEVFORGE_MANAGED=") and kv.split(b"=", 1)[1].decode(
                    errors="replace"
                ) == run_id:
                    return int(name)
        time.sleep(0.3)
    return None


def stop_pid(pid: int) -> None:
    """SIGSTOP：进程存活（管道不 EOF）但零输出——确定性构造「活着但沉默」。
    SIGKILL 不行：管道立刻 EOF，RPC 层直接报错，失活路径轮不到（v1/v2 实测教训）。"""
    import signal
    os.kill(pid, signal.SIGSTOP)


def latest_run(server: Server, project: str, task: str) -> dict | None:
    _, body = server.req("GET", f"/projects/{project}/tasks/{task}/runs")
    runs = body.get("runs", []) if isinstance(body, dict) else []
    return runs[-1] if runs else None


def main() -> int:
    r = Reporter("test_16_run_liveness")

    # ---- 场景 A：IDLE_MS 小值 + 外部杀 pi → 会话平线 → 失活收割 ----
    server = Server(env_extra={"DEVFORGE_RUN_IDLE_MS": str(IDLE_MS)})
    try:
        server.start()
        ws = server.home / "workspace" / "demo-app"
        server.req("POST", "/projects", {"name": "demo-app", "local_path": str(ws)})
        server.req("POST", "/projects/demo-app/tasks", {"id": "live", "sop": "planning"})
        _, manifest = server.req("GET", "/config/manifest")
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        if (key := real_api_key("newapi")):
            manifest["api_key"] = key
        status, _ = server.req("PUT", "/config/manifest", manifest, timeout=180.0)
        r.check(status == 202, "PUT manifest == 202（剥包降级：活性面与插件无关）", f"status={status}")
        prof = server.home / ".devforge" / "profiles" / "plan-agent"
        deadline = time.time() + 60
        while time.time() < deadline and not (prof / "settings.json").exists():
            time.sleep(1)
        r.check((prof / "settings.json").exists(), "plan-agent profile 就绪", f"{prof}")
        # 模型链路补偿：无 models.json 时首调秒挂（provider 端点未知），run 自灭——
        # 失活路径要求 run 先活着（会话在长）再平线
        compensate_profile(server)

        t0 = time.time()
        status, _ = server.req("POST", "/projects/demo-app/tasks/live/start")
        r.check(status == 202, "planning start == 202", f"status={status}")

        # 等首个 run 出现并拿到 run_id（runs API）
        run_id = None
        deadline = time.time() + 30
        while time.time() < deadline and not run_id:
            run = latest_run(server, "demo-app", "live")
            if run:
                run_id = run["id"]
            time.sleep(0.3)
        r.check(bool(run_id), "run 已创建", f"run_id={run_id}")

        # 等 pi 进程出现（spawn 完成标记），随后 SIGKILL 制造确定性平线
        pid = find_run_pi(run_id) if run_id else None
        r.check(bool(pid), "真 pi 进程在跑（可被外部观测）", f"pid={pid}")
        # 先确认会话已在增长（活着），再杀——排除 run 自灭的假阳性
        session_dir = server.home / ".devforge" / "sessions" / "plan"
        deadline = time.time() + 30
        alive = False
        while time.time() < deadline:
            files = list(session_dir.glob("*.jsonl")) if session_dir.is_dir() else []
            if files and max(f.stat().st_size for f in files) > 0:
                alive = True
                break
            time.sleep(0.3)
        r.check(alive, "会话文件已在增长（run 先活着再被杀）", f"{session_dir}")
        if pid and alive:
            stop_pid(pid)
            # 平线窗口：IDLE_MS + watchdog 轮询余量；且不得早于 IDLE_MS 到点
            # （早于即说明是 EOF 快路径而非失活收割——时序断言防假阳性）
            stop_at = time.time()
            deadline = time.time() + (IDLE_MS / 1000.0) + 25
            final = None
            failed_at = None
            while time.time() < deadline:
                run = latest_run(server, "demo-app", "live")
                if run and run["status"] in ("Settled", "Failed"):
                    final = run["status"]
                    failed_at = time.time()
                    break
                time.sleep(0.5)
            since_stop = (failed_at - stop_at) if failed_at else -1
            r.check(
                final == "Failed" and since_stop >= (IDLE_MS / 1000.0) - 1.5,
                "会话平线超阈 → 失活收割 run Failed（D002；进程存活但沉默，时序符合闲置阈值）",
                f"final={final}, since_stop={since_stop:.1f}s, idle={IDLE_MS}ms",
            )
            _, task_body = server.req("GET", "/projects/demo-app/tasks")
            r.check(
                task_body["tasks"][0]["status"] == "Failed",
                "任务终态同步 Failed（失活路径）",
                f"status={task_body['tasks'][0]['status']}",
            )
    finally:
        server.stop()

    # ---- 场景 B：未配置 IDLE_MS → 杀 pi 平线也不自动收割（默认不杀），cancel 收场 ----
    server2 = Server()
    try:
        server2.start()
        ws2 = server2.home / "workspace" / "demo-app"
        server2.req("POST", "/projects", {"name": "demo-app", "local_path": str(ws2)})
        server2.req("POST", "/projects/demo-app/tasks", {"id": "live2", "sop": "planning"})
        _, manifest = server2.req("GET", "/config/manifest")
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        if (key := real_api_key("newapi")):
            manifest["api_key"] = key
        server2.req("PUT", "/config/manifest", manifest, timeout=180.0)
        prof = server2.home / ".devforge" / "profiles" / "plan-agent"
        deadline = time.time() + 60
        while time.time() < deadline and not (prof / "settings.json").exists():
            time.sleep(1)
        compensate_profile(server2)

        server2.req("POST", "/projects/demo-app/tasks/live2/start")
        run_id2 = None
        deadline = time.time() + 30
        while time.time() < deadline and not run_id2:
            run = latest_run(server2, "demo-app", "live2")
            if run:
                run_id2 = run["id"]
            time.sleep(0.3)
        pid2 = find_run_pi(run_id2) if run_id2 else None
        r.check(bool(pid2), "对照组：真 pi 在跑", f"pid={pid2}")
        if pid2:
            stop_pid(pid2)
            # 观察 IDLE_MS + 余量：无配置 → 不得自动收割（run 仍 Running）
            time.sleep((IDLE_MS / 1000.0) + 6)
            run = latest_run(server2, "demo-app", "live2")
            r.check(
                bool(run) and run["status"] == "Running",
                "未配置 IDLE_MS 时平线不自动收割（零硬限制默认）",
                f"status={run['status'] if run else None}",
            )
            # 恢复手段 = cancel 端点
            status, _ = server2.req("POST", "/projects/demo-app/tasks/live2/cancel")
            r.check(status == 200, "cancel 端点恢复手段可用 == 200", f"status={status}")
            deadline = time.time() + 20
            final2 = None
            while time.time() < deadline:
                run = latest_run(server2, "demo-app", "live2")
                if run and run["status"] in ("Settled", "Failed"):
                    final2 = run["status"]
                    break
                time.sleep(0.5)
            r.check(final2 == "Failed", "cancel 后 run 进 Failed（人工恢复路径）", f"final={final2}")
    finally:
        server2.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
