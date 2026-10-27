# 勘察事实 — unit-src-12（src/change/plan_audit.rs）

> 只记「有什么」，不记「为什么」。解释见 `docs/contexts/src/change/plan_audit.rs.md` 与 `_meta/gaps.md`。
> 本文件按当前树（781 行）重写；旧一版记录的是 `mod.rs` + `prepare.rs`，与 `_meta/inventory.json` 现行归属不符。

## 文件与模块位置

- 文件：`src/change/plan_audit.rs`（781 行）。
- 模块声明：`src/change/mod.rs:17` `pub mod plan_audit;`；`src/change/mod.rs:36` `pub use plan_audit::{PlanAudit, audit as audit_plan, review_instruction};`（只转发三项；`plan_facts` / `audit_verdicts` / `audit_plan_file` / `path_refs` / 常量走模块路径）。
- 文件头注释（`src/change/plan_audit.rs:1` 至 `:6`）：变更计划的机械审计（P009 T007b）；分工 D012——引擎算确定性（路径存在性 / 白名单齐不齐 / 验收条目能不能判），语义判断交审查官；审计结果整段注入审查指令，审查官被要求不得重算。
- 依赖：`std::path::Path`（`src/change/plan_audit.rs:9`）、`crate::error::Result`（`src/change/plan_audit.rs:11`）。生产无外部 crate；测试用 `tempfile`。

## 公开条目（符号 → 代码位置）

| 符号 | 位置 | 形态 |
|---|---|---|
| `PLAN_BLOCKS` | `src/change/plan_audit.rs:13` | `pub const [&str; 6]` = 需求复述 / 影响面 / 要改的 / 白名单 / 验收标准 / 风险 |
| `PATH_MIGRATIONS` | `src/change/plan_audit.rs:28` | `pub const &[(&str, &str)]`，7 条旧路径→新路径 |
| `normalize_path` | `src/change/plan_audit.rs:39` | `pub fn(&str) -> &str` |
| `VERDICT_MUST_ASK` | `src/change/plan_audit.rs:55` | `pub const &str` = `[必须裁决]` |
| `VERDICT_SELF` | `src/change/plan_audit.rs:58` | `pub const &str` = `[自决]` |
| `VERDICT_ANSWER_FIELD` | `src/change/plan_audit.rs:61` | `pub const &str` = `用户裁决：`（全角冒号） |
| `VerdictAudit` | `src/change/plan_audit.rs:65` | `#[derive(Debug,Clone,Default,PartialEq,Eq)] pub struct { pub issues: Vec<String>, pub facts: Vec<String> }` |
| `VerdictAudit::is_clean` | `src/change/plan_audit.rs:73` | `pub fn(&self) -> bool` = `issues.is_empty()` |
| `VerdictAudit::render_facts` | `src/change/plan_audit.rs:78` | `pub fn(&self) -> String`；facts 空返回「（无）」+换行，否则每条 `- {f}\n` |
| `audit_verdicts` | `src/change/plan_audit.rs:98` | `pub fn(&str) -> VerdictAudit` |
| `PlanAudit` | `src/change/plan_audit.rs:226` | `#[derive(Debug,Clone,Default,PartialEq,Eq)] pub struct { pub issues: Vec<String>, pub path_refs: usize, pub missing_blocks: Vec<String> }` |
| `PlanAudit::is_clean` | `src/change/plan_audit.rs:236` | `pub fn(&self) -> bool` = `issues.is_empty()` |
| `PlanAudit::render` | `src/change/plan_audit.rs:241` | `pub fn(&self) -> String`：首行「命中 path:line N 条；缺失块：…；问题 N 条」+ 每条 `- {i}\n` |
| `audit` | `src/change/plan_audit.rs:261` | `pub fn(&str, &Path) -> PlanAudit`（四项检查，见下） |
| `path_refs` | `src/change/plan_audit.rs:318` | `pub fn(&str) -> Vec<(String, u32)>` |
| `plan_facts` | `src/change/plan_audit.rs:387` | `pub fn(&str) -> (Vec<String>, Vec<String>)`（验收条款, 白名单） |
| `review_instruction` | `src/change/plan_audit.rs:409` | `pub fn(&str, &Path, &str, &PlanAudit) -> String` |
| `audit_plan_file` | `src/change/plan_audit.rs:442` | `pub fn(&Path, &Path) -> Result<PlanAudit>`（读文件后转调 `audit`） |

