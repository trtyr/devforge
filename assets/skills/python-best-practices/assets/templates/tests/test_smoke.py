"""冒烟:不装任何厂商 SDK、不连网,主链路用默认 Mock 实现也能跑绿。

把"系统能不能跑"与"模型可不可用/要不要钱/网通不通"解耦。
"""

from __PACKAGE_NAME__.ports.factory import make_llm


def test_default_pipeline_runs_offline() -> None:
    llm = make_llm()  # 默认 mock
    out = llm.complete("ping")
    assert isinstance(out, str) and out
