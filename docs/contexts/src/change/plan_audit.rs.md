---
type: module
module_path: src/change/plan_audit.rs
unit: unit-src-12
description: 变更计划的机械审计与裁决门——只算确定性（路径/白名单/验收可判/留痕），语义判断交审查官
---

# src/change/plan_audit.rs

变更计划的机械审计（P009 T007b）。分工是 D012：**引擎算确定性的东西，审查官只做语义判断**；本模块只回答「路径存不存在 / 白名单齐不齐 / 验收条目能不能判 / 必须裁决的条目有没有留痕」，不算语义、也不替审查下结论。审计结果整段注入审查指令，审查官被要求**不得重算**（`src/change/plan_audit.rs:1` 至 `src/change/plan_audit.rs:6`）。

## 职责

**负责**

- 计划体检 `audit`：六块齐全性、path:line 指向的路径是否存在、白名单文件是否存在、「要改的」是否 ⊆ 白名单、验收标准是否可观测。
- 裁决门 `audit_verdicts`：只判**结构与留痕**——标了「必须裁决」的歧义条目有没有非空的「用户裁决：」字段；其余情况（歧义块缺失、未分类条目、散文声明）只产出确定性事实。
- 表格抽取 `plan_facts`：给编排层回读「验收标准」条款与「白名单」。
- 审查指令拼装 `review_instruction`：把计划路径、计划全文、引擎审计、裁决门事实注入给审查 subagent。
- 事实源常量：六块名 `PLAN_BLOCKS`、仓库手术迁移表 `PATH_MIGRATIONS`、三个问询环字面量（`VERDICT_MUST_ASK` / `VERDICT_SELF` / `VERDICT_ANSWER_FIELD`）。
- path:line 引用提取 `path_refs`、路径归一化 `normalize_path`、读文件版入口 `audit_plan_file`。

**不负责**

- 不判语义：「这个歧义该不该问用户」「影响面漏了哪个调用方」由审查官判（`src/change/plan_audit.rs:49` 至 `:54`）。
- 不写计划、不改计划、不跑 git：纯文本 + 文件存在性检查。
- 不注入自身结果到会话：注入动作在 `src/change/session.rs:319`。
- 不校验行号是否超出文件真实行数：只判路径存在（见「缺口」）。

## 公开成员

