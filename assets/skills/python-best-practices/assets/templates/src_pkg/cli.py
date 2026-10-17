"""入口:启动时配置日志一次,然后跑业务。

跑法:`uv run __PACKAGE_NAME__`(console script,见 pyproject [project.scripts])
或 `uv run python -m __PACKAGE_NAME__.cli`。两者都「从包进入」,beartype hook 必装上;
别 `python src/__PACKAGE_NAME__/cli.py`(散文件跑法下本文件自身不被 instrument,见 §3)。

出口层也不 print:setup_logging() 已配好 console handler,走 logger 才能统一带上
时间/级别/来源,并被 APP_IS_DEBUG 一把调级。ruff 的 T20 会盯着这条。
"""

import logging

from __PACKAGE_NAME__.core.logging import setup_logging
from __PACKAGE_NAME__.ports.factory import make_llm

logger = logging.getLogger(__name__)


def main() -> None:
    setup_logging()
    llm = make_llm()  # 唯一装配缝;默认 mock,离线可跑
    logger.info("已装配 LLM 实现:%s", type(llm).__name__)
    # TODO: 业务逻辑


if __name__ == "__main__":
    main()
