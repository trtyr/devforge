#!/usr/bin/env python3
"""test_25 · 真实故障自动重启实证（plan 010 · T009 / M4）。

故障注入：本地 HTTP 代理（测试进程内守护线程），doc-review profile 的
baseUrl 指向它。代理规则（自整定，不猜 pi 内部重试次数）：
- 第一个 POST /chat/completions 必回 500；
- 之后只要与上一请求间隔 ≤120s 也回 500（审查重试风暴期）；
- 一旦出现 >120s 的静默间隔（= SOP 已耗尽 3 次换实例重试 → run 级重启 →
  doc-agent 重写期不产生代理流量），永久切换为透传真上游。

预期链路（D004 口径）：
单元审查 500 ×3（SOP 内换实例重试耗尽，抛 System）
→ run 级自动重启（WARN + `run.retry` 事件，attempt=2，退避 10s）
→ 第 2 次尝试：代理已透传 → 审查通过 → run Settled。

验收断言：
- SSE 出现 `run.retry` 且 attempt=2；黑板 state.json attempt==2；
- 最终 Settled、两单元文档齐全；
- `_meta/runs/<id>/report.md` 含「尝试: 2」；
- 代理日志：恰好 3 次 500 + ≥1 次透传 200。
"""

import json
import os
import shutil
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

DOC_TIMEOUT = float(os.environ.get("DOC_TIMEOUT", "7200"))
FAIL_N = int(os.environ.get("FAULT_FAIL_N", "3"))
UPSTREAM = "https://newapi.trtyr.top"

ALPHA_PY = '''"""alpha 包：求和。"""


def alpha_sum(xs: list[int]) -> int:
    """返回列表元素之和（空列表返回 0）。"""
    return sum(xs)
'''

BETA_PY = '''"""beta 包：计数。"""


def beta_count(xs: list[int]) -> int:
    """返回列表长度（不看元素内容）。"""
    return len(xs)
'''


class FaultProxy:
    """静默间隔规则版故障代理：500 直到 >GAP 秒静默，然后永久放行。"""

    GAP_S = 120.0

    def __init__(self, fail_n: int):
        self.count = 0
        self.lock = threading.Lock()
        self.log: list[str] = []
        self.seen_first = False
        self.forwarding = False
        self.last_ts = 0.0
        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):  # 静默默认 stderr 噪音
                pass

            def _forward(self, method: str):
                outer.lock.acquire()
                outer.count += 1
                n = outer.count
                now = time.time()
                decision = "forward"
                if not outer.forwarding:
                    if not outer.seen_first:
                        outer.seen_first = True
                        decision = "fail"
                    elif now - outer.last_ts > outer.GAP_S:
                        outer.forwarding = True
                        outer.log.append(f"#{n} 静默 {now - outer.last_ts:.0f}s > {outer.GAP_S:.0f}s → 放行")
                        decision = "forward"
                    else:
                        decision = "fail"
                outer.last_ts = now
                outer.lock.release()
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length) if length else None
                if method == "POST" and decision == "fail":
                    outer.log.append(f"#{n} {method} {self.path} -> 500 (injected)")
                    payload = json.dumps(
                        {"error": {"message": f"injected upstream 500 (fault #{n})",
                                   "type": "server_error"}}
                    ).encode()
                    self.send_response(500)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(payload)))
                    self.end_headers()
                    self.wfile.write(payload)
                    return
                # 透传真上游
                req = urllib.request.Request(
                    UPSTREAM + self.path,
                    data=body,
                    method=method,
                    headers={
                        k: v
                        for k, v in self.headers.items()
                        if k.lower() in ("authorization", "content-type", "accept")
                    },
                )
                try:
                    resp = urllib.request.urlopen(req, timeout=300)
                except urllib.error.HTTPError as e:
                    resp = e  # 上游 4xx/5xx 原样转发
                outer.log.append(f"#{n} {method} {self.path} -> {resp.status} (forwarded)")
                self.send_response(resp.status)
                ctype = resp.headers.get("Content-Type") or "application/json"
                self.send_header("Content-Type", ctype)
                self.send_header("Transfer-Encoding", "chunked")
                self.end_headers()
                while True:
                    chunk = resp.read(1024)
                    if not chunk:
                        break
                    self.wfile.write(f"{len(chunk):x}\r\n".encode())
                    self.wfile.write(chunk)
                    self.wfile.write(b"\r\n")
                    self.wfile.flush()
                self.wfile.write(b"0\r\n\r\n")
                self.wfile.flush()

            def do_POST(self):
                self._forward("POST")

            def do_GET(self):
                self._forward("GET")

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.httpd.daemon_threads = True
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)

    def start(self):
        self.thread.start()

    def stop(self):
        self.httpd.shutdown()

    @property
    def base(self) -> str:
        return f"http://127.0.0.1:{self.port}/v1"

    def fails(self) -> int:
        return sum(1 for l in self.log if "-> 500" in l)

    def forwards(self) -> int:
        return sum(1 for l in self.log if "-> 200" in l or "-> 4" in l or "-> 5" in l and "injected" not in l)


def patch_base_url(server: Server, profile: str, proxy_base: str) -> bool:
    """把某 profile 的 models.json 里指向真上游的 baseUrl 全部改指代理。"""
    p = server.home / ".devforge" / "profiles" / profile / "models.json"
    if not p.exists():
        return False

    def walk(o):
        if isinstance(o, dict):
            return {
                k: (proxy_base if k == "baseUrl" and isinstance(v, str) and "newapi.trtyr.top" in v else walk(v))
                for k, v in o.items()
            }
        if isinstance(o, list):
            return [walk(x) for x in o]
        return o

    data = json.loads(p.read_text(encoding="utf-8"))
    p.write_text(json.dumps(walk(data), ensure_ascii=False, indent=2), encoding="utf-8")
    return True


