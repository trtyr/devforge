#!/usr/bin/env bash
# 本 skill 自己的零警告门:让 skill 的工具脚本吃自己的狗粮。
# scripts/ 下的脚本必须通过本标准自己定义的那套规则,否则这个 skill 就是在教一套、做另一套。
#
# 规则不另立一份:--config / --config-file 直接指向 assets/templates/pyproject.toml,
# 也就是被 scaffold 出去的项目所用的那一份,两边不可能各自漂移。
# assets/templates/ 下的模板本身不在这里检:它们含 __PACKAGE_NAME__ 占位,
# 必须先 scaffold 才可解析,由生成项目自己的 ./ci.sh 负责。
#
# 工具经 uvx 拉隔离副本:干净机器免预装,也不会误用 Anaconda / 全局那份 ruff/mypy。
# 版本钉死:门最重要的属性是确定性——「红了就是你的错」。一条会因上游发版而突然变红的门,
# 只会训练人忽略红灯。想要「新规则先在 skill 里暴露」这个收益,应当靠主动定期升级取得,
# 而不是靠随机变红。
# 升级方式:改下面的版本号 → 跑一次本门 → 修掉新规则报出的问题 → 提交。
# 注意这里只钉本 skill 自检门用的工具;模板 assets/templates/pyproject.toml 的 dev 组
# 是生成给用户项目的,钉不钉由用户自己决定,不受此处影响。
set -euo pipefail

RUFF_VERSION="0.16.1"
MYPY_VERSION="2.3.0"

cd "$(dirname "$0")"
CONFIG="assets/templates/pyproject.toml"

uvx "ruff@${RUFF_VERSION}" format --check --config "$CONFIG" scripts/
uvx "ruff@${RUFF_VERSION}" check --config "$CONFIG" scripts/
uvx "mypy@${MYPY_VERSION}" --strict --config-file "$CONFIG" scripts/
# 附录漂移守卫:references/standard.md 的附录代码块必须与 assets/templates/ 逐字一致。
# 这类漂移不会让任何测试变红,只会让照着文档写的人(和 AI)按一份已不存在的代码干活。
uv run --no-project --python 3.13 python scripts/check_appendix_sync.py
echo "✓ 全绿"
