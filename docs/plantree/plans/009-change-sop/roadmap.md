# change · 路线图

> 顺序按依赖排：Phase 0（门 0）→ A（plan）→ B（coding 门）→ C（testing 门）→ D（收尾）。
> 每条都写验收，没验收的不算做完。

## Phase 0 · 门 0 与基线

- [x] **(T000) 共用前置：无标记重推有界**——coding/testing 两个 loop 各加计数与常量，连续 3 次无标记 → 第 4 次判**质量失败**（`DevforgeError::Quality`，不触发自动重启，plan 010 D004）；反向测试覆盖。
  - 验收：连续 4 次不发标记的 mock 会必须报错（非无限循环）；既有零硬限制测试不受影响。
  - **证据**：commit `0a55ea3`（`MAX_CONTINUE_NUDGES` / `MAX_TEST_CONTINUE_NUDGES` = 3，两条 `repeated_missing_marker_fails_fast`）。
  - 影响：005 decisions/015、007 decisions/D013；与 change 同步生效。

- [x] **(T001) 基线建立**——无 `.git` → `git init` + **全量提交**（提交信息固定前缀）；有 `.git` → 记录 HEAD。
  - 验收：一个新 clone 的裸目录跑完门 0 后 `git log --oneline` 有且仅有一条 `devforge: baseline`；基线 sha 落 `baseline.txt`。
  - **证据**：commit `99104ac`（`change::gate0::ensure_baseline`，5 单测）。
- [x] **(T002) `doc_level` 开关**——`none`（默认）/ `essential` / `complete`；`essential` 时先跑 doc-from-zero 轻量档。
  - 验收：`doc_level=none` 时零文档动作、直接进 plan；`essential` 时产出 `_meta/inventory.json` 后进 plan。
  - **证据**：commit `34e5461`（`change::docs::apply`：none 零动作 / essential 产清单+词表 / complete 返 `FullChainRequired`）。
- [x] **(T003) 黑板与 change-id**——生成 `change-<date>-<slug>`，建 `docs/plantree/<change-id>/`。
  - 验收：重复触发同一任务不生成第二个 change-id（幂等）。
  - **证据**：commit `684aae3`（`change::change_id`，marker 幂等，目录被清也不换号）。

## Phase A · plan（本次的重点）
- [x] **(T004) `change-plan-agent` profile**——deepseek-flash + codegraph MCP + `plan-tree` 技能；与 `plan-agent` 平行不干扰。
  - **证据**：commit `60a9d48`（profile 定义 + 六块提示词；MVP 规划双 Agent 断言不变）。
- [x] **(T005) `change-plan-review` profile**——grok-4.7 + codegraph（核调用关系）+ `plan-tree`；与 `plan-review` 平行。
  - **证据**：同上 `60a9d48`。
- [x] **(T006) 提示词：六块产出**——需求复述/影响面/三张表/边界/**验收标准**/风险；影响面每条必须 `path:line`；**不写实施顺序**（归 coding）、**不写回归命令**（归 coding/testing）、不拿测试结果当证据。
  - 验收：提示词锚点单测（六块标题 + codegraph-first + 白名单 + 验收标准 + 不排顺序/不写命令）。
  - **证据**：commit `60a9d48` + `ff16c6b`（`change_plan_prompts_carry_six_blocks_and_forbidden_items`）。
- [x] **(T007) 审查判据（只审计划）**——影响面无证据 = FAIL；影响面漏掉需求涉及的入口/调用方 = FAIL；验收条目不可观测 = FAIL；回归命令**只读不跑**，判「写得够不够具体」。
  - 验收：反向用例单测（缺证据 / 验收条目写“功能正常”）；「漏调用方」靠 codegraph 判，属于审查官语义面（提示词锚点断言）。
  - **证据**：commit `ff16c6b`（审查指令携带五条判据 + `nonexistent_path_is_listed` / `vague_acceptance_is_flagged`）。
- [x] **(T007b) 引擎侧计划审计**——`path:line` 存在性、白名单路径存在性、「要改的」⊆ 白名单，机械输出注入审查（审查官不重算）。
  - 验收：故意写一个不存在的路径 → 审计里必须逐条列出。
  - **证据**：commit `ff16c6b`（`change::plan_audit::audit`，四项机械检查）。
