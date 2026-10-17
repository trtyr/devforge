"""一致性契约(conformance kit):任何号称实现了某 Protocol 的类
(Mock 与真实后端)都必须跑过同一组行为不变量——可插拔只在所有插头行为一致时才安全。

真实 adapter 在装了 SDK / 有 key 时加入对应列表;默认只测 Mock。
"""

import pytest

from __PACKAGE_NAME__.adapters.mock import MockEmbedder
from __PACKAGE_NAME__.ports.protocols import Embedder

EMBEDDERS: list[Embedder] = [MockEmbedder()]


@pytest.mark.parametrize("impl", EMBEDDERS)
def test_embed_is_deterministic(impl: Embedder) -> None:
    assert impl.embed(["hello", "world"]) == impl.embed(["hello", "world"])


@pytest.mark.parametrize("impl", EMBEDDERS)
def test_embed_preserves_count(impl: Embedder) -> None:
    assert len(impl.embed(["x", "y", "z"])) == 3
