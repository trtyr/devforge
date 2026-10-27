# 勘察事实 — unit-src-07（src/change/docs.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/docs.rs.md` 与 `_meta/gaps.md`。

## 文件与模块位置

- 文件：`src/change/docs.rs`。
- 模块声明：`src/change/mod.rs:13` `pub mod docs;`；`src/change/mod.rs:30` `pub use docs::{DocAction, apply as apply_doc_level, essential_ready};`。
- 文件头模块注释：`doc_level` 档位在引擎侧的落点（P009 D002）——文档完整度是调用方的决定，引擎不自动探测「仓里有没有 docs/」。

## 公开条目（符号 → 代码位置）

| 符号 | 位置 | 类型 |
|---|---|---|
| `DocAction` | `#17` | `#[derive(Debug,Clone,Copy,PartialEq,Eq)] pub enum`（`Skipped`/`EssentialDone`/`FullChainRequired`） |
| `apply` | `#31` | `pub fn(&Path, DocLevel) -> Result<DocAction>`：按档位执行文档动作（essential 写事实层） |
| `essential_ready` | `#52` | `pub fn(&Path) -> bool`：轻量事实层是否已存在（是否真写 `inventory.json`） |
| `inventory_path` | `#56` | `pub fn(&Path) -> PathBuf`：`docs/contexts/_meta/inventory.json` 路径 |

## 行为/测试事实（`#[cfg(test)]`，3 例）

- `none_level_touches_no_docs`（`#72`）：`apply(repo, DocLevel::None)` → `DocAction::Skipped`，且不建 `docs` 目录。
- `essential_level_emits_inventory_then_lets_plan_proceed`（`#83`）：`apply(repo, DocLevel::Essential)` → `DocAction::EssentialDone`；真产出 `docs/contexts/_meta/inventory.json`（可反序列化为 `Inventory` 且含至少一个模块条目）；`essential_ready()` 为真；不产正文 md（`docs/contexts` 下非 `_meta` 且 `.md` 结尾的文件应为空）。
- `complete_level_defers_to_doc_sop_and_writes_nothing`（`#107`）：`apply(repo, DocLevel::Complete)` → `DocAction::FullChainRequired`，不写任何东西、不建 `docs` 目录。

## 口径（代码内可见规则）

- 三档：`DocLevel::None` → `Skipped`（零动作）；`DocLevel::Essential` → 真的写 `docs/contexts/_meta/inventory.json` 与 `glossary.md`，返回 `EssentialDone`；`DocLevel::Complete` → 不写任何东西，返回 `FullChainRequired`（文档由 doc-from-zero SOP 的 agent 写，引擎不能假装写完）。
- `apply`（Essential 分支）：`build_inventory(repo)?` → 建 `docs/contexts/_meta` 目录 → `inv.save(meta/inventory.json)?` → 写 `glossary.md`（`export_glossary(&inv)`）。IO 错误用 `Context`（`.ctx`）包。
- `essential_ready`：`inventory_path(repo).is_file()`（幂等判断用）。
- 错误面：`crate::error::{Context, Result}`（`src/change/docs.rs:8`）；依赖 `crate::change::gate0::DocLevel` 与 `crate::docgen::inventory`。
- 测试辅助（内部）：`repo_with_code`、`walk`。

## crate 内调用点（消费者）

- `src/change/prepare.rs:9` `use crate::change::docs::{self, DocAction}`；`:24` `pub doc_action: DocAction` 字段；`:57` `docs::apply(repo, level)?`（doc_level 落点）；`:58` 对 `FullChainRequired` 分派；`:182` 断言 `EssentialDone`。
- `src/change/mod.rs:13/30` 声明与转发导出（`apply as apply_doc_level`）。

## 外部依赖

- 错误面：`crate::error::{Context, Result}`。
- 依赖模块：`crate::change::gate0::DocLevel`、`crate::docgen::inventory::{build_inventory, export_glossary, Inventory}`。
- 标准库 `Path`/`PathBuf`。