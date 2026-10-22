# 变更计划 · change-20261021-2026-10-21-c3-e2e-textkit-change-run-tes

目标仓：`/home/trtyr/code-work/devforge`
基线：`5d7bfd023f50fd6463d796cad5bd9c479f03d7c3`（`docs/plantree/changes/change-20261021-2026-10-21-c3-e2e-textkit-change-run-tes/baseline.txt`）
上一场：`change-20261021-change-sop-path-line-src-config-manifest`（已 Settled，HEAD=`5d7bfd0`）
文档档位：none（计划阶段照常读已有文档；不跑文档流水线）
本场是**自举第三场·单靶**：只治「判负不可诊断 → 回注黑箱」，**不做**死亡诊断、不做续跑加固（后续场次）。

---

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

2026-10-21 c3 场自举，内层 textkit change run（夹具 `tests/e2e/test_26_change_e2e.py`，昨晚同一夹具 31 PASS/0 FAIL Settled 过）今天终态 Failed。链条是：

- 内层引擎日志（隔离 HOME `/var/tmp/devforge-e2e-4qg9f51z/.devforge/logs/devforge.log.jsonl`）WARN「测试失败回注 coding」，消息正文是「测试阶段未通过，按 `<route>coding</route>` 回去处理。请自己定位原因（本消息不提供文件位置）。失败摘要：- 测试阶段判定失败（未给出 issues 明细）」；
- 随后 SOP 质量失败（**不重试**）：测试未通过，已回注 coding…… 终态 Failed；
- 内层黑板 `textkit/c1/change/state.json`：`phase=coding`、`review_rounds=1`。

要修的是一件事：**判负必须可诊断**。拆成三条：

1. **协议硬化**：任何审查角色的 FAIL 判定必须携带**非空** `<issues>`；无 issues 的 FAIL 按**协议违规**处理（要求重审，有界），不许带着黑箱进回注。逐个排查其余审查角色（coding-review / plan-review / doc-review）有没有同一条「FAIL 不附原因」的通道，同类同修。
2. **回注质量**：route 回注消息携带的失败摘要必须含 issues **逐条原文**（保留定位），让 coding 方能定位；「请自己定位原因」这类零信息话术不许再出现。
3. **证明**：单测构造「无 issues 的 FAIL」→ 引擎按设计拒收/重审；e2e 或集成测试证明回注消息含 issues 原文。验收标准逐条可机械对账。

明确不做：不引入新的外部依赖；遵守 `assets/skills/coding-discipline/SKILL.md`（本变更只改引擎与提示词，不改纪律本身）；范围仅此一靶；不写实施顺序与回归命令。

**顺带登记（本靶不修）**：run 级 `attempt=2` 的启动期基础设施重试，在连续三场自举的 boot 阶段都出现过（10-20 深夜一场、10-21 c2/c3 两场），成因未查 → 值得单开后续靶子，本计划只在 §6 的 R12 记一句。

### 1.2 根因定位（代码级）

**「判负不附原因」共有 5 条通道，逐个排查结果**（第 5 条已合规，本次照抄它的口径）：

| # | 通道 | 位置 | 现状 |
| --- | --- | --- | --- |
| 1 | change SOP 的 plan-review / coding-review / test-review | `src/sop/coding_protocol.rs:94` `parse_review_report` | FAIL 分支 `:110-117`（缺 next）与主返回 `:147-153` 都允许 issues 为空（`extract_lines` 找不到块 → 空 Vec），且 `protocol_violation: false` → **判负照常生效、零原因** |
| 2 | MVP testing SOP 的 test-review | `src/sop/testing_protocol.rs:174` `parse_test_review_report` | `:192-202` 与 `:232-243` 同样允许空 issues |
| 3 | MVP 规划 SOP 的 plan-review | `src/sop/output.rs:40` `parse_verdict` | `:52-55` 的 FAIL 分支调 `extract_issues`（`:72-86`）→ 找不到 `<issues>` 块就返回**空清单** `Verdict::Fail { issues: vec![] }` → `src/sop/planning.rs:118-131` 回注 plan agent 的消息只剩前缀、**零条问题**（与本次同病）。注意 `:63-69` 的 `conservative_fail` **已**返回非空 issues——协议违例走的是它，空清单只出现在「协议块齐全但 `<issues>` 缺失」这条路 |
| 4 | doc-fix 通道的 doc-review | `src/sop/testing_protocol.rs:272` `parse_doc_review_report` | `:288-293` 的 FAIL 分支（`protocol_violation: false` 在 `:292`）不判违例 → `src/sop/doc_fix.rs:113-124` 回注 doc-fix 的消息零条问题 |
| 5 | doc-from-zero 通道的 doc-review | `src/sop/doc_protocol.rs:82` `parse_doc_review` | **已合规**：`:116-122` 对「FAIL 无 issues」判 `protocol_violation` 并把理由作为唯一 issue —— 本次照抄这个形状 |

**本次实证日志里那句占位文案的真正出处不是 test-review，是 test agent 的 route**：`src/change/session.rs:355-360` —— test agent 输出 `<route>coding</route>` 但没附 `<issues>` → 引擎造一句「测试阶段判定失败（未给出 issues 明细）」占位 → `src/change/orchestrator.rs:261` 拼成回注消息 → 零信息进 coding → coding 无从下手 → 终态黑箱。

**回注消息本身还在二次阉割**：`src/change/testing_gate.rs:115` `route_back_message` 写死「请自己定位原因（本消息不提供文件位置）」，并用 `:126` `strip_locations` 把**所有** `path:line` 剥成「`<位置略>`」——**连失败测试自己文件的 `tests/change_ask_ring.rs:294` 也一起剥**。即：即便 issues 非空，coding 拿到的也只是被剥过的摘要。

**审查侧协议违例在 change SOP 没有重审通道**：`src/change/session.rs:243`（plan-review）/ `:332`（coding-review）/ `:400`（test-review）遇 `protocol_violation` 直接 `Err(Quality)` 终止；MVP 侧则是「有界重审后 Quality」（`src/sop/coding.rs:81`+`:153-165`、`src/sop/testing.rs:75`+`:192-210`、`src/sop/doc_protocol.rs:13`+`src/sop/doc_fix.rs:97-112`）—— 两种口径并存。

**提示词侧一条要求都没有**：`src/config/manifest.rs:536`（收尾块 `:561-573`）/`:634`（收尾块 `:650-660`）/`:701`（收尾块 `:715-723`）三个 change review 提示词只给了协议块形状（plan-review 那一处 `:573` 只说「FAIL 时逐条列清」，没写「空 issues = 协议违例」）；`:664` 的 change-test-agent 提示词 `:687` 还写着「回注只写失败现象与摘要，不猜文件位置——定位是 coding 的事」；`src/change/session.rs:154` `testing_kickoff` 只说「并附失败摘要」。

**运行时审查指令是第二层提示词，一样没写**（role prompt 之外的每轮 kickoff 文案）——共 7 处收尾块：change 侧 `src/change/plan_audit.rs:409`（plan-review，被 `src/change/session.rs:241` 调用）、`src/change/session.rs:109`（coding-review）、`src/change/session.rs:128`（test-review）；MVP 侧 `src/sop/planning.rs:31`（plan-review）、`src/sop/coding.rs:47`（coding-review）、`src/sop/testing.rs:43`（test-review）、`src/sop/doc_fix.rs:39`（doc-review）。它们都只说「输出协议块（… + issues）」，**没有一句说「FAIL 必须带非空 issues」** → 审查方不知道空清单会被当协议违例，模型就可能继续发空清单。

**计数口径（写死在这里，§5 的验收标准按此逐条对账）**：

- **审查重审**（Q2）：违例上限 3（`src/sop/coding.rs:81` `MAX_PROTOCOL_VIOLATIONS`；计数在 `:157`，终止判在 `:158`）→ **第 4 次违例才终止**；**轮次 = 尝试次数（含成功那一次）** → 「重审 3 次后才通过」= 轮次 **4**（1 次初审 + 3 次重审）。
- **route 追补**（Q3）：**3 次追补 = 总共 4 次回复**（1 次初次 + 3 次追补），**第 4 次仍不带 `<issues>` 才终止** → 「连续 3 次不带就死」是错的写法。
- 两者的上限语义一致（3 = 允许的失败次数，第 4 次终止），但**计数对象不同**：审查数的是「审查尝试次数」，追补数的是「追补次数」（不含初次回复）。

