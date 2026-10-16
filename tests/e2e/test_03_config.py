#!/usr/bin/env python3
"""test_03 · 配置管理 API：manifest GET/PUT + 真实 profile 渲染验证（无网络路径）。"""

import json
import sys
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
            agent_names == ["plan-agent", "plan-review"],
            "默认双 Agent", f"{agent_names}",
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
        r.check(status == 200, "PUT manifest == 200", f"status={status}, resp={resp}")
        r.check(
            isinstance(resp, dict) and resp.get("rendered") == 2,
            "PUT 触发重渲染 2 个 profile", f"{resp}",
        )

        # 3. 真实 profile 目录验证（隔离 HOME/.devforge/profiles/）
        profiles = server.home / ".devforge" / "profiles"
        for agent in ("plan-agent", "plan-review"):
            pd = profiles / agent
            agents_md = pd / "AGENTS.md"
            settings = pd / "settings.json"
            r.check(agents_md.exists(), f"{agent} AGENTS.md 渲染", f"{agents_md}")
            r.check(settings.exists(), f"{agent} settings.json 渲染", f"{settings}")
            if settings.exists():
                cfg = json.loads(settings.read_text())
                r.check(
                    cfg.get("defaultProvider") == "newapi"
                    and cfg.get("defaultModel") == "DeepSeek flash",
                    f"{agent} settings 来自模型池",
                    f"{cfg.get('defaultProvider')}/{cfg.get('defaultModel')}",
                )
            # 双记录：PUT 全空包后 settings 不应有 packages 数组残留为非空
            if settings.exists():
                cfg = json.loads(settings.read_text())
                pkgs = cfg.get("packages", [])
                r.check(pkgs == [], f"{agent} settings packages 为空（无包清单）", f"{pkgs}")

        # 4. PUT 未知模型引用 → 400
        bad = json.loads(json.dumps(stripped))
        bad["agents"][0]["model"] = "ghost-model"
        status, body = server.req("PUT", "/config/manifest", bad)
        r.check(
            status in (400, 500),
            "未知模型引用被拒", f"status={status}, body={body}",
        )
        if status == 500:
            r.issue(
                "P3-ISSUE-01", "low",
                "未知模型引用返回 500 而非 400（resolve_model 的 Config 错误被归类为 internal）",
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
