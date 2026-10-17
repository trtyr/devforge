"""唯一装配缝:按 settings 选实现并注入。默认 mock,未知配置显式抛错。

业务代码不写裸厂商名,只调 make_*();切换实现 = 改一个 env。

配置在**调用时**经 get_settings() 读,不在 import 时捕获模块级 settings:
后者会把 provider 分支冻死在 import 那一刻,测试再也换不动(见 core/settings.py)。
"""

from __PACKAGE_NAME__.adapters.mock import MockEmbedder, MockLLM
from __PACKAGE_NAME__.core.settings import get_settings
from __PACKAGE_NAME__.ports.protocols import LLM, Embedder


def make_llm() -> LLM:
    match get_settings().llm_provider:
        case "mock":
            return MockLLM()
        case "openai":
            # lazy import:只有选了该 provider 才碰 SDK
            from __PACKAGE_NAME__.adapters.openai_llm import OpenAILLM

            return OpenAILLM()
        case other:
            raise ValueError(f"未知 llm_provider: {other!r}")


def make_embedder() -> Embedder:
    match get_settings().embedder_provider:
        case "mock":
            return MockEmbedder()
        case other:
            raise ValueError(f"未知 embedder_provider: {other!r}")
