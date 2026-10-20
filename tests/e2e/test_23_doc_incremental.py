#!/usr/bin/env python3
"""test_23 · doc-from-zero 增量模式端到端（P008 · T025 / D021）。

同一 fixture 仓跑两轮，验证：
- 第 1 轮：**没有 base_sha** → 自动降级全量（开关开着也不乱来），成功后基线推进到 HEAD；
- 第 2 轮：只改 alpha 包并提交 → **只重派 alpha 单元**，beta 单元产物一个字节不动；
- 召回：alpha 的新函数名出现在重写后的文档里；
- 成本：第 2 轮耗时与 token 明显低于第 1 轮（同仓同模型，差异来自少跑一个单元）。

开关（`DEVFORGE_DOC_INCREMENTAL=1`）由服务器进程环境带入；两轮共用同一台服务器。
"""

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream, UiAutoPilot, compensate_profile, real_api_key  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
DOC_TIMEOUT = float(os.environ.get("DOC_TIMEOUT", "7200"))

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

ALPHA_EXTRA = '''

def alpha_extra(xs: list[int]) -> list[int]:
    """把每个元素乘二并返回新列表。"""
    return [x * 2 for x in xs]
'''


def git(repo: Path, *args: str) -> str:
    out = subprocess.run(
        ["git", "-c", "user.name=e2e", "-c", "user.email=e2e@localhost", *args],
        cwd=repo,
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} 失败：{out.stderr}")
    return out.stdout.strip()


def state_path(server: Server, project: str, task: str) -> Path:
    return (
        server.home
        / ".devforge"
        / "plans"
        / project
        / task
        / "doc-from-zero"
        / "state.json"
    )


def load_state(server: Server, project: str, task: str) -> dict:
    p = state_path(server, project, task)
    if not p.exists():
        return {}
    return json.loads(p.read_text(encoding="utf-8"))


def run_once(server: Server, project: str, task: str, timeout: float) -> tuple[str | None, float]:
    """触发一次 start 并等到 Settled/Failed，返回 (终态, 墙钟秒)。"""
    with SseStream(server) as sse:
        autopilot = UiAutoPilot(server, sse, default_answer="继续")
        autopilot.start()
        status, _ = server.req("POST", f"/projects/{project}/tasks/{task}/start", None)
        if status != 202:
            autopilot.stop()
            return f"start-status={status}", 0.0
        t0 = time.time()
        final = None
        while time.time() - t0 < timeout:
            _, body = server.req("GET", f"/projects/{project}/tasks/{task}/runs")
            runs = body.get("runs", [])
            if runs and runs[-1]["status"] in ("Settled", "Failed"):
                final = runs[-1]["status"]
                break
            time.sleep(10.0)
        autopilot.stop()
        return final, time.time() - t0


def newest_report(docs: Path) -> tuple[Path | None, str]:
    runs = docs / "_meta" / "runs"
    dirs = [p for p in runs.iterdir() if p.is_dir()] if runs.is_dir() else []
    if not dirs:
        return None, ""
    latest = max(dirs, key=lambda p: p.stat().st_mtime)
    report = latest / "report.md"
    return report, report.read_text(encoding="utf-8") if report.exists() else ""


def parse_tokens(text: str) -> int:
    for line in text.splitlines():
        if "用量" in line and "tokens" in line:
            digits = "".join(c for c in line.split("tokens")[0] if c.isdigit())
            if digits:
                return int(digits)
    return 0


def snapshot_files(repo: Path, state: dict) -> dict[str, tuple[int, int]]:
    """unit → {相对路径: (mtime_ns, size)}（产物目录相对仓库根）。"""
    files = (state.get("data") or {}).get("files") or {}
    snap: dict[str, tuple[int, int]] = {}
    for unit, rels in files.items():
        for rel in rels or []:
            p = repo / "docs" / "contexts" / rel
            if p.exists():
                st = p.stat()
                snap[f"{unit}:{rel}"] = (st.st_mtime_ns, st.st_size)
    return snap


