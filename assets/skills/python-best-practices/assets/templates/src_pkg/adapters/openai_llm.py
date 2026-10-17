"""示例真实 adapter:SDK 只在方法内 lazy import,厂商异常归一到 ProviderError。

运行时:openai 进可选 extras([project.optional-dependencies] openai),
未选此 provider 不需要它。
开发期:dev 组亦装 openai,否则 mypy 看不到 SDK 类型、本文件退化成 Any 而漏检。
"""

from __PACKAGE_NAME__.ports.exceptions import ProviderError


class OpenAILLM:
    def complete(self, prompt: str, /) -> str:
        # lazy 导入:未装 SDK 则 ImportError(配置/环境错,响亮上抛)
        from openai import OpenAI, OpenAIError

        try:
            client = OpenAI()
            resp = client.responses.create(model="gpt-4o", input=prompt)
            return resp.output_text
        except OpenAIError as e:  # 仅厂商/网络错归一;程序错(TypeError 等)照常上抛
            raise ProviderError(f"OpenAI 调用失败: {e}") from e
