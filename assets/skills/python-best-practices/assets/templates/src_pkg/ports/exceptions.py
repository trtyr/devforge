"""自有边界异常。

所有厂商 / 网络 / 超时 / API 错误在 adapter 边界归一到 ProviderError;
程序错(KeyError / TypeError 等)照常上抛、**绝不吞**进降级路径。
"""


class ProviderError(Exception):
    """外部 provider(LLM / embedding / 向量库等)调用失败。"""
