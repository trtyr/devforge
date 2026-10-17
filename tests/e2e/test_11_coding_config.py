#!/usr/bin/env python3
"""test_11 · P005 编码 SOP 配置面：manifest PUT 渲染四 profile 的编码专属产物。

覆盖功能点（对照 P005 plan tree topics/agent-supply.md + decisions/007/013/014）：
- mcp.json：coding 双 profile 含 codegraph（stdio + 遥测关闭）与 context7；
  规划 profile 不生成该文件（decisions/007 per-agent stdio + 配置统一入渲染）
- 四语言 skills 真内容落盘（vendor 原样，decisions/006）
- 双提示词协议锚点（stage-complete / stage-verdict）

执行期发现（记录不修）：
- F2：pi-lens 的 npm 依赖 @ast-grep/cli postinstall 在本机失败（手工复现：
  "Failed to move @ast-grep/cli binaries into place"）→ 安装器报错 → render_all
  中断 → 后续 profile（coding-review）不渲染。因此本脚本剥离全部安装包验证
  纯渲染面；「pi-lens 真实安装+RPC 可见性」作为 F2 挂起项归报告。
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_11_coding_config")
    server = Server()
    try:
        server.start()

        # 1. PUT：剥全部安装包（F2 规避），纯渲染面验证（零网络，秒级）
        _, manifest = server.req("GET", "/config/manifest")
        manifest["base_packages"] = []
        for a in manifest["agents"]:
            a["extra_packages"] = []
        status, resp = server.req("PUT", "/config/manifest", manifest, timeout=120.0)
        r.check(status == 202, "PUT manifest == 202（异步受理）", f"status={status}")

        # 2. 四 profile 渲染齐（coding-review/AGENTS.md 是最后一个产物=全齐标记）
        prof = server.home / ".devforge" / "profiles"
        deadline = time.time() + 60
        all_done = False
        while time.time() < deadline:
            all_done = all(
                (prof / n / "AGENTS.md").exists()
                for n in ("plan-agent", "plan-review", "coding-agent", "coding-review")
            )
            if all_done:
                break
            time.sleep(1)
        r.check(all_done, "四 profile 全部渲染（含 coding-review，render_all 不中断）",
                f"elapsed={time.time()-t0 if (t0:=time.time()) else 0:.0f}s")

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

        # 6. F2 立档（记录不修）
        r.issue(
            "F2-PI-LENS-NPM-FAIL", "high",
            "pi-lens 的 npm 依赖 @ast-grep/cli postinstall 本机失败（Failed to move binaries），"
            "致安装器报错且 render_all 中断——后续 profile 不渲染、config.rendered 缺失。"
            "影响：D014 诊断层在真实安装路径不可用（需上游/环境修复）。"
            "证据：手工 `npm install pi-lens` 复现 + 本轮 render_all 中断现场。",
        )

    finally:
        server.stop()

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