### 1.3 歧义

- [必须裁决] **Q1 · 回注给 coding 的失败摘要要不要携带 path:line 定位** —— 需求 2（issues 逐条原文、让 coding 方能定位）与既有 D018/D019 + `src/change/testing_gate.rs:126` 的剥定位规则**直接冲突**；且现状把失败测试自己文件的行号也剥掉了。影响验收口径（回注消息里到底有什么）。
  备选：A 携带 issues 原文含 path:line／B 仍剥离但补「期望 vs 实际 + 复现命令」／C 最小改动只治黑箱；倾向 A（本次实证的死因正是「coding 拿不到任何抓手」）。
  用户裁决：A 保留行号（推荐）
- [必须裁决] **Q2 · change SOP 里「审查输出协议违例」的处置** —— 现状是立即 Quality 终止（`src/change/session.rs:243`/`:332`/`:400`），MVP 侧是有界重审（`src/sop/coding.rs:81`、`src/sop/testing.rs:75`）；需求写「走既有 MAX_PROTOCOL_VIOLATIONS 语义/要求重审」，两种读法都通。影响验收口径（run 会不会再给模型一次机会）。
  备选：A 有界重审（上限 3）后终止／B 维持立即终止／C 只重审一次即终止；倾向 A（与 MVP 侧同口径，模型第一次不听话不该直接判死）。
  用户裁决：重审×3（推荐）
- [必须裁决] **Q3 · test agent 输出 `<route>coding</route>` 却没附 `<issues>`（本次实证的实际黑箱通道，`src/change/session.rs:355-360`）时引擎怎么办** —— 影响验收口径（拒收重问 / 直接死 / 继续造占位文案，三种 run 终态完全不同）。
  备选：A 拒收 + 追补 ×3／B 直接 Quality 终止／C 维持现状（造占位文案）；倾向 A（回注消息必须永远带 issues 原文，这是需求 2 的硬要求）。
  用户裁决：拒收+追补×3（推荐）
- [自决] **Q4 · 「同类同修」覆盖到哪些路径** —— 需求 1 点名 coding-review / plan-review / doc-review 三个**角色**但没点**路径**；逐个排查后有 5 条通道（§1.2 表）。用户裁决覆盖 change SOP 三角色 + MVP 规划路径的 plan-review；在此之上**自决追加** doc-fix 通道的 doc-review（`src/sop/testing_protocol.rs:292`）一并修——理由：需求 1 逐字点名 doc-review 角色要「同类同修」，doc-from-zero 通道已合规而 doc-fix 通道是同一角色的另一条通道，留着就是同一个洞；改动是同族解析器里的一处判定 + 一条单测，代价可忽略。若审查官认定超范围，撤掉这一条即可（其余条目不受影响）。
- [自决] **Q5 · 「回注带行号」要不要连带放开 test agent 的「不猜文件位置」限制** —— D019（testing 不给 codegraph）不变；但 test agent 知道**自己写的测试文件**的行号与断言消息（那是事实不是猜测），现行提示词 `src/config/manifest.rs:687` 一律不许写位置。默认：**只放开「失败测试自身的位置」**（测试文件:行、断言消息、期望 vs 实际），仍禁止去猜产品源码位置。理由：回注要可诊断就得有能跳过去的抓手；纯行为描述在没有行号时仍要 coding 全仓搜。
- [自决] **Q6 · 「FAIL 无 issues」在哪一层拒收** —— 解析器层（`parse_review_report` 等直接把「FAIL 无 issues」判成 `protocol_violation`）还是编排层（解析器照旧、编排层再判）。默认：**解析器层**，形状照 `src/sop/doc_protocol.rs:116-122`。理由：解析器是所有调用方（change 三角色 + MVP coding/testing/doc-fix）的唯一收口点，编排层判要改三处。
- [自决] **Q7 · MVP 规划路径（`src/sop/output.rs`）怎么表示协议违例** —— `Verdict` 枚举只有 `Pass` / `Fail { issues }`，没有违例信号。默认：**新增 `Verdict::ProtocolViolation { reason }` 变体**（让类型承担约束），规划循环收到它就换新实例重审（上限 3）且**不打断 plan agent**，超限 Quality。理由：不新增变体就只能用「issues 里塞一句协议说明」去骗 plan agent——那正是本靶要治的「把协议问题当被审方问题回注」。
- [自决] **Q8 · 证明面落在哪** —— 需求 3 允许「e2e 或集成测试」。默认：**确定性证明全部落在仓内 Rust 层**（解析器单测 + `src/change/session.rs` 会话层单测 + `src/change/orchestrator.rs` 编排层集成测试），e2e 侧只加一条**条件式**日志守卫（真链若出现「测试失败回注 coding」WARN，其消息必须含 issues 明细、不得含占位文案/零信息话术；没有该日志行则此守卫不适用）。理由：真链是否回注取决于模型行为，把断言押在模型行为上 = 假红；仓内集成测试是确定性的，且能逐条对账。
- [自决] **Q9 · 零信息话术的清理范围** —— 除 `src/change/testing_gate.rs:117` 的固定文案外，`src/change/session.rs:358`/`:368` 两条占位文案（「测试阶段判定失败（未给出 issues 明细）」「测试阶段判定需求/计划问题（未给出 issues 明细）」）在 Q3 裁决下**不再产生**。默认：**两条占位文案随拒收逻辑一起删**，不留「兜底占位」这条退路。理由：需求 2 明确「零信息话术不许再出现」；留着兜底 = 黑箱以「兜底」形态复活。

---

## 2 影响面

### 2.1 直接命中

