"""配置缝的验收测试:证明「换配置 / 换 provider」这一整类分支真的测得到。

没有 override_settings 这条缝时,settings 是 import 期冻结的单例,
`monkeypatch.setenv("APP_...")` 对它毫无作用,factory 的 provider 分支
除默认那条以外**一条都跑不到**——覆盖率看着还行,实则是死区。
"""

from collections.abc import Callable

import pytest

from __PACKAGE_NAME__.adapters.mock import MockLLM
from __PACKAGE_NAME__.adapters.openai_llm import OpenAILLM
from __PACKAGE_NAME__.core.settings import Settings
from __PACKAGE_NAME__.ports.factory import make_llm


def test_default_provider_is_mock() -> None:
    """默认离线可跑:不设任何 env 时装配出的是 Mock(§9 的硬性验收)。"""
    assert isinstance(make_llm(), MockLLM)


def test_provider_switches_to_openai(
    override_settings: Callable[..., Settings],
) -> None:
    """换 env 就换实现;构造 adapter 不碰 SDK、不需要 key,所以离线也能测这条分支。"""
    settings = override_settings(APP_LLM_PROVIDER="openai")
    assert settings.llm_provider == "openai"
    assert isinstance(make_llm(), OpenAILLM)


def test_unknown_provider_raises(override_settings: Callable[..., Settings]) -> None:
    """未知配置显式抛错,不静默退回默认实现(§6 不静默失败)。"""
    override_settings(APP_LLM_PROVIDER="no-such-vendor")
    with pytest.raises(ValueError, match="未知 llm_provider"):
        make_llm()


def test_nested_field_override(override_settings: Callable[..., Settings]) -> None:
    """env_nested_delimiter 生效:APP_RETRIEVER__TOP_K 能逐项覆盖嵌套配置(§7.5)。"""
    settings = override_settings(APP_RETRIEVER__TOP_K="99")
    assert settings.retriever.top_k == 99
