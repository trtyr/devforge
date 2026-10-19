# T012/T013/T014 · 条目生成 / 汇编 / 词表导出 证据

- 日期：2026-10-19
- 改动：`src/docgen/inventory.rs`（`export_glossary`）、`src/docgen/lint.rs`（空节 + Mermaid 校验）、`src/server/mod.rs`（门 0 写词表）

## 一、词表导出（D011：清单即词表）

`export_glossary(inv)` 从清单**自动生成** `docs/contexts/_meta/glossary.md`：

- 按 `unit_id` 分组，符号按规范名排序
- 每条：`- \`canonical\` — \`path:line\`（别名：…）`
- 末尾登记「未支持符号抽取的语言（缺口）」
- 文件首行写明「由 inventory 自动导出，勿手改」——它不是第二权威源

门 0 在写 `inventory.json` 后立即写词表（`run_doc_from_zero`）。

## 二、必填节从「缺节」升级到「空节」

新增检查：必填节**标题存在但正文为空**（下一标题前全是空行）→ **FAIL**，报告行号指向该标题。
理由：`## 参数` 下面什么都不写，和"没写这一节"是同一种偷懒；契约要求不适用必须写 `N/A` 并给理由。

## 三、Mermaid 源码校验（D005：只产源码不渲染，但源码必须能解析）

| 情况 | 判定 |
|---|---|
| ` ```mermaid ` 块首行为 `flowchart/graph/sequenceDiagram/classDiagram/erDiagram/stateDiagram/gantt/pie/mindmap/timeline/journey/gitGraph` 开头 | 通过 |
| 块为空 | FAIL |
| 首行不是已知图类型 | FAIL（行号指向块内容起始行） |
| 块未闭合 | FAIL |

## 四、汇编层「只合成不新增事实」

这一层由三件机械检查兜住（不靠 agent 自觉）：
1. 实体存在性：汇编里提到的每个实体必须真实存在（t4）
2. 置信度诚实性：标 🟢 必须给得出 `path:line`（t8）
3. 缺节/空节/Mermaid：结构完整（本节 t9）

人工审查面（终审）再看"上层断言是否可溯源到叶子"，见 t11。

## 五、单测（6 条新增）

`glossary_is_derived_from_inventory` / `mermaid_block_must_start_with_diagram_kind` / `empty_mermaid_block_fails` / `required_section_with_empty_body_fails`（行号指向标题行）/ 以及 t8 的 5 条（同一批文件）。

## 六、门禁

`cargo test` **247 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。