- `src/change/session.rs:355-360` —— test agent `<route>coding</route>` 但无 `<issues>` → 造占位文案「测试阶段判定失败（未给出 issues 明细）」。**本次实证日志里那句占位文案的真正出处**（Q3 落点：拒收 + 追补 ×3，占位文案删除）。
- `src/change/session.rs:365-370` —— 同上，`<route>plan</route>` 分支的占位文案「测试阶段判定需求/计划问题（未给出 issues 明细）」。
- `src/change/session.rs:201-215` —— `review_once()`：change SOP 三个审查角色（plan/coding/test-review）共用的审查入口；现在 `protocol_violation` 由上层直接判死（`:243`/`:332`/`:400`），**没有重审通道**（Q2 落点：有界重审 ×3）。
- `src/change/session.rs:184-196` —— `issues_of()`：从 agent 回复里抠 `<issues>` 清单（route 通道的 issues 提取）；Q3 的「这次到底带没带 issues」的判据落在这里。
- `src/change/session.rs:154-171` —— `testing_kickoff()`：「失败时输出 `<route>coding</route>`（计划问题用 `<route>plan</route>`）并附失败摘要」——「附失败摘要」不是硬要求（没写「逐条 `<issues>` 原文」）。
- `src/change/testing_gate.rs:115-124` —— `route_back_message()`：回注消息模板，写死「请自己定位原因（本消息不提供文件位置）」（Q1 落点：删话术）。
- `src/change/testing_gate.rs:126-140` —— `strip_locations()`：把**所有** `path:line` 剥成「`<位置略>`」（含失败测试自己文件的行号）（Q1 落点：删函数；连带 `:209-222` 的既有单测 `route_back_message_carries_no_location` 改写）。
- `src/change/plan_audit.rs:409` —— change 侧 plan-review 的**运行时审查指令**收尾块：「末尾输出协议块：`<stage-verdict>PASS|FAIL</stage-verdict>` + `<next>advance|fix</next>` + `<issues>`（FAIL 时逐条，带 path:line）」——只说形状，**没写「FAIL 必须带非空 issues」**；它被 `src/change/session.rs:241` 的 `review_instruction(...)` 拼进每轮 kickoff → **判负无原因的第一现场**。
- `src/change/session.rs:109` —— change 侧 coding-review 的运行时审查指令收尾块：「… + `<issues>`（FAIL 时带 path:line）」——同样没写「必须非空」。
- `src/change/session.rs:128` —— change 侧 test-review 的运行时审查指令收尾块：「… + `<issues>`」——同样没写「必须非空」。
- `src/sop/planning.rs:31` —— MVP plan-review 的 `REVIEW_INSTRUCTION` 收尾：「不通过：`<verdict>FAIL</verdict>` 加 `<issues>` 清单（每行一条，`- ` 开头）」——没说「必须非空」。
- `src/sop/coding.rs:47` —— MVP coding-review 的 `coding_review_instruction` 收尾：「再在回复末尾输出协议块（stage-verdict / next / issues / notes）」——没说「必须非空」。
- `src/sop/testing.rs:43` —— MVP test-review 的 `testing_review_instruction`（函数头 `:38`）收尾：「…（stage-verdict / next / FAIL 时 reroute + issues / PASS 时 notes）」——没说「必须非空」。
- `src/sop/doc_fix.rs:39` —— MVP doc-review 的 `doc_review_instruction`（函数头 `:34`）收尾：「…（stage-verdict / issues / notes）」——没说「必须非空」。
- `src/change/session.rs:342` / `:409` / `:418` —— **审查轮次的三个生产端**：`run_coding` 的 `reviews: 1`（`:342`）、`run_testing` 的 `reviews: 1`（`:409`）、`plan_reviews()` 写死 `1`（`:418-420`）——Q2 把有界重审扩到 change SOP 三个审查角色（**含 plan-review**）后，这三处都要改成**实际轮次**，否则重审轮次静默少记（plan-review 一场最多少记 3 轮）。
- `src/change/orchestrator.rs:269-275` —— `TestingTurn::RoutePlan` 分支：`:270` 拼回注消息（`route_back_message("plan", &failures)`）、`:274` 抛 Quality（消息里带 `msg`）→ **plan 路由的第二个出口**（coding 分支在 `:255-268`）；`:260` 的行内注释「回注只带摘要（不给 path:line 定位）——测试 agent 读行为不读代码」也是旧口径落点。
- `src/change/testing_gate.rs:112-114` —— `route_back_message()` 的文档注释：「**只给失败摘要，不带文件行号定位**」+「不改变『不给定位』这条硬约束」→ 删掉 `strip_locations`（`:126`）后这两句当场变成「文档与代码不一致」（旧口径落点）。
- `src/change/testing_gate.rs:1-10` —— 模块头注释「回注不带定位」（引 D018/D019）→ 口径变了要同步。
- `src/sop/coding_protocol.rs:94-154` —— `parse_review_report()`：change SOP 的 plan-review（`src/change/session.rs:243`）、coding-review（`:332`）、test-review（`:400`）与 MVP coding SOP（`src/sop/coding.rs:153`）的**共同**判定入口；`:110-117`（FAIL 缺 next 分支）与 `:147-153`（主返回）两处都允许空 issues 且 `protocol_violation: false`。
- `src/sop/testing_protocol.rs:174-244` —— `parse_test_review_report()`：MVP testing SOP 的 test-review 判定入口（`src/sop/testing.rs:192`）；`:192-202` 与 `:232-243` 两处允许空 issues。
- `src/sop/testing_protocol.rs:272-305` —— `parse_doc_review_report()`：doc-fix 通道的 doc-review 判定入口（`src/sop/doc_fix.rs:97`）；`:288-293` 的 FAIL 分支（`protocol_violation: false` 在 `:292`）不判违例（Q4 自决追加项）。
- `src/sop/output.rs:40-58` —— `parse_verdict()`：MVP 规划 SOP 的 plan-review 判定入口（`src/sop/planning.rs:114`）；`:52-55` 的 FAIL 分支经 `extract_issues`（`:72-86`）产出**可能为空**的清单（`protocol_violation` 信号由 `:63-69` 的 `conservative_fail` 承担，与空清单是两条路）；既有单测 `:119 fail_without_issues_block_is_empty_list` **与本靶口径直接冲突**。
- `src/sop/planning.rs:114` / `:118-131` —— 规划循环的审查段：`parse_verdict` 调用在 `:114`，`Verdict::Fail { issues }` 臂在 `:118`，回注 plan agent 的消息组装在 `:120-131`；issues 为空时回注消息只剩前缀（零信息）。Q7 落点（新增违例信号 + 有界重审 + 不打断 plan agent）。
- `src/sop/doc_protocol.rs:116-122` —— doc-from-zero 通道的 doc-review：**已经是**本靶要的口径（FAIL 无 issues → `protocol_violation` + 理由作为唯一 issue）→ 本次照抄的样板，不改。
- `src/config/manifest.rs:687` —— `CHANGE_TEST_AGENT_PROMPT`（常量 `:664`）的纪律条「回注只写失败现象与摘要，不猜文件位置——定位是 coding 的事」：与 Q1 裁决（回注带行号）冲突，且没要求「必须附 `<issues>` 逐条明细」（Q5 落点）。
- `src/config/manifest.rs:701` —— `CHANGE_TEST_REVIEW_PROMPT`（收尾块 `:715-723`）：协议块示例里有 `<issues>`，但没写「FAIL 必须带非空 issues」。
- `src/config/manifest.rs:633` —— `CHANGE_CODING_REVIEW_PROMPT`（文档注释 `:633` / 常量 `:634`，收尾块 `:650-660`）：同上。
- `src/config/manifest.rs:536` —— `CHANGE_PLAN_REVIEW_PROMPT`（收尾块 `:561-573`）：同上——`:573` 只说「PASS 时 `issues` 留空；FAIL 时逐条列清该改什么」，没写「空 issues = 协议违例」。
- `src/config/manifest.rs:926` / `:953` / `:973` —— `builtin()` 里三个 change review agent 与提示词的绑定行（`role_prompt: CHANGE_PLAN_REVIEW_PROMPT` / `CHANGE_CODING_REVIEW_PROMPT` / `CHANGE_TEST_REVIEW_PROMPT`）→ 改提示词常量即改这三个 agent 的系统提示词（生效路径）。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:172` —— 「与 MVP testing 的差别」表第 4 行：「回归失败直接丢回 coding | 测试 Agent 不看代码、**不做定位**；它只负责证伪，定位是实现方的事」——**与 Q1/Q5 裁决直接冲突**（本靶之后 test agent 要报失败测试自身的位置）。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:211` —— §3.8「标记与回退」的「回归红 → `<route>coding</route>`，不定位、不分析原因」——同一口径的第二处。
- `tests/change_ask_ring.rs:294` —— `review_prompt_carries_the_two_new_review_points`：直接断言 `CHANGE_PLAN_REVIEW_PROMPT` 含「留痕核对」「分类恰当性」→ **本靶要改的提示词正是它的断言对象**（改提示词不改这条断言 → 变红）。
- `tests/change_ask_ring.rs:529` —— `e2e_fixture_matches_builtin_and_carries_the_ask_package`：逐字段比对 e2e 夹具与 `ConfigManifest::builtin()` → **本靶要重生成的夹具正是它的断言对象**。
- `tests/change_ask_ring.rs:557` —— `decision_ledger_registers_d024_and_annotates_d009`：读 `docs/plantree/plans/009-change-sop/decisions/README.md` 并断言「`## D024 ·` 恰好出现一次」→ **本靶要写 D025 的台账正是它读的文件**（D025 不得写成 `## D024 ·`，否则计数变 2 直接红）。
- `tests/e2e/test_27_change_ask_ring.py:32` —— `FIXTURE = HERE / "fixtures" / "change_agents.json"`：e2e 侧读**同一份**夹具 → 本靶重生成夹具后，这条 e2e 也要跟着绿（夹具与 builtin 不一致 = 假绿/假红都在这里）。
- `src/change/orchestrator.rs:255-268` —— 测试回注 coding 的编排段：`:261` 拼消息、`:262` 打 WARN、`:266` 抛 Quality（消息里带 `msg`）→ 回注内容的**最终出口**（run 的失败原因文本）。
- `src/change/orchestrator.rs:53` —— `TestingTurn::RouteCoding` 的文档注释「回归/测试失败 → 回注 coding（不给定位）」→ 口径变了要同步。

### 2.2 调用链（谁调用它 / 它调用谁）