- [ ] **(T008) plan 阶段端到端** — **已移出代码阶段**（用户 2026-10-20：代码阶段只写单测，不跑端到端），与 M1 一起入验收阶段。

## Phase B · coding 门

- [x] **(T008b) `change-coding-agent` / `change-coding-review` profile**——coding 侧与 MVP 同款供给（codegraph + **context7** + 四语言 best-practices + 提示词换 change 纪律）；review 侧 grok-4.7 + codegraph，能跑只读命令。
  - 验收：单测断言两个 profile 的 mcp / model / skills 符合 D016。
  - **证据**：commit `19bfc9f`（13 profile 断言同步）。

- [x] **(T009) 越界二分与往返**——coding 完成后引擎算 diff 对白名单：全内 → testing；越界且 agent 已 `<route>plan</route>` + `<impact-gap>`（文件/理由/证据）→ 打回 plan 补边界（`change-plan-review` 只审增量，仍不看代码）→ 重进 coding；越界无申报 → FAIL（范围膨胀）。
  - 验收：两个反向用例各一条单测——故意改白名单外且不申报 → FAIL；故意改白名单外但带 impact-gap → 回 plan 且白名单被追加。
  - **证据**：commit `583a725`（`change::boundary`） + `57a56aa`（编排层全回路）。
- [x] **(T013) change 编排器**——不要照搬 `run_coding`：它有基线、有 plan 产出、允许 coding → plan 的**反向回退**；往返计数上限 3。
  - 验收：单测能重现「coding → route plan → 改边界 → 重进 coding」全回路；第 4 次往返必须停下并报错。
  - **证据**：commit `57a56aa`（`change::orchestrator`，ChangeSession 注入；全回路与第四次拒绝均有单测）；`history.md` 逐次落盘由 `change::history` 实现（commit `726f650`）。
- [x] **(T010) 回归前置（coding）**——coding 动手前自己判断怎么跑测试（README/CI/包管理器），跑一遍并落 `evidence/baseline-test.log`；红就不许动。
  - 验收：既有测试红 → coding 阶段直接失败，错误信息含自己跑的命令与失败摘要。
  - **证据**：commit `7739a61`（`change::coding_gate::check_baseline_log`）。
- [x] **(T010b) 实施步骤与按步提交**——coding 先产 `impl-steps.md`（每步：文件/验收标准/怎么验）再执行，每步一提交。
  - 验收：一个至少三步的变更，提交数与 impl-steps 步骤数一致，且每条提交信息含步骤号。
  - **证据**：commit `7739a61`（`check_step_commits` 步骤号一一对账）。
- [x] **(T010c) 越界二分实现**——引擎比对 diff 与白名单，无申报 → FAIL；带 `<impact-gap>` → 回 plan 补边界。
  - 验收：两个反向用例各一条单测（上述两种情形）。
  - **证据**：commit `583a725`。

## Phase C · testing 门

- [x] **(T011) 回归 + 变更 e2e（testing）**——test agent 先跑全套回归（`evidence/regression.log`），红则直接 `<route>coding</route>`；再为变更点写 e2e，输出验收对照表（每条验收标准 → 测试 `path:line`）。
  - 验收：既有套件人为打断后 run 必须 FAIL 并回注 coding；验收标准漏一条 → FAIL。
  - **证据**：commit `c862b5f`（`change::testing_gate`：退出码矛盾即 FAIL、缺一条即零覆盖、回注不带定位）；双 profile 见 `fdfe365`（`change-test-agent` / `change-test-review`，黑盒不给 codegraph）。
- [x] **(T011b) 续跑检查点**——checkpoint 住黑板 `plans/<p>/<t>/change/state.json`（引擎写，010 D022a/D001），存阶段/基线/往返/已完成步骤与验收项/审查轮次；断了能从断点续。
  - 验收：在 testing 中途模拟进程退出，重跑能从 testing 继续，往返计数不丢。
  - **证据**：commit `8a2c222`（`change::state` + `resume_keeps_roundtrips_across_sessions`）。

## Phase D · 收尾

- [x] **(T012) 变更报告（无 Agent）**——引擎写 `changes/<id>/report.md`：墙钟、diff 统计、审查轮次、往返次数、用量（会话账汇总）、验收对账结果。
  - 验收：故意漏一条验收标准 → 报告里能看出来（对账未过）。
  - **证据**：commit `8a2c222`（`change::report`，未对账项逐条列出）。

