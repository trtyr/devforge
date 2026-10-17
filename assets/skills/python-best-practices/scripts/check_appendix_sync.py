#!/usr/bin/env python3
"""附录同步守卫:references/standard.md 的附录代码块必须与 assets/templates/ 逐字一致。

附录里贴的是「样板代码」,模板目录里放的是「真正发货的那份」。两处各自演进,
读文档的人(和 AI)就会照着一份**已经不存在**的代码写——这是最贵的一类漂移,
因为它不会让任何测试变红,只会让下游默默做错。实测已两次踩中,所以交给机器。

做法:扫描 `### \\`<path>\\`` 形式的小节标题,取紧随其后的第一个围栏代码块,
与 assets/templates/ 下的对应文件比对(`myproj` ↔ `__PACKAGE_NAME__` 归一)。

用法:
    python check_appendix_sync.py [SKILL_ROOT]   # 默认取本文件所在 skill 根

只依赖标准库。挂在 skill 自检门 ci.sh 里——不挂进门的检查器等于不存在。
"""

import re
import sys
from pathlib import Path

# 附录里包名写死成这个具体名字,模板里是占位符;比对前归一到同一侧
DOC_PACKAGE = "myproj"
PLACEHOLDER = "__PACKAGE_NAME__"

# `### \`<path>\`` 或 `### \`<path>\`(补充说明)`;只取反引号里的第一段作为路径
_HEADING = re.compile(r"^###\s+`([^`]+)`.*$")
_FENCE = re.compile(r"^```")


def _template_path(templates: Path, doc_path: str) -> Path:
    """把附录标题里的项目内路径映射到模板目录里的路径。

    `src/<pkg>/X` → `src_pkg/X`(模板用 src_pkg 这个中性目录名,scaffold 时才落成
    `src/<真实包名>/`);其余路径原样对应。
    """
    parts = doc_path.split("/")
    if len(parts) >= 3 and parts[0] == "src":
        return templates.joinpath("src_pkg", *parts[2:])
    return templates.joinpath(*parts)


def _normalize(text: str) -> str:
    """归一:占位包名换成文档里的具体名,末尾换行统一。"""
    return text.replace(PLACEHOLDER, DOC_PACKAGE).rstrip("\n") + "\n"


def _extract_blocks(md: str) -> list[tuple[str, int, str]]:
    """抽出 (路径, 代码块起始行号, 代码块内容)。

    只认「小节标题之后遇到的第一个围栏块」,中间允许有空行和说明文字;
    若在遇到围栏之前先撞上下一个标题,则该小节没有附带代码块,跳过。
    """
    blocks: list[tuple[str, int, str]] = []
    lines = md.splitlines()
    pending: tuple[str, int] | None = None
    i = 0
    while i < len(lines):
        line = lines[i]
        if m := _HEADING.match(line):
            pending = (m.group(1), i + 1)
            i += 1
            continue
        if pending and _FENCE.match(line):
            path, _ = pending
            start = i + 1
            body: list[str] = []
            i += 1
            while i < len(lines) and not _FENCE.match(lines[i]):
                body.append(lines[i])
                i += 1
            blocks.append((path, start + 1, "\n".join(body)))
            pending = None
        i += 1
    return blocks


def check(skill_root: Path) -> list[str]:
    standard = skill_root / "references" / "standard.md"
    templates = skill_root / "assets" / "templates"
    if not standard.is_file():
        return [f"找不到 {standard}。"]
    if not templates.is_dir():
        return [f"找不到 {templates}。"]

    problems: list[str] = []
    checked = 0
    for doc_path, lineno, doc_code in _extract_blocks(
        standard.read_text(encoding="utf-8")
    ):
        tpl = _template_path(templates, doc_path)
        if not tpl.is_file():
            # 标题里的反引号内容不是模板文件(例如 `covers:` 这类行内说明),跳过。
            continue
        checked += 1
        want = _normalize(tpl.read_text(encoding="utf-8"))
        got = _normalize(doc_code)
        if want != got:
            problems.append(
                f"standard.md:{lineno} 的附录代码块 `{doc_path}` 与模板"
                f" {tpl.relative_to(skill_root)} 不一致。\n"
                f"      模板是发货的那一份,请以它为准更新附录"
                f"(比对时 {PLACEHOLDER} 记作 {DOC_PACKAGE})。\n"
                f"      首处差异:{_first_diff(want, got)}"
            )
    if checked == 0:
        problems.append(
            "附录里一个可比对的代码块都没找到:标题格式应为 ``### `路径` ``,"
            "且路径要能对应到 assets/templates/ 下的文件。"
            "本检查静默通过等于没检查,所以这里当作失败。"
        )
    return problems


def _first_diff(want: str, got: str) -> str:
    w, g = want.splitlines(), got.splitlines()
    for n, (a, b) in enumerate(zip(w, g, strict=False), start=1):
        if a != b:
            return f"第 {n} 行\n        模板: {a!r}\n        附录: {b!r}"
    common = min(len(w), len(g))
    if len(w) != len(g):
        longer, side = (w, "模板") if len(w) > len(g) else (g, "附录")
        return f"第 {common + 1} 行起只有{side}有内容:{longer[common]!r}"
    return "(仅行尾空白差异)"


def main() -> int:
    root = (
        Path(sys.argv[1]).resolve()
        if len(sys.argv) > 1
        else Path(__file__).resolve().parent.parent
    )
    problems = check(root)
    if problems:
        print(f"✗ 附录与模板漂移({len(problems)} 项):\n")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("✓ 附录代码块与 assets/templates/ 逐字一致。")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
