#!/usr/bin/env python3
"""test_03 · 配置管理 API：manifest GET/PUT + 真实 profile 渲染验证（无网络路径）。"""

import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_03_config")
    server = Server()
    try:
        server.start()

        # 1. GET 默认清单（内置 planning）
        status, body = server.req("GET", "/config/manifest")
        r.check(status == 200, "GET manifest == 200", f"status={status}")
        pool_ids = [m["id"] for m in body.get("model_pool", [])]
        r.check("deepseek-flash" in pool_ids, "默认模型池含 deepseek-flash", f"{pool_ids}")
        agent_names = [a["name"] for a in body.get("agents", [])]
        r.check(
            agent_names
            == ["plan-agent", "plan-review", "coding-agent", "coding-review"],
            "默认四 Agent（P005 注册表扩容）", f"{agent_names}",
        )
        # 四件套原文照录校验（用户拍板清单）
        base = body.get("base_packages", [])
        r.check(
            "git:github.com/NVlabs/SoL-Pi" in base and "npm:pi-blackhole" in base,
            "必装四件套原文在清单", f"{base}",
        )

        # 2. PUT 无包清单（规避真网络安装）——验证渲染产物
        stripped = body.copy()
        stripped["base_packages"] = []
        for a in stripped["agents"]:
            a["extra_packages"] = []
        status, resp = server.req("PUT", "/config/manifest", stripped)
        r.check(status == 202, "PUT manifest == 202（异步化受理）", f"status={status}, resp={resp}")
        r.check(
            isinstance(resp, dict) and resp.get("ok") is True and resp.get("async") is True,
            "PUT 受理体含 ok/async 标记", f"{resp}",
        )

        # 3. 真实 profile 目录验证（隔离 HOME/.devforge/profiles/）
        # 渲染已在后台异步执行：轮询 settings.json 就绪（含 pi 安装的完整渲染，上限 120s）
        profiles = server.home / ".devforge" / "profiles"
        expected_model = {"plan-agent": "mokeywork/deepseek-flash", "plan-review": "mokeywork/grok-4.7"}
        for agent in ("plan-agent", "plan-review"):
            pd = profiles / agent
            settings = pd / "settings.json"
            for _ in range(120):
                if settings.exists():
                    break
                time.sleep(1.0)
            agents_md = pd / "AGENTS.md"
            r.check(agents_md.exists(), f"{agent} AGENTS.md 渲染", f"{agents_md}")
            r.check(settings.exists(), f"{agent} settings.json 渲染", f"{settings}")
            if settings.exists():
                cfg = json.loads(settings.read_text())
                r.check(
                    cfg.get("defaultProvider") == "newapi"
                    and cfg.get("defaultModel") == expected_model[agent],
                    f"{agent} settings 来自模型池（真 model_id）",
                    f"{cfg.get('defaultProvider')}/{cfg.get('defaultModel')}",
                )
                # P2-ISSUE-04 修复断言：运行时生存键入 settings
                r.check(
                    cfg.get("retry", {}).get("maxRetries") == 3
                    and cfg.get("httpIdleTimeoutMs") == 60000
                    and cfg.get("defaultThinkingLevel") == "high"
                    and cfg.get("compaction", {}).get("enabled") is False,
                    f"{agent} settings 运行时生存键（retry/idle 趋快判/thinking/compaction）",
                    f"retry={cfg.get('retry')}, idle={cfg.get('httpIdleTimeoutMs')}",
                )
            # 双记录：PUT 全空包后 settings 不应有 packages 数组残留为非空
            if settings.exists():
                cfg = json.loads(settings.read_text())
                pkgs = cfg.get("packages", [])
                r.check(pkgs == [], f"{agent} settings packages 为空（无包清单）", f"{pkgs}")
            # P2-ISSUE-05 修复断言：无 skills_source 时内置真实 plan-tree skill 兜底落盘
            skill = pd / "skills" / "plan-tree" / "SKILL.md"
            r.check(skill.exists(), f"{agent} 内置 plan-tree skill 兜底渲染", f"{skill}")
            # P2-ISSUE-03 修复断言：models.json 分发（providers 表 + 预算）
            mj = pd / "models.json"
            if mj.exists():
                models_cfg = json.loads(mj.read_text())
                prov = models_cfg.get("providers", {}).get("newapi", {})
                pool = prov.get("models", [])
                budgets = {m.get("id"): (m.get("contextWindow"), m.get("maxTokens")) for m in pool}
                r.check(
                    budgets.get("mokeywork/deepseek-flash") == (1_000_000, 65_536),
                    f"{agent} models.json 分发带 1M/64K 预算", f"{budgets}",
                )

        # 4. PUT 未知模型引用 → 400（P3-ISSUE-01 已修：变体优先映射 + 同步预验证）
        bad = json.loads(json.dumps(stripped))
        bad["agents"][0]["model"] = "ghost-model"
        status, body = server.req("PUT", "/config/manifest", bad)
        r.check(status == 400, "未知模型引用 == 400", f"status={status}, body={body}")
        if status != 400:
            r.issue(
                "P3-ISSUE-01", "high",
                "回归：未知模型引用未返回 400（修复后应同步预验证 fail fast）",
                f"PUT manifest model=ghost-model → {status}",
            )

        # 5. 清单真源落盘校验（PUT 后 manifest.json 应更新——main 启动即生成真源路径）
        manifest_file = server.home / ".devforge" / "manifest.json"
        if manifest_file.exists():
            on_disk = json.loads(manifest_file.read_text())
            if on_disk.get("base_packages") == []:
                r.check(True, "PUT 同步落盘清单真源", f"{manifest_file}")
            else:
                r.check(False, "PUT 同步落盘清单真源", "盘上内容未更新")
                r.issue(
                    "P3-ISSUE-02", "high",
                    "PUT manifest 只更新内存，未落盘真源（main.rs 构造 ServerState 时未传 manifest_path，"
                    "ServerState.manifest_path 恒 None → PUT 的保存分支永不执行），重启后配置丢失",
                    f"PUT 后 {manifest_file} 的 base_packages 仍为旧值",
                )
        else:
            r.issue(
                "P3-ISSUE-02", "high",
                "PUT manifest 未落盘真源（manifest_path 未配置），重启后配置丢失",
                f"{manifest_file} 不存在",
            )

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