## Phase E · 接线到 server（2026-10-20）

> 本阶段只做装配与可测性：真实 pi 会话已接上，但**验证仍是单测**（mock session / mock client），
> 真实端到端归验收阶段（T008/M3）。

- [x] **(T014) 注册 SOP + 引擎分发**——`SopRegistry` 新增 `change`，`BuiltinRpcEngine::execute` 分发到 `run_change`。
  - 验收：单测证明 change 不再落「未接线」分支（卡在 profile 门卫即证明已路由）；ghost 仍拒。
  - **证据**：commit `aaf7029`（`dispatch_routes_change_to_its_engine` / `dispatch_rejects_unwired_sop`）。
- [x] **(T015) 门 0 预检**——`change::prepare`：change-id/黑板目录 → doc_level（none/essential 落地，complete 拒跑并指向 doc-from-zero）→ 基线（无 `.git` 则 init+全量提交）→ baseline.txt 与黑板骨架。
  - 验收：裸仓跑完门 0 后有且仅有一条 baseline 提交且 baseline.txt==sha；重复启动复用 change-id 且不重建仓库。
  - **证据**：commit `aaf7029`（`src/change/prepare.rs`，7 单测）。
- [x] **(T016) 真实 ChangeSession + 收尾**——六 profile 装配（三个长驻干活 + 三个每轮新审查）；plan 审查注入机械审计；coding 先验基线绿/步骤对账/取 diff；testing 先看 route 再验回归与对账表；`finalize` 从会话账汇总量、写 `report.md`。
  - 验收：session 层八条假会话单测；进程内全链（plan→coding→testing→report）产出 phase=done 与报告；`start` 对 change 续跑/fresh 与 010 同语义。
  - **证据**：commit `454a3e4`（session）+ `d0f0984`（finalize/全链）+ `0763201`（start 续跑）；**审计后修正** `65db146`：
    - coding 拆两轮：**基线取证轮**（只跑既有测试写 `evidence/baseline-test.log`，此时不改码）→ 引擎验红→绿了才发**改码轮**指令（修“门在生产者之前”的自锁）；
    - coding review / test review 两个实例真正 `open()/review()`（之前只装配未调用）；FAIL → Quality；
    - 越界申报 `<route>plan</route>` 在 `require_marker` **之前**判定（合规申报不打标记，旧顺序会误判）；
    - `review_rounds` 由会话上报经编排器写入 state 与报告（不再恒为 0）。
  - **审计第二轮修正** `b79e785`：
    - `change_dir` 改回**目标仓** `docs/plantree/changes/<id>`（与提示词/D003 一致，`baseline.txt` 与 `change-plan.md` 同处）；
    - 黑板骨架与 session 读写统一到 `plans/<p>/<t>/change/`（补 `testing/reports`），agent cwd 也设在这里；`requirement.md` 留在任务根；
    - 抽 `build_ctx` 并新增单测断言「计划路径在目标仓 / 证据根在 SOP 级黑板 / 骨架目录存在」；
    - plan 审查轮次经 `plan_reviews` 计入报告（plan+coding+testing 三轮合计有单测）。
- **运行契约（新增，调用方须知）**：
  - 开跑前把需求写进黑板 `plans/<project>/<task>/requirement.md`（没有就拒跑，Quality）；
  - 文档档位用环境变量 `DEVFORGE_DOC_LEVEL=none|essential|complete`（默认 `none`）；
  - 日期可用 `DEVFORGE_TODAY=YYYY-MM-DD` 钉住（CI / 复现用）。

## 里程碑

- **M0**：门 0 可用（基线 + doc_level + change-id）— ✅ 单测级（T001–T003）
- **M1**：plan 阶段端到端跑通，六块齐全且审查可判 — 代码侧就绪（T004–T007b），**端到端归验收阶段**（T008）
- **M2**：白名单机械校验 + 回归前置生效（边界外改动被真拦住）— ✅ 单测级（T009/T010/T010b/T010c）
- **M3**：一次真实变更端到端（plan→coding→testing→报告），证据齐全 — 编排器与报告就绪（T011–T013），**真实跑归验收阶段**

## Deferred

- 多变更并行（沿 MVP 串行口径）
- 需求来自 issue 系统（先自然语言，Q001）
- 自动语义影响传播（那是 `doc-from-zero` 的 D015 增量模式，等它先落地）
