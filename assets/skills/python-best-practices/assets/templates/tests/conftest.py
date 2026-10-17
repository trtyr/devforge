"""测试专属 fixtures 放这里,不进 src/。

另:**pytest 必须从仓库根跑**。在深子目录下跑 `pytest` 会收集到 0 个用例
(exit 5,输出只写 "no tests ran"),看着像"没红",实则什么都没测。

下面这条守卫让"跑错位置"当场失败。**但它只在本文件被加载时生效**:从 `src/` 深处
跑 `pytest` 时 tests/ 压根没被收集,本 conftest 也就不会加载,守卫不会响——那种情况
靠的是 pytest 自己的 exit 5。它真正兜住的是"tests/ 被收集了、但 CWD 不在仓库根"
(如 `cd src/pkg && pytest ../../tests`)。所以"永远在仓库根跑门"这条纪律仍需自己守。
"""

import os
from collections.abc import Callable, Iterator
from pathlib import Path

import pytest

from __PACKAGE_NAME__.core.settings import Settings, get_settings


def pytest_configure() -> None:
    cwd = Path.cwd()
    if os.getenv("APP_ROOT_DIR") or (cwd / ".project-root").is_file():
        return
    raise pytest.UsageError(
        f"pytest 必须从仓库根运行(当前 CWD:{cwd},未找到 .project-root)。\n"
        "深子目录下 pytest 会静默收集 0 个用例。"
        "请 cd 到仓库根再跑 `uv run pytest` 或 `./ci.sh`。"
    )


@pytest.fixture
def override_settings(
    monkeypatch: pytest.MonkeyPatch,
) -> Iterator[Callable[..., Settings]]:
    """在单个测试内换配置,让 get_settings() 按新环境重建。

    用法(变量名就是 APP_ 前缀那套,嵌套字段用 `__` 连接):

        def test_openai_branch(override_settings: Callable[..., Settings]) -> None:
            override_settings(APP_LLM_PROVIDER="openai")
            assert isinstance(make_llm(), OpenAILLM)

    为什么需要它:配置是进程级单例,光 `monkeypatch.setenv(...)` 改不动——实例在
    import 期就构造完了。本 fixture 设完环境变量再 `get_settings.cache_clear()`,
    下一次 get_settings() 才会重读环境;teardown 再清一次、环境由 monkeypatch 还原,
    不污染别的测试。

    刻意走真实的「env → Settings」链路,而不是往生产代码里塞一个注入点:测的就是
    部署时实际生效的那条路径,顺带证明 §7.5 的优先级链没写错。
    """

    def _override(**env: str) -> Settings:
        for key, value in env.items():
            monkeypatch.setenv(key, value)
        get_settings.cache_clear()
        return get_settings()

    yield _override
    get_settings.cache_clear()