- `src/server/change_engine.rs:66` `run_change` → `src/change/orchestrator.rs:119` `run` → `src/change/session.rs:302` `run_coding` / `:351` `run_testing` → `src/change/session.rs:201` `review_once` → `src/sop/coding_protocol.rs:94` `parse_review_report` → `src/sop/rpc_bridge.rs:145` `RpcReviewFactory::open`（**每轮新实例** → Q2 的「换全新实例重审」在生产侧天然可行）。
- `src/sop/coding.rs:153` `parse_review_report` → `:154-165` 协议违例 → 有界重审（`MAX_PROTOCOL_VIOLATIONS = 3`，`:81`）→ 解析器一改，MVP coding SOP 的「FAIL 无 issues」**自动**流进这条既有重审通道（`src/sop/coding.rs` 本身不改）。
- `src/sop/testing.rs:192` `parse_test_review_report` → `:193-210` 协议违例 → 有界重审（`MAX_TEST_PROTOCOL_VIOLATIONS = 3`，`:75`）→ 同上，自动受益。
- `src/sop/doc_fix.rs:97` `parse_doc_review_report` → `:98-112` 协议违例 → 有界重审（`MAX_DOC_PROTOCOL_VIOLATIONS = 3`，`src/sop/doc_protocol.rs:13`）→ 同上，自动受益。
- `src/sop/planning.rs:114` `parse_verdict` → `:118-131` 的 `Verdict::Fail` 臂回注 plan agent → 本次新增的违例分支在这里分流（不打断 plan agent，Q7）。
- `src/change/orchestrator.rs:261` `route_back_message` → `src/change/testing_gate.rs:115` → 消息进 `src/change/orchestrator.rs:266` 的 Quality 文本 → run 的失败原因（用户可见的那一面）。
- `src/change/mod.rs:45` —— `route_back_message` 的 re-export 面：被删的 `strip_locations` 是模块私有函数，导出面不变。
- `src/sop/mod.rs:31`/`:36` —— `parse_review_report` / `Verdict` 的 re-export 面：新增 `Verdict` 变体是同一类型的内部形状，导出面不变（仓内唯一匹配点 `src/sop/planning.rs:118`）。
- `src/change/session.rs:239-242` → `src/change/plan_audit.rs:409` —— plan-review 的运行时指令由 `review_instruction(...)` 生成（其收尾块 `:409` 是「判负必须附 issues」的第二层落点）；同一份指令还携带 `plan_audit::audit` 与裁决门事实（`src/change/plan_audit.rs:406-407`）→ 改 `:409` 的收尾块**不动**其余指令内容（审点、事实注入都不变）。
- `src/change/session.rs:342` / `:409` / `:418-420` → `src/change/orchestrator.rs:142` / `:174` / `:201` / `:234` —— **审查轮次的生产端与消费端**：session 把审查轮次**写死**（`:342` coding、`:409` testing、`:418` plan-review 的 `plan_reviews()`），orchestrator 直接累加到 `state.review_rounds`（四个累加点：`:142` plan、`:174` coding、`:201` amend 后的 plan、`:234` testing）→ Q2 引入重审后，轮次必须由 session 按**实际**重审次数给出（三处全改），orchestrator 的累加口径**不变**（改动落在 session，不落在 orchestrator）。漏掉 `:418` 的后果：plan-review 的重审轮次**静默少记** `state.review_rounds`（一场最多少记 3 轮），报告里的审查轮次与真实轮次对不上。

### 2.3 隐式耦合（grep 几乎拿不到的那类）

- **run 的失败原因文本是回注内容的两个出口**：`src/change/orchestrator.rs:266`（coding 路由：「测试未通过，已回注 coding。{回注消息}」）与 `src/change/orchestrator.rs:274`（plan 路由：「测试发现计划问题，已回注 plan。{回注消息}」）→ 调用方在 run 状态里看到的就是这两句 + 回注消息；回注消息写成什么样，run 就 diagnosable 到什么程度（占位文案 = 终态黑箱）。两条路由共用 `src/change/testing_gate.rs:115` 的同一个模板 → 改一处两条都生效。
- **WARN 日志是真链的取证面**：`src/change/orchestrator.rs:262` 的 `tracing::warn!` 落在 `$HOME/.devforge/logs/devforge.log.jsonl`（`src/main.rs:143-146`：`HOME` → `root = $HOME/.devforge` → `logging::init(&root.join("logs"), "devforge")`，文件名约定见 `src/logging.rs:15`）→ 本次实证的引文就出自这里；e2e 侧的条件式守卫也看这个文件，但 e2e 的 `$HOME` 是**隔离目录**（`tests/e2e/common.py:60-62` 建 `server.home`，`:86` 把 `HOME` 换成它）→ 守卫必须读 `<server.home>/.devforge/logs/devforge.log.jsonl`，写宿主 `~/.devforge/...` 就是恒不触发（假绿）。
- **断言面在仓内另一处**：`tests/change_ask_ring.rs:294`（断言 `CHANGE_PLAN_REVIEW_PROMPT` 的两个审点）、`:529`（夹具 vs builtin 逐字段）、`:557`（读决策台账、断言 `## D024 ·` 恰好一次）+ `tests/e2e/test_27_change_ask_ring.py:32`（读同一份夹具）—— 本靶要改的**提示词 / 夹具 / 台账**正好是这四处断言的对象 → 改错一处即红（尤其 D025 不能写成 `## D024 ·`）。
- **两个「3」不能混用**：Q3 的「追补 ×3」是**同一条 testing 轮内**的重问；`src/change/state.rs:121` `bump_test_reroute` 记的是**跨 run** 的回注次数（上限 `src/change/orchestrator.rs:21` `MAX_TEST_REROUTES = 3`）→ 混用会让续跑把「追补次数」当成「回注次数」而提前收摊。
- **协议块解析的「首个 vs 最后一个」语义不一致**：`src/sop/coding_protocol.rs:171` `extract_block` 用 `text.find`（**首个** `<tag>`），而 `src/sop/doc_protocol.rs:132` `extract_tag` 用 `rfind`（**最后一个**）→ 「FAIL 无 issues」的判定必须沿用同一族解析器的口径，否则会出现「第一个 `<issues>` 块为空、后面还有一块」被误判成无 issues。
- **提示词 ↔ 夹具 ↔ 漂移守卫三处同源**：`src/config/manifest.rs` 的提示词常量 → `tests/e2e/fixtures/change_agents.json`（**必须用生成器重生成**：`src/config/manifest.rs:1338` `dump_change_agents_fixture`（`#[ignore]`），不许手改）→ 漂移守卫 `src/config/manifest.rs:1356 change_agents_fixture_matches_builtin` 逐字段比对 → 改了提示词不同步重生成，e2e 夹具即红。
- **零信息话术散在三处**：`src/change/testing_gate.rs:117`（消息模板）、`src/config/manifest.rs:687`（test-agent 纪律条）、`src/change/orchestrator.rs:53`（文档注释）→ 只删一处会出现「模板里删了、别处还在教 agent 不给定位」的半拉子状态。
- **D018 的台账标题与新口径相反**：`docs/plantree/plans/009-change-sop/decisions/README.md:123`「D018 · 回归失败直接丢回 coding，不做定位」→ 必须加旁注（只改代码不改台账 = 下次自举读到的还是旧口径）。
- **roadmap 的历史证据行会与新口径并存**：`docs/plantree/plans/009-change-sop/roadmap.md:69` 的 T011 证据行写着「回注不带定位」→ 历史交付记录不改（改写等于篡改历史），只**追加**新条目 + 由 D025 旁注消歧。
- **工作区未跟踪产物**：`git status` 显示未跟踪的 `.pi/goals/archived/*.md`、`docs/plantree/changes/`（含本计划）、`tests/e2e/results/*.json` → 未跟踪文件**不进** `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188` 的 `changed_files`），不触发越界门，但会污染提交 → 白名单外的这些路径一律不许 `git add`。
- **断点不需要新字段**：Q2 的重审是同一次审查调用内换实例重试，Q3 的追补是同一条 testing 轮内的重问，都不跨 run → `src/change/state.rs:113`（`bump_review`）与 `src/change/state.rs:121`（`bump_test_reroute`）的既有字段够用（跨 run 计数已有 `test_reroutes`）。

