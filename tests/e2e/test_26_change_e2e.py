#!/usr/bin/env python3
"""test_26 · change SOP 真链端到端（plan 009 · T008 / M1+M3 验收）。

requirement → start {"sop":"change"} → 真实 pi 六 profile 走完：
门 0（baseline/change-id）→ plan（六块计划+审查）→ coding（基线取证→改码→按步提交→审查）
→ testing（回归→新测试→反向验证→审查）→ 引擎写 report.md。

fixture：`textkit` 小 Python 包（stdlib unittest），带 git 基线。
需求：新增 `count_words`（含单测与 README 同步）——小到一次 coding 能完成，
大到能撑起验收对账表。

断言：
- run Settled；
- 计划落目标仓 `docs/plantree/changes/<id>/change-plan.md`（含六块与白名单）+ baseline.txt；
- git log 出现 `change: <id> step N` 按步提交；
- `count_words` 真实存在且 unittest 全绿（验收标准兑现）；
- `report.md` 含验收对账；
- 黑板 change/state.json 检查点存在。
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

CHANGE_TIMEOUT = float(os.environ.get("CHANGE_TIMEOUT", "10800"))  # 默认 3 小时

PKG_INIT = "from .clean import normalize_whitespace, squeeze_spaces\n\n__all__ = [\"normalize_whitespace\", \"squeeze_spaces\"]\n"

CLEAN_PY = '''"""文本清洗：空白规范化。"""


def normalize_whitespace(text: str) -> str:
    """把连续空白压成单个空格，并去掉首尾空白。"""
    return " ".join(text.split())


def squeeze_spaces(text: str) -> str:
    """仅把连续空格压成一个，不动换行与制表符。"""
    while "  " in text:
        text = text.replace("  ", " ")
    return text
'''

TEST_CLEAN = '''"""clean 模块单测（stdlib unittest）。"""

import unittest

from textkit.clean import normalize_whitespace, squeeze_spaces


class NormalizeWhitespaceTests(unittest.TestCase):
    def test_basic(self):
        self.assertEqual(normalize_whitespace("  a   b  "), "a b")

    def test_empty(self):
        self.assertEqual(normalize_whitespace(""), "")


class SqueezeSpacesTests(unittest.TestCase):
    def test_only_spaces(self):
        self.assertEqual(squeeze_spaces("a    b"), "a b")

    def test_keeps_newline(self):
        self.assertEqual(squeeze_spaces("a\\n\\nb"), "a\\n\\nb")


if __name__ == "__main__":
    unittest.main()
'''

README = '''# textkit

极小文本工具库（change SOP e2e fixture）。

## 公开接口

- `normalize_whitespace(text: str) -> str`：连续空白压成单空格并去首尾。
- `squeeze_spaces(text: str) -> str`：仅压缩连续空格，保留换行/制表符。

## 运行测试

```bash
python3 -m unittest discover -s tests -v
```

（stdlib unittest，无第三方依赖。）
'''

REQUIREMENT = """给 textkit 新增一个公开函数 `count_words(text: str) -> int`：

