#!/usr/bin/env python3
"""devforge 端到端测试公共设施（纯标准库）。

- Server: 隔离 HOME 启动真服务器子进程 + 健康探针 + 关闭
- req(): REST 请求（urllib/http.client）
- SseStream: /api/v1/events 流式订阅（后台线程收集）
- Reporter: PASS/FAIL 检查记录 + 问题清单（发现问题只记录不修）+ 结果落盘
"""

from __future__ import annotations

import http.client
import json
import os
import socket
import subprocess
import tempfile
import threading
import time
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[2]
SERVER_BIN = PROJECT_ROOT / "target" / "debug" / "devforge"
RESULTS_DIR = Path(__file__).resolve().parent / "results"

# 环境性错误退出码（脚本自身跑崩，区别于功能 FAIL）
ENV_ERROR = 2


def free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Server:
    """隔离 HOME 的真服务器子进程。"""

    def __init__(self, port: int | None = None, home: str | None = None,
                 env_extra: dict[str, str] | None = None):
        self.port = port or free_port()
        # home 优先级：显式参数 > DEVFORGE_E2E_HOME 环境变量（外滩应答器与测试绑同一隔离 HOME）> 随机临时目录
        self.home = Path(home) if home else Path(
            os.environ.get("DEVFORGE_E2E_HOME") or tempfile.mkdtemp(prefix="devforge-e2e-")
        )
        self.proc: subprocess.Popen | None = None
        self.log_path = self.home / "server-stderr.log"
        # 服务器工作目录（围栏：隔离 spawn 子进程的落盘污染，同时用于观测 plan tree 实际落点）
        self.cwd = self.home / "server-cwd"
        # 额外环境变量（如 DEVFORGE_RUN_TIMEOUT_MS / DEVFORGE_TOKEN），合并进子进程环境
        self.env_extra = dict(env_extra or {})

    @property
    def base(self) -> str:
        return f"http://127.0.0.1:{self.port}/api/v1"

    def start(self, timeout: float = 20.0) -> "Server":
        if not SERVER_BIN.exists():
            raise RuntimeError(
                f"服务器二进制不存在: {SERVER_BIN}（先 cargo build，或跑 run_all.py）"
            )
        self.home.mkdir(parents=True, exist_ok=True)
        env = {
            **{k: v for k, v in os.environ.items() if k != "HOME"},
            "HOME": str(self.home),
            "DEVFORGE_ADDR": f"127.0.0.1:{self.port}",
            **self.env_extra,
        }
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        log = open(self.log_path, "wb")
        self.cwd.mkdir(parents=True, exist_ok=True)
        self.proc = subprocess.Popen(
            [str(SERVER_BIN)], env=env, stdout=subprocess.DEVNULL, stderr=log,
            cwd=str(self.cwd),
        )
        deadline = time.time() + timeout
        last_err: Exception | None = None
        while time.time() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(f"服务器提前退出（exit={self.proc.returncode}），见 {self.log_path}")
            try:
                status, _ = self.req("GET", "/projects")
                if status == 200:
                    return self
            except OSError as e:  # 连接未就绪
                last_err = e
            time.sleep(0.2)
        raise RuntimeError(f"服务器未在 {timeout}s 内就绪: {last_err}")

    def stop(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=5)
        if self.proc:
            self.log_path.with_suffix("").joinpath()  # no-op，保持路径语义
        print(f"[server] 已停止（HOME={self.home}）")

    def req(self, method: str, path: str, body: dict | None = None, timeout: float = 10.0,
            headers_extra: dict | None = None):
        """REST 请求。返回 (status, parsed_json_or_text)。"""
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
        payload = json.dumps(body).encode() if body is not None else None
        headers = {"Content-Type": "application/json"} if payload else {}
        if headers_extra:
            headers.update(headers_extra)
        conn.request(method, f"/api/v1{path}", payload, headers)
        resp = conn.getresponse()
        status = resp.status
        data = resp.read()
        conn.close()
        try:
            parsed = json.loads(data) if data else None
        except (json.JSONDecodeError, UnicodeDecodeError):
            parsed = data.decode(errors="replace")
        return status, parsed


