"""确定性离线默认实现(default,不是测试桩)。

不装 SDK、不连网也能跑通主链路并通过测试;同输入同输出,不被随机性污染。
"""

import hashlib

from __PACKAGE_NAME__.ports.protocols import LLM, Embedder, Reranker


class MockLLM:
    def complete(self, prompt: str, /) -> str:
        return f"[mock] {prompt[:40]}"


class MockEmbedder:
    def embed(self, texts: list[str], /) -> list[list[float]]:
        out: list[list[float]] = []
        for t in texts:
            h = int(hashlib.sha256(t.encode()).hexdigest(), 16)
            out.append([(h % 1000) / 1000.0])
        return out


class MockReranker:
    def rerank(self, query: str, docs: list[str], /) -> list[int]:
        return list(range(len(docs)))


# 结构化类型自检(可删):确认 Mock 满足 Protocol
_l: LLM = MockLLM()
_e: Embedder = MockEmbedder()
_r: Reranker = MockReranker()