---

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `src/sop/coding_protocol.rs` | ①`parse_review_report` 的两处 FAIL 分支（`:110-117`、`:147-153`）在 issues 为空时判 `protocol_violation = true`，理由作为唯一 issue（形状照 `src/sop/doc_protocol.rs:116-122`）；②补单测：FAIL 无 issues / 空 `<issues>` 块 → 违例；FAIL 带 issues → 不违例 | 需求 1 |
| `src/sop/testing_protocol.rs` | ①`parse_test_review_report` 的两处 FAIL 分支（`:192-202`、`:232-243`）同样判违例（MVP testing 通道）；②`parse_doc_review_report` 的 FAIL 分支（`:288-293`，`protocol_violation: false` 在 `:292`）同样判违例（doc-fix 的 doc-review，Q4 自决追加项）；③补单测 | 需求 1 |
| `src/sop/output.rs` | ①`parse_verdict`（`:40`）的 FAIL 分支（`:52-55`，经 `extract_issues` `:72-86`）在无 issues 时产出**协议违例信号**（新增 `Verdict::ProtocolViolation { reason }`，Q7；注意 `:63-69` 的 `conservative_fail` 已有非空 issues 的违例路径，两者不要混用）；②既有单测 `:119 fail_without_issues_block_is_empty_list` 按新口径改写（口径变更，不是放松断言）；③补单测 | 需求 1 |
| `src/sop/planning.rs` | ①规划循环新增协议违例分支（判据在 `:114` 的 `parse_verdict` 结果、`:118` 的 `Verdict::Fail` 臂旁）：换新审查实例重审（上限 3，新增 `MAX_PLAN_PROTOCOL_VIOLATIONS` 常量），**不打断 plan agent**（不把协议问题回注给被审方），超限 Quality；②`REVIEW_INSTRUCTION` 的收尾（`:31`）写清「`<verdict>FAIL</verdict>` 必须配非空 `<issues>` 清单，空清单按协议违例处理」；③补单测（连续违例 → 重审 → 终止） | 需求 1 |
| `src/sop/coding.rs` | ①`coding_review_instruction` 的收尾（`:47`）写清「FAIL 时 `<issues>` 必须非空，空清单按协议违例处理」；②既有的协议违例重审循环（`:153-165`）与上限常量（`:81`）**不改**；③补单测（指令文本含新锚点） | 需求 1 |
| `src/sop/testing.rs` | ①`testing_review_instruction`（函数头 `:38`）的收尾（`:43`）写清「FAIL 时 `<issues>` 必须非空，空清单按协议违例处理」；②既有的协议违例重审循环（`:192-210`）与上限常量（`:75`）**不改**；③补单测（指令文本含新锚点） | 需求 1 |
| `src/sop/doc_fix.rs` | ①`doc_review_instruction`（函数头 `:34`）的收尾（`:39`）写清「FAIL 时 `<issues>` 必须非空，空清单按协议违例处理」；②既有的协议违例重审循环（`:98-112`）与上限常量（`src/sop/doc_protocol.rs:13`）**不改**；③补单测（指令文本含新锚点） | 需求 1 |
| `src/change/plan_audit.rs` | ①`review_instruction()`（`:386`）的**运行时审查指令收尾块**（`:409`）写清「`<issues>` 必须非空：FAIL 必带逐条问题，空清单按协议违例处理」；②其余审点/事实注入（`:394-407`）一字不动；③补单测（指令文本含新锚点） | 需求 1 |
| `src/change/session.rs` | ①`review_once`（`:201-215`）改成有界重审 ×3（Q2，`:243`/`:332`/`:400` 三处调用点共用）；②`run_testing`（`:355-370`）对「route 无 issues」**拒收 + 追补 ×3**，并删掉两条占位文案（Q3、Q9）；③`issues_of`（`:184`）与协议块提取口径对齐（首个 `<issues>` 块）；④`testing_kickoff`（`:154`）写清「必须附 `<issues>` 逐条明细」；⑤**两处运行时审查指令收尾块**（`:109` coding-review、`:128` test-review）写清「FAIL 时 `<issues>` 必须非空，空清单按协议违例处理」；⑥**审查轮次按实际重审次数记账**（三处生产端：`:342` coding、`:409` testing、`:418-420` 的 `plan_reviews()` 全部由写死的 `1` 改为实际轮次；消费端 `src/change/orchestrator.rs:142`/`:174`/`:201`/`:234` 的累加口径不变）；⑦既有单测同步（`:954`/`:976`/`:1037` 三条直接踩在改动面上） | 需求 1、2、3 |
| `src/change/testing_gate.rs` | ①`route_back_message`（`:115`）删掉「请自己定位原因（本消息不提供文件位置）」话术；②**不再剥 `path:line`**（删 `strip_locations` `:126`）；③两处注释同步：模块头（`:1-10`「回注不带定位」）与 `route_back_message` 的文档注释（`:112-114`「只给失败摘要，不带文件行号定位」/「不改变『不给定位』这条硬约束」）——删了 `strip_locations` 后这两处不当场改就是新的「文档与代码不一致」；④既有单测 `:209 route_back_message_carries_no_location` 按新口径改写；⑤补新单测（带行号 / 无零信息话术） | 需求 2、3 |
| `src/change/orchestrator.rs` | ①集成测试：回注消息含 issues 原文与行号、不含零信息话术与「<位置略>」（Q1/Q3 的端到端断言）；②两处旧口径注释同步：`:53`（`TestingTurn::RouteCoding` 文档注释「不给定位」）与 `:260`（行内注释「回注只带摘要（不给 path:line 定位）」）；③`state.review_rounds` 的累加口径**不改**（`:142`/`:174`/`:201`/`:234` 直接累加 `turn.reviews`/`plan_reviews()`，实数由 session 按实际重审次数给出——改动落在 `src/change/session.rs:342`/`:409`/`:418`，本文件不新增逻辑） | 需求 2、3 |
| `src/config/manifest.rs` | ①`CHANGE_TEST_AGENT_PROMPT`（常量 `:664`、纪律条 `:687`）写清「回注必须附 `<issues>` 逐条明细（含失败测试自身位置与期望 vs 实际）」（Q5）；②三个 change review 提示词（`CHANGE_PLAN_REVIEW_PROMPT` 常量 `:536`、收尾块 `:561-573`；`CHANGE_CODING_REVIEW_PROMPT` 文档注释 `:633` / 常量 `:634`、收尾块 `:650-660`；`CHANGE_TEST_REVIEW_PROMPT` 常量 `:701`、收尾块 `:715-723`）写清「FAIL 必须带非空 `<issues>`，否则按协议违例处理」；③补提示词锚点单测（体例同 `:1035`/`:1417`） | 需求 1、2 |
| `tests/e2e/fixtures/change_agents.json` | 提示词随 builtin 同步 → **必须用生成器重生成**（`src/config/manifest.rs:1338`），不许手改（漂移守卫 `src/config/manifest.rs:1356`；另一处守卫 `tests/change_ask_ring.rs:529`） | 需求 1、2 |
| `tests/e2e/test_26_change_e2e.py` | 加**条件式**日志守卫：真链日志里若出现「测试失败回注 coding」WARN，其消息必须含 issues 明细、不得含「未给出 issues 明细」「请自己定位原因」「本消息不提供文件位置」（Q8） | 需求 3 |
| `docs/plantree/plans/009-change-sop/decisions/README.md` | 登记 **D025**（判负必须可诊断：FAIL 必须附 issues + 无 issues 判负按协议违例 + 回注带原文与行号 + 无 issues 的回注拒收）；给 D018（`:123`）加「定位口径由 D025 改写」的旁注（两边都标，只标一边等于旧决策看着仍有效） | 需求 1、2 |
| `docs/plantree/plans/009-change-sop/topics/agents-change.md` | 三处同步为新口径（判负必须带明细、回注带原文与行号）：`:172`（「与 MVP testing 的差别」表第 4 行「测试 Agent 不看代码、**不做定位**」）、`:210`（「测试 review FAIL → `<route>coding</route>`」）、`:211`（「回归红 → 不定位、不分析原因」） | 需求 1、2 |
| `docs/plantree/plans/009-change-sop/roadmap.md` | Phase C **追加**一条本场次条目（判负可诊断）；**不改**既有条目（T004/T008/T011 等）的历史描述与证据行 | 需求 1、2、3 |

### 3.2 受影响不改

