#!/usr/bin/env python3
"""test_11 · P005 编码 SOP 配置面：manifest PUT 渲染四 profile 的编码专属产物。

覆盖功能点（对照 P005 plan tree topics/agent-supply.md + decisions/007/013/014）：
- 全供给安装：base_packages + extra_packages（rpiv-todo/pi-lens）真装不剥——
  F2 已根因定论（006 decisions/001：/tmp ENOSPC 被上游吞错伪装，非上游缺陷），
  健康盘面下全供给安装回归
- mcp.json：coding 双 profile 含 codegraph（stdio + 遥测关闭）与 context7；
  规划 profile 不生成该文件（decisions/007 per-agent stdio + 配置统一入渲染）
- 四语言 skills 真内容落盘（vendor 原样，decisions/006）
- 双提示词协议锚点（stage-complete / stage-verdict）
- 插件装机证据：settings.json packages 登记 + node_modules 落盘
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream  # noqa: E402


def main() -> int:
    r = Reporter("test_11_coding_config")
    server = Server()
    try:
        server.start()

        # 1. PUT：全供给真装（F2 已定论非上游缺陷，006 decisions/001；
        #    git clone SoL-Pi + npm install pi-lens 需要网络，预算 300s）
        _, manifest = server.req("GET", "/config/manifest")
        t0 = time.time()
        with SseStream(server) as sse:
            status, resp = server.req("PUT", "/config/manifest", manifest, timeout=300.0)
            r.check(status == 202, "PUT manifest == 202（异步受理，全供给）", f"status={status}")

            # 2. 完成信号 = config.rendered SSE（render_all 串行装完四角色才发；
            #    AGENTS.md 在装包前落盘，不能当就绪信号——v4 实测竞态教训）
            ev = sse.wait_for(
                lambda e: e.get("type") == "config.rendered" and e.get("rendered") == 4,
                timeout=420.0,
            )
            elapsed = time.time() - t0
            r.check(
                ev is not None,
                "config.rendered == 4（四角色渲染+安装全完成）",
                f"elapsed={elapsed:.0f}s, ev={ev}",
            )

        # 2b. 四 profile 渲染齐（佐证）
        prof = server.home / ".devforge" / "profiles"
        all_done = all(
            (prof / n / "AGENTS.md").exists()
            for n in ("plan-agent", "plan-review", "coding-agent", "coding-review")
        )
        r.check(all_done, "四 profile 全部渲染（含 coding-review，render_all 不中断）",
                f"elapsed={elapsed:.0f}s")

        # 3. mcp.json（pi 原生 schema；decisions/007/013）
        ca = prof / "coding-agent"
        mcp = json.loads((ca / "mcp.json").read_text()).get("mcpServers", {})
        cg = mcp.get("codegraph", {})
        r.check(
            cg.get("command") == "codegraph" and cg.get("args") == ["serve", "--mcp"],
            "coding-agent mcp.json 含 codegraph serve --mcp", f"{cg}",
        )
        r.check(
            cg.get("env", {}).get("CODEGRAPH_TELEMETRY") == "0",
            "codegraph 遥测关闭（D013 卫生条款）", f"{cg.get('env')}",
        )
        c7 = mcp.get("context7", {})
        r.check(
            c7.get("command") == "npx" and "@upstash/context7-mcp" in c7.get("args", []),
            "coding-agent mcp.json 含 context7", f"{c7}",
        )
        cr_mcp_body = json.loads((prof / "coding-review" / "mcp.json").read_text()).get("mcpServers", {})
        r.check(
            set(cr_mcp_body) == {"codegraph", "context7"},
            "coding-review 同配双 MCP", f"{list(cr_mcp_body)}",
        )
        r.check(
            not (prof / "plan-agent" / "mcp.json").exists(),
            "规划 profile 不生成 mcp.json（P001 行为不回归）",
        )

        # 4. 四语言 skills 真内容落盘（vendor 原样，decisions/006）——双 profile 都要有
        for agent in ("coding-agent", "coding-review"):
            sk = prof / agent / "skills"
            rust = sk / "rust-best-practices" / "SKILL.md"
            r.check(rust.exists() and "Rust Strict Standard" in rust.read_text(),
                    f"{agent}: rust-best-practices 原样落盘", f"{rust}")
            go = sk / "go-best-practices" / "SKILL.md"
            r.check(go.exists() and "Go Strict Standard" in go.read_text(),
                    f"{agent}: go-best-practices 原样落盘", f"{go}")
            ts = sk / "ts-js-best-practices"
            r.check(
                (ts / "SKILL.md").exists() and (ts / "javascript-strict.md").exists(),
                f"{agent}: ts-js 双文件原样落盘", f"{ts}",
            )
            py_skill = sk / "python-best-practices"
            r.check(
                (py_skill / "SKILL.md").exists()
                and (py_skill / "references" / "standard.md").exists()
                and (py_skill / "scripts" / "scaffold.py").exists(),
                f"{agent}: python 章程全量（SKILL+references+scripts）", f"{py_skill}",
            )
        pa_skills = {d.name for d in (prof / "plan-agent" / "skills").iterdir()}
        r.check(
            pa_skills == {"plan-tree"},
            "规划 profile skills 不受扩容影响", f"{pa_skills}",
        )

        # 5. 双提示词协议锚点（与 T002 schema 逐字对齐）
        ca_prompt = (ca / "AGENTS.md").read_text()
        r.check("<stage-complete>N</stage-complete>" in ca_prompt, "coding-agent 提示词含阶段完成协议")
        r.check("严禁写端到端测试" in ca_prompt, "coding-agent 提示词含单测禁 e2e（D003）")
        r.check("projectPath" in ca_prompt, "coding-agent 提示词含 projectPath 教学（Q004）")
        cr_prompt = (prof / "coding-review" / "AGENTS.md").read_text()
        r.check("<stage-verdict>PASS或FAIL</stage-verdict>" in cr_prompt, "review 提示词含 Report 协议")
        r.check("对照实际代码与文档" in cr_prompt, "review 提示词含树完成锚点（D011）")

        # 6. 插件装机证据（供给升级回归：pi-lens/rpiv-todo 真装不剥）
        ca_settings = json.loads((ca / "settings.json").read_text())
        pkgs = " ".join(str(p) for p in ca_settings.get("packages", []))
        r.check(
            "pi-lens" in pkgs and "rpiv-todo" in pkgs,
            "coding-agent settings 登记双插件（rpiv-todo/pi-lens）",
            f"packages={ca_settings.get('packages')}",
        )
        astgrep = ca / "npm" / "node_modules" / "@ast-grep" / "cli" / "ast-grep"
        r.check(
            astgrep.exists(),
            "pi-lens 依赖 @ast-grep/cli 二进制真落盘（ENOSPC 修复回归）",
            f"{astgrep}",
        )
        cr_settings = json.loads((prof / "coding-review" / "settings.json").read_text())
        cr_pkgs = " ".join(str(p) for p in cr_settings.get("packages", []))
        r.check(
            "pi-lens" in cr_pkgs,
            "coding-review 亦登记 pi-lens（D014 双 profile 供给）",
            f"packages={cr_settings.get('packages')}",
        )

    finally:
        server.stop()

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
