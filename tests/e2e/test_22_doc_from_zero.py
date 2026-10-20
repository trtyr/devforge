#!/usr/bin/env python3
"""test_22 · doc-from-zero SOP 端到端（M3 验收，P008）。

fixture：一个**无文档、无 git 历史**的小 Python 仓（入口 + 两个模块），
直接 start {"sop":"doc-from-zero"}，验证：
- 门 0：引擎侧清单 + 三道闭包 + 单元编号 → _meta/inventory.json / glossary.md
- 单元滚动：doc-agent 写、doc-review 审，PASS 落 state.json
- 终审：全量审计 + 运行账单 _meta/runs/<run-id>/report.md
- 五项审计可复算：`devforge doc-audit <docs> --repo <repo>` 退出码 0
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
DEVFORGE_BIN = REPO_ROOT / "target" / "debug" / "devforge"
NINE = (
    "plan-agent",
    "plan-review",
    "coding-agent",
    "coding-review",
    "test-agent",
    "test-review",
    "doc-fix",
    "doc-agent",
    "doc-review",
)
DOC_TIMEOUT = float(os.environ.get("DOC_TIMEOUT", "7200"))  # 默认 120 分钟；真仓可用环境变量放大

ENTRY_PY = '''"""极简书目 CLI（doc-from-zero fixture：无文档、无 git）。"""

from booklib.core import add_book, list_books


def main() -> None:
    add_book("三体", "刘慈欣")
    add_book("球状闪电", "刘慈欣")
    for row in list_books():
        print(f"{row['id']}: {row['title']} / {row['author']}")


if __name__ == "__main__":
    main()
'''

CORE_PY = '''"""书目核心逻辑：内存存储，进程内有效。"""

BOOKS: list[dict] = []
_NEXT_ID = 1


def add_book(title: str, author: str) -> dict:
    """新增一本书并返回它的记录。title/author 均为必填字符串。"""
    global _NEXT_ID
    record = {"id": _NEXT_ID, "title": title, "author": author}
    _NEXT_ID += 1
    BOOKS.append(record)
    return record


def list_books() -> list[dict]:
    """按插入顺序返回全部书。"""
    return list(BOOKS)


def _reset_for_tests() -> None:
    """测试专用：清空内存状态（私有，不应出现在文档条目标题里）。"""
    global _NEXT_ID
    BOOKS.clear()
    _NEXT_ID = 1
'''


def newest_session_bytes(server: Server, kind: str) -> int:
    sd = server.home / ".devforge" / "sessions" / kind
    files = list(sd.glob("**/*.jsonl")) if sd.is_dir() else []
    return max((f.stat().st_size for f in files), default=0)


def main() -> int:
    r = Reporter("test_22_doc_from_zero")
    server = Server()
    timeline: list[dict] = []
    try:
        # 配置真源是宿主 ~/.devforge/manifest.json（模型选择、agent 供给都在那里，不在代码里）；
        # 隔离 HOME 启动时会拿内置默认重新生成，所以先把宿主配置种进去，
        # 否则测的是「内置默认」而不是「真实配置」。
        host_manifest = Path.home() / ".devforge" / "manifest.json"
        if host_manifest.exists():
            dest = server.home / ".devforge" / "manifest.json"
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(host_manifest, dest)
        server.start()
        # 剥包快渲染（本脚本验文档机制，不验装包）——但 **MCP 不剥**：
        # codegraph 是 doc-agent 的“代码事实层”，剥了它就等于让实习生去写档案却把档案柜锁了。
        # mcp.json 只是配置（command+args），不产生安装成本。
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
                lambda e: e.get("type") == "config.rendered" and e.get("rendered") == 9,
                timeout=300.0,
            )
            r.check(ev is not None, "九 profile 渲染完成（剥包快路径）", f"{ev}")
        compensated = compensate_profile(server)
        r.check(
            set(["models.json", "auth.json"]).issubset(set(compensated)),
            "测试侧补偿（配置全量复刻）",
            f"{compensated}",
        )
        # MCP 不得被剥：doc-agent 必须有 codegraph，否则 codegraph-first 纪律形同虚设
        mcp_json = server.home / ".devforge" / "profiles" / "doc-agent" / "mcp.json"
        r.check(mcp_json.exists(), "doc-agent 渲染出 mcp.json")
        if mcp_json.exists():
            mcp = json.loads(mcp_json.read_text(encoding="utf-8"))
            names = [s.get("name") for s in mcp.get("mcpServers", mcp).values()] if isinstance(mcp, dict) else []
            r.check(
                "codegraph" in json.dumps(mcp),
                "doc-agent 带 codegraph MCP（真实供给）",
                f"{str(mcp)[:300]}",
            )

        # ---- 目标仓：默认用受控 fixture；DOC_TARGET_REPO 指向真实仓时直接跑真仓 ----
        external = os.environ.get("DOC_TARGET_REPO")
        if external:
            repo = Path(external).resolve()
            r.check(repo.is_dir(), f"目标仓存在（{repo}）")
            r.check(
                not (repo / "docs" / "contexts").exists(),
                "目标仓初始无 docs/contexts（不覆盖旧文档）",
            )
            project_name = repo.name
        else:
            repo = server.home / "workspace" / "book-app"
            (repo / "booklib").mkdir(parents=True, exist_ok=True)
            (repo / "main.py").write_text(ENTRY_PY, encoding="utf-8")
            (repo / "booklib" / "__init__.py").write_text("", encoding="utf-8")
            (repo / "booklib" / "core.py").write_text(CORE_PY, encoding="utf-8")
            r.check(not (repo / "docs").exists(), "fixture 初始无文档")
            r.check(not (repo / ".git").exists(), "fixture 初始无 git 历史")
            project_name = "book-app"

        status, _ = server.req(
            "POST", "/projects", {"name": project_name, "local_path": str(repo)}
        )
        r.check(status == 201, f"项目 {project_name} 创建", f"status={status}")
        status, body = server.req(
            "POST",
            "/projects/{}/tasks".format(project_name),
            {"id": "d1", "sop": "doc-from-zero"},
        )
        r.check(status == 201, "任务 d1（sop=doc-from-zero）创建", f"status={status}, body={body}")

        # ---- 触发 ----
        with SseStream(server) as sse:
            autopilot = UiAutoPilot(server, sse, default_answer="继续")
            autopilot.start()
            status, _ = server.req(
                "POST", f"/projects/{project_name}/tasks/d1/start", None
            )
            r.check(status == 202, "start doc-from-zero == 202", f"status={status}")

            t0 = time.time()
            growth: list[tuple[int, int]] = []
            final = None
            while time.time() - t0 < DOC_TIMEOUT:
                _, runs_body = server.req(
                    "GET", f"/projects/{project_name}/tasks/d1/runs"
                )
                runs = runs_body.get("runs", [])
                if runs and runs[-1]["status"] in ("Settled", "Failed"):
                    final = runs[-1]["status"]
                    break
                timeline.extend(sse.events[len(timeline):])
                growth.append(
                    (round(time.time() - t0), newest_session_bytes(server, "doc"))
                )
                time.sleep(10.0)
            timeline.extend(sse.events[len(timeline):])
            autopilot.stop()
            elapsed = time.time() - t0

            if final is None:
                r.issue(
                    "C22-DOC-TIMEOUT",
                    "high",
                    f"doc-from-zero run {DOC_TIMEOUT}s 未结束（增长曲线 {growth[-6:]}）",
                )
                return r.finish()
            r.check(final == "Settled", f"run 终态 Settled（耗时 {elapsed:.0f}s）", f"final={final}")

        docs = repo / "docs" / "contexts"
        meta = docs / "_meta"
        r.check(docs.is_dir(), "产物目录 docs/contexts 存在（D016）")

        # 门 0 产物
        inv_path = meta / "inventory.json"
        r.check(inv_path.exists(), "门 0：inventory.json 落盘")
        inv = json.loads(inv_path.read_text(encoding="utf-8")) if inv_path.exists() else {}
        entries = inv.get("entries", [])
        modules = [e for e in entries if e.get("kind") == "module"]
        r.check(len(modules) >= 3, f"清单含目标仓的模块条目（{len(modules)}）")
        r.check(
            all(e.get("unit_id") for e in modules),
            "清单：每个 module 都有单元编号（D020）",
        )
        r.check((meta / "glossary.md").exists(), "门 0：glossary.md 自动导出")
        gate = meta / "gate0-closures.md"
        r.check(gate.exists(), "门 0：闭包校验证据落盘")

        # 单元滚动（plan 010 D001：检查点住黑板，不在产物目录）
        state_path = server.home / ".devforge" / "plans" / "devforge" / "d1" / "doc-from-zero" / "state.json"
        r.check(state_path.exists(), "检查点 state.json 落盘（黑板）")
        state = json.loads(state_path.read_text(encoding="utf-8")) if state_path.exists() else {}
        done = (state.get("data") or {}).get("done", [])
        r.check(len(done) >= 1, f"至少一个单元通过审查并落盘（{done}）")
        r.check(
            not (meta / "state.json").exists(),
            "产物目录不得再出现 state.json（运行状态不寄居产物）",
        )

        # 正文文档
        body_docs = [p for p in docs.rglob("*.md") if "_meta" not in p.parts]
        r.check(len(body_docs) >= 1, f"正文文档存在（{len(body_docs)} 篇）", f"{body_docs[:5]}")

        # 终审 + 运行账单
        runs_dir = meta / "runs"
        run_dirs = [p for p in runs_dir.iterdir()] if runs_dir.is_dir() else []
        r.check(bool(run_dirs), "运行账单目录 _meta/runs/<run-id>/ 存在")
        if run_dirs:
            report = run_dirs[0] / "report.md"
            r.check(report.exists(), "运行账单 report.md 存在")
            if report.exists():
                text = report.read_text(encoding="utf-8")
                r.check("审查轮次" in text, "账单含审查轮次")
                r.check("单元完成" in text, "账单含完成单元")
                r.check("墙钟" in text, "账单含墙钟（t12 契约）")
                r.check("用量" in text and "tokens" in text, "账单含 token 用量（会话账，非 Agent 自述）")

        # 会话取证：干活方与审查方都真跑过
        r.check(newest_session_bytes(server, "doc") > 0, "doc-agent 会话有内容")
        r.check(newest_session_bytes(server, "doc-reviews") > 0, "doc-review 会话有内容")

        # ---- 五项审计可复算（机械部分）----
        r.check(DEVFORGE_BIN.exists(), f"`{DEVFORGE_BIN}` 存在（审计可复算）")
        if DEVFORGE_BIN.exists() and inv_path.exists():
            proc = subprocess.run(
                [str(DEVFORGE_BIN), "doc-audit", str(docs), "--repo", str(repo)],
                capture_output=True,
                text=True,
                timeout=300,
            )
            r.check(
                proc.returncode == 0,
                "doc-audit 复算退出码 0（覆盖 + lint）",
                f"rc={proc.returncode}\nstdout={proc.stdout[-2000:]}\nstderr={proc.stderr[-500:]}",
            )
            r.check("覆盖" in proc.stdout, "doc-audit 输出覆盖结论", proc.stdout[-400:])
            r.check(
                "doc-lint:" in proc.stdout or "通过" in proc.stdout,
                "doc-audit 输出 lint 结论",
                proc.stdout[-400:],
            )

        return r.finish()
    finally:
        server.stop()


if __name__ == "__main__":
    sys.exit(main())
