#!/usr/bin/env python3
"""test_04 · 鉴权：默认放行路径 + token 强制路径可达性。

已知约束：binary main.rs 未暴露 token 配置入口（无 env/CLI 参数），
强制路径（Some(token) → 401）无法从外部触发——按约定记录为发现，不修。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import Reporter, Server  # noqa: E402


def main() -> int:
    r = Reporter("test_04_auth")
    server = Server()
    try:
        server.start()

        # 1. 默认（token 未配置）→ 放行
        status, _ = server.req("GET", "/projects")
        r.check(status == 200, "token 未配置时放行", f"status={status}")

        # 2. 带任意 token 头 → 仍放行（None 配置不校验，符合设计）
        conn_ok = server.req(
            "GET", "/projects", headers_extra={"x-devforge-token": "whatever"}
        )
        r.check(
            conn_ok[0] == 200, "任意 token 头在 None 配置下仍放行", f"status={conn_ok[0]}"
        )

        # 3. 强制路径可达性：服务器无 token 配置入口 → 记录发现
        r.issue(
            "P3-ISSUE-03", "medium",
            "token 鉴权预留位无启动配置入口（main.rs 恒置 None，无 env/CLI 参数），"
            "401 强制路径无法端到端验证（单测已覆盖逻辑）",
            "src/main.rs 未读取任何 token 相关配置；ServerState.token 恒 None",
        )

    finally:
        server.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
