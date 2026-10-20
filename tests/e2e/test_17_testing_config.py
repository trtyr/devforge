#!/usr/bin/env python3
"""test_17 · P007 测试 SOP 配置面：manifest 供给（D007）+ 四新 profile 渲染产物。

覆盖功能点（对照 P007 decisions/D001-D012）：
- manifest 供给面：test-agent（rpiv-todo+ask-user / context7 / python skill）；
  test-review 极简；doc-fix=coding 家族供给；doc-review 极简——
  「外部知识给，内部捷径扣」（codegraph/pi-lens 不给测试侧）
- 剥包快渲染（无网络安装）：八 profile config.rendered == 8
- 黑盒纪律硬禁令 + 白名单锚点（D001）；reroute 协议锚点（D003）；
  Runbook 七件套契约（D008）；findings 台账（D009）
- mcp.json 有无矩阵：test-agent 仅 context7（无 codegraph）；
  test-review/doc-review 无；doc-fix 双 MCP
- skills 矩阵：test-agent 仅 python 全量；doc-fix 四语言；两个 review 无
"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server, SseStream  # noqa: E402

EIGHT = (
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


def main() -> int:
    r = Reporter("test_17_testing_config")
    server = Server()
    try:
        server.start()

        # 1. manifest 供给面（D007：外部知识给，内部捷径扣）
        _, manifest = server.req("GET", "/config/manifest")
        agents = {a["name"]: a for a in manifest.get("agents", [])}

        ta = agents.get("test-agent", {})
        r.check(
            ta.get("extra_packages")
            == ["npm:@juicesharp/rpiv-todo", "npm:@juicesharp/rpiv-ask-user-question"],
            "test-agent 差异化包 = todo + ask-user（无 pi-lens）",
            f"{ta.get('extra_packages')}",
        )
        r.check(
            [m.get("name") for m in ta.get("mcp", [])] == ["context7"],
            "test-agent MCP 仅 context7（codegraph 黑盒不给）",
            f"{[m.get('name') for m in ta.get('mcp', [])]}",
        )
        r.check(
            ta.get("skills") == ["python-best-practices"],
            "test-agent skills 仅 python-best-practices", f"{ta.get('skills')}",
        )
        r.check(ta.get("model") == "deepseek-flash", "test-agent → deepseek-flash", f"{ta.get('model')}")

        tr = agents.get("test-review", {})
        r.check(
            not tr.get("extra_packages") and not tr.get("mcp") and not tr.get("skills"),
            "test-review 极简（无包/无 MCP/无 skills）", f"{tr}",
        )
        r.check(tr.get("model") == "grok-4.7", "test-review → grok-4.7", f"{tr.get('model')}")

        df = agents.get("doc-fix", {})
        ca = agents.get("coding-agent", {})
        r.check(
            df.get("extra_packages") == ca.get("extra_packages"),
            "doc-fix 沿用 coding-agent 家族供给（triage 需读代码）",
            f"{df.get('extra_packages')}",
        )
        r.check(
            [m.get("name") for m in df.get("mcp", [])] == ["codegraph", "context7"],
            "doc-fix 双 MCP（codegraph+context7）", f"{[m.get('name') for m in df.get('mcp', [])]}",
        )
        r.check(len(df.get("skills", [])) == 4, "doc-fix 四语言 skills", f"{df.get('skills')}")

        dr = agents.get("doc-review", {})
        r.check(
            not dr.get("extra_packages") and not dr.get("mcp") and not dr.get("skills"),
            "doc-review 极简", f"{dr}",
        )
        r.check(dr.get("model") == "grok-4.7", "doc-review → grok-4.7", f"{dr.get('model')}")

        # 2. 剥包快渲染（无网络安装）：八 profile rendered == 8
        stripped = json.loads(json.dumps(manifest))
        stripped["base_packages"] = []
        for a in stripped["agents"]:
            a["extra_packages"] = []
        t0 = time.time()
        with SseStream(server) as sse:
            status, resp = server.req("PUT", "/config/manifest", stripped, timeout=120.0)
            r.check(status == 202, "PUT 剥包 manifest == 202", f"status={status}")
            ev = sse.wait_for(
                lambda e: e.get("type") == "config.rendered" and e.get("rendered") == 11,
                timeout=120.0,
            )
            elapsed = time.time() - t0
            r.check(
                ev is not None,
                "config.rendered == 11（全角色渲染全完成，剥包快路径）",
                f"elapsed={elapsed:.0f}s, ev={ev}",
            )

        prof = server.home / ".devforge" / "profiles"
        r.check(
            all((prof / n / "AGENTS.md").exists() for n in EIGHT),
            "八 profile 全部渲染", f"{prof}",
        )

        # 3. 黑盒纪律硬禁令 + 白名单锚点（D001）
        ta_prompt = (prof / "test-agent" / "AGENTS.md").read_text()
        for anchor in (
            "禁止读取任何实现文件内容",
            "路径可以看，内容不能读",
            "白名单",
            "<stage-complete>N</stage-complete>",
            "coverage-matrix",
            "tests/integration",
            "pytest-rerunfailures",
            "Runbook",
            "findings",
            "严禁写单元测试",
            "依赖获取",
            "不得改动被测系统",
            "禁止在套件内运行整套测试",
            "短超时",
            "进程组",
            "passed / failed / skipped",
        ):
            r.check(anchor in ta_prompt, f"test-agent 提示词锚点：{anchor}")

        # 4. reroute 协议锚点（D003）+ 三判据
        tr_prompt = (prof / "test-review" / "AGENTS.md").read_text()
        for anchor in (
            "<reroute>test-side或doc-side</reroute>",
            "<stage-verdict>PASS或FAIL</stage-verdict>",
            "<next>fix或advance或finish</next>",
            "业务关系",
            "极端用户面",
            "coverage-matrix",
            "环境复用",
            "问题一次列全",
            "一票否决",
            "发现挂死风险先停跑",
        ):
            r.check(anchor in tr_prompt, f"test-review 提示词锚点：{anchor}")

        # 5. 文档双 Agent 锚点（D004）
        df_prompt = (prof / "doc-fix" / "AGENTS.md").read_text()
        for anchor in ("只改文档，不改代码", "第三方", "Runbook", "findings", "测试依赖获取"):
            r.check(anchor in df_prompt, f"doc-fix 提示词锚点：{anchor}")
        dr_prompt = (prof / "doc-review" / "AGENTS.md").read_text()
        for anchor in ("plan tree", "Runbook", "findings", "<stage-verdict>PASS或FAIL</stage-verdict>", "测试依赖获取"):
            r.check(anchor in dr_prompt, f"doc-review 提示词锚点：{anchor}")

        # 6. mcp.json 有无矩阵（pi 原生 schema）
        ta_mcp = json.loads((prof / "test-agent" / "mcp.json").read_text()).get("mcpServers", {})
        r.check(
            set(ta_mcp) == {"context7"},
            "test-agent mcp.json 仅 context7（无 codegraph）", f"{list(ta_mcp)}",
        )
        df_mcp = json.loads((prof / "doc-fix" / "mcp.json").read_text()).get("mcpServers", {})
        r.check(
            set(df_mcp) == {"codegraph", "context7"}
            and df_mcp.get("codegraph", {}).get("env", {}).get("CODEGRAPH_TELEMETRY") == "0",
            "doc-fix mcp.json 双 MCP + 遥测关闭（D013）", f"{list(df_mcp)}",
        )
        for agent in ("test-review", "doc-review"):
            r.check(
                not (prof / agent / "mcp.json").exists(),
                f"{agent} 无 mcp.json（极简供给）",
            )

        # 7. skills 矩阵
        ta_skills = {d.name for d in (prof / "test-agent" / "skills").iterdir()}
        r.check(
            ta_skills == {"python-best-practices"},
            "test-agent skills 目录仅 python", f"{ta_skills}",
        )
        py = prof / "test-agent" / "skills" / "python-best-practices"
        r.check(
            (py / "SKILL.md").exists()
            and (py / "references" / "standard.md").exists()
            and (py / "scripts" / "scaffold.py").exists(),
            "test-agent python 章程全量（SKILL+references+scripts）", f"{py}",
        )
        df_skills = {d.name for d in (prof / "doc-fix" / "skills").iterdir()}
        r.check(
            df_skills
            == {"rust-best-practices", "go-best-practices", "ts-js-best-practices", "python-best-practices"},
            "doc-fix 四语言 skills", f"{df_skills}",
        )
        for agent in ("test-review", "doc-review"):
            sk = prof / agent / "skills"
            empty = (not sk.exists()) or not any(sk.iterdir())
            r.check(empty, f"{agent} 无 skills（极简）", f"{sk}")

        # 8. models.json 分发（1M/64K 预算，P2-ISSUE-03 机制复用）
        for agent in ("test-agent", "test-review", "doc-fix", "doc-review"):
            models_text = (prof / agent / "models.json").read_text()
            r.check(
                "1000000" in models_text and "65536" in models_text,
                f"{agent} models.json 带 1M/64K 预算",
            )

    finally:
        server.stop()

    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
