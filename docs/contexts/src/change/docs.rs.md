---
type: module
module_path: src/change/docs.rs
unit: unit-src-07
description: doc_level 档位在引擎侧的落点（none/essential/complete 三档文档动作）
---

# src/change/docs.rs

`doc_level` 档位在引擎侧的落点（P009 D002）。

## 职责

本模块把 `doc_level` 档位转成引擎该做的确定性文档动作。文档完整度是**调用方的决定**，引擎不自动探测「仓里有没有 docs/」。

**负责**：
- 按档位分发：`DocAction` 枚举（`Skipped` / `EssentialDone` / `FullChainRequired`）。
- `apply`：`none` → 零动作；`essential` → 真的写轻量事实层（`docs/contexts/_meta/inventory.json` + `docs/contexts/_meta/glossary.md`）后放行 plan；`complete` → 不写任何东西，声明该跑完整文档链（`FullChainRequired`）。
- `essential_ready`：轻量事实层是否已存在（`essential` 幂等判断）。
- `inventory_path`：事实层清单路径。

**不负责**：不跑 doc-from-zero 完整文档链（真跑需要 agent，本层只声明 `FullChainRequired`）；不探测仓库中是否已有文档；不写正文文档（正文归 doc-from-zero SOP）。

## 公开成员

| 成员 | 类型 | 文档 |
|---|---|---|
| `DocAction` | 枚举 | [DocAction](#docaction) |
| `apply` | 函数 | [apply](#apply) |
| `essential_ready` | 函数 | [essential_ready](#essential_ready) |
| `inventory_path` | 函数 | [inventory_path](#inventory_path) |

## 依赖

- 上游 / 消费方：
  - `src/change/mod.rs:13` `pub mod docs;`；`:30` `pub use docs::{DocAction, apply as apply_doc_level, essential_ready};`。
  - `src/change/prepare.rs:9` `use crate::change::docs::{self, DocAction}`；`:24` `pub doc_action: DocAction` 字段；`:57` `docs::apply(repo, level)?` 作为 doc_level 落点；`:58` 对 `FullChainRequired` 分派；`:182` 断言 `EssentialDone`。
- 向下依赖：`src/change/gate0.rs`（`DocLevel`，`src/change/docs.rs:11`）；清单模块（src/docgen/inventory/ 下的 `build_inventory`、`export_glossary`、`Inventory` 的 `save`；该模块正随 ARC-R6 拆分，行号不稳定，故只给符号）；`src/error.rs`（`Context` / `Result`，`src/change/docs.rs:12`）；标准库 `Path`/`PathBuf`。
- 外部 crate：生产无；测试用 `tempfile`、`serde_json`。

## 核心口径

- **none**（默认）：零文档动作，直接进 plan → `Skipped`。
- **essential**：只产轻量事实层（清单 + 词表）后进 plan → 真的写 `docs/contexts/_meta/inventory.json` 与 `docs/contexts/_meta/glossary.md`，返回 `EssentialDone`。
- **complete**：完整文档链由 doc-from-zero SOP 执行，本模块只声明该动作 → 不写任何东西，返回 `FullChainRequired`（引擎不能假装自己写完了）。

---

### DocAction

- 职责：表达引擎该做的文档动作（三态）。只声明动作，不承载 IO。
- 构造 / 初始化：由 `apply` 按 `DocLevel` 分派返回；无手动构造必要。
- 公开成员（变体）：
  - `Skipped` — `none` 档：什么都不做。
  - `EssentialDone` — `essential` 档：已产出轻量事实层（`docs/contexts/_meta/inventory.json` + `docs/contexts/_meta/glossary.md`）。
  - `FullChainRequired` — `complete` 档：需要调用方/编排层去跑 doc-from-zero（本层不跑）。
- 生命周期：值对象，函数返回后由调用方消费（编排分派）。
- 典型用法：`prepare` 按 `doc_action` 判定后续动作：`FullChainRequired` 触发完整链分派（`src/change/prepare.rs:58`）。
- 派生 trait：`Debug, Clone, Copy, PartialEq, Eq`。（🟢 CONFIRMED `src/change/docs.rs:17`）

### apply

- 摘要：按档位执行文档动作。
- 行为语义（🟢 CONFIRMED `src/change/docs.rs:31`）：
  - `None` → `Ok(Skipped)`，不建任何目录。
  - `Complete` → `Ok(FullChainRequired)`，不写任何东西。
  - `Essential` → `build_inventory(repo)?`；建 `docs/contexts/_meta` 目录 → 写 `docs/contexts/_meta/inventory.json` 与 `docs/contexts/_meta/glossary.md`（IO 错误用 `Context` 包装）→ `Ok(EssentialDone)`。IO 错误向上传播为 `Result`。
  - 幂等关联：`essential` 之后 `essential_ready(repo)` 返回真。
- 参数：`repo: &Path` — 目标仓库根；`level: DocLevel` — 档位（`gate0` 解析）。见 `src/change/gate0.rs:24`。
- 返回：`Result<DocAction>`。
- 异常：`Result` — 触发条件：`build_inventory` 失败、建目录失败、`save`/写词表失败（`Context` 包装，文案含「建元数据目录」「写词表」）。`complete`/`none` 不返回错误。
- 示例：`apply(tmp, DocLevel::None)` → `Skipped` 且不建 `docs`（`src/change/docs.rs:72`）；`apply(tmp, DocLevel::Essential)` → `EssentialDone` 且真产出清单、`essential_ready()` 为真、不产正文（`src/change/docs.rs:83`）；`apply(tmp, DocLevel::Complete)` → `FullChainRequired` 且不建 `docs`（`src/change/docs.rs:107`）。

### essential_ready

- 摘要：轻量事实层是否已经存在（`essential` 幂等判断用）。
- 行为语义：返回 `inventory_path(repo).is_file()`——清单文件存在即为已就绪。无 IO 副作用（只读检查）、幂等。（🟢 CONFIRMED `src/change/docs.rs:52`）
- 参数：`repo: &Path`。
- 返回：`bool`。
- 异常：N/A。
- 示例：`essential` 档跑过后 `essential_ready` 返回 `true`（`src/change/docs.rs:83`）。

### inventory_path

- 摘要：事实层清单路径。
- 行为语义：以 `repo` 为根拼接出 `docs/contexts/_meta/inventory.json` 路径，纯路径拼接、无 IO。幂等、无副作用。（🟢 CONFIRMED `src/change/docs.rs:56`）
- 参数：`repo: &Path`。
- 返回：`PathBuf`。
- 异常：N/A。
- 示例：清单实际位于 `docs/contexts/_meta/inventory.json`（`src/change/docs.rs:83` 断言其为文件）。

## 测试覆盖（`#[cfg(test)]`）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `none_level_touches_no_docs` | none 零动作、不建 docs | `src/change/docs.rs:72` |
| `essential_level_emits_inventory_then_lets_plan_proceed` | essential 产清单+词表、放行、幂等、不产正文 | `src/change/docs.rs:83` |
| `complete_level_defers_to_doc_sop_and_writes_nothing` | complete 交 doc-from-zero、不写任何东西 | `src/change/docs.rs:107` |
