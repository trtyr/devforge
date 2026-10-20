# change · 路线图

> 顺序按依赖排：Phase 0（门 0）→ A（plan）→ B（coding 门）→ C（testing 门）→ D（收尾）。
> 每条都写验收，没验收的不算做完。

## Phase 0 · 门 0 与基线

- [ ] **(T000) 共用前置：无标记重推有界**——coding/testing 两个 loop 各加计数与常量，连续 3 次无标记 → 第 4 次判基础设施故障；反向测试覆盖。
  - 验收：连续 4 次不发标记的 mock 会必须报错（非无限循环）；既有零硬限制测试不受影响。
  - 影响：005 decisions/015、007 decisions/D013；与 change 同步生效。

- [ ] **(T001) 基线建立**——无 `.git` → `git init` + **全量提交**（提交信息固定前缀）；有 `.git` → 记录 HEAD。
  - 验收：一个新 clone 的裸目录跑完门 0 后 `git log --oneline` 有且仅有一条 `devforge: baseline`；基线 sha 落 `baseline.txt`。
- [ ] **(T002) `doc_level` 开关**——`none`（默认）/ `essential` / `complete`；`essential` 时先跑 doc-from-zero 轻量档。
  - 验收：`doc_level=none` 时零文档动作、直接进 plan；`essential` 时产出 `_meta/inventory.json` 后进 plan。
- [ ] **(T003) 黑板与 change-id**——生成 `change-<date>-<slug>`，建 `docs/plantree/<change-id>/`。
  - 验收：重复触发同一任务不生成第二个 change-id（幂等）。

## Phase A · plan（本次的重点）

- [ ] **(T004) `change-plan-agent` profile**——deepseek-flash + codegraph MCP + `plan-tree` 技能；与 `plan-agent` 平行不干扰。
- [ ] **(T005) `change-plan-review` profile**——grok-4.7 + codegraph（核调用关系）+ `plan-tree`；与 `plan-review` 平行。
- [ ] **(T006) 提示词：六块产出**——需求复述/影响面/三张表/边界/**验收标准**/风险；影响面每条必须 `path:line`；**不写实施顺序**（归 coding）、**不写回归命令**（归 coding/testing）、不拿测试结果当证据。
  - 验收：提示词锚点单测（六块标题 + codegraph-first + 白名单 + 验收标准 + 不排顺序/不写命令）。
- [ ] **(T007) 审查判据（只审计划）**——影响面无证据 = FAIL；影响面漏掉需求涉及的入口/调用方 = FAIL；验收条目不可观测 = FAIL；回归命令**只读不跑**，判「写得够不够具体」。
  - 验收：三条反向用例各一条单测（缺证据 / 漏调用方 / 验收条目写“功能正常”）。
- [ ] **(T007b) 引擎侧计划审计**——`path:line` 存在性、白名单路径存在性、「要改的」⊆ 白名单，机械输出注入审查（审查官不重算）。
  - 验收：故意写一个不存在的路径 → 审计里必须逐条列出。
- [ ] **(T008) plan 阶段端到端**——真实小需求跑通 plan，产出六块齐全、审查 PASS。

## Phase B · coding 门

- [ ] **(T008b) `change-coding-agent` / `change-coding-review` profile**——coding 侧与 MVP 同款供给（codegraph + **context7** + 四语言 best-practices + 提示词换 change 纪律）；review 侧 grok-4.7 + codegraph，能跑只读命令。
  - 验收：单测断言两个 profile 的 mcp / model / skills 符合 D016。

- [ ] **(T009) 越界二分与往返**——coding 完成后引擎算 diff 对白名单：全内 → testing；越界且 agent 已 `<route>plan</route>` + `<impact-gap>`（文件/理由/证据）→ 打回 plan 补边界（`change-plan-review` 只审增量，仍不看代码）→ 重进 coding；越界无申报 → FAIL（范围膨胀）。
  - 验收：两个反向用例各一条单测——故意改白名单外且不申报 → FAIL；故意改白名单外但带 impact-gap → 回 plan 且白名单被追加。
- [ ] **(T013) change 编排器**——不要照搬 `run_coding`：它有基线、有 plan 产出、允许 coding → plan 的**反向回退**；往返计数上限 3，每次写 `docs/plantree/changes/<change-id>/history.md`。
  - 验收：单测能重现「coding → route plan → 改边界 → 重进 coding」全回路；第 4 次往返必须停下并报错。
- [ ] **(T010) 回归前置（coding）**——coding 动手前自己判断怎么跑测试（README/CI/包管理器），跑一遍并落 `evidence/baseline-test.log`；红就不许动。
  - 验收：既有测试红 → coding 阶段直接失败，错误信息含自己跑的命令与失败摘要。
- [ ] **(T010b) 实施步骤与按步提交**——coding 先产 `impl-steps.md`（每步：文件/验收标准/怎么验）再执行，每步一提交。
  - 验收：一个至少三步的变更，提交数与 impl-steps 步骤数一致，且每条提交信息含步骤号。
- [ ] **(T010c) 越界二分实现**——引擎比对 diff 与白名单，无申报 → FAIL；带 `<impact-gap>` → 回 plan 补边界。
  - 验收：两个反向用例各一条单测（上述两种情形）。

## Phase C · testing 门

- [ ] **(T011) 回归 + 变更 e2e（testing）**——test agent 先跑全套回归（`evidence/regression.log`），红则直接 `<route>coding</route>`；再为变更点写 e2e，输出验收对照表（每条验收标准 → 测试 `path:line`）。
  - 验收：既有套件人为打断后 run 必须 FAIL 并回注 coding；验收标准漏一条 → FAIL。
- [ ] **(T011b) 续跑检查点**——`changes/<id>/state.json` 由引擎写（阶段/基线/往返/已完成步骤与验收项/审查轮次），断了能从断点续。
  - 验收：在 testing 中途模拟进程退出，重跑能从 testing 继续，往返计数不丢。

## Phase D · 收尾

- [ ] **(T012) 变更报告（无 Agent）**——引擎写 `changes/<id>/report.md`：墙钟、diff 统计、审查轮次、往返次数、用量（会话账汇总）、验收对账结果。
  - 验收：故意漏一条验收标准 → 报告里能看出来（对账未过）。

## 里程碑

- **M0**：门 0 可用（基线 + doc_level + change-id）
- **M1**：plan 阶段端到端跑通，六块齐全且审查可判
- **M2**：白名单机械校验 + 回归前置生效（边界外改动被真拦住）
- **M3**：一次真实变更端到端（plan→coding→testing→报告），证据齐全

## Deferred

- 多变更并行（沿 MVP 串行口径）
- 需求来自 issue 系统（先自然语言，Q001）
- 自动语义影响传播（那是 `doc-from-zero` 的 D015 增量模式，等它先落地）