| 文件 / 位置 | 为什么可以不改 |
| --- | --- |
| `src/sop/doc_protocol.rs:116-122` | doc-from-zero 通道的 doc-review **已按本靶口径**判违例（FAIL 无 issues → `protocol_violation` + 理由作为唯一 issue）→ 已是正确形态，本次照抄不改 |
| `src/sop/coding.rs:153-165`、`src/sop/testing.rs:192-210`、`src/sop/doc_fix.rs:97-112` | 三处**既有**的「协议违例 → 有界重审（上限 3）」循环：解析器一改，「FAIL 无 issues」自动流进这些既有通道 → 循环**本身不改**（同文件只改审查指令的收尾文案，见 §3.1）；**文案漏述**见 §6 R15：这三处的错误文案只列了「缺 stage-verdict/next 块」，本靶之后「FAIL 无 issues」也走同一通道 |
| `src/sop/mod.rs:31`/`:36` | re-export 面不变（`Verdict` 已在 `:36` 导出；新增变体是类型内部形状；新上限常量定义在各自模块内，不跨模块用） |
| `src/change/mod.rs:45` | `route_back_message` 的导出面不变（被删的 `strip_locations` 是模块私有函数） |
| `src/change/orchestrator.rs:21`/`:24` | `MAX_TEST_REROUTES` / `MAX_PLAN_FIXES` 不动：Q3 的「追补 ×3」是同一条 testing 轮内的重问，与跨 run 的回注计数是**两个**计数器 |
| `src/change/state.rs` | 断点字段不新增：Q2 的重审是同一次审查调用内换实例重试、Q3 的追补是同一条 testing 轮内的重问，都不跨 run（跨 run 计数已有 `test_reroutes`） |
| `src/change/boundary.rs:41-45` | `parse_route` 不动（route 取值解析与「有没有带 issues」无关） |
| `src/change/coding_gate.rs`、`src/change/report.rs`、`src/change/history.rs`、`src/change/docs.rs`、`src/change/gate0.rs`、`src/change/prepare.rs`、`src/change/change_id.rs` | 与判负/回注无关，本靶不扩围（**注意**：`src/change/plan_audit.rs` **不在**这一行——它的运行时审查指令收尾块（`:409`）要改，已列入 §3.1） |
| `src/config/skill_assets.rs`、`assets/skills/**` | 不新增 skill；工程纪律 skill 一字不动（本变更**遵守**它，不改它） |
| `src/server/change_engine.rs` | 只做编排接线，不解析审查输出 → 不动 |
| `src/runstate/**`、`src/engine/**` | 断点机制与子进程机制不变 |
| MVP 侧 Agent 的供给面（`src/config/manifest.rs:1090`/`:1117` 两条既有断言守着） | 本靶不碰供给（ask 包那一套是上一场的活） |
| `src/config/manifest.rs:1142` `builtin_has_eleven_agents_and_four_base_packages` | Agent 数量与基础包不变 |
| `docs/plantree/plans/009-change-sop/roadmap.md` 的既有条目 | 历史交付记录（含证据 commit），改写等于篡改历史 → 以**追加**方式登记 |
| `docs/plantree/plans/010-run-resume/**` | 010 决策链与本靶无关 |
| `tests/e2e/` 其余脚本（含 `tests/e2e/common.py`） | 本靶不需要新的 e2e 观测字段；其余脚本只作回归面 |
| `tests/change_ask_ring.rs` | **不改**（它断言的是上一场的问询环契约：`:294` 审点锚点、`:529` 夹具漂移、`:557` D024 台账）；本靶只改被它断言的对象（提示词 / 夹具 / 台账），断言本身不动 → 列入 §3.3 必回归 |
| `tests/e2e/test_27_change_ask_ring.py` | **不改**（`:32` 读同一份夹具）；本靶重生成夹具后它必须跟着绿 → 列入 §3.3 必回归 |
| `src/change/prepare.rs`、`src/change/gate0.rs` | 门 0 与基线不变 |

### 3.3 必须回归验证

| 模块 | 为什么风险在这里 |
| --- | --- |
| `src/sop/coding_protocol.rs:243`-`:331` 的既有单测 | 解析器语义变了（FAIL 无 issues：从「普通 FAIL」变「协议违例」），既有断言不得变红 |
| `src/sop/testing_protocol.rs:365`-`:456` 的既有单测 | 同上（test-review 通道） |
| `src/sop/output.rs:119` `fail_without_issues_block_is_empty_list` | **与本靶口径直接冲突**，必须按新口径改写（是口径变更，不是回归红；改写时别写成「放松断言」） |
| `src/sop/planning.rs:135`-`:329` 的既有单测 | 规划循环加了分支，既有「回注给 plan agent」的断言不得变红 |
| `src/sop/coding.rs:314`-`:465` 的既有单测（含 `FAIL_FIX_ONE` 夹具 `:315`） | 共享解析器改了 → MVP coding SOP 的行为不得回归；且本靶还要改它的审查指令收尾（`:47`） |
| `src/sop/testing.rs:492` `protocol_violation_retries_review_not_agent`、`:511` `repeated_protocol_violations_fail_fast` | 既有「违例 → 重审 → 有界终止」语义不得回归（本次只把「FAIL 无 issues」也送进这条通道）；**计数口径**（§1.2）：上限 3 = 允许 3 次违例，第 4 次违例终止；本靶还要改它的审查指令收尾（`:43`） |
| `src/sop/doc_fix.rs` 的既有单测 | 共享解析器（`parse_doc_review_report`）改了 + 审查指令收尾（`:39`）要改 → doc-fix 通道不得回归 |
| `src/change/plan_audit.rs` 的既有单测（`audit` 四条检查、裁决门、审查指令文本） | 本靶要改 `review_instruction()` 的收尾块（`:409`）→ 指令文本断言不得变红；`audit()`（`:238`）四条机械检查行为一个字都不能变 |
| `src/change/session.rs:954` `coding_review_failure_fails_the_stage`、`:976` `testing_review_is_invoked_and_can_fail`、`:1037` `testing_route_coding_returns_failures` | 这三条直接踩在改动面上（审查 FAIL 路径 ×2、route 路径 ×1） |
| `src/change/testing_gate.rs:209` `route_back_message_carries_no_location` | **与 Q1 裁决直接冲突**，必须按新口径改写（口径变更，不是回归红） |
| `src/change/orchestrator.rs:466`-`:560` 的既有单测 | 回注路径的编排行为不得回归（回注计数、越界二分、往返上限） |
| `src/config/manifest.rs:1035`/`:1090`/`:1117`/`:1356`/`:1417` | 提示词锚点、逐 Agent 供给断言、夹具漂移守卫、testing 提示词协议锚点——提示词一改，这些是唯一的「提示词与门没漂移」防线 |
| `tests/change_ask_ring.rs:294` `review_prompt_carries_the_two_new_review_points` | 直接断言 `CHANGE_PLAN_REVIEW_PROMPT` 含「留痕核对」「分类恰当性」→ **本靶要改的提示词正是它的断言对象**（改提示词不改断言 = 变红） |
| `tests/change_ask_ring.rs:529` `e2e_fixture_matches_builtin_and_carries_the_ask_package` | 夹具 vs builtin 的逐字段漂移守卫：本靶重生成 `tests/e2e/fixtures/change_agents.json` 后它必须绿（手改夹具即红） |
| `tests/change_ask_ring.rs:557` `decision_ledger_registers_d024_and_annotates_d009` | 读本靶要写的决策台账，断言「`## D024 ·` 恰好出现一次」→ **D025 不能写成 `## D024 ·`**，否则计数变 2 直接红 |
| `tests/e2e/test_27_change_ask_ring.py:32` | 读同一份 `change_agents.json` 夹具（与 `tests/change_ask_ring.rs:529` 同一守卫的 e2e 版）→ 夹具重生成后必须跟着绿 |
| `tests/e2e/test_26_change_e2e.py` | 真链：改了引擎与提示词后 change run 仍要 Settled（回注守卫是条件式的） |
| `tests/e2e/test_19_testing_full_chain.py`、`tests/e2e/test_20_testing_fixture.py` | MVP testing SOP 真链（共享解析器 + doc-review 通道都动了） |
| `tests/e2e/test_08_e2e_planning.py` | MVP 规划 SOP 真链（`src/sop/output.rs` + `src/sop/planning.rs` 动了） |
| 仓内单测（`cargo test`） | 新增/改动的单测全在白名单文件内（无新增文件） |
| 部署侧（不在仓内） | `~/.devforge/manifest.json` 缓存与 profile 重渲染（`src/main.rs:152`）→ 仓内改完不等于用户态生效 |

---

## 4 白名单

- `src/sop/coding_protocol.rs`
- `src/sop/testing_protocol.rs`
- `src/sop/output.rs`
- `src/sop/planning.rs`
- `src/sop/coding.rs`
- `src/sop/testing.rs`
- `src/sop/doc_fix.rs`
- `src/change/plan_audit.rs`
- `src/change/session.rs`
- `src/change/testing_gate.rs`
- `src/change/orchestrator.rs`
- `src/config/manifest.rs`
- `tests/e2e/fixtures/change_agents.json`
- `tests/e2e/test_26_change_e2e.py`
- `docs/plantree/plans/009-change-sop/decisions/README.md`
- `docs/plantree/plans/009-change-sop/topics/agents-change.md`
- `docs/plantree/plans/009-change-sop/roadmap.md`