| 成员 | 种类 | 位置 | 条目 |
|---|---|---|---|
| `PLAN_BLOCKS` | pub const | `src/change/plan_audit.rs:13` | [PLAN_BLOCKS](#plan_blocks) |
| `PATH_MIGRATIONS` | pub const | `src/change/plan_audit.rs:28` | [PATH_MIGRATIONS](#path_migrations) |
| `normalize_path` | pub fn | `src/change/plan_audit.rs:39` | [normalize_path](#normalize_path) |
| `VERDICT_MUST_ASK` | pub const | `src/change/plan_audit.rs:55` | [三个问询环字面量](#三个问询环字面量) |
| `VERDICT_SELF` | pub const | `src/change/plan_audit.rs:58` | 同上 |
| `VERDICT_ANSWER_FIELD` | pub const | `src/change/plan_audit.rs:61` | 同上 |
| `VerdictAudit` | pub struct | `src/change/plan_audit.rs:65` | [VerdictAudit](#verdictaudit) |
| `VerdictAudit` 的 `is_clean` | pub fn | `src/change/plan_audit.rs:73` | [VerdictAudit 的 is_clean](#verdictaudit-的-is_clean) |
| `VerdictAudit` 的 `render_facts` | pub fn | `src/change/plan_audit.rs:78` | [VerdictAudit 的 render_facts](#verdictaudit-的-render_facts) |
| `audit_verdicts` | pub fn | `src/change/plan_audit.rs:98` | [audit_verdicts](#audit_verdicts) |
| `PlanAudit` | pub struct | `src/change/plan_audit.rs:226` | [PlanAudit](#planaudit) |
| `PlanAudit` 的 `is_clean` | pub fn | `src/change/plan_audit.rs:236` | [PlanAudit 的 is_clean](#planaudit-的-is_clean) |
| `PlanAudit` 的 `render` | pub fn | `src/change/plan_audit.rs:241` | [PlanAudit 的 render](#planaudit-的-render) |
| `audit` | pub fn | `src/change/plan_audit.rs:261` | [audit](#audit) |
| `path_refs` | pub fn | `src/change/plan_audit.rs:318` | [path_refs](#path_refs) |
| `plan_facts` | pub fn | `src/change/plan_audit.rs:387` | [plan_facts](#plan_facts) |
| `review_instruction` | pub fn | `src/change/plan_audit.rs:409` | [review_instruction](#review_instruction) |
| `audit_plan_file` | pub fn | `src/change/plan_audit.rs:442` | [audit_plan_file](#audit_plan_file) |

私有符号（`VAGUE_ACCEPTANCE` 与 10 个 helper）见「内部符号」。

## 依赖

**上游 / 消费方**

| 消费方 | 用法 | 位置 |
|---|---|---|
| `src/change/mod.rs` | `pub mod plan_audit;` + 转发导出 `PlanAudit`、`audit as audit_plan`、`review_instruction` | `src/change/mod.rs:17`、`src/change/mod.rs:36` |
| `src/change/session.rs` | plan 轮先跑裁决门（不干净直接 `PlanTurn` 的 `Fail`，不进审查轮），再跑 `audit_plan_file`，再拼 `review_instruction` 给审查实例 | `src/change/session.rs:304`、`src/change/session.rs:310`、`src/change/session.rs:319` |
| `src/change/session.rs` | 验收对账表逐行校验 `path_refs` 非空；`plan_facts` 包装成会话方法 | `src/change/session.rs:540`、`src/change/session.rs:569` |
| `src/change/orchestrator.rs` | trait 默认 `plan_facts` 返回两个空 vec（脚本化 session 不读计划）；编排层在两处调用 | `src/change/orchestrator.rs:119`、`src/change/orchestrator.rs:197`、`src/change/orchestrator.rs:272` |
| `src/config/manifest.rs` | 双向钉住提示词：change-plan 提示词必须含全部 `PLAN_BLOCKS`；提示词必须含三个字面量与 `ask-user-question` | `src/config/manifest_tests.rs:8`、`src/config/manifest_tests.rs:29` |
| `tests/change_ask_ring.rs` | 黑盒走公开 API：字面量逐字断言、裁决门四种形态、审查指令锚点 | `tests/change_ask_ring.rs:17`、`tests/change_ask_ring.rs:296`、`tests/change_ask_ring.rs:335`、`tests/change_ask_ring.rs:394` |
| `tests/f6_observability/guards.rs` | 直接调 `audit` | `tests/f6_observability/guards.rs:438` |
| `tests/change_failure_context/guards.rs` | 用 `normalize_path` 归一化步提交里的历史路径 | `tests/change_failure_context/guards.rs:116` |

**向下依赖**

- `Result`（`src/change/plan_audit.rs:11`）与 `DevforgeError`（在 `audit_plan_file` 内联构造 `System` 变体）。
- 标准库：`Path`（`src/change/plan_audit.rs:9`）、标准库文件读取 读文件、`String` 拼装。无外部 crate、无 git、无网络。
- 测试：`tempfile`（dev-dependency，`Cargo.toml:27`）。两个用例读仓内文档做守卫（`docs/plantree/plans/009-change-sop/decisions/README.md` 与 `docs/plantree/plans/009-change-sop/topics/agents-change.md`，见测试表）。

## 核心口径

**D012 分工线**（🟢 CONFIRMED `src/change/plan_audit.rs:1` 至 `:6`）

| 层 | 判什么 | 产物 |
|---|---|---|
| 引擎（本模块） | 路径存在、白名单齐、验收可判、留痕在不在 | `issues`（硬失败）+ `facts`（确定性事实） |
| 审查官（subagent） | 影响面完整性、分类恰当性、结构缺失、风险诚实 | `PASS`/`FAIL` + issues |

**`audit` 的四项检查**（🟢 CONFIRMED `src/change/plan_audit.rs:261` 至 `:316`）

| # | 检查 | 判据 | 失败文案 |
|---|---|---|---|
| 1 | 六块齐全 | 对 `PLAN_BLOCKS` 每项做**子串**包含判断（不是标题判断） | 「六块不全，缺：…」 |
| 2 | path:line 存在 | `path_refs` 每条拼到 `repo` 下看是否存在；同时累加 `path_refs` 计数 | 「影响面引用了不存在的路径：…」 |
| 3 | 白名单自洽 | 白名单每个文件必须存在；「要改的」每项必须在白名单里；白名单不得为空 | 「白名单里的文件不存在」「…不在白名单里」「白名单为空…」 |
| 4 | 验收可观测 | 「验收标准」块内非标题行含 `VAGUE_ACCEPTANCE` 任一词且不含 `→` | 「验收标准不可观测…」 |

**裁决门的三档结果**（🟢 CONFIRMED `src/change/plan_audit.rs:98` 至 `:143`）

| 输入形态 | 结果 | 落在哪 |
|---|---|---|
| 条目含「[必须裁决]」且无「用户裁决：」非空文本 | **硬失败** | `issues`（plan 轮直接打回，不进审查） |
| 条目含「[必须裁决]」且有非空答复 | 通过 | 只进统计 fact |
| 条目含「[自决]」 | 通过 | 只进统计 fact |
| 条目两种标记都没有 | 通过（不判） | `facts`（未分类条目） |
| 歧义块缺失 | 通过（不判） | `facts`（一条，直接返回） |
| 顶层散文含「必须」+「裁决」 | 通过（不判） | `facts`（散文声明） |
| 每次调用 | 必追加 | `facts`（统计「必须裁决 X / 自决 Y / 未分类 Z」） |

**防漂移双写**（🟢 CONFIRMED `src/config/manifest_tests.rs:8`、`src/config/manifest_tests.rs:29`）

- 六块名与三个字面量同时出现在提示词与本模块：`src/config/manifest.rs` 的单测反向断言提示词含这些常量，改一边不改另一边会变红。

**仓库手术迁移表**（🟢 CONFIRMED `src/change/plan_audit.rs:25` 至 `:37`）

- `PATH_MIGRATIONS` 是「历史步提交 vs 现状白名单」两侧对账的**单点事实源**；比对前经 `normalize_path` 归一化。表里 7 条见下方条目。

---

### PLAN_BLOCKS

- **摘要**：变更计划六块的名称表，供 `audit` 判齐全、供提示词与提示词单测对齐。
- **取值**（字面量，`src/change/plan_audit.rs:13` 至 `:20`）：
  ```text
  需求复述 / 影响面 / 要改的 / 白名单 / 验收标准 / 风险
  ```
- **行为语义**：`pub const [&str; 6]`；`audit` 逐项做子串包含判断，缺项进 `missing_blocks` 与一条 issue。只判「文里出现过这六个词」，不判标题层级、不判顺序（🟡 INFERRED：判据是 `contains`，`src/change/plan_audit.rs:265`）。
- **参数 / 返回 / 异常**：N/A —— 常量，无调用面。
- **示例**：`src/config/manifest_tests.rs:8` 遍历它断言提示词覆盖六块。

### PATH_MIGRATIONS

- **摘要**：仓库手术造成的「旧路径 → 现状路径」登记表，7 条。
- **取值**（🟢 CONFIRMED `src/change/plan_audit.rs:28` 至 `:36`）：

  ```text
  旧路径（现状树里已不存在）          → 现状路径
  src/sop/doc_fix.rs                  → src/sop/doc/fix.rs
  src/sop/doc_from_zero.rs            → src/sop/doc/runner.rs
  src/engine/retry.rs                 → src/engine/policy.rs
  tests/f6_observability.rs           → tests/f6_observability/main.rs
  tests/change_review_protocol.rs     → tests/change_review_protocol/main.rs
  tests/change_failure_context.rs     → tests/change_failure_context/main.rs
  src/server/tests.rs                 → src/server/tests/mod.rs
  ```

  左列路径在现状树里**故意不存在**——它们只在历史步提交里出现，比对前先经 `normalize_path` 映射到右列。

- **行为语义**：`pub const &[(&str, &str)]`；供 `normalize_path` 线性查表。表是「事实源」，新增迁移必须登记在这里（注释见 `src/change/plan_audit.rs:25` 至 `:27`）。
- **参数 / 返回 / 异常**：N/A —— 常量。
- **示例**：集成测试用它把步提交里的历史文件名映射到现状（`tests/change_failure_context/guards.rs:116`）。

### normalize_path

- **摘要**：把历史路径映射到现状路径；不在表里的原样返回。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:39` 至 `:46`）：按 `PATH_MIGRATIONS` 顺序精确相等匹配（不做前缀/通配），命中返回新路径，否则返回入参。幂等（新路径本身不在「旧」列）、无副作用、零分配（返回借用）。
- **参数**：`path: &str` — 待归一的路径（通常来自历史步提交的文件名）。
- **返回**：`&str` —— 生命周期同入参。
- **异常**：N/A —— 纯查表，无失败路径。
- **示例**：`tests/change_failure_context/guards.rs:116` 读取步提交文件名后先归一化，再与现状白名单比对。

### 三个问询环字面量

同一批字面量既在提示词里、也在本模块里；唯一防漂移手段是提示词单测双向钉住（注释写在 `src/change/plan_audit.rs:47` 至 `:53`）。

**`VERDICT_MUST_ASK`**

- 名称：`VERDICT_MUST_ASK`（`src/change/plan_audit.rs:55`）
- 取值：`[必须裁决]`
- 类型：`pub const &str`
- 影响：歧义条目带它**必须**有非空「用户裁决：」留痕，否则 plan 轮硬失败（`src/change/plan_audit.rs:111` 至 `:121`）；审查指令第 6 条判据也引用同一字面量（`src/change/plan_audit.rs:428`）。
- 示例：集成测试逐字断言取值（`tests/change_ask_ring.rs:296`）。

**`VERDICT_SELF`**

- 名称：`VERDICT_SELF`（`src/change/plan_audit.rs:58`）
- 取值：`[自决]`
- 类型：`pub const &str`
- 影响：标它的条目**不触发**留痕要求（只计数），分类是否恰当由审查官判（`src/change/plan_audit.rs:122`）。
- 示例：`tests/change_ask_ring.rs:297` 断言字面量；用例 `src/change/plan_audit_tests.rs` 断言「全是自决项不该有问题」。

**`VERDICT_ANSWER_FIELD`**

- 名称：`VERDICT_ANSWER_FIELD`（`src/change/plan_audit.rs:61`）
- 取值：`用户裁决：`（**全角冒号**）
- 类型：`pub const &str`
- 影响：`answer_of` 先按它切分取非空文本；为容忍笔误还额外认半角「用户裁决:」（`src/change/plan_audit.rs:213` 至 `:223`）。
- 示例：`tests/change_ask_ring.rs:298`、用例 `src/change/plan_audit_tests.rs`（续行与同行两种写法都通过）。

### VerdictAudit

- **职责**：裁决门的结论载体。`issues` = 硬失败（plan 轮据此打回）；`facts` = 确定性事实（注入审查指令，交审查官做语义判断）。不做判断本身。
- **构造 / 初始化**：由 `audit_verdicts` 返回；派生 `Default` 可空构造（两个字段都是空 vec）。派生 `Debug, Clone, Default, PartialEq, Eq`（🟢 CONFIRMED `src/change/plan_audit.rs:64`）。
- **公开成员**：
  - `issues`（`Vec<String>`）— 硬失败条目；plan 轮非空即打回（`src/change/session.rs:306`）。
  - `facts`（`Vec<String>`）— 确定性事实；每条一行渲染进审查指令。
- **生命周期**：值对象，单次审计内产生、随调用结束丢弃；不持久化。
- **典型用法**：`session` 先 `audit_verdicts` 判硬门，再把 `review_instruction` 里同一函数的结果以「裁决门事实」段落注入（`src/change/plan_audit.rs:413`、`src/change/plan_audit.rs:436`）。

### VerdictAudit 的 is_clean

- **摘要**：有没有硬失败。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:73` 至 `:75`）：即 `issues` 为空；只看 `issues`，不看 `facts`（事实再多也算 clean）。
- **参数**：N/A —— 只作用于 `&self`。
- **返回**：`bool`。
- **异常**：N/A。
- **示例**：全是自决项时为 true（`src/change/plan_audit_tests.rs`）。

### VerdictAudit 的 render_facts

- **摘要**：把 `facts` 渲染成可注入的文本块。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:78` 至 `:88`）：`facts` 为空 → 返回「（无）」加换行；否则每条拼成 `- {f}` 加换行。不排序、不转义。
- **参数**：N/A —— 只作用于 `&self`。
- **返回**：`String`。
- **异常**：N/A。
- **示例**：被 `review_instruction` 放在「裁决门事实」段（`src/change/plan_audit.rs:436`）。

### audit_verdicts

- **摘要**：裁决门——核对计划里标了「必须裁决」的条目有没有留痕，其余只记事实。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:98` 至 `:143`）：
  1. 取「歧义块」（`ambiguity_block`）：**最后一个**含「歧义」的标题之后、到下一个标题前的行；取不到 → 只 push 一条 fact 并返回（issues 仍空）。
  2. 拆顶层条目与顶层散文（`top_level_entries`）：顶层列表行（行首无缩进且以 `- ` 或 `* ` 开头）成条目，缩进续行并入上一条目，分隔线不算散文。
  3. 逐条目分类计数：含「[必须裁决]」→ 用 `answer_of` 找答复，找不到就把「缺留痕」push 进 `issues`（文案含条目首行、`ask-user-question` 指令与「不许带默认值自决放行」）；含「[自决]」→ 只计数；两者都没有 → push 一条「未分类」fact。
  4. 顶层散文同时含「必须」与「裁决」→ push 一条「散文声明」fact。
  5. 末尾必 push 一条统计 fact（必须裁决 / 自决 / 未分类 三个计数）。
  - 幂等、无副作用、纯字符串处理；不读文件、不看仓库。
- **参数**：`plan: &str` — 计划全文。
- **返回**：`VerdictAudit`。
- **异常**：N/A —— 返回结构体表达结论，无 `Result`、无 panic 路径（`expect` 不在本函数内）。
- **示例**（5 种形态各有用例）：缺留痕硬失败（`src/change/plan_audit_tests.rs`）、有留痕通过（`src/change/plan_audit_tests.rs`）、字段空 = 没问过（`src/change/plan_audit_tests.rs`）、全自决通过（`src/change/plan_audit_tests.rs`）、未分类只进事实（`src/change/plan_audit_tests.rs`）。

### PlanAudit

- **职责**：计划体检的结论载体。`issues` = 确定性发现（审查官不得重算）；`path_refs` = 命中的 path:line 条数（证据密度参考）；`missing_blocks` = 缺失的六块。
- **构造 / 初始化**：由 `audit` / `audit_plan_file` 返回；派生 `Default` 可空构造。派生 `Debug, Clone, Default, PartialEq, Eq`（🟢 CONFIRMED `src/change/plan_audit.rs:225`）。
- **公开成员**：
  - `issues`（`Vec<String>`）— 四项检查的失败条目，顺序即检查顺序。
  - `path_refs`（`usize`）— 计划里识别到的 path:line 条数；只统计、不判重。
  - `missing_blocks`（`Vec<String>`）— 缺失的块名（取自 `PLAN_BLOCKS`）。
- **生命周期**：值对象，单次审计内产生，随后被 `render` 成文本注入审查指令。
- **典型用法**：`session` 用 `audit_plan_file` 得到它，再原样传给 `review_instruction`（`src/change/session.rs:310`、`src/change/session.rs:319`）。

### PlanAudit 的 is_clean

- **摘要**：计划体检有没有发现问题。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:236` 至 `:238`）：即 `issues` 为空；不看 `missing_blocks`（后者已在 issues 里体现）。
- **参数**：N/A —— 只作用于 `&self`。
- **返回**：`bool`。
- **异常**：N/A。
- **示例**：六块齐全、路径都在、白名单自洽、验收可观测时为 true（`src/change/plan_audit.rs:494`）。

### PlanAudit 的 render

- **摘要**：渲染成注入审查指令的文本块。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:241` 至 `:256`）：首行「命中 path:line N 条；缺失块：无|A、B；问题 N 条」，随后每条 issue 一行 `- {i}` 加换行。无 issue 时只有首行。不排序、不转义。
- **参数**：N/A —— 只作用于 `&self`。
- **返回**：`String`。
- **异常**：N/A。
- **示例**：被 `review_instruction` 放在「引擎审计」段（`src/change/plan_audit.rs:434`）。

### audit

- **摘要**：审计计划文本，返回确定性发现。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:261` 至 `:316`）：
  1. 六块：`PLAN_BLOCKS` 逐项 `contains`；缺项进 `missing_blocks` 并 push 一条 issue。
  2. `path_refs` 每条：拼到 `repo` 下判存在；不存在 push issue；无论存不存在都把计数 +1。
  3. 白名单：`section_paths(plan, "白名单")` 每个文件判存在；「要改的」每项必须出现在白名单；白名单为空 push issue。
  4. 验收：`section_lines(plan, "验收标准")` 非空非标题行含模糊措辞且无 `→` → push issue。
  - 幂等（同一计划 + 同一仓状态 → 同一结论）、只读仓库、无副作用；不做去重（同一路径重复出现会重复报）。
- **参数**：
  - `plan: &str` — 计划全文。
  - `repo: &Path` — 目标仓库根，用于 `repo.join(path).exists()`。
- **返回**：`PlanAudit`。
- **异常**：N/A —— 无 IO 失败面（IO 只在 `audit_plan_file` 里）；不做 `?` 传播。
- **示例**：干净计划零 issue（`src/change/plan_audit.rs:494`）；幽灵路径被点名（`:506`）；缺块被点名（`:518`）；「要改的」越界被点名（`:527`）；模糊验收被点名（`:542`）；空白名单被点名（`:554`）。

### path_refs

- **摘要**：从任意文本里抽出 path:line 形状的引用。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:318` 至 `:326`）：按空白与反引号、括号、竖线、中文标点切 token；每个 token 去掉首尾的逗号/分号/冒号后交 `split_path_line`；`split_path_line` 要求右侧冒号后全是数字、路径含 `.` 且不以 `/` 开头。不校验文件是否存在（那是 `audit` 的事），不依赖正则。
- **参数**：`text: &str` — 任意文本（计划全文、验收表的测试引用等）。
- **返回**：`Vec<(String, u32)>` — `(路径, 行号)`，保持出现顺序，不去重。
- **异常**：N/A —— 纯解析，无失败路径。
- **示例**：`session` 用它对验收表的「测试引用」判空——空则报「没有指到测试 path:line」（`src/change/session.rs:540`）；`audit` 用它统计与判存在。

### plan_facts

- **摘要**：抽取计划里的「验收标准」条款与「白名单」，供编排层进 coding/testing 用。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:387` 至 `:406`）：
  - 验收条款：只在「验收标准」块内、trim 后以 `-` 或 `*` 开头的行；去掉首个 `-`/`*` 再 trim；丢弃空串与以 `#` 开头的行。表格行、普通段落、小标题一律不收（否则对账表会多出「鬼条款」）。
  - 白名单：`section_paths(plan, "白名单")`，去重保序。
  - 纯文本处理、无 IO、幂等。
- **参数**：`plan: &str` — 计划全文。
- **返回**：`(Vec<String>, Vec<String>)` —— 第一项验收条款，第二项白名单路径。
- **异常**：N/A。
- **示例**：验收标准写成 markdown 表格时只收列表行（`src/change/plan_audit.rs:473`，注释记 t26 v3 的「23 条鬼条款」）；`session` 的 `plan_facts` 方法读文件后调用它（`src/change/session.rs:574`）。

### review_instruction

- **摘要**：拼 plan 审查指令：角色与禁令 + 计划路径 + 8 条判据 + 引擎审计 + 裁决门事实 + 计划全文 + 输出协议。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:409` 至 `:440`）：用格式化模板依次写入——
  1. 「你是 change-plan-review」+ 不看 diff / 不跑验收命令 / 不写任何文件；
  2. 计划文件绝对路径（反引号包裹）与「全文附在下面」；
  3. 8 条判据（影响面完整性 / 证据真实 / 白名单自洽 / 验收可观测 / 风险诚实 / 留痕核对 / 分类恰当性 / 结构缺失），其中第 6 至 8 条引用三个 verdict 字面量；
  4. `## 引擎审计（确定性，勿质疑、勿重算）` + 引擎审计文本（`PlanAudit` 的 `render`）；
  5. `## 裁决门事实（确定性，勿质疑、勿重算）` + 内部重新调用 `audit_verdicts(plan_text)` 的 `render_facts()`；
  6. `## 计划全文（<path>）` + markdown 围栏内的计划原文；
  7. 末尾要求 `<stage-verdict>PASS|FAIL</stage-verdict>` + `<next>advance|fix</next>` + `<issues>`。
  - 幂等（纯字符串拼装）、无 IO、无副作用。
- **参数**：
  - `change_id: &str` — 写进角色句，标审查对象。
  - `plan_path: &Path` — 计划文件路径；必须给出，因为审查子进程的工作目录是黑板而不是目标仓（注释见 `src/change/plan_audit.rs:405` 至 `:407`）。
  - `plan_text: &str` — 计划全文，原样嵌入围栏。
  - `audit: &PlanAudit` — 前一步的体检结果。
- **返回**：`String` —— 完整指令文本。
- **异常**：N/A —— 无失败路径。
- **示例**：用例断言指令含「不看 diff」「不跑验收命令」「勿质疑、勿重算」「影响面完整性」「验收标准可观测」与输出协议锚点，并含计划路径与全文（`src/change/plan_audit.rs:562`）；另一例断言含「裁决门事实」与三个新审点（`src/change/plan_audit_tests.rs`）。

### audit_plan_file

- **摘要**：读计划文件并审计——编排层用的便捷入口。
- **行为语义**（🟢 CONFIRMED `src/change/plan_audit.rs:442` 至 `:447`）：`std::fs::read_to_string(plan_path)`；失败包成 `System`（文案「读计划 … 失败」）；成功则转调 `audit(&text, repo)`。只读，不写。
- **参数**：`plan_path: &Path` — 计划文件；`repo: &Path` — 判路径存在性的仓库根。
- **返回**：`Result<PlanAudit>`。
- **异常**：`DevforgeError` 的 `System` 变体 —— 计划文件不可读（不存在 / 权限 / 非 UTF-8）。调用方 `session` 会把它再包成 `Quality` 错误（`src/change/session.rs:310` 至 `:315`）。
- **示例**：`src/change/session.rs:310` 在裁决门通过后调用。

## 内部符号

| 符号 | 位置 | 摘要 + 行为语义 |
|---|---|---|
| `VAGUE_ACCEPTANCE` | `src/change/plan_audit.rs:23` | 模糊措辞表（功能正常 / 正常工作 / 体验良好 / 没问题）。`audit` 用它判「验收不可观测」：命中且该行没有 `→` 才报。 |
| `ambiguity_block` | `src/change/plan_audit.rs:146` | 取最后一个含「歧义」的标题之后、到下一个标题之前的行；后出现的同名标题覆盖先前的（`section_lines` 同语义）。无该标题返回 `None`。 |
| `Entry` | `src/change/plan_audit.rs:163` | 私有条目结构：`first` 记首行（报错时引用），`text` 记首行 + 全部续行（「用户裁决：」常写在续行）。 |
| `is_top_level_list` | `src/change/plan_audit.rs:171` | 判顶层列表行：`- ` 或 `* ` 开头（必须带空格，故 `---` 不算）。 |
| `is_separator` | `src/change/plan_audit.rs:176` | 判分隔线：长度 ≥3 且全由 `-`/`*`/`_` 组成；分隔线不进散文集合。 |
| `top_level_entries` | `src/change/plan_audit.rs:184` | 拆块：无缩进列表行起新条目，缩进行并入上一条目，无缩进非列表、非分隔线的行进散文；空行跳过。返回 `(Vec<Entry>, Vec<String>)`。 |
| `answer_of` | `src/change/plan_audit.rs:213` | 先按全角「用户裁决：」再按半角「用户裁决:」切分，取其后 trim 后的非空文本；都拿不到返回 `None`。注释说明半角容忍是为了不让一个标点误判烧掉一轮修复。 |
| `split_path_line` | `src/change/plan_audit.rs:329` | 右侧冒号后必须全数字且非空；路径必须含 `.` 且不以 `/` 开头；能解析成 `u32` 才算 path:line。 |
| `section_lines` | `src/change/plan_audit.rs:342` | 取某块内容行：标题行含子串即进入该块，遇任意下一个标题退出；同名标题后出现的覆盖先前的。 |
| `section_paths` | `src/change/plan_audit.rs:359` | 块内按空白/反引号/竖线/中文标点切 token，trim 首尾标点后交 `looks_like_file`，命中且未出现过才收（去重保序）。 |
| `looks_like_file` | `src/change/plan_audit.rs:372` | 判「像文件路径」：非空、不以 `/` 开头、以 12 种扩展名之一结尾（rs/py/ts/tsx/js/go/java/kt/md/toml/json/yaml）、无空白。 |

## 测试覆盖（`#[cfg(test)]`，19 例）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `plan_facts_ignores_table_rows_and_keeps_bullets` | 验收条款只收列表行，表格行/段落不收 | `src/change/plan_audit.rs:473` |
| `clean_plan_passes_with_no_issues` | 干净计划零 issue、`path_refs ≥ 2`、六块齐 | `src/change/plan_audit.rs:494` |
| `nonexistent_path_is_listed` | 幽灵 path:line 被点名 | `src/change/plan_audit.rs:506` |
| `missing_block_is_reported` | 缺「风险」块 → `missing_blocks` + 「六块不全」 | `src/change/plan_audit.rs:518` |
| `file_outside_whitelist_is_flagged` | 「要改的」出现白名单外文件 → issue | `src/change/plan_audit.rs:527` |
| `vague_acceptance_is_flagged` | 「功能正常」无箭头 → 不可观测 | `src/change/plan_audit.rs:542` |
| `empty_whitelist_is_flagged` | 白名单空 → issue | `src/change/plan_audit.rs:554` |
| `review_instruction_carries_audit_plan_text_and_forbids_rerun` | 指令含禁令锚点、计划路径、计划全文 | `src/change/plan_audit.rs:562` |
| `must_ask_entry_without_answer_field_is_a_hard_issue` | 缺留痕 = 硬失败，且文案指路 `ask-user-question` | `src/change/plan_audit_tests.rs` |
| `must_ask_entry_with_answer_field_passes` | 续行/同行两种留痕都通过 | `src/change/plan_audit_tests.rs` |
| `must_ask_entry_with_empty_answer_field_is_a_hard_issue` | 字段在但答复空 = 没问过 | `src/change/plan_audit_tests.rs` |
| `all_self_decided_entries_pass` | 全 `[自决]` 不触发留痕要求 | `src/change/plan_audit_tests.rs` |
| `unclassified_entry_is_a_fact_not_a_failure` | 未分类只进 fact | `src/change/plan_audit_tests.rs` |
| `prose_declaration_is_a_fact_not_a_failure` | 散文声明只进 fact | `src/change/plan_audit_tests.rs` |
| `missing_ambiguity_block_is_a_fact_not_a_failure` | 歧义块缺失只进 fact | `src/change/plan_audit_tests.rs` |
| `last_ambiguity_heading_wins` | 同名标题取最后一个 | `src/change/plan_audit_tests.rs` |
| `review_instruction_carries_verdict_facts_and_new_criteria` | 指令含裁决门事实与三个新审点 | `src/change/plan_audit_tests.rs` |
| `decision_ledger_registers_d024_and_annotates_d009` | 决策台账 D024 只登记一次且 D009 带旁注 | `src/change/plan_audit_tests.rs` |
| `agents_change_doc_matches_the_implemented_contract` | 文档口径守卫：载体三字面量 + 禁默认放行 + ask 包 + 两审点 | `src/change/plan_audit_tests.rs` |

测试夹具：`repo()` 建临时仓并写两个源文件（`src/change/plan_audit.rs:453`）；`good_plan()` 六块齐全的计划文本（`src/change/plan_audit.rs:461`）；`plan_with_ambiguity(items)` 带「歧义」小节的计划（`src/change/plan_audit_tests.rs`）。后两个用例读仓内文档做守卫（决策台账、agents-change 文档），依赖工作目录由 `CARGO_MANIFEST_DIR` 定位——**在仓库外跑测试也能读到**，但要求这两份文档存在。

## 测试运行与环境

- **工具链前提**：edition 2024，需 Rust 1.85+。多 toolchain 且无默认值时显式指定：
  ```bash
  RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable cargo --version
  ```
- **依赖安装**：仅 `tempfile`（dev-dependency，`Cargo.toml:27`）；不需要 git、端口、外部服务或网络。
- **隔离**：`repo()` 用 `tempfile` 临时目录，所有路径判存在都落在临时仓；两个文档守卫用例只读仓内 markdown，不写。
- **命令**（🟢 CONFIRMED，本单元实跑：`cargo test --lib change::plan_audit`）：
  ```bash
  CARGO_TARGET_DIR=$(mktemp -d)/target cargo test --lib change::plan_audit
  ```
  结果：19 passed、0 failed，退出码 0；构建产物写在仓外临时目录，仓内 target 目录未被写入。
- **不得绕过**：19 个用例里有 11 个是「错实现形态」守卫（`src/change/plan_audit_tests.rs` 起的注释都写明「本用例必须变红」）；把 `audit` 或 `audit_verdicts` 的对应检查注释掉，这些用例立刻转红。
- **不适用声明**：无服务、无端口、无健康检查（N/A —— 库内纯函数模块）；无运行时配置项（N/A —— 不读环境变量与配置文件）；无种子数据（N/A —— 输入是计划文本，测试自造）；公开接口清单即本文「公开成员」表（N/A —— 无 HTTP/CLI 端点，CLI 侧由 `devforge` 子命令间接使用）。

## 相关文档

- 上下游：[src/change/session.rs](./session.rs.md)（生产调用点：裁决门 → 体检 → 审查指令）、[src/change/orchestrator.rs](./orchestrator.rs.md)（trait 默认 `plan_facts`）、[src/config/manifest.rs](../config/manifest.rs.md)（提示词双向钉住）、[src/change/mod.rs](./mod.rs.md)（转发导出）
- 同族：[src/change/boundary.rs](./boundary.rs.md)（另一条机械门：边界越界）、[src/change/testing_gate.rs](./testing_gate.rs.md)（验收对账）
- 缺口：`_meta/gaps.md`（路径迁移表与 lint 的 `find_existing` 各自维护、path:line 不校验行号范围、`looks_like_file` 扩展名白名单偏窄等，本单元登记）