- 语义：按空白切分统计单词数；连续空白算一个分隔符；空串或纯空白返回 0。
- 落点：`textkit/clean.py`，并从包 `__init__` 导出。
- 测试：在 `tests/test_clean.py` 补对应单测（至少覆盖：普通句子 / 多重空白 / 空串 / 纯空白）。
- 文档：README「公开接口」清单同步加一行。
"""


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


def main() -> int:
    r = Reporter("test_26_change_e2e")
    server = Server()
    try:
        host_manifest = Path.home() / ".devforge" / "manifest.json"
        if host_manifest.exists():
            dest = server.home / ".devforge" / "manifest.json"
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(host_manifest, dest)
        server.start()

        # ---- manifest：宿主 9 agents + change 六件套（夹具；模型统一映射到实证可用渠道）----
        fixture = Path(__file__).resolve().parent / "fixtures" / "change_agents.json"
        change_agents = json.loads(fixture.read_text(encoding="utf-8"))
        r.check(len(change_agents) == 6, "change 六件套夹具加载", f"{[a['name'] for a in change_agents]}")
        for a in change_agents:
            a["model"] = "minimax-m3.1-flash"  # deepseek/grok 渠道今日不可用；机制见证统一用可用渠道

        key = real_api_key("newapi")
        _, manifest = server.req("GET", "/config/manifest")
        have = {a["name"] for a in manifest["agents"]}
        manifest["api_key"] = key
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        manifest["agents"].extend(a for a in change_agents if a["name"] not in have)
        with SseStream(server) as sse:
            status, _ = server.req("PUT", "/config/manifest", manifest, timeout=300.0)
            r.check(status == 202, "PUT manifest（+change 六件套）== 202", f"status={status}")
            ev = sse.wait_for(
                lambda e: e.get("type") == "config.rendered"
                and int(e.get("rendered") or 0) >= 15,
                timeout=300.0,
            )
            r.check(ev is not None, "15 profile 渲染完成（含 change 六件套）", f"{ev}")
        compensate_profile(server)
        # change agents 的 profile 也补模型/钥匙（补偿只覆盖既有名单，逐个确认）
        for a in change_agents:
            p = server.home / ".devforge" / "profiles" / a["name"] / "models.json"
            r.check(p.exists(), f"{a['name']} 渲染出 models.json")

        # ---- fixture 仓（带 git 基线）----
        repo = server.home / "workspace" / "textkit"
        (repo / "textkit").mkdir(parents=True, exist_ok=True)
        (repo / "tests").mkdir(parents=True, exist_ok=True)
        (repo / "textkit" / "__init__.py").write_text(PKG_INIT, encoding="utf-8")
        (repo / "textkit" / "clean.py").write_text(CLEAN_PY, encoding="utf-8")
        (repo / "tests" / "test_clean.py").write_text(TEST_CLEAN, encoding="utf-8")
        (repo / "tests" / "__init__.py").write_text("", encoding="utf-8")
        (repo / "README.md").write_text(README, encoding="utf-8")
        git(repo, "init", "-q", "-b", "main")
        git(repo, "add", "-A")
        git(repo, "commit", "-q", "-m", "baseline: textkit with clean module")
        # 自证：fixture 测试本身是绿的
        probe = subprocess.run(
            ["python3", "-m", "unittest", "discover", "-s", "tests"],
            cwd=repo, capture_output=True, text=True, timeout=120,
        )
        r.check(probe.returncode == 0, "fixture 基线测试绿", probe.stdout[-200:])

        project = "textkit"
        status, _ = server.req("POST", "/projects", {"name": project, "local_path": str(repo)})
        r.check(status == 201, "项目创建", f"status={status}")
        status, body = server.req(
            "POST", f"/projects/{project}/tasks", {"id": "c1", "sop": "change"}
        )
        r.check(status == 201, "任务 c1（sop=change）创建", f"status={status}")

        # 需求原文写任务黑板（run_change 从这里读）
        req_path = server.home / ".devforge" / "plans" / project / "c1" / "requirement.md"
        req_path.parent.mkdir(parents=True, exist_ok=True)
        req_path.write_text(REQUIREMENT, encoding="utf-8")
        r.check(req_path.exists(), "requirement.md 落任务黑板")

        # ---- 触发真链 ----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="继续")
            autopilot.start()
            status, _ = server.req("POST", f"/projects/{project}/tasks/c1/start", None)
            r.check(status == 202, "start change == 202", f"status={status}")
            t0 = time.time()
            final = None
            while time.time() - t0 < CHANGE_TIMEOUT:
                _, runs_body = server.req("GET", f"/projects/{project}/tasks/c1/runs")
                runs = runs_body.get("runs", [])
                if runs and runs[-1]["status"] in ("Settled", "Failed"):
                    final = runs[-1]["status"]
                    break
                time.sleep(15.0)
            autopilot.stop()
            elapsed = time.time() - t0

        r.check(final == "Settled", f"run 终态 Settled（{elapsed/60:.0f}min）", f"final={final}")
        if final != "Settled":
            return r.finish()

        changes_dir = repo / "docs" / "plantree" / "changes"
        ids = sorted(p.name for p in changes_dir.iterdir() if p.is_dir()) if changes_dir.is_dir() else []
        r.check(bool(ids), f"目标仓出现变更目录（{ids}）")
        if not ids:
            return r.finish()
        cid = ids[0]
        cdir = changes_dir / cid

        plan = cdir / "change-plan.md"
        r.check(plan.exists(), "change-plan.md 落目标仓（D003）")
        if plan.exists():
            text = plan.read_text(encoding="utf-8")
            for block in ["需求复述", "影响面", "白名单", "验收标准", "风险"]:
                r.check(block in text, f"计划含「{block}」块")
        r.check((cdir / "baseline.txt").exists(), "门 0 baseline.txt 存在")

        report = cdir / "report.md"
        r.check(report.exists(), "report.md 引擎已写")
        if report.exists():
            rtext = report.read_text(encoding="utf-8")
            r.check("验收对账" in rtext, "报告含验收对账")
            r.check("墙钟" in rtext, "报告含墙钟")

        log = git(repo, "log", "--oneline")
        steps = [l for l in log.splitlines() if cid in l and "step" in l]
        r.check(
            len(steps) >= 2,
            f"按步提交留痕（{len(steps)} 条）",
            f"{steps[:4]}",
        )

        # 验收兑现：函数在、测试绿、README 同步
        clean = (repo / "textkit" / "clean.py").read_text(encoding="utf-8")
        r.check("def count_words" in clean, "count_words 已实现")
        probe = subprocess.run(
            ["python3", "-m", "unittest", "discover", "-s", "tests"],
            cwd=repo, capture_output=True, text=True, timeout=120,
        )
        r.check(probe.returncode == 0, "变更后测试全绿", probe.stdout[-300:])
        readme = (repo / "README.md").read_text(encoding="utf-8")
        r.check("count_words" in readme, "README 公开接口已同步")

        checkpoint = (
            server.home / ".devforge" / "plans" / project / "c1" / "change" / "state.json"
        )
        r.check(checkpoint.exists(), "黑板 change/state.json 检查点存在")

        return r.finish()
    finally:
        server.stop()


if __name__ == "__main__":
    sys.exit(main())