新增测试文件：**无**。Rust 单测内联在被改文件的 `#[cfg(test)]` 模块；e2e 断言追加进已有的 `tests/e2e/test_26_change_e2e.py`，不新增 e2e 脚本。

白名单之外不得有任何改动（白名单共 17 个文件，上面的清单就是全部；未跟踪产物与计划阶段产物的处理见 §2.3 与 §6 R14——它们既不在白名单里，也不许被 `git add` 进本变更的提交）。

---

## 5 验收标准

- 给定一段 `<stage-verdict>FAIL</stage-verdict>` 且**没有** `<issues>` 块的审查回复 → `parse_review_report` 返回的 `protocol_violation` 为 `true`，且其 `issues` 恰好一条、文本同时含「FAIL」与「issues」。
- 给定一段 `<stage-verdict>FAIL</stage-verdict>` + `<next>fix</next>` + **空** `<issues></issues>` → `parse_review_report` 的 `protocol_violation` 为 `true`。
- 给定一段 `<stage-verdict>FAIL</stage-verdict>` + `<next>fix</next>` + `<issues>- 断言与文档矛盾</issues>` → `parse_review_report` 的 `protocol_violation` 为 `false` 且 `issues` 长度为 1（既有行为不回归）。
- 给定一段 `<stage-verdict>FAIL</stage-verdict>` + `<next>fix</next>` 但缺 `<issues>` 块 → `parse_review_report` 判 `protocol_violation = true`。
- 给定 `parse_test_review_report` 收到同样的「FAIL 无 issues」文本 → `protocol_violation` 为 `true`（MVP testing 的 test-review 通道）。
- 给定 `parse_doc_review_report`（doc-fix 的 doc-review 通道）收到「FAIL 无 issues」→ `protocol_violation` 为 `true`。
- 给定 `parse_verdict` 收到 `<verdict>FAIL</verdict>` 且无 `<issues>` → 返回**协议违例信号**（`Verdict::ProtocolViolation`），不再返回空清单的 `Fail`。
- 给定 `parse_verdict` 收到 `<verdict>FAIL</verdict>` + 非空 `<issues>` → 仍返回 `Verdict::Fail { issues }`（既有行为不回归）。
- 给定 MVP 规划循环的一次审查回复是「FAIL 无 issues」→ **没有任何**回注消息发给 plan agent（plan agent 收到的消息条数不变），而是换一个新审查实例重审。
- 给定 MVP 规划循环连续 4 次审查都是「FAIL 无 issues」→ 规划阶段以 Quality 失败结束，错误文本同时含「协议违例」与「issues」。
- 给定 change SOP 的一次审查（plan-review / coding-review / test-review 任一）回复是「FAIL 无 issues」→ 引擎**不**立刻终止，而是换一个全新审查实例重审该轮。
- 给定 change SOP 连续 **4** 次审查都是「FAIL 无 issues」（= 3 次违例上限已用满 + 第 4 次）→ run 以 Quality 结束，错误文本同时含「协议违例」与「issues」（可诊断、不黑箱）。
- 给定 change SOP 的一次审查因「FAIL 无 issues」**重审了 3 次后才通过** → 该阶段上报的审查轮次是 **4** 而不是 1（轮次 = 尝试次数 = 1 次初审 + 3 次重审；`src/change/session.rs:342`/`:409`/`:418` 三处生产端全部给出实际轮次，`src/change/orchestrator.rs:142`/`:174`/`:201`/`:234` 累加后 `state.review_rounds` 不少记）。
- 给定 plan-review 一次因「FAIL 无 issues」**重审了 2 次** → `plan_reviews()` 返回 **3**（1 次初审 + 2 次重审，不是写死的 1），且 `state.review_rounds` 计入这 3 轮。
- 给定 change SOP 的一次审查回复是「缺 `<stage-verdict>` 块」→ 行为与改动前一致（协议违例 → 重审；沿用 `src/sop/coding.rs:153-165` 的既有口径）。
- 给定 test agent 回复 `<route>coding</route>` 且**带** `<issues>` → `run_testing` 返回 `TestingTurn::RouteCoding { failures }`，`failures` 逐条等于 issues 原文（`tests/change_ask_ring.rs:294` 这类行号不被破坏）。
- 给定 test agent 回复 `<route>coding</route>` 且**不带** `<issues>` → 引擎**不**产生任何 `TestingTurn::RouteCoding`（不造占位文案），而是向 test agent 会话再发一条消息，该消息同时含「`<issues>`」与「重发」两个锚点。
- 给定 test agent **连续 4 次**回复都带 `<route>coding</route>` 且不带 `<issues>`（= 1 次初次 + 3 次追补，3 次追补用满）→ `run_testing` 以 Quality 结束，错误文本同时含「未附」与「`<issues>`」。
- 给定 test agent 在**第 3 次追补**后带上了 `<issues>` → 引擎正常回注 coding（3 次追补用满仍不算失败），回注消息逐字含该条 issues 原文。
- 给定 test agent 回复 `<route>plan</route>` 且不带 `<issues>` → 与 `<route>coding</route>` 同口径（拒收 + 追补 + 上限 3）。
- 给定 `route_back_message` 的入参首项是「断言失败 tests/change_ask_ring.rs:294 count_words('a b') 返回 1，期望 2」→ 消息逐字含 `tests/change_ask_ring.rs:294` 与「期望 2」。
- 给定 `route_back_message` 的输出 → **不含**「请自己定位原因」、**不含**「本消息不提供文件位置」、**不含**「<位置略>」。
- 给定 `route_back_message("plan", [...])` → 同样不含上述三处零信息/阉割痕迹，且含 `<route>plan</route>`。
- 给定 change 编排器收到的 failures 首项是「断言失败 tests/change_ask_ring.rs:294」→ 抛出的 Quality 错误文本逐字含该条 issues 原文与行号（回注内容在 run 的失败原因里可见）。
- 给定 change 编排器收到的 failures 里含 `path:line` → 抛出的 Quality 错误文本里该行号未被替换（不回退成「<位置略>」）。
- 给定 `CHANGE_TEST_AGENT_PROMPT` → 含「`<issues>`」锚点与「失败测试」定位锚点，且**不含**「不猜文件位置」这句旧纪律。
- 给定 `CHANGE_TEST_REVIEW_PROMPT` / `CHANGE_CODING_REVIEW_PROMPT` / `CHANGE_PLAN_REVIEW_PROMPT` → 各自含「FAIL 必须带」与「issues」两个锚点。
- 给定 7 处**运行时审查指令**的收尾块（`src/change/plan_audit.rs:409`、`src/change/session.rs:109`、`src/change/session.rs:128`、`src/sop/planning.rs:31`、`src/sop/coding.rs:47`、`src/sop/testing.rs:43`、`src/sop/doc_fix.rs:39`）→ 每一处都含「必须非空 / FAIL 必须带 issues」的锚点（提示词层与指令层两处都要写，只写一层 = 审查方仍可能发空清单）。
- 给定 e2e 夹具 `tests/e2e/fixtures/change_agents.json` → 六个 change agent 条目的提示词与 builtin 逐字段一致（漂移守卫转绿；必须由生成器产出，不得手改）。
- 给定 e2e 真链跑完 → run 状态仍是 `Settled`（本靶不扩需求，但改了引擎与提示词，真链必须回归）。
- 给定 e2e 真链的日志文件 `<server.home>/.devforge/logs/devforge.log.jsonl`（e2e 的服务器 HOME 是隔离目录：`tests/e2e/common.py:60-62`、`:86`；`src/main.rs:143-146` 以 `$HOME` 定位日志目录）里出现「测试失败回注 coding」→ 该行消息必须含至少一条 issues 明细，且不含「未给出 issues 明细」「请自己定位原因」「本消息不提供文件位置」（该日志行不存在时本条不适用）。
- 给定 `docs/plantree/plans/009-change-sop/decisions/README.md` → 含 D025 一节（判负必须可诊断），且 D018 标题带「定位口径由 D025 改写」的旁注。
- 给定 `docs/plantree/plans/009-change-sop/topics/agents-change.md` → 「与 MVP testing 的差别」表第 4 行（`:172`）不再写「测试 Agent 不看代码、不做定位」；「回归红 → 回注」那条（`:211`）不再写「不定位、不分析原因」，改为写「回注带失败明细（含测试位置与期望 vs 实际）」。
- 给定本变更的提交内容 → 白名单外无任何改动（工作区里的未跟踪产物一律不 `git add`）。

