# __PACKAGE_NAME__

按 python-project-standard 生成。`uv sync` 安装,`uv run pytest` 测试,`./ci.sh`(或 `make check`)跑完整零警告门(**必须在仓库根跑**)。
`uv run __PACKAGE_NAME__` 跑 console script 入口;`make hooks` 把那条门钉进 git pre-push(不引入 pre-commit 框架:门只能有一条)。
约束见根级 `CLAUDE.md`,完整标准见 skill 的 `references/standard.md`。