def load_state(server: Server, project: str, task: str) -> dict:
    p = (
        server.home
        / ".devforge"
        / "plans"
        / project
        / task
        / "doc-from-zero"
        / "state.json"
    )
    if not p.exists():
        return {}
    return json.loads(p.read_text(encoding="utf-8"))


def main() -> int:
    r = Reporter("test_25_fault_retry")
    proxy = FaultProxy(FAIL_N)
    proxy.start()
    r.check(True, f"故障代理就绪 :{proxy.port}（静默间隔规则，GAP={FaultProxy.GAP_S:.0f}s）")
    server = Server()
    try:
        host_manifest = Path.home() / ".devforge" / "manifest.json"
        if host_manifest.exists():
            dest = server.home / ".devforge" / "manifest.json"
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(host_manifest, dest)
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
                lambda e: e.get("type") == "config.rendered" and int(e.get("rendered") or 0) >= 9,
                timeout=300.0,
            )
            r.check(ev is not None, "profile 渲染完成", f"{ev}")
        compensate_profile(server)
        # 补偿之后再改 baseUrl（补偿会覆盖 profile 配置）
        ok = patch_base_url(server, "doc-review", proxy.base)
        r.check(ok, "doc-review baseUrl 已指向故障代理", f"{proxy.base}")

        repo = server.home / "workspace" / "fault-app"
        (repo / "alpha").mkdir(parents=True, exist_ok=True)
        (repo / "beta").mkdir(parents=True, exist_ok=True)
        (repo / "alpha" / "__init__.py").write_text("", encoding="utf-8")
        (repo / "beta" / "__init__.py").write_text("", encoding="utf-8")
        (repo / "alpha" / "a.py").write_text(ALPHA_PY, encoding="utf-8")
        (repo / "beta" / "b.py").write_text(BETA_PY, encoding="utf-8")

        project = "fault-app"
        status, _ = server.req("POST", "/projects", {"name": project, "local_path": str(repo)})
        r.check(status == 201, "项目创建", f"status={status}")
        status, _ = server.req(
            "POST", f"/projects/{project}/tasks", {"id": "d1", "sop": "doc-from-zero"}
        )
        r.check(status == 201, "任务创建", f"status={status}")

        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="继续")
            autopilot.start()
            status, _ = server.req("POST", f"/projects/{project}/tasks/d1/start", None)
            r.check(status == 202, "start == 202", f"status={status}")
            t0 = time.time()
            final = None
            while time.time() - t0 < DOC_TIMEOUT:
                _, body = server.req("GET", f"/projects/{project}/tasks/d1/runs")
                runs = body.get("runs", [])
                if runs and runs[-1]["status"] in ("Settled", "Failed"):
                    final = runs[-1]["status"]
                    break
                time.sleep(10.0)
            autopilot.stop()
            elapsed = time.time() - t0
            retries = [e for e in sse.events if e.get("type") == "run.retry"]

        r.check(final == "Settled", f"run 终态 Settled（{elapsed:.0f}s）", f"final={final}")
        if final != "Settled":
            r.issue("C25-NOT-SETTLED", "high", f"故障注入后未恢复：final={final}", "\n".join(proxy.log))
            return r.finish()

        r.check(bool(retries), f"观测到 run.retry 事件（{len(retries)} 次）", f"{retries}")
        r.check(
            any(e.get("attempt") == 2 for e in retries),
            "retry 事件 attempt 递增到 2",
            f"{retries}",
        )
        st = load_state(server, project, "d1")
        r.check(
            (st.get("attempt") or 0) == 2,
            "黑板 state.json attempt==2（重启落盘）",
            f"attempt={st.get('attempt')}",
        )

        docs = repo / "docs" / "contexts"
        body_docs = [
            p.relative_to(docs).as_posix()
            for p in docs.rglob("*.md")
            if "_meta" not in p.parts
        ] if docs.is_dir() else []
        r.check(len(body_docs) >= 6, f"两单元文档齐全（{len(body_docs)} 篇）", f"{sorted(body_docs)[:8]}")

        # 报告记录尝试次数（T009 验收点）
        runs_dir = docs / "_meta" / "runs"
        reports = sorted(runs_dir.glob("*/report.md")) if runs_dir.is_dir() else []
        r.check(bool(reports), "运行账单存在")
        if reports:
            text = reports[-1].read_text(encoding="utf-8")
            r.check(
                "- 尝试: 2" in text,
                "report.md 记录尝试次数（尝试: 2）",
                [l for l in text.splitlines() if "尝试" in l][:1].__str__(),
            )

        r.check(proxy.fails() >= 3, f"代理注入了故障（{proxy.fails()} 次 500）", "至少耗尽一轮实例重试")
        r.check(proxy.forwarding, "代理已切换放行（静默触发）", "重启后审查走通")
        r.check(proxy.forwards() > 0, f"代理透传真上游 {proxy.forwards()} 次", "重启后审查走通")
        print("== 代理日志（尾 12 条）==")
        for l in proxy.log[-12:]:
            print("  " + l)

        return r.finish()
    finally:
        server.stop()
        proxy.stop()


if __name__ == "__main__":
    sys.exit(main())
