# 勘察事实 — unit-src-30（src/config/skill_assets.rs）

> 只记「有什么」，不记「为什么」。解释与置信度见 `docs/contexts/src/config/skill_assets.rs.md`；看不出来的写 `docs/contexts/_meta/gaps.md`。

## 文件与模块位置

- 路径 `src/config/skill_assets.rs`，229 行（HEAD 184abb8）。
- 在 `src/config/mod.rs:9` 声明为 `pub mod skill_assets;`；**不在** `src/config/mod.rs:11`-13 的三条 `pub use` 里（config 模块面看不到本文件任何名字）。
- 文件头注释 `:1`-4：内置 skill 资产（编译期嵌入）；`skills_source` 未提供时的兜底内容源。

## 常量与函数（逐条）

- `:7` `pub const PLAN_TREE_SKILL_MD: &str = include_str!("../../assets/skills/plan-tree/SKILL.md");`
- `:10` `pub const DOC_CONTRACT_SKILL_MD: &str = include_str!("../../assets/skills/doc-contract/SKILL.md");`
- `:13`-14 `pub const CODING_DISCIPLINE_SKILL_MD: &str = include_str!("../../assets/skills/coding-discipline/SKILL.md");`
- `:18` `pub fn builtin_skill_files(skill: &str) -> Option<Vec<(&'static str, &'static str)>>`
  - `:19`-24 `match skill`：`"plan-tree"` → `vec![("SKILL.md", PLAN_TREE_SKILL_MD)]`；`"doc-contract"` → 同形；`"coding-discipline"` → 同形；`other` → `builtin_language_files(other)`（原样转发，不做别的处理）。
- `:29`-71 私有常量 40 个 `(&str, &str)` 元组（`RUSTBESTPRACTICES_0`、`GOBESTPRACTICES_0`、`TSJSBESTPRACTICES_0..1`、`PYTHONBESTPRACTICES_0..35`），每项 =（相对 skill 目录路径，`include_str!` 内容）。
- `:74` `pub fn builtin_language_files(skill: &str) -> Option<Vec<(&'static str, &'static str)>>`
  - `:75`-117 `match skill`：`"rust-best-practices"` → 1 项；`"go-best-practices"` → 1 项；`"ts-js-best-practices"` → 2 项（`SKILL.md` + `javascript-strict.md`）；`"python-best-practices"` → 36 项；`_` → `None`。
- 其余内容：`#[cfg(test)] mod tests`（`:120`-229），6 个 `#[test]`，无 `#[ignore]`。

## 资产事实（磁盘侧）

| skill | `assets/skills/<name>/` 文件数 | 代码登记数 | 备注 |
|---|---|---|---|
| `plan-tree` | 1 | 1（`SKILL.md`） | 正文含 `# plan-tree` 与「目录约定」（由测试断言） |
| `doc-contract` | 1 | 1 | 正文含 `name: doc-contract`、必填、`N/A`、反模式、`path:line`、Runbook 七件套（测试断言） |
| `coding-discipline` | 1 | 1 | 正文含 12 条锚点、`- **反例**：`×≥12、`- **证据形态**：`×≥12、来源 `cursor/plugins/pstack` / `MIT` / `Lauren Tan`（测试断言） |
| `rust-best-practices` | 1 | 1 | 正文含 `Rust Strict Standard`（测试断言） |
| `go-best-practices` | 1 | 1 | 正文含 `Go Strict Standard`（测试断言） |
| `ts-js-best-practices` | 2 | 2 | `SKILL.md` + `javascript-strict.md` |
| `python-best-practices` | 57 | 36 | 多出的 21 个是 `.mypy_cache/`（19）与 `.ruff_cache/`（2）缓存产物，未被 `include_str!` 收录 |

- python 资产登记清单（36 项，`:36`-71`）：`SKILL.md`、`ci.sh`、`references/standard.md`、`scripts/{check_appendix_sync,check_conformance,scaffold}.py`、`assets/templates/` 下 30 项（`.env.example`、`.gitignore`、`.project-root`、`CLAUDE.md`、`Makefile`、`README.md`、`ci.sh`、`configs/settings.yaml`、`docs/adr/0001-record-architecture-decisions.md`、`pyproject.toml`、`scripts/check_drift.py`、`src_pkg/**` 14 项、`tests/**` 4 项）。

## 控制流（`render` 侧如何用到）

- `src/config/render.rs:10` `use crate::config::skill_assets::builtin_skill_files;`
- `src/config/render.rs:101`-134 skills 分发循环：`skills_source` 为 `Some` → `copy_dir` 源目录；为 `None` → `builtin_skill_files(skill)`，`Some(files)` 逐文件 `create_dir_all` + `fs::write`（父目录按需建）；`None` → 建占位目录 + `README.md`（`:124`-130`）。
- 本文件无 IO、无进程、无网络；所有内容在编译期由 `include_str!` 嵌入。

## 测试（6 个，行号 = 函数首行）

- `:125` `plan_tree_skill_has_frontmatter_and_body`：`SKILL.md` 存在、以 `---` 开头、含 `# plan-tree` 与「目录约定」。
- `:135` `doc_contract_skill_carries_frontmatter_templates_and_rules`：frontmatter + `name: doc-contract` + 必填 + `N/A` + 反模式 + `path:line` + Runbook 七件套。
- `:149` `unknown_skill_returns_none`：`builtin_skill_files("ghost")` 为 `None`。
- `:154` `coding_discipline_skill_is_registered_with_pi_loadable_frontmatter`：以 `---` 开头、含 `name: coding-discipline`、`description:` 值非空、且不含 `disable-model-invocation: true`。
- `:176` `coding_discipline_skill_carries_twelve_entries_with_counterexamples_and_evidence`：12 个条目锚点 + 反例/证据形态各 ≥12 + 来源许可三串。
- `:213` `language_skills_vendor_coverage`：rust 1 项含 `Rust Strict Standard`；go 含 `Go Strict Standard`；ts 2 项；python 含 `SKILL.md`、`references/standard.md`、`scripts/` 前缀项；rust 正文以 `---` 开头。

## 依赖与消费

- 上游：`src/config/render.rs`（唯一消费者，`:10` `use`、`:112` 调用）。
- 下游：无——只读编译期嵌入的 `&'static str`；不依赖其它 crate 模块。
- 外部：`assets/skills/` 下 7 个目录（文件缺失即编译失败，`include_str!` 语义）。
- 许可/来源登记：`docs/plantree/plans/005-coding-stage/evidence/skills-sources.md`（存在）。

## 可重放命令

- `grep -n 'pub fn builtin' src/config/skill_assets.rs` → `:18`、`:74`。
- `grep -c 'include_str!' src/config/skill_assets.rs` → 43（3 个 pub 常量 + 40 个私有元组）。
- `find assets/skills/python-best-practices -type f | wc -l` → 57；去掉 `.mypy_cache/`、`.ruff_cache/` 后 36。