## 内部符号（私有）

| 符号 | 位置 | 形态 |
|---|---|---|
| `VAGUE_ACCEPTANCE` | `src/change/plan_audit.rs:23` | `const [&str; 4]` = 功能正常 / 正常工作 / 体验良好 / 没问题 |
| `ambiguity_block` | `src/change/plan_audit.rs:146` | `fn(&str) -> Option<Vec<&str>>`：取**最后一个**含「歧义」的标题之后到下一个标题前的行 |
| `Entry` | `src/change/plan_audit.rs:163` | 私有 struct `{ first: String, text: String }` |
| `is_top_level_list` | `src/change/plan_audit.rs:171` | `fn(&str) -> bool`：`- ` 或 `* ` 开头（带空格） |
| `is_separator` | `src/change/plan_audit.rs:176` | `fn(&str) -> bool`：长度 ≥3 且全由 `-`/`*`/`_` 组成 |
| `top_level_entries` | `src/change/plan_audit.rs:184` | `fn(&[&str]) -> (Vec<Entry>, Vec<String>)`：拆顶层条目 + 顶层散文 |
| `answer_of` | `src/change/plan_audit.rs:213` | `fn(&str) -> Option<&str>`：先找全角「用户裁决：」再找半角「用户裁决:」，取其后非空文本 |
| `split_path_line` | `src/change/plan_audit.rs:329` | `fn(&str) -> Option<(String, u32)>`：右侧冒号后全数字、路径含 `.`、不以 `/` 开头 |
| `section_lines` | `src/change/plan_audit.rs:342` | `fn(&str, &str) -> Vec<&str>`：标题含子串即进入该块，遇下一个标题退出（后出现的同名标题覆盖先前） |
| `section_paths` | `src/change/plan_audit.rs:359` | `fn(&str, &str) -> Vec<String>`：块内按空白/反引号/竖线/中文标点切 token，取像文件路径的（去重保序） |
| `looks_like_file` | `src/change/plan_audit.rs:372` | `fn(&str) -> bool`：不以 `/` 开头、以 12 种扩展名之一结尾、无空白 |

## `audit` 的四项检查（顺序即代码顺序）

1. 六块：`PLAN_BLOCKS` 逐项做**子串**包含判断（`plan.contains(block)`）；缺任一 → `missing_blocks` 记名 + 一条 issue「六块不全，缺：…」（`src/change/plan_audit.rs:265` 至 `:273`）。
2. path:line 存在性：`path_refs(plan)` 逐条 `repo.join(&path).exists()`；`path_refs` 计数累加到 `PlanAudit.path_refs`；不存在 → issue「影响面引用了不存在的路径：{path}:{line}」（`src/change/plan_audit.rs:276` 至 `:282`）。
3. 白名单：`section_paths(plan, "白名单")` 每个文件 `repo.join(f).exists()`，不存在 → issue；`section_paths(plan, "要改的")` 每项必须在白名单中出现，否则 issue；白名单为空 → issue（`src/change/plan_audit.rs:285` 至 `:302`）。
4. 验收可观测：`section_lines(plan, "验收标准")` 中非空、非标题行若包含 `VAGUE_ACCEPTANCE` 任一词且不含 `→` → issue「验收标准不可观测…」（`src/change/plan_audit.rs:305` 至 `:312`）。

## `audit_verdicts` 的判据（结构性，不含语义）