class SseStream:
    """后台线程订阅 /api/v1/events，逐事件收集（data: 行的 JSON）。

    用法:
        with SseStream(server) as sse:
            ... 触发领域动作 ...
            ev = sse.wait_for(lambda e: e.get("type") == "run.started")
    """

    def __init__(self, server: Server, connect_timeout: float = 10.0):
        self.conn = http.client.HTTPConnection("127.0.0.1", server.port, timeout=connect_timeout)
        self.conn.request(
            "GET", "/api/v1/events", headers={"Accept": "text/event-stream"}
        )
        self.resp = self.conn.getresponse()
        if self.resp.status != 200:
            raise RuntimeError(f"SSE 订阅失败: {self.resp.status}")
        # 读超时与连接超时分离：服务器 keepalive 间隔 15s，读超时必须远大于它，
        # 否则静默段先触发 socket 超时把泵线程杀死（复测第 18/19 试 autopilot 耳聋真因，
        # 曾误诊为产品双通道断裂——实测 UiRelay.requests 即共享 events 通道）
        if self.conn.sock is not None:
            self.conn.sock.settimeout(120.0)
        self.events: list[dict] = []
        self._stop = threading.Event()
        self.thread = threading.Thread(target=self._pump, daemon=True)
        self.thread.start()

    def _pump(self) -> None:
        while not self._stop.is_set():
            try:
                while not self._stop.is_set():
                    line = self.resp.readline()
                    if not line:
                        return
                    text = line.decode(errors="replace").strip()
                    if not text.startswith("data: "):
                        continue  # SSE 注释/keep-alive 行
                    payload = text[len("data: "):]
                    try:
                        self.events.append(json.loads(payload))
                    except json.JSONDecodeError:
                        continue
            except TimeoutError:
                # 读超时（OSError 子类）：静默段超时——继续等，不退泵
                # （服务器 15s keepalive 会喂活连接；退泵 = SSE 永久耳聋）
                continue
            except (OSError, ValueError):
                return  # 连接关闭即退出

    def wait_for(self, predicate, timeout: float = 30.0) -> dict | None:
        """轮询已收集事件直到谓词命中或超时。返回命中事件或 None。"""
        deadline = time.time() + timeout
        seen = 0
        while time.time() < deadline:
            while seen < len(self.events):
                ev = self.events[seen]
                seen += 1
                if predicate(ev):
                    return ev
            time.sleep(0.1)
        return None

    def close(self) -> None:
        self._stop.set()
        try:
            self.conn.close()
        except OSError:
            pass

    def __enter__(self) -> "SseStream":
        return self

    def __exit__(self, *exc) -> None:
        self.close()


def real_api_key(provider: str = "newapi") -> str | None:
    """读真实 pi auth.json 的 key（仅本机，用于让 pi 真实调用模型；读不到返回 None）。"""
    real = Path.home() / ".pi" / "agent" / "auth.json"
    try:
        data = json.loads(real.read_text())
        return data[provider]["key"]
    except (OSError, KeyError, json.JSONDecodeError):
        return None


