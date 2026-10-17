#!/usr/bin/env python3
"""按本规范生成一个新 Python 项目骨架。

用法:
    python scaffold.py <package_name> [--target DIR] [--domains a b c]

例:
    python scaffold.py myrag --target ~/code/myrag --domains ingestion retrieval

整树镜像 assets/templates/(src_pkg → src/<package>),替换 __PACKAGE_NAME__,
并创建可导航的领域骨架。之后:cd 进项目 → `uv sync` → `./ci.sh`。
"""

import argparse
import stat
from pathlib import Path

SKILL_ROOT = Path(__file__).resolve().parent.parent
TEMPLATES = SKILL_ROOT / "assets" / "templates"
CONFORMANCE = SKILL_ROOT / "scripts" / "check_conformance.py"
PLACEHOLDER = "__PACKAGE_NAME__"


def _target_rel(rel: Path, package: str) -> Path:
    """把模板内相对路径映射到项目内路径(src_pkg → src/<package>)。"""
    parts = list(rel.parts)
    if parts and parts[0] == "src_pkg":
        parts = ["src", package, *parts[1:]]
    return Path(*parts)


def _domain_readme(package: str, domain: str) -> str:
    """领域包的就近契约 README(根级 CLAUDE.md 只放路由表,细节就近放这里)。

    frontmatter 里的 `covers:` 是漂移保险丝:scripts/check_drift.py 逐条检查
    列出的路径是否还存在,删了/改名了就 CI 红。语法见规范 §11。
    """
    return f"""---
covers: [src/{package}/{domain}/__init__.py]
---

# 领域:{domain}

> 就近契约。根级 `CLAUDE.md` 只放路由表,本领域的细节写在这里。
> 上面的 `covers:` 列出本文档描述的代码路径,`scripts/check_drift.py` 会逐条
> 验证它们仍然存在——文档描述的东西被删/改名即 CI 红。新增模块顺手补进去。

## 职责
(这个领域负责什么,不负责什么。)

## 对外 API
`from {package}.{domain} import ...` —— 公共符号在 `__init__.py` 里 thin re-export。

## 不变量
- (本领域必须始终成立的约束;能机械检查的就写成测试。)
"""


def scaffold(package: str, target: Path, domains: list[str]) -> None:
    if not package.isidentifier():
        raise SystemExit(f"包名非法(必须是合法 Python 标识符):{package!r}")
    for d in domains:
        if not d.isidentifier():
            raise SystemExit(f"领域名非法:{d!r}")

    # 1. 整树镜像 + 占位替换
    for src in TEMPLATES.rglob("*"):
        if not src.is_file():
            continue
        rel = src.relative_to(TEMPLATES)
        dst = target / _target_rel(rel, package)
        dst.parent.mkdir(parents=True, exist_ok=True)
        text = src.read_text(encoding="utf-8").replace(PLACEHOLDER, package)
        dst.write_text(text, encoding="utf-8")
        if src.name.endswith(".sh"):  # ci.sh 可执行
            dst.chmod(dst.stat().st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)

    # 2. 结构检查器随项目出厂:它是 stdlib-only、接受项目根参数的独立脚本,
    #    生成项目的 ./ci.sh 会调它。skill 里的那份是源,项目里的是快照——
    #    和其它模板文件同样的关系。不进项目就等于「结构不变量不在门里」。
    conformance_dst = target / "scripts" / "check_conformance.py"
    conformance_dst.parent.mkdir(parents=True, exist_ok=True)
    conformance_dst.write_text(
        CONFORMANCE.read_text(encoding="utf-8"), encoding="utf-8"
    )

    # 3. 可导航的领域骨架(domain-first,见规范 §7.9)+ 就近契约 README
    pkg_dir = target / "src" / package
    for d in domains:
        (pkg_dir / d).mkdir(parents=True, exist_ok=True)
        (pkg_dir / d / "__init__.py").write_text(
            f"# 领域:{d}。可 re-export 公共 API"
            "(此包在 beartype hook 之后导入,被检查)。\n",
            encoding="utf-8",
        )
        (pkg_dir / d / "README.md").write_text(
            _domain_readme(package, d), encoding="utf-8"
        )

    # 4. 运行期可写目录占位。
    #    .gitignore 用 `data/*` + `!data/.gitkeep`(而不是整目录忽略),
    #    否则这两个 .gitkeep 永远进不了 git,目录在新 clone 里根本不存在。
    for d in ("data", "logs"):
        keep = target / d / ".gitkeep"
        keep.parent.mkdir(parents=True, exist_ok=True)
        keep.touch()

    print(f"✓ 已生成项目:{target}")
    print("  下一步:")
    print(f"    cd {target}")
    print("    uv sync            # 一次性 editable 安装,之后改 src/ 即时生效")
    print("    uv run pytest      # 仅测试(必须在仓库根跑)")
    print("    ./ci.sh            # 完整零警告门(也可 make check)")


def main() -> None:
    ap = argparse.ArgumentParser(
        description="按 python-project-standard 生成项目骨架。"
    )
    ap.add_argument("package", help="包名(合法 Python 标识符,如 myrag)")
    ap.add_argument("--target", default=".", help="目标目录(默认当前)")
    ap.add_argument(
        "--domains",
        nargs="*",
        default=["ingestion", "retrieval", "generation", "agents", "pipelines"],
        help="按 domain-first 创建的领域包(见 §7.9)",
    )
    args = ap.parse_args()
    base = Path(args.target).resolve()
    target = base / args.package if args.target == "." else base
    scaffold(args.package, target, list(args.domains))


if __name__ == "__main__":
    main()