- 无「歧义」标题 → **只进 facts**（一条「歧义块缺失…」），issues 空（`src/change/plan_audit.rs:100` 至 `:106`）。
- 条目含 `VERDICT_MUST_ASK`：计数 must；`answer_of` 为 `None` → **进 issues**（文案含标记、条目首行、`ask-user-question` 与「不许带默认值自决放行」）（`src/change/plan_audit.rs:111` 至 `:121`）。
- 否则含 `VERDICT_SELF`：计数 self_decided，不产生 issues（`:122` 至 `:123`）。
- 否则：计数 unclassified，**进 facts**（`:124` 至 `:129`）。
- 顶层散文同时含「必须」与「裁决」 → 进 facts（`:131` 至 `:137`）。
- 末尾必定 push 一条统计 fact「歧义块统计：必须裁决 X 条 / 自决 Y 条 / 未分类 Z 条」（`:138` 至 `:141`）。

## `plan_facts` 的抽取口径

- 验收标准：`section_lines(plan, "验收标准")` 中 trim 后以 `-` 或 `*` 开头的行；去掉首字符 `-`/`*` 再 trim；丢弃空串与以 `#` 开头者（`src/change/plan_audit.rs:387` 至 `:406`）。
- 白名单：`section_paths(plan, "白名单")`。

## `review_instruction` 的组成（格式化模板）

1. 角色与禁令：change-plan-review、不看 diff、不跑验收命令、不写文件。
2. 计划文件路径（反引号）+ 「全文附在下面」。
3. 8 条判据（影响面完整性 / 证据真实 / 白名单自洽 / 验收可观测 / 风险诚实 / 留痕核对 / 分类恰当性 / 结构缺失）。
4. `## 引擎审计（确定性，勿质疑、勿重算）` + `audit.render()`。
5. `## 裁决门事实（确定性，勿质疑、勿重算）` + `audit_verdicts(plan_text).render_facts()`。
6. `## 计划全文（<path>）` 后接 markdown 围栏全文。
7. 末尾要求 `<stage-verdict>PASS|FAIL</stage-verdict>` + `<next>advance|fix</next>` + `<issues>`。

## 测试（`#[cfg(test)]`，19 例，`src/change/plan_audit.rs:449` 起）

| 测试 | 位置 |
|---|---|
| `plan_facts_ignores_table_rows_and_keeps_bullets` | `src/change/plan_audit.rs:473` |
| `clean_plan_passes_with_no_issues` | `:494` |
| `nonexistent_path_is_listed` | `:506` |
| `missing_block_is_reported` | `:518` |
| `file_outside_whitelist_is_flagged` | `:527` |
| `vague_acceptance_is_flagged` | `:542` |
| `empty_whitelist_is_flagged` | `:554` |
| `review_instruction_carries_audit_plan_text_and_forbids_rerun` | `:562` |
| `must_ask_entry_without_answer_field_is_a_hard_issue` | `:608` |
| `must_ask_entry_with_answer_field_passes` | `:626` |
| `must_ask_entry_with_empty_answer_field_is_a_hard_issue` | `:643` |
| `all_self_decided_entries_pass` | `:655` |
| `unclassified_entry_is_a_fact_not_a_failure` | `:663` |
| `prose_declaration_is_a_fact_not_a_failure` | `:676` |
| `missing_ambiguity_block_is_a_fact_not_a_failure` | `:689` |
| `last_ambiguity_heading_wins` | `:702` |
| `review_instruction_carries_verdict_facts_and_new_criteria` | `:715` |
| `decision_ledger_registers_d024_and_annotates_d009` | `:735` |
| `agents_change_doc_matches_the_implemented_contract` | `:759` |

测试夹具：`repo()`（建临时仓 + `src/a.rs` + `src/b.rs`，`src/change/plan_audit.rs:453`）、`good_plan()`（六块齐全的计划文本，`:461`）、`plan_with_ambiguity(items)`（带 `### 1.1 歧义与本计划采用的默认` 小节的计划，`:593`）。

## crate 内 / 集成测试调用点