def compensate_profile(server: Server, provider: str = "newapi") -> list[str]:
    """测试侧交叉校验（原为补偿；修复阶段后渲染器已原生覆盖 1–4 项，
    此处保留执行以验证渲染产物与真实环境一致性，异常即暴露渲染回归）：
    1. models.json 从真实 pi 配置复制进 profile——显式 contextWindow 1M + maxTokens 65536
       （渲染器不分发 provider 定义，P2-ISSUE-03；缺失时 pi 用默认输出预算，
       reasoning 模型会烧光预算产出空正文——首轮卡死根因之一）
    2. auth.json 改为 provider 键控形状——渲染器形状缺陷（P2-ISSUE-01）
    3. settings.json 注入真实环境运行时键：retry（3 次×5s 退避，中转抖动的生存配置）、
       defaultThinkingLevel、compaction、hideThinkingBlock——渲染器只写 defaultProvider/
       defaultModel 两键（真实环境有、profile 缺 → 同一站点我方重试混过、对方一次即死）
    4. 插件配置原封复制（用户拍板 2026-10-17「原封不动抄过来」；装什么跟 Plan Tree 走，
       不擅自增减）：pi-blackhole-config.json / config/rpiv-web-tools.json / sol-pi.json
       （对应已装四件套中三个有配置载体的插件；未装插件的配置、本地 extensions、MCP 均不搬）
    返回实际执行的补偿清单。
    """
    import shutil
    done = []
    real_dir = Path.home() / ".pi" / "agent"
    real_models = real_dir / "models.json"
    real_auth = real_dir / "auth.json"
    real_settings = real_dir / "settings.json"
    for agent in ("plan-agent", "plan-review"):
        pd = server.home / ".devforge" / "profiles" / agent
        if not pd.is_dir():
            continue
        if real_models.exists():
            shutil.copy(real_models, pd / "models.json")
            if "models.json" not in done:
                done.append("models.json")
        try:
            key = json.loads(real_auth.read_text())[provider]["key"]
            (pd / "auth.json").write_text(
                json.dumps({provider: {"type": "api_key", "key": key}})
            )
            if "auth.json" not in done:
                done.append("auth.json")
        except (OSError, KeyError, json.JSONDecodeError):
            pass
        # 3) settings.json 运行时键注入（以渲染产物为基底，保留 defaultProvider/defaultModel/packages）
        try:
            real_s = json.loads(real_settings.read_text())
            sp = pd / "settings.json"
            cur = json.loads(sp.read_text()) if sp.exists() else {}
            for k in ("retry", "defaultThinkingLevel", "compaction", "hideThinkingBlock"):
                if k in real_s:
                    cur[k] = real_s[k]
            # 流挂死快判：idle 超时降为 60s（默认 300s 被中转慢滴喂活永不触发）——
            # 挂死流 60s 判错 → pi agent 级 retry（3 次）自动重试，替代人工杀跑重掷
            cur["httpIdleTimeoutMs"] = 60000
            sp.write_text(json.dumps(cur, indent=2, ensure_ascii=False))
            if "settings.json" not in done:
                done.append("settings.json")
        except (OSError, json.JSONDecodeError):
            pass
        # 4) 插件配置原封复制（用户拍板「原封不动抄过来」；范围=Plan Tree 记录的已装插件）
        verbatim = [
            (real_dir / "pi-blackhole" / "pi-blackhole-config.json",
             pd / "pi-blackhole" / "pi-blackhole-config.json"),
            (real_dir / "config" / "rpiv-web-tools.json", pd / "config" / "rpiv-web-tools.json"),
            (real_dir / "sol-pi.json", pd / "sol-pi.json"),
        ]
        for src, dst in verbatim:
            try:
                if src.exists():
                    dst.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copy(src, dst)
                    name = "plugins-config"
                    if name not in done:
                        done.append(name)
            except OSError:
                pass
        # 4b) blackhole 状态刷屏缓解（测试侧补偿，非修复；诊断实验 v3/v5 实证：
        #     blackhole 以 ~7.7 次/秒发 setStatus，devforge 桥每条阻塞回环 ~0.3s，
        #     管道背压+广播 Lagged 丢事件 → 桥永久阻塞 → run 卡死。关闭状态栏与
        #     worker 通知两条高频源，其余 29 键原封）
        try:
            bh = pd / "pi-blackhole" / "pi-blackhole-config.json"
            cfg = json.loads(bh.read_text())
            cfg["statusBar"] = False
            cfg["showWorkerNotifications"] = False
            bh.write_text(json.dumps(cfg, indent=2, ensure_ascii=False))
        except (OSError, json.JSONDecodeError):
            pass
        # 5) 真实 plan-tree skill 分发（测试侧补偿；渲染器只给占位 README——P2-ISSUE-05。
        #    诊断实锤：占位 skill 导致 agent 对全盘 `find /` 找规范，单次工具执行 10-30 分钟，
        #    窗口必爆（五次停滞的真凶）。真实 skill 源在用户环境的 craft/docs/plan-tree）
        try:
            real_skill = Path.home() / ".pi" / "agent" / "skills" / "craft" / "docs" / "plan-tree"
            dst_skill = pd / "skills" / "plan-tree"
            if real_skill.is_dir():
                if dst_skill.exists():
                    shutil.rmtree(dst_skill)
                shutil.copytree(real_skill, dst_skill)
                if "real-skill" not in done:
                    done.append("real-skill")
        except OSError:
            pass
    return done


