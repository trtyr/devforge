#!/usr/bin/env bash
# 唯一的零警告门:人和 agent 共用的「是否完成」判据。任一项非零即整体失败。
# 必须从仓库根运行(深子目录下 pytest 会静默收集 0 个用例)。
set -euo pipefail

# 一律经 `uv run` 落到项目环境:`uv sync` 之后不必先 activate .venv,也不会误用全局工具。
uv run ruff format --check .
uv run ruff check .
uv run mypy .
uv run python scripts/check_conformance.py .   # 结构不变量(hook / 零 SDK 外泄 / 绝对导入…)
uv run python scripts/check_drift.py           # 文档/工件漂移守卫
uv run pytest                                  # 运行时类型(beartype)+ 行为 + 冒烟 + warnings-as-errors
echo "✓ 全绿"
