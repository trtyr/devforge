"""所有外部 AI 依赖的最小接口(Protocol 缝)。

核心与领域代码只依赖这里,**绝不 import 任何厂商 SDK**。接口越窄能塞的实现越多,
"换模型"从重构降级为加一个类。
"""

from typing import Protocol, runtime_checkable


@runtime_checkable
class LLM(Protocol):
    def complete(self, prompt: str, /) -> str: ...


@runtime_checkable
class Embedder(Protocol):
    def embed(self, texts: list[str], /) -> list[list[float]]: ...


@runtime_checkable
class Reranker(Protocol):
    def rerank(self, query: str, docs: list[str], /) -> list[int]: ...