class UiAutoPilot:
    """AI 替用户应答：SSE 收 ui.request → 自动 POST answer（用户委托 AI 代操作）。"""

    def __init__(self, server: Server, sse: SseStream, default_answer: str = "完成"):
        self.server = server
        self.sse = sse
        self.default_answer = default_answer
        self.answered: list[dict] = []
        self._seen: set[str] = set()
        self._stop = threading.Event()
        self.thread = threading.Thread(target=self._pump, daemon=True)

    def start(self) -> None:
        self.thread.start()

    def stop(self) -> None:
        self._stop.set()

    def _pump(self) -> None:
        while not self._stop.is_set():
            for ev in self.sse.events:
                if ev.get("type") == "ui.request" and ev.get("id") not in self._seen:
                    self._seen.add(ev["id"])
                    try:
                        status, _ = self.server.req(
                            "POST", f"/ui/{ev['id']}/answer", self._smart_answer(ev)
                        )
                        self.answered.append({"id": ev["id"], "status": status,
                                              "question": ev.get("question")})
                        print(f"[autopilot] 应答 {ev['id']}（{ev.get('question')}）→ {status}")
                    except OSError as e:
                        print(f"[autopilot] 应答失败: {e}")
            time.sleep(0.2)

    def _smart_answer(self, ev: dict) -> dict:
        """按请求形状应答（联调校准）：question 内嵌裸 JSON，解析 method/options/title；
        select → 选项标签（完成确认类挑完成项，否则首个）；其余 → 默认文本。"""
        payload = {}
        raw = ev.get("question") or ""
        if isinstance(raw, str) and "{" in raw:
            try:
                payload = json.loads(raw[raw.index("{"):])
            except json.JSONDecodeError:
                payload = {}
        method = str(payload.get("method") or "").lower()
        title = str(payload.get("title") or "")
        options = payload.get("options") or []
        labels = [
            (o.get("label") if isinstance(o, dict) else str(o)) for o in options
        ]
        pick = self.default_answer
        if method == "select" and labels:
            done_keys = ("完成", "是", "确认", "已", "通过")
            if any(k in title for k in done_keys):
                hit = [l for l in labels if any(k in l for k in done_keys)]
                pick = hit[0] if hit else labels[0]
            else:
                pick = labels[0]
        return {"value": pick}


def wait_run_status(server: Server, project: str, task: str, statuses: set[str], timeout: float = 180.0):
    """轮询任务 run 状态直到进入给定状态集合或超时。返回 (final_status, runs_body) 或 (None, last)。"""
    deadline = time.time() + timeout
    last = None
    while time.time() < deadline:
        status, body = server.req("GET", f"/projects/{project}/tasks/{task}/runs")
        if status == 200 and body.get("runs"):
            last = body
            final = body["runs"][0]["status"]
            if final in statuses:
                return final, body
        time.sleep(1.0)
    return None, last


class Reporter:
    """PASS/FAIL 检查与问题记录。脚本正常跑完返回 0（功能 FAIL 由报告说话）。"""

    def __init__(self, script: str):
        self.script = script
        self.checks: list[dict] = []
        self.issues: list[dict] = []
        RESULTS_DIR.mkdir(parents=True, exist_ok=True)

    def check(self, ok: bool, name: str, detail: str = "") -> bool:
        self.checks.append({"name": name, "ok": bool(ok), "detail": detail})
        mark = "PASS" if ok else "FAIL"
        line = f"[{mark}] {name}"
        if detail:
            line += f" — {detail}"
        print(line)
        return bool(ok)

    def issue(self, issue_id: str, severity: str, desc: str, evidence: str = "") -> None:
        """记录发现的问题（只记录，不修——用户约定）。"""
        self.issues.append(
            {"id": issue_id, "severity": severity, "desc": desc, "evidence": evidence}
        )
        print(f"[ISSUE] {issue_id} ({severity}): {desc}")

    def finish(self) -> int:
        out = {
            "script": self.script,
            "checks": self.checks,
            "issues": self.issues,
            "passed": sum(1 for c in self.checks if c["ok"]),
            "failed": sum(1 for c in self.checks if not c["ok"]),
        }
        (RESULTS_DIR / f"{self.script}.json").write_text(
            json.dumps(out, ensure_ascii=False, indent=2)
        )
        print(
            f"== {self.script}: {out['passed']} PASS / {out['failed']} FAIL, "
            f"{len(self.issues)} issues（结果已落 results/{self.script}.json）"
        )
        return 0
