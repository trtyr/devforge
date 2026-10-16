#!/usr/bin/env python3
"""test_01 · 项目管理 API：创建（含真实目录落地）/重复/列表/详情/404/异常路径。"""

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_01_projects")
    server = Server()
    try:
        server.start()

        workdir = tempfile.mkdtemp(prefix="devforge-e2e-work-")
        target = Path(workdir) / "fresh-project"

        # 1. 创建 → 201 + 字段回显
        status, body = server.req(
            "POST", "/projects",
            {"name": "alpha", "local_path": str(target)},
        )
        r.check(status == 201, "创建项目 == 201", f"status={status}, body={body}")
        r.check(
            isinstance(body, dict) and body.get("name") == "alpha",
            "响应回显项目名", f"body={body}",
        )

        # 2. 真实目录落地（从 0 项目在指定位置建目录——decisions/001 + T004）
        r.check(target.is_dir(), "本地目录真实创建", f"{target}")

        # 3. 重复创建 → 409
        status, body = server.req(
            "POST", "/projects",
            {"name": "alpha", "local_path": str(target)},
        )
        r.check(status == 409, "重复创建 == 409", f"status={status}, body={body}")

        # 4. 列表包含该项目
        status, body = server.req("GET", "/projects")
        names = [p["name"] for p in body.get("projects", [])]
        r.check(status == 200 and "alpha" in names, "列表包含新项目", f"{names}")

        # 5. 详情 200 + 字段；404 路径
        status, body = server.req("GET", "/projects/alpha")
        r.check(status == 200 and body.get("local_path"), "详情含 local_path", f"{body}")
        status, _ = server.req("GET", "/projects/ghost")
        r.check(status == 404, "不存在项目 == 404", f"status={status}")

        # 6. 异常路径：local_path 不可创建（/dev/null 下建子目录必败）
        status, _ = server.req(
            "POST", "/projects",
            {"name": "badpath", "local_path": "/dev/null/impossible"},
        )
        r.check(
            status in (400, 409, 500),
            "不可创建路径返回错误码", f"status={status}",
        )
        # 观察状态一致性：失败的项目是否残留在列表里（潜在半注册问题）
        status, body = server.req("GET", "/projects")
        names = [p["name"] for p in body.get("projects", [])]
        if "badpath" in names:
            r.issue(
                "P1-ISSUE-01", "medium",
                "目录创建失败的项目仍残留在项目列表（半注册状态：store 已插入但 ensure_local_dir 失败）",
                f"POST /projects local_path=/dev/null/impossible → {status}，但 GET /projects 含 badpath",
            )
        else:
            r.check(True, "失败创建未残留脏数据", "badpath 不在列表中")

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
