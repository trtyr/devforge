# change · 路线图

> 顺序按依赖排：Phase 0（门 0）→ A（plan）→ B（coding 门）→ C（testing 门）→ D（收尾）。
> 每条都写验收，没验收的不算做完。

## Phase 0 · 门 0 与基线

- [ ] **(T001) 基线建立**——无 `.git` → `git init` + **全量提交**（提交信息固定前缀）；有 `.git` → 记录 HEAD。
  - 验收：一个新 clone 的裸目录跑完门 0 后 `git log --oneline` 有且仅有一条 `devforge: baseline`；基线 sha 落 `baseline.txt`。
- [ ] **(T002) `doc_level` 开关**——`none`（默认）/ `essential` / `complete`；`essential` 时先跑 doc-from-zero 轻量档。
  - 验收：`doc_level=none` 时零文档动作、直接进 plan；`essential` 时产出 `_meta/inventory.json` 后进 plan。
- [ ] **(T003) 黑板与 change-id**——生成 `change-<date>-<slug>`，建 `docs/plantree/<change-id>/`。
  - 验收：重复触发同一任务不生成第二个 change-id（幂等）。

## Phase A · plan（本次的重点）

- [ ] **(T004) `change-plan` profile**——deepseek-flash + codegraph MCP + plan 系列技能；与 `plan-agent` 平行不干扰。
- [ ] **(T005) `change-review` profile**——plan-review 基础上加 codegraph（用来核对影响面证据是否真实存在）。
- [ ] **(T006) 提示词：六块产出**——需求复述/影响面/三张表/边界/验收命令/风险；影响面每条必须 `path:line`。
  - 验收：提示词锚点单测（六块标题 + codegraph-first + 白名单 + 验收命令）。
- [ ] **(T007) 审查判据**——影响面无 codegraph 证据 = FAIL；边界与影响面不自洽 = FAIL；验收命令审查官必须亲自跑一遍。
  - 验收：三条反向用例各有一条单测（故意缺证据 / 边界漏文件 / 命令跑不过 → 必须 FAIL）。
- [ ] **(T008) plan 阶段端到端**——真实小需求跑通 plan，产出六块齐全、审查 PASS。

## Phase B · coding 门

- [ ] **(T009) 越界二分与往返**——coding 完成后引擎算 diff 对白名单：全内 → testing；越界且 agent 已 `<route>plan</route>` + `<impact-gap>`（文件/理由/证据）→ 打回 plan 补边界（增量过审后重进 coding）；越界无申报 → FAIL（范围膨胀）。
  - 验收：两个反向用例各一条单测——故意改白名单外且不申报 → FAIL；故意改白名单外但带 impact-gap → 回 plan 且白名单被追加。
- [ ] **(T013) change 状态机编排器**——不照搬 MVP 单向流水：plan ⇄ coding ⇄ testing 带路由，往返计数上限 3，每次往返写 `docs/plantree/changes/<change-id>/history.md`。
  - 验收：单测能重现「coding → route plan → 改边界 → 重进 coding」全回路；第 4 次往返必须停下并报错。
- [ ] **(T010) 回归前置**——coding 动手前先跑既有测试（命令来自计划），不绿不许改；证据落黑板。
  - 验收：既有测试红 → coding 阶段直接失败，且错误信息含测试命令与失败摘要。

## Phase C · testing 门

- [ ] **(T011) 回归 + 新增**——测试阶段先复跑既有套件，再补变更测试；回归失败 = 阻塞。
  - 验收：既有套件人为打断后 run 必须 FAIL。

## Phase D · 收尾

- [ ] **(T012) 文档同步 + 变更报告**——`docs-update` 或 `docs-sync` 对齐文档，`doc-lint` 必须 0 违规；报告含 diff 统计、回归结果、影响面 vs 实际改动对照。
  - 验收：故意让文档与改动不一致，收尾门必须报出来。

## 里程碑

- **M0**：门 0 可用（基线 + doc_level + change-id）
- **M1**：plan 阶段端到端跑通，六块齐全且审查可判
- **M2**：白名单机械校验 + 回归前置生效（边界外改动被真拦住）
- **M3**：一次真实变更端到端（plan→coding→testing→收尾），证据齐全

## Deferred

- 多变更并行（沿 MVP 串行口径）
- 需求来自 issue 系统（先自然语言，Q001）
- 自动语义影响传播（那是 `doc-from-zero` 的 D015 增量模式，等它先落地）