def main() -> int:
    r = Reporter("test_23_doc_incremental")
    # 增量开关在服务器进程环境里读；第一轮没有 base_sha，会自己降级全量
    server = Server(env_extra={"DEVFORGE_DOC_INCREMENTAL": "1"})
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
                lambda e: e.get("type") == "config.rendered" and e.get("rendered") == 15,
                timeout=300.0,
            )
            r.check(ev is not None, "15 profile 渲染完成", f"{ev}")
        compensate_profile(server)

        # ---- fixture：两个独立包（两个单元）+ git 历史（增量需要基线）----
        repo = server.home / "workspace" / "incr-app"
        (repo / "alpha").mkdir(parents=True, exist_ok=True)
        (repo / "beta").mkdir(parents=True, exist_ok=True)
        (repo / "alpha" / "__init__.py").write_text("", encoding="utf-8")
        (repo / "beta" / "__init__.py").write_text("", encoding="utf-8")
        (repo / "alpha" / "a.py").write_text(ALPHA_PY, encoding="utf-8")
        (repo / "beta" / "b.py").write_text(BETA_PY, encoding="utf-8")
        git(repo, "init", "-q")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", "baseline")
        head1 = git(repo, "rev-parse", "HEAD")
        r.check(bool(head1), "fixture 有 git 基线", head1[:12])

        project = "incr-app"
        status, _ = server.req("POST", "/projects", {"name": project, "local_path": str(repo)})
        r.check(status == 201, "项目创建", f"status={status}")
        status, body = server.req(
            "POST", f"/projects/{project}/tasks", {"id": "d1", "sop": "doc-from-zero"}
        )
        r.check(status == 201, "任务创建", f"status={status}, body={body}")

        # ---- 第 1 轮：降级全量 ----
        final1, t1 = run_once(server, project, "d1", DOC_TIMEOUT)
        r.check(final1 == "Settled", f"第 1 轮 Settled（{t1:.0f}s）", f"final={final1}")
        if final1 != "Settled":
            return r.finish()

        docs = repo / "docs" / "contexts"
        st1 = load_state(server, project, "d1")
        data1 = st1.get("data") or {}
        done1 = data1.get("done", [])
        r.check(len(done1) >= 2, f"第 1 轮两个单元都完成（{done1}）")
        r.check(
            data1.get("base_sha") == head1,
            "成功后基线推进到 HEAD（D021）",
            f"{data1.get('base_sha')} vs {head1}",
        )
        report1, text1 = newest_report(docs)
        tokens1 = parse_tokens(text1)
        r.check(tokens1 > 0, "第 1 轮账单有 token 用量", f"{tokens1}")
        snap1 = snapshot_files(repo, st1)
        r.check(len(snap1) >= 2, f"第 1 轮登记了各单元产物（{len(snap1)} 个文件）", f"{sorted(snap1)[:6]}")

        # ---- 改 alpha 并提交 ----
        (repo / "alpha" / "a.py").write_text(ALPHA_PY + ALPHA_EXTRA, encoding="utf-8")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", "alpha: add alpha_extra")
        head2 = git(repo, "rev-parse", "HEAD")
        r.check(head2 != head1, "第二次提交产生新 HEAD", head2[:12])

        # ---- 第 2 轮：增量 ----
        final2, t2 = run_once(server, project, "d1", DOC_TIMEOUT)
        r.check(final2 == "Settled", f"第 2 轮 Settled（{t2:.0f}s）", f"final={final2}")
        if final2 != "Settled":
            return r.finish()

        st2 = load_state(server, project, "d1")
        data2 = st2.get("data") or {}
        inc = data2.get("incremental") or {}
        units2 = inc.get("units") or []
        r.check(bool(units2), "状态记录了本轮增量受影响集合", f"{units2}")

        alpha_unit = next((u for u in done1 if "alpha" in u), None)
        beta_unit = next((u for u in done1 if "beta" in u), None)
        r.check(alpha_unit is not None and beta_unit is not None, "单元按包划分", f"{done1}")
        if alpha_unit and beta_unit:
            r.check(alpha_unit in units2, "受影响单元含 alpha", f"{units2}")
            r.check(beta_unit not in units2, "未受影响的 beta 不重派", f"{units2}")

        # 未被重写：beta 产物 mtime/size 一个不变；alpha 的必须变（重写）
        snap2 = snapshot_files(repo, st2)
        beta_before = {k: v for k, v in snap1.items() if k.startswith(f"{beta_unit}:")}
        beta_after = {k: v for k, v in snap2.items() if k.startswith(f"{beta_unit}:")}
        r.check(
            bool(beta_before) and beta_before == beta_after,
            "beta 单元产物未被重写（mtime+size 不变）",
            f"before={beta_before} after={beta_after}",
        )
        alpha_before = {k: v for k, v in snap1.items() if k.startswith(f"{alpha_unit}:")}
        alpha_after = {k: v for k, v in snap2.items() if k.startswith(f"{alpha_unit}:")}
        r.check(
            alpha_before != alpha_after,
            "alpha 单元产物被重写",
            f"before={alpha_before} after={alpha_after}",
        )

        # 召回：新函数名进了重写后的文档
        hit = [
            p.relative_to(docs).as_posix()
            for p in docs.rglob("*.md")
            if "_meta" not in p.parts and "alpha_extra" in p.read_text(encoding="utf-8", errors="ignore")
        ]
        r.check(bool(hit), "召回：新函数 alpha_extra 出现在文档里", f"{hit[:5]}")

        report2, text2 = newest_report(docs)
        tokens2 = parse_tokens(text2)
        r.check(tokens2 > 0, "第 2 轮账单有 token 用量", f"{tokens2}")
        r.check(tokens2 < tokens1, "增量轮 token 少于全量轮", f"{tokens2} < {tokens1}")
        r.check(t2 < t1, "增量轮墙钟短于全量轮", f"{t2:.0f}s < {t1:.0f}s")
        r.check(
            data2.get("base_sha") == head2,
            "增量成功后基线推进到新 HEAD",
            f"{data2.get('base_sha')} vs {head2}",
        )

        print(f"== 数字：第 1 轮 {t1:.0f}s / {tokens1} tokens；第 2 轮 {t2:.0f}s / {tokens2} tokens")
        return r.finish()
    finally:
        server.stop()


if __name__ == "__main__":
    sys.exit(main())
