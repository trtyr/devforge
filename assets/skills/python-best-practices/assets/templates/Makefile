check:                    # 完整门禁(唯一正确性判据);也可直接 ./ci.sh
	./ci.sh

dev-test:                 # 本地快速迭代:只跑测试,不跑 lint/漂移(完整判据仍是 ./ci.sh)
	uv run pytest

fmt:
	uv run ruff format .
	uv run ruff check --fix .

hooks:                    # 一次性:把同一条门钉在 push 前(不引入 pre-commit 框架)
	@test -d .git/hooks || { echo "不是 git 仓库(没有 .git/hooks/),先 git init。"; exit 1; }
	@printf '#!/bin/sh\nexec ./ci.sh\n' > .git/hooks/pre-push
	@chmod +x .git/hooks/pre-push
	@echo "✓ 已装 .git/hooks/pre-push → ./ci.sh"