---

## 6 风险

- **R1 · Q1 裁决推翻了既有 D018 的口径（台账、topic 文档与代码注释要同步，否则下次自举读到的还是旧口径）**：`docs/plantree/plans/009-change-sop/decisions/README.md:123` 的 D018 标题「回归失败直接丢回 coding，不做定位」、`docs/plantree/plans/009-change-sop/topics/agents-change.md:172` 的「测试 Agent 不看代码、不做定位」、`docs/plantree/plans/009-change-sop/topics/agents-change.md:211` 的「回归红 → 不定位、不分析原因」，与 `src/change/testing_gate.rs:7-9` 的模块注释、`src/change/testing_gate.rs:112-114` 的 `route_back_message` 文档注释（「只给失败摘要，不带文件行号定位」/「不改变『不给定位』这条硬约束」）、`src/change/orchestrator.rs:53` 的文档注释、`src/change/orchestrator.rs:260` 的行内注释（「回注只带摘要（不给 path:line 定位）」）、`src/config/manifest.rs:687` 的 test-agent 纪律条是同一句旧口径的**八个落点** → 本次必须**八处一起同步**（旁注 + 新决策 D025 + topic 两行 + testing_gate 两处注释 + orchestrator 两处注释 + 提示词一条），只改一处就会出现「代码说带定位、台账说不给定位」的半拉子状态；其中 `src/change/testing_gate.rs:112-114` 与 `src/change/orchestrator.rs:260` 尤其危险——它们紧贴被改的代码，不改就是当场可见的「文档与代码不一致」。
- **R2 · 既有单测里有两条断言与新口径直接冲突**：`src/sop/output.rs:119 fail_without_issues_block_is_empty_list` 与 `src/change/testing_gate.rs:209 route_back_message_carries_no_location` → 必须**按新口径改写**（口径变更），不是「为了让测试变绿而放松断言」；改写要在提交信息里写清是口径变更。
- **R3 · 判负附 issues 会让审查输出变长**：审查提示词要求「FAIL 必须带 issues」→ 审查回复变长、tokens 变多；更长的回复有截断风险（`<issues>` 块未闭合 → 解析器按「缺 issues」判违例 → 重审 ×3 → 死）。缓解：提示词要求「每条一行、只列可定位的要点」；重审通道本身就是兜底。
- **R4 · Q2/Q3 的重试会多烧模型调用**：change 侧一次审查最多多 3 次重审、一次 testing 轮最多多 3 次追补；两者都发生在**同一次 run 内**、不写断点（续跑不会重放），但会拉长单场时长。
- **R5 · 协议违例重审 ×3 只是把「死」推后，不解决根因**：模型系统性不按格式输出时，重审 3 次后仍以 Quality 死（诊断明确，但不是根因修复）——这是把 MVP 侧既有语义（`src/sop/coding.rs:81`）统一到 change 侧，不新增风险。
- **R6 · Q1 放开行号后，test agent 可能顺手去猜产品源码位置**：D019（不给 codegraph）本意是挡住这个；允许它报「失败测试自身的位置」后，边界靠提示词守。缓解：提示词只许写测试自身位置；即便它写了源码位置，对 coding 无害（coding 本就能看源码），真正的风险是「为了猜源码位置而去读源码」——由供给面（不给 codegraph）兜住。
- **R7 · Q3 的追补对话与 test agent 的「无标记重推」是两件事**：`src/change/session.rs:174` 的 `require_marker` 判「无 `<stage-complete>`」是**另一种**协议问题；Q3 只治「有 route 但无 issues」。两条别混：追补消息的措辞不能像「重推」，否则 agent 会重新跑一遍测试（浪费且可能改状态）。
- **R8 · 文档与代码不一致（既有，需消歧）**：`docs/plantree/plans/009-change-sop/roadmap.md:69` 的 T011 证据行写着「回注不带定位」→ 那是历史交付记录，本靶**不改**（只追加），由 D025 旁注消歧。
- **R9 · MVP 侧行为外溢（白名单扩到 17 个文件的原因）**：`parse_review_report` / `parse_test_review_report` / `parse_doc_review_report` / `parse_verdict` 都是**共享**解析器 → 改一处影响 change 三角色 + MVP coding/testing/doc-fix/规划共 7 条通道；本靶还必须改 **7 处运行时审查指令收尾**（`src/change/plan_audit.rs:409`、`src/change/session.rs:109`、`src/change/session.rs:128`、`src/sop/planning.rs:31`、`src/sop/coding.rs:47`、`src/sop/testing.rs:43`、`src/sop/doc_fix.rs:39`）——只改 role prompt（`src/config/manifest.rs`）不改指令层，审查方仍可能发空清单（两层都会收到 kickoff）。因此白名单从 13 扩到 17。另一层外溢：本仓是应用不是库，但 `Verdict` 在 `src/sop/mod.rs:36` 对外 re-export，新增变体对仓外使用者是破坏性变更（仓内唯一匹配点 `src/sop/planning.rs:118`）→ 记一笔。
- **R10 · e2e 真链的模型不确定性**：真链是否回注取决于模型行为 → e2e 侧的回注断言只能做成**条件式**（有回注日志才断言），不能做成「必须出现回注」（那是假红）；且日志必须读 e2e 服务器的**隔离 HOME**（`<server.home>/.devforge/logs/devforge.log.jsonl`）——读宿主 `~/.devforge/...` 会恒不命中（假绿，守卫形同虚设）。
- **R11 · 夹具必须重生成、台账不能写成 D024**（两处守卫）：提示词改了 → `tests/e2e/fixtures/change_agents.json` 必须用生成器（`src/config/manifest.rs:1338` `dump_change_agents_fixture`，`#[ignore]`）重生成；手改会被漂移守卫（`src/config/manifest.rs:1356`、另一处 `tests/change_ask_ring.rs:529`）打红。另外 `tests/change_ask_ring.rs:557` 断言台账里「`## D024 ·`」**恰好出现一次** → 本靶新增的决策标题必须是 `## D025 ·`（写成 `## D024 ·` 会让计数变 2 直接红）。
- **R12 · 启动期基础设施重试（本靶只登记不修）**：run 级 `attempt=2` 的启动期重试在连续三场自举的 boot 阶段都出现过（2026-10-20 深夜一场、2026-10-21 c2/c3 两场），成因未查 → 值得单开后续靶子（死亡诊断方向），本靶不扩围。
- **R13 · 缓存/构建产物**：`target/` 不入白名单；`~/.devforge/manifest.json` 缓存（`src/main.rs:152`）→ 仓内改完不等于用户态生效（部署侧动作：重存清单 + 重渲染 profile）。
- **R14 · 未跟踪产物污染提交**：`git status` 里的 `.pi/goals/archived/*.md`、`tests/e2e/results/*.json`、`docs/plantree/changes/`（含本计划）→ 未跟踪文件不进 `git diff --name-only <baseline>`（不触发越界门），但会被 `git add -A` 卷进提交 → 提交前逐个核对（本变更按步提交，不许 `-A`）。
- **R15 · MVP 侧既有错误文案会漏述新成因（本靶只登记不修）**：`src/sop/coding.rs:160`「审查连续 N 轮输出协议违例（**缺少 stage-verdict/next 块**）」、`src/sop/testing.rs:199`「测试审查连续 N 轮输出协议违例（**缺 stage-verdict/next**）」、`src/sop/doc_fix.rs:103`「文档审查连续 N 轮输出协议违例（**缺 stage-verdict**）」——本靶之后「FAIL 无 issues」也走同一条通道，文案里的成因枚举会**漏掉这一种**。为什么不属于黑箱：这三句后面都跟着 `report.issues.join("；")`（`src/sop/coding.rs:162`/`src/sop/testing.rs:201`/`src/sop/doc_fix.rs:105`），实际违例原因仍在文本里 → 可诊断，只是「原因枚举」不全。本靶**不改**这三处文案（会把白名单扩到三个 MVP 文件，且与「范围仅此一靶」相抵），登记为后续清理项；**风险**：读日志的人可能按括注里的枚举去排查（找不到「缺块」就以为查错了方向）→ 缓解：错误文本尾部已含真实原因，且 change SOP 侧的新错误文案（§5「协议违例 + issues」口径）不依赖这三句。
