# T012/T013/T014 · 条目生成 / 汇编 / 词表导出 证据（复核版）

- 复核日期：2026-10-20（初版 2026-10-19）
- 口径：每条结论都给「契约条款 / 提示词锚点 / 代码 test:line / 实跑产出」中的至少两处可回溯证据
- 复跑：`cargo test --lib` 436 通过 / 0 失败；`cargo clippy --all-targets` 0 警告；`test_22` 第 3、4 轮实测见 `evidence/t021-m3-e2e.md`

## T012 · 条目文档生成

| 面 | 证据 |
| --- | --- |
| 契约（唯一权威） | `assets/skills/doc-contract/SKILL.md` §二 七类条目模板（入口/模块/函数/类/配置/数据模型/错误码）、§三 必填节分级（缺节 = FAIL，内部符号只记 finding）、§六 门禁表 |
| 提示词 | `src/config/manifest.rs:312` `DOC_AGENT_PROMPT`：「按 `doc-contract` skill 写条目」「不适用必须写 `N/A` 并给理由（省略不等于不适用）」「每个单元至少一篇本模块文档（frontmatter `type: module`）」 |
| 提示词锚点测试 | `src/config/manifest.rs:1135` `doc_agent_prompt_carries_protocol_and_guards`（断言 `N/A`、`devforge doc-lint`、`type: module` 相关锚点） |
| 机械门禁 | `src/docgen/lint.rs:518` `check_required_sections`；缺节单测 `:1094 missing_required_section_is_failure_with_line`，空节单测 `:1464 required_section_with_empty_body_fails`（正文为空与缺节同罪） |
| 实跑产出 | `t021` 第 3/4 轮：两个单元落盘 `['unit-booklib-01','unit-root-02']`，10 篇正文含 `api/add-book`、`data/books`、`booklib-core`、`entry/main-cli` 等；`doc-audit` 复算覆盖 100%、lint 0 失败 → rc=0 |

结论：生成侧不是「提示词说说」——契约给模板、lint 给可判 FAIL 的门禁、fixture 给「真有文档且过审计」的产出。

## T013 · 汇编（总览 / 架构 / 数据流 / Runbook）

| 面 | 证据 |
| --- | --- |
| 契约 | `doc-contract` §五.3：汇编层每个新事实必须能追溯到叶子证据——**上层断言不许凭空** |
| 提示词（本轮补齐） | `DOC_AGENT_PROMPT` 新增硬约束：「**汇编层只合成**：总览 / 架构 / 数据流 / Runbook 不新增事实，每条上层断言能追溯到叶子；图一律给 **Mermaid 源码**（`flowchart`/`sequenceDiagram` 等），不贴截图与 ASCII 图」 |
| 锚点测试 | `doc_agent_prompt_carries_protocol_and_guards` 新增三条断言：`汇编层只合成`、`Mermaid 源码`、`总览 / 架构 / 数据流 / Runbook` |
| 机械门禁 | 实体存在性 `lint.rs:1281 ghost_symbol_is_reported_with_line` / `:1301 ghost_path_is_reported`；别名只告警 `:1327 alias_usage_is_warning_not_failure`；跨模块重复正文按段落判 `audit.rs:522 duplicate_mentions_flags_repeated_symbol` / `:542 duplicated_content_flags_same_paragraph` |
| Mermaid 源码校验（D005） | `lint.rs:324 check_mermaid`：块首必须是已知图类型，空块/未闭合/未知类型 FAIL；单测 `:1435 mermaid_block_must_start_with_diagram_kind`、`:1456 empty_mermaid_block_fails` |
| 实跑产出 | `t021` 第 3/4 轮产出 `README` 与 `runbook`（汇编层文档）；终审覆盖「上层断言可溯源」由 doc-review 语义判定 + 上述机械检查共同兜住 |

诚实边界：fixture 是 40 行小仓，汇编层只自然长出 README/runbook，没有多模块架构图可画——**Mermaid 路径由 lint 单测与提示词锚点保证，未经大仓实跑观测**。这一条留给增量实证（T025）顺带观察。

## T014 · 词表导出与同名异写校验

| 面 | 证据 |
| --- | --- |
| 契约（D011） | 清单即词表：`glossary.md` 是 inventory 的自动视图，不是第二权威源 |
| 实现 | `src/docgen/inventory.rs:943 export_glossary`：按 unit 分组、符号按规范名排序、每条 `- \`canonical\` — \`path:line\`（别名：…）`、末尾登记「未支持符号抽取的语言（缺口）」 |
| 落盘 | 门 0：`src/server/mod.rs:417-421` 写 `inventory.json` 后立即写 `glossary.md`；change 门 0 essential 档同口径 `src/change/docs.rs:40-44` |
| 单测 | `inventory.rs:1297 glossary_is_derived_from_inventory`（断言「自动导出」与符号出现） |
| 同名异写 | `lint` 别名分支：能解析的别名降为提示（`:1327`），只在终审判定；幽灵实体直接 FAIL（`:1281`/`:1301`） |
| 跨模块重复 | `audit.rs` 按正文段落判重复（`:522`/`:542`），排除 `_meta/`（`:532`/`:553`） |
| 实跑产出 | `t021` 门 0 断言清单里含「`_meta/glossary.md` 自动导出」；三轮 run 均落盘 |

## 勾选口径

三项均满足：契约条款存在且被 test 锚定 → 引擎侧有机械门禁 → fixture 有真实产出。roadmap 已勾选并指向本文件。
