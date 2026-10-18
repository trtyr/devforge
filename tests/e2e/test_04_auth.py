#!/usr/bin/env python3
"""test_04 · 鉴权：默认放行路径 + token 强制路径端到端（006 修复回归）。

006 变更（commit a73f92f）：--token CLI 与 DEVFORGE_TOKEN env 入口落地，
401 强制路径现在可以从外部真实触发——本脚本以双服务器实例覆盖两条路径：
默认实例（未配置 → 放行）+ 强制实例（DEVFORGE_TOKEN → 401/200 矩阵）。
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
    finally:
        server.stop()

    # 3. token 强制路径端到端（006 修复回归：DEVFORGE_TOKEN env → ServerState.token）
    guarded = Server(env_extra={"DEVFORGE_TOKEN": "secret-e2e"})
    try:
        guarded.start()

        # 3a. 无头 → 401
        status, body = guarded.req("GET", "/projects")
        r.check(
            status == 401,
            "配置 token 后无头请求 == 401",
            f"status={status}, body={body}",
        )

        # 3b. 错误 token → 401
        status, _ = guarded.req(
            "GET", "/projects", headers_extra={"x-devforge-token": "wrong"}
        )
        r.check(status == 401, "错误 token == 401", f"status={status}")

        # 3c. 正确 token → 200
        status, body = guarded.req(
            "GET", "/projects", headers_extra={"x-devforge-token": "secret-e2e"}
        )
        r.check(status == 200, "正确 token == 200", f"status={status}, body={body}")

        # 3d. SSE 事件流同在鉴权面内（同 router layer）：无头 → 401
        #（授权侧的流式 200 会长挂，不在脚本里等——授权放行已由 3c 覆盖）
        status, _ = guarded.req("GET", "/events")
        r.check(status == 401, "SSE 事件流无头同样 401（鉴权覆盖全 API 面）", f"status={status}")

    finally:
        guarded.stop()
    return r.finish()


if __name__ == "__main__":
    sys.exit(main())