- `src/change/session.rs:304` `audit_verdicts(&plan_text)`；`:306` 至 `:311` 不干净直接 `PlanTurn::Fail`（不进审查轮）；`:310` `audit_plan_file(&plan_path, &self.ctx.repo)`；`:319` `review_instruction(...)` 喂给 plan 审查实例；`:540` `path_refs(&m.test_ref)` 校验验收表每条指向测试 path:line；`:574` `plan_facts(&text)`。
- `src/change/orchestrator.rs:119` 是 trait 默认实现（返回两个空 vec），`src/change/session.rs:569` 覆盖；编排层在 `src/change/orchestrator.rs:197`、`:272` 调 `session.plan_facts()`。
- `src/config/manifest.rs:489` 断言 change-plan 提示词含全部 `PLAN_BLOCKS`；`:510` 至 `:512` 断言提示词含三个 verdict 字面量与 `ask-user-question`。
- `src/change/mod.rs:36` 转发导出。
- 集成测试：`tests/change_ask_ring.rs:17` 至 `:18` 导入 `audit`、`audit_verdicts`、`review_instruction` 与三个字面量；`:296` 至 `:298` 断言字面量逐字；`:335`、`:353`、`:363`、`:375`、`:384`、`:411` 走裁决门；`:394`、`:420` 检查审查指令；`tests/f6_observability/guards.rs:438` 调 `audit`；`tests/change_failure_context/guards.rs:116` 调 `normalize_path`。

## 代码内可见的其它事实

- `PATH_MIGRATIONS` 共 7 条（`src/change/plan_audit.rs:28` 至 `:36`），注释声明它是「仓库手术的文件迁移登记（单点事实源）」（`:25` 至 `:27`）。
- `VERDICT_ANSWER_FIELD` 用全角冒号（`src/change/plan_audit.rs:61`）；`answer_of` 额外容忍半角写法（`:213` 至 `:223`，注释说明是避免「一个标点误判烧掉一轮修复」）。
- `audit` 不做去重：同一路径重复出现会重复计数/重复报 issue（按代码逐条 push 推断）。
- `section_paths` 按 12 种扩展名判「像文件」（`src/change/plan_audit.rs:373` 至 `:384`），不含 `.yaml` 之外的配置类扩展（如 `.yml`、`.sh`、`.sql`）。
- `plan_facts` 的验收条款只认列表行；表格行、段落、小标题一律不收（`:387` 至 `:406`，用例 `:473` 给出「23 条鬼条款」背景）。

## round 门禁回修（phase3，整份 corpus）

审查打回口径：doc-lint 以**整份 docs corpus** 为分母，不得以「不属本单元」豁免。本轮按 `PATH_MIGRATIONS`（`src/change/plan_audit.rs:28` 至 `:36`）全量重定位旧路径→现状路径，并逐条重核行号（不是机械替换文件名）：

| 旧引用 | 现状 |
|---|---|
| `tests/change_failure_context.rs:17`-18 / `:18` / `:21`-23 / `:24` / `:25` / `:278` 等 | `tests/change_failure_context/main.rs:18` 至 `:27`（import 区）/ `:20` 至 `:22`（orchestrator use，含 `run as run_change`）/ `:23`（session）/ `:24` 至 `:26`（state）/ `:27`（testing_gate）；断言在 `tests/change_failure_context/context.rs:39` / `:59` / `:240` / `:254` |
| `src/server/tests.rs:9` / `:128` / `:132` / `:1234` 等 | `src/server/tests/mod.rs:11` / `:130` / `:134`；`ConfigManifest::builtin()` 另见 `src/server/tests/http.rs:36` / `:53` / `:71` |
| `tests/f6_observability.rs:278` / `:282` | `tests/f6_observability/fixture.rs:27` / `:31` |
| `tests/change_review_protocol.rs` | `tests/change_review_protocol/main.rs:25` |

改动落在 7 篇 src 文档（`change/mod.rs.md`、`change/orchestrator.rs.md`、`change/session.rs.md`、`change/state.rs.md`、`change/testing_gate.rs.md`、`config/installer.rs.md`、`config/mod.rs.md`）、8 份 `_meta/facts/*.md` 与 1 条 `_meta/gaps.md`。

- `devforge doc-lint docs/contexts --repo .` → **退出码 0；doc-lint: 通过（0 违规）**。
- 保留不改：`_meta/reviews/*.md` 是历次审查的**历史快照**（review-18/19/20/21/28 里的旧路径记录的是当时事实），改写会篡改审计记录；`src/change/plan_audit.rs.md` 的 7 处旧路径在 `text` 围栏内，是迁移表的正文。
