# 变更计划 · change-20261022-1-2026-10-21-c3-textkit-run-test-agent-r

目标仓：`/home/trtyr/code-work/devforge`
基线：`ad485549e0981b402bd53d64f2dc7e07c79e47b7`（`docs/plantree/changes/change-20261022-1-2026-10-21-c3-textkit-run-test-agent-r/baseline.txt`）
上一场：`change-20261021-coding-agent-c4-path-line-coding-src-cha`（已 Settled，HEAD=`ad48554`）
文档档位：none（计划阶段照常读已有文档；不跑文档流水线）
本场是**自举第五场·单靶**：只治「**审查输出协议硬化**」——**违例可重审**、**空箱必拒收**、**实例失败 ≠ 判负**、**路由判定只认尾部协议位**；**不做**死亡诊断（F6）、**不做**续跑加固（F7）、**不改**回注消息的渲染口径（D025 已定、c5 已落地）。

---

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

上一场（c4）把「失败明细送达 coding agent」这条管子接通了；本场治的是它上游的另一半：**审查输出与回注输出这两条协议本身没有牙齿**。四件事：

1. **违例可重审（裁决 A）** —— 审查输出协议违例（缺协议块 / 未知值 / 自相矛盾 / **FAIL 未附 issues**）必须在**协议层**被认成「**审查官自己不合规**」，而不是「被审对象被判负」。处置 = 换**全新审查实例**重审，上限 3 次，超限才 Quality 终止。落地范围：change 侧**三个**审查角色（plan / coding / test，`src/change/session.rs:256`/`:352`/`:420` 三处现在是「违例即 Quality」）+ MVP 侧 plan-review（`src/sop/output.rs:52-55`）。MVP 侧 coding / testing 循环本来就是这个口径（C19-R1，`src/sop/coding.rs:146-170`、`src/sop/testing.rs:185-215`），本靶把它们与 change 侧并到**同一份共享机制**上（常量与结构复用，**不复制三份**）。
2. **空箱必拒收（裁决 B）** —— test agent 回注 `<route>coding</route>`（以及对称的 `<route>plan</route>`）**不带** `<issues>` 时，引擎**不再合成占位文案**「测试阶段判定失败（未给出 issues 明细）」（`src/change/session.rs:376-381`、`:386-391`），而是**拒收**这条回注并追一条「请补齐失败明细后重发」；连续 3 次仍不带 → Quality。
3. **实例级失败 ≠ 真判负（交付要求 2）** —— 审查实例自己失败（无会话文件 / 空回复 / 模型通道错误 / 进程早退）必须在协议层与「真判负」分开：走**换新实例重审**（上限 3）→ 耗尽后按**基础设施故障**（`DevforgeError::System`）上抛 → 触发 run 级自动重启（从断点继续），对齐 doc SOP 的既有参照（`src/sop/doc_from_zero.rs:329-347`）。**真判负**（审查给了 FAIL + issues）保持现状（Quality 终止）。
4. **路由判定只认尾部协议位（用户裁决 Q1）** —— c5 那次死亡的真因不是「审查实例失败」，而是 `parse_route`（`src/change/boundary.rs:40-45`）取全篇**第一个** `<route>` 出现处：模型正文里的一句散文「未触发 `<route>coding</route>`」被当成了回退指令 → 空 issues → 占位文案 → 回注 → Quality。路由块改为**只认回复尾部协议位**（route 块之后只允许 `<issues>` / `<impact-gap>` 块与空白）。

**明确不做（边界）**：

- 死亡诊断（F6）、续跑加固（F7）不在本靶；
- doc 侧两处**记遗留不修**：`src/sop/testing_protocol.rs:292`（不判违例）、`src/sop/doc_protocol.rs:116`（已判，本靶只作参照实现）；
- 不改回注消息的渲染口径（D025 已定、c5 已落地：`src/change/testing_gate.rs:115` 的三出口同源不动）；
- 不引入新外部依赖；遵守 `assets/skills/coding-discipline/SKILL.md`（本变更**遵守**它，不改它）。

### 1.2 两次生产命中的事实链（实测取证，不是推测）

**命中 1（c3 场，2026-10-21 上午）**：test agent 回注 `<route>coding</route>` 不附 `<issues>` → 引擎在 `src/change/session.rs:376-381` 合成占位文案「测试阶段判定失败（未给出 issues 明细）」→ 回注 coding → coding 无从下手 → 终态 Quality。同一夹具前夜 31 PASS / 0 FAIL。

**命中 2（c5 场主 run，2026-10-21 晚）—— 真因与台账记载不同，本计划按实测取证**（现场在 `/var/tmp/devforge-e2e-pabf6gyx`）：

- `.devforge/logs/devforge.log.jsonl`：`12:18:57` 「基础设施故障，自动重启 run（从断点继续）」attempt=2；`14:30:48` 「测试失败回注 coding … 失败摘要：- 测试阶段判定失败（未给出 issues 明细）」＋「SOP 质量失败（不重试）」；`14:36:20` 重启；`15:45:08` 「change SOP 通过」rounds=7。
- `.devforge/sessions/change-testing/2026-10-21T12-18-58-024Z_01a15e66-*.jsonl`：死亡轮里 test agent 的**最终回复末尾是 `<stage-complete>1</stage-complete>`**（它自认通过：543 passed / 0 failed，验收 17/17），但正文散文里有一句「……先跑既有全套 → exit 0，15 例全绿，**未触发 `<route>coding</route>`**」。
- `.devforge/sessions/change-test-reviews/`：**只有 `review-0/2026-10-21T15-40-57-025Z_*.jsonl`**（来自 15:40 的成功轮）——死亡轮里 **test review 从未被调用过**（`src/change/session.rs:371-395` 的 route 分支在 `require_marker`（`:395`）与审查（`:420`）**之前**就 return 了）。

**结论（写进本计划，影响本靶口径）**：

- 「test-review 实例未写任何会话文件」是**结果不是原因**——审查实例压根没被开过；「被引擎按 FAIL 未附 issues 处理」实际是「引擎把 test agent 正文里的散文提及当成了回退路由」，**判负通道与实例失败通道在代码里根本没有分开**（`src/change/session.rs:371-395` 一条分支同时承担「路由」「判负」「失败明细」三件事）。
- 台账 `docs/plantree/ideas/selfhost-findings-20261021.md:66-73`（F5）把这次死亡记成「审查报错路径被当成测试判负」——**方向对、归因不准**；本靶按实测把根因定位到「路由判定过松 + 空箱被合成占位文案」，并把「实例失败 ≠ 判负」补成独立的第三条通道。

### 1.3 代码事实（定位到行）

| # | 事实 | 位置 |
| --- | --- | --- |
| 1 | change 侧 plan 审查「协议违例 → Quality」（一次即终止，不重审） | `src/change/session.rs:256-262` |
| 2 | change 侧 coding 审查「协议违例 → Quality」 | `src/change/session.rs:352-357` |
| 3 | change 侧 test 审查「协议违例 → Quality」 | `src/change/session.rs:420-425` |
| 4 | change 侧三处「审查真判负 → Quality 终止」（含「变更测试审查未通过：」的空清单形态） | `src/change/session.rs:358-367`、`:426-435` |
| 5 | change 侧审查调用点：三个角色共用一个 `review_once`（开一个实例、审一次、解析） | `src/change/session.rs:214-228`（定义）、`:250-254`/`:348-351`/`:417-419`（三个调用点） |
| 6 | `review_once` 把「审查会话无有效回复（空回复）」当 System 基础设施故障直接上抛（不换实例、不区分判负） | `src/change/session.rs:220-225` |
| 7 | 审查实例级失败（进程早退 / 模型通道错误）在 RPC 层已经是错误（不是结论） | `src/sop/rpc_bridge.rs:76-90`（空回复→System）、`src/engine/client.rs:141-155`（子进程早退→System）、`src/engine/client.rs:283-292`（prompt 失败→`DevforgeError::Agent`） |
| 8 | test agent 回注分支：route 命中即 return，**先于**标记检查与审查 | `src/change/session.rs:371-395`（route 分支 `:372-393`，标记检查 `:395`，审查 `:417-419`） |
| 9 | 空 issues 时合成占位文案（两处：coding 路由 / plan 路由） | `src/change/session.rs:376-381`、`:386-391` |
| 10 | `parse_route` 取全篇**第一个** `<route>` 出现处（散文提及即命中），且被 coding 轮、testing 轮、越界二分三处共用 | `src/change/boundary.rs:40-45`（定义）、`src/change/session.rs:331`、`src/change/session.rs:372`、`src/change/boundary.rs:112` |
| 11 | `<issues>` 块解析取**第一个** `<issues>` 出现处，未闭合则吞到文末 | `src/change/session.rs:197-212` |
| 12 | 共享解析器 `parse_review_report` 对「FAIL 且无 issues」**不判违例**（产出 `protocol_violation = false` + 空清单） | `src/sop/coding_protocol.rs:94-153`（FAIL 缺 next 分支 `:110-120`、正常分支 `:145-153`） |
| 13 | MVP plan-review 解析器 `parse_verdict` 对「FAIL 无 issues」产出**空清单** `Verdict::Fail { issues: [] }` | `src/sop/output.rs:40-61`（FAIL 分支 `:52-55`） |
| 14 | MVP plan-review 循环把「违例原因」当 issue **回注规划 agent**（零校验自动重查），与裁决 A 的「换新实例重审」不同口径 | `src/sop/planning.rs:105-133`（`parse_verdict` 调用点 `:114`、回注 `:118-127`） |
| 15 | MVP coding / testing 循环的违例重审口径（换新实例、≤3、超限 Quality）——本靶要对齐的参照 | `src/sop/coding.rs:146-170`（上限常量 `:81`）、`src/sop/testing.rs:185-215`（上限常量 `:75`） |
| 16 | doc 侧已判「FAIL 无 issues → 违例」= **参照实现** | `src/sop/doc_protocol.rs:116-124` |
| 17 | doc 侧另一处**不判**违例 = 本靶**记遗留不修** | `src/sop/testing_protocol.rs:272-304`（`:292` 附近的 `DocReviewReport` 分支） |
| 18 | doc 侧「实例失败 → 换新实例重试 → 耗尽后 System」的既有参照实现 | `src/sop/doc_from_zero.rs:314-360`（空回复 `:325-341`、会话异常 `:342-357`、上限常量 `src/sop/doc_protocol.rs:20`） |
| 19 | 只有 `System` 类错误会触发 run 级自动重启（从断点继续） | `src/engine/retry.rs:63-68`、调用点 `src/server/mod.rs:1243-1270` |
| 20 | Quality 类错误 = 明确 Failed、**不**自动重启 | `src/server/mod.rs:1002-1015`（`map_intervention`） |
| 21 | 仓内源码守卫：「协议违例上限不得写成 System」（否则会被自动重启）——守的是三个文件的**旧文案** | `src/server/tests/mod.rs:524-540` |
| 22 | change 侧审查轮次计数是**硬编码 1**（`reviews: 1`），`plan_reviews()` 也恒返回 1 | `src/change/session.rs:362`、`:429`、`:438-440`；消费点 `src/change/orchestrator.rs:150`/`:182`/`:211`/`:244` |
| 23 | test review 会话工厂（`per_round_in` → `sessions/change-test-reviews/review-N`） | `src/server/change_engine.rs:173-178`、目录清单 `:266-280` |
| 24 | 生产者侧提示词**从未要求** test agent 用 `<issues>` 块、也**没要求**路由块放末尾 | `src/config/manifest.rs:687`（test agent 纪律条「回注只写失败现象与摘要」）、`:697`（「并把回注声明（如果失败）附上：`<route>coding|plan</route>`」）、`src/change/session.rs:167-180`（`testing_kickoff`「失败时输出 `<route>coding</route>`……并附失败摘要」）、`src/change/session.rs:142-165`（`coding_kickoff`「越界必须输出 `<route>plan</route>` + `<impact-gap>`」） |
| 25 | 审查侧提示词**已经**要求「FAIL 时逐条列 issues」→ 新判定是给既有契约装牙齿，不是新契约 | `src/config/manifest.rs:566-568`（plan review）、`:655-657`（coding review）、`:718-720`（test review）、`src/change/session.rs:110`、`:133`、`src/change/plan_audit.rs:409` |
| 26 | 提示词 ↔ e2e 夹具的漂移守卫（改了 change 角色提示词就必须重生成夹具） | 生成器 `src/config/manifest.rs:1334-1352`、守卫 `src/config/manifest.rs:1354-1374`、孪生守卫 `tests/change_ask_ring.rs:528-546` |
| 27 | 决策台账与 topic 文档各有守卫盯着（追加新条目不得删改既有锚点） | `src/change/plan_audit.rs:711-730`（台账 D024）、`:735-757`（agents-change.md 问询环锚点）、`src/change/coding_gate.rs:490-502`（台账 D023）、孪生 `tests/change_ask_ring.rs:556-575`、`:578-597` |
| 28 | 上一场留下的白名单守卫用**硬编码**基线与白名单扫 `BASELINE..HEAD`，不刷新必红 | `tests/change_ask_ring.rs:31`、`:34-45`、`:604-628` |
| 29 | 既有单测锁着「占位文案逐字送达」这条**被本靶推翻**的口径 | `tests/change_failure_context/main.rs:57-58`（`PLACEHOLDER_ISSUE` 常量）、`:746-800`（a13）、`src/change/session.rs:906-931`（`the_placeholder_issue_text_is_delivered_verbatim`） |
| 30 | 既有单测锁着「MVP plan-review 的违例回注规划 agent」这条**被本靶推翻**的口径 | `src/sop/planning.rs:315-328`（`missing_verdict_is_conservative_fail_then_remediation`）、`src/sop/output.rs:119-123`（`fail_without_issues_block_is_empty_list`）、`:125-135`（`missing_or_unknown_verdict_is_conservative_fail`） |
| 31 | doc 与代码已不一致（先于本靶）：topic 文档写「测试 review FAIL → `<route>coding</route>`」，代码是「测试 review FAIL → Quality 终止」 | `docs/plantree/plans/009-change-sop/topics/agents-change.md:210` vs `src/change/session.rs:426-435` |
| 32 | doc 与代码已不一致（先于本靶）：差异表写「连续 3 次无标记 → 判基础设施故障」，代码判**质量失败** | `docs/plantree/plans/009-change-sop/topics/vs-mvp.md:34` vs `src/sop/coding.rs:158-165` |
| 33 | `ProdChangeSession` 加字段 `plan_review_instances`（`src/change/session.rs:54-64`）后，仓内**所有字面量构造点**都必须补该字段：生产装配点 `src/server/change_engine.rs:180`、仓内单测构造点（`src/change/session.rs:625` 起的一批）、仓外夹具（`tests/change_failure_context/main.rs:771`、`tests/change_ask_ring.rs:179`）——其中**只有** `src/server/change_engine.rs` 落在原 §4 白名单之外 | `src/server/change_engine.rs:180-189`、`src/change/session.rs:54-64` |

### 1.4 歧义

- [必须裁决] **Q1 · c5 真因（路由判定过松）是否纳入本靶** —— 需求给的裁决 A/B/C 都没覆盖「路由判定过松」这条根因；但不修它，c5 那次死亡（模型散文提及 `<route>coding</route>` → 空箱 → 占位文案 → Quality）在协议硬化之后**仍会原样复现**（占位文案没了，但空箱死法还在）。影响验收口径（路由块解析规则、提示词是否要写「放末尾」、既有 `parse_route` 单测怎么改）。
  备选：A 纳入并收紧到「只认末尾独立行」／B 纳入但只限 testing 段（`src/change/session.rs:372`）／C 不纳入、记遗留留给下一靶。
  用户裁决：纳入：只认末尾（推荐）
- [必须裁决] **Q2 · MVP plan-review 的违例处置范围** —— 裁决 C 只点名 `src/sop/output.rs:60`（FAIL 无 issues → 空清单 → 规划 agent 收到零条回注）这一处黑箱；但 MVP plan-review 现行口径是把**所有**违例（缺 verdict 块、未知 verdict）当 issue 回注规划 agent（`src/sop/planning.rs:114-127`），与裁决 A 的「换全新实例重审」不同机制。影响验收口径（既有单测 `src/sop/planning.rs:315-328` 的期望值要不要改、`issues_total` 记不记违例）。
  备选：A 窄（只把「FAIL 无 issues」判违例，其余违例维持回注口径）／B 宽（plan-review 全部协议违例都换新实例重审）。
  用户裁决：机制也统一
- [必须裁决] **Q3 · 审查实例级失败耗尽后的最终处置** —— 现状：change 侧 `review_once`（`src/change/session.rs:220-225`）把空回复当 System 上抛 → 触发**整 run** 自动重启（重跑整个 testing 段）；模型通道错误是 `DevforgeError::Agent`（`src/engine/client.rs:283-292`）→ 既不重启也不换实例、直接判死。需求只说「前者走重审/基础设施语义」。影响验收口径（耗尽后是 System 还是 Quality、要不要先换新实例）。
  备选：A 换新实例重审 ≤3 → 耗尽后 System（基础设施语义，触发 run 级自动重启）／B 换新实例重审 ≤3 → 耗尽后 Quality／C 不做实例级重试，直接 System 上抛。
  用户裁决：统一走 System 重启（推荐）
- [必须裁决] **Q4 · 「FAIL 未附 issues → 违例」的判定落点** —— 判定放在解析器层（`src/sop/coding_protocol.rs`）会让 **MVP coding 循环**（`src/sop/coding.rs:146-170`，同一个解析器）同步受益（它的 FAIL 空箱现在会回注 coding agent 一条空清单）；放在 change 侧调用点则不动共享解析器、判定写两处。影响验收口径（MVP coding 循环的行为变不变）。
  备选：A 放共享解析器（`src/sop/coding_protocol.rs`）／B 只在 change 侧判。
  用户裁决：解析器层（推荐）
- [自决] **Q5 · 「尾部协议位」的精确判定** —— 取：**`<route>…</route>` 块之后只允许出现 `<issues>` / `<impact-gap>` 协议块与空白**；块之前允许任意散文；`<route>` 块本身独占一行（块内无其他文字）。理由：①「route 之后只允许协议块」正好覆盖两种合法形态（test agent：route + issues；coding agent：route + impact-gap）；②散文在 route **之前**无害（c5 的散文提及就在回复中段，route 之后的散文才是判据）；③「独占一行」把散文里的行内提及（「未触发 `<route>coding</route>`」）直接排除。**不**把 `<issues>` / `<impact-gap>` 的块定位也一起锚到尾部（见 Q10 的遗留登记）——半锚定会让「路由认尾部、明细认全文」这种口径更难解释。
- [自决] **Q6 · 空箱拒收的适用范围** —— `<route>coding</route>` 与 `<route>plan</route>` **对称处理**（两处占位文案同款）。理由：靶名是「空箱必拒收」；`src/change/session.rs:376-391` 两个分支是同一段代码的复制，判负语义完全同款；只修 coding 路由会留下 plan 路由的同款黑箱。
- [自决] **Q7 · 拒收次数与追补次数的形状** —— 复用 MVP 的 3（`src/sop/coding.rs:64` `MAX_CONTINUE_NUDGES`）：**第 1～3 次不带 issues 各追补一次，第 4 次仍不带 → Quality**。理由：MVP 同款形状的文案就是「连续 3 次未输出 `<stage-complete>`，判定为质量失败」（`src/sop/coding.rs:161`）而代码在第 4 次才判死（`nudges > MAX`），裁决 B 的「连续 3 次仍不带才 Quality」在两种读法下都成立，取与 MVP 同形的那一种（对 agent 更宽容，且常量/判定形状复用）。
- [自决] **Q8 · 拒收计数器的生命周期** —— **落在 testing 轮内（session-local），不落断点**。理由：追补发生在**同一次 `run_testing` 调用内**（引擎再问一次 test agent），引擎不会在追补途中死掉 → 计数天然有界；落断点要动 `ChangeState`（`src/change/state.rs:40-62`）并新增缺键兼容面（上一场刚踩过 `Option` 缺键那条线），收益为零。残留：run 因别的原因重启后计数清零（登记在 §6 R9）。
- [自决] **Q9 · 共享机制落点与「预算」生命周期** —— 新模块 `src/sop/review_gate.rs`：两个上限常量（违例 3、实例失败 3）+ 一个「预算」对象（违例计数 / 实例失败计数）+ 一个「换新实例重审」helper（每轮开新实例 → 三通道分类 → 违例重审≤3→Quality／实例失败重审≤3→System／否则交出判负）。**预算由调用方决定生命周期**：MVP coding / testing 循环 = 整个 run 一个（保住既有的「跨阶段累计违例」语义，`src/sop/coding.rs:124`、`:157-166`）；change 侧 = 每次审查调用一个（change 侧一次审查就是一次判负机会）；MVP plan-review = 整个 review 循环一个。
- [自决] **Q10 · 不锚到尾部的两处宽松解析（遗留登记）** —— `<issues>` 块（`src/change/session.rs:197-212`，取第一个出现处、未闭合则吞到文末）与 `<impact-gap>` 块（`src/change/boundary.rs:47-66`，同样取第一个出现处）**本靶不改**：改它们要动越界二分门（`src/change/boundary.rs:112-124`）的取块方式，coding agent 若把声明写在回复中间就会从「认得」变成「不认得」→ 范围膨胀 Quality（本靶已经因为收紧 route 引入同类风险，不再叠加）。登记为遗留（§6 R8）。
- [自决] **Q11 · 真判负通道保持不变** —— 审查给了 **FAIL + 非空 issues** 时，change 侧仍是 Quality 终止（`src/change/session.rs:358-367`、`:426-435`），MVP 侧仍是回注被审 agent（`src/sop/coding.rs:187-193`、`src/sop/testing.rs:215-224`、`src/sop/planning.rs:118-127`）。理由：裁决只硬化「违例」与「实例失败」两条通道；真判负的回注/终止语义是既有产品决策，改它属另一次裁决（且会牵动 D025 的回注口径）。
- [自决] **Q12 · 审查轮次计数** —— change 侧 `reviews` / `plan_reviews()` 从**硬编码 1** 改为**实际开过的审查实例数**（违例重审与实例失败重试都计入）→ 报告里的「审查轮次」字段（`src/change/report.rs`、`src/change/state.rs:68`）如实反映重审。理由：否则协议违例被藏进「1 轮」里，运维看不出发生过重审；MVP 侧本来就是这个口径（`src/sop/coding.rs:146` 的 `review_rounds += 1` 在重审时也加）。
- [自决] **Q13 · 生产者侧提示词要不要补协议契约** —— **补**：`src/config/manifest.rs:697`（test agent 角色提示词）与 `src/change/session.rs:167-180`（`testing_kickoff`）写明「回注必须用 `<issues>` 块逐条列失败明细，路由块与明细放在回复末尾」；`src/config/manifest.rs:610-618`（coding agent 声明块）与 `src/change/session.rs:142-165`（`coding_kickoff`）写明「声明块放在回复末尾」。理由：①现状**从未**要求过 `<issues>` 块（事实 #24）——空箱不是 agent 顽劣，是契约缺失；②路由收紧后「块必须放末尾」若不写进提示词，agent 把块写在中间就会被引擎忽略 → 落进既有终止路径（本靶不引入重推环，§6 R7）。代价：改 change 角色提示词 → e2e 夹具必须重生成（事实 #26，夹具在白名单内）。
- [自决] **Q14 · 文档登记落点** —— 决策台账新增 `## D026 ·`（`docs/plantree/plans/009-change-sop/decisions/README.md`，写明三条裁决 + 三通道分层 + 尾部协议位 + **遗留清单**）；`topics/agents-change.md` 与 `topics/vs-mvp.md` 同步口径（顺带修事实 #31、#32 两处**先于本靶**的文档与代码不一致）；`roadmap.md` 追加本场条目。**不改** `docs/plantree/ideas/selfhost-findings-20261021.md`（该文件按其自述「由操作员维护」，见文件头）——F5 的状态更新与遗留登记由操作员在本场 Settled 后做。
- [自决] **Q15 · doc 侧两处「记遗留不修」的登记落点** —— 记在 D026 的「遗留」小节 + 本计划 §6 R8：①`src/sop/testing_protocol.rs:292` 附近（doc 单元审查解析器不判「FAIL 无 issues」）；②`src/sop/doc_protocol.rs:116` 已判但 doc SOP 的**重审上限/实例失败口径**与本靶不同（doc 侧用 `MAX_DOC_PROTOCOL_VIOLATIONS` / `MAX_DOC_REVIEW_RETRIES` 两套常量，`src/sop/doc_protocol.rs:13`/`:20`）——本靶不动 doc 侧任何代码。

---

## 2 影响面

### 2.1 直接命中

- `src/change/session.rs:214-228` —— `review_once`：change 侧**三个**审查角色共用的唯一审查入口（开一个实例 → 审一次 → 解析）。本靶把它换成共享 helper（三通道分类 + 有界重审），**三处调用点不改调用形态**。
- `src/change/session.rs:250-254` —— plan 审查调用点（`review_once(&mut *self.plan_review, …)`）：协议违例现在直接 Quality（`:256-262`），改后走「换新实例重审」。
- `src/change/session.rs:348-351` —— coding 审查调用点：同上（`:352-357`）。
- `src/change/session.rs:417-419` —— test 审查调用点：同上（`:420-425`）。
- `src/change/session.rs:371-395` —— test agent 回注分支（route 判定 + 空 issues 占位文案合成）：**裁决 B 的落点**（拒收 + 追补，删占位文案）。
- `src/change/session.rs:386-391` —— plan 路由的空 issues 占位文案（「测试阶段判定需求/计划问题（未给出 issues 明细）」）：按 Q6 对称处理。
- `src/change/session.rs:197-212` —— `issues_of`：失败明细的**唯一来源**（本靶不改块定位，见 Q10；但它读的那份回复文本的路由判定变了）。
- `src/change/session.rs:142-165` / `:167-180` —— `coding_kickoff` / `testing_kickoff`：生产者侧运行时提示词（补「`<issues>` 块」「放末尾」契约，见 Q13）。
- `src/change/session.rs:99-118` / `:121-133` —— `coding_review_instruction` / `testing_review_instruction`：审查指令（已含「FAIL 时带 issues」的契约锚点，**本靶不改**）。
- `src/change/session.rs:362` / `:429` / `:438-440` —— 审查轮次计数（`reviews: 1` / `plan_reviews() -> 1`）：按 Q12 改为实际实例数。
- `src/change/boundary.rs:40-45` —— `parse_route`：**Q1 的落点**（只认尾部协议位）。
- `src/change/boundary.rs:102-125` —— `classify`：越界二分门（用 `parse_route` 判申报，`:112`）——路由收紧后，coding agent 的申报块必须在尾部才被认（事实 #10 的三个调用点之一）。
- `src/change/boundary.rs:47-66` —— `parse_impact_gaps`：`<impact-gap>` 块解析（本靶不改，见 Q10；但它与 route 块配对使用）。
- `src/sop/coding_protocol.rs:94-153` —— `parse_review_report`：**Q4 的落点**（FAIL 且 issues 为空 → `protocol_violation = true`）。
- `src/sop/output.rs:40-61` —— `parse_verdict`：MVP plan-review 解析器（Q2/Q4：FAIL 无 issues → 违例，不再产出空清单）。
- `src/sop/output.rs:63-70` —— `conservative_fail`（缺/未知 verdict → 违例原因当 issue）：按 Q2「机制统一」，违例不再当 issue 产出（改成协议的违例通道）。
- `src/sop/planning.rs:105-133` —— MVP plan-review 循环：违例 → 换新实例重审（≤3）→ 超限 Quality（Q2 的落点）。
- `src/sop/coding.rs:146-170` —— MVP coding 循环的审查轮：改用共享 helper + 共享上限常量（Q9）。
- `src/sop/coding.rs:81` —— `MAX_PROTOCOL_VIOLATIONS`：改为引用共享常量（单一事实源）。
- `src/sop/testing.rs:185-215` —— MVP testing 循环的审查轮：同上。
- `src/sop/testing.rs:75` —— `MAX_TEST_PROTOCOL_VIOLATIONS`：同上。
- `src/sop/review_gate.rs:1`（**新文件**，全文） —— 共享机制落点：两个上限常量（违例 3、实例失败 3）+ 预算对象 + 三通道分类 + 换新实例重审 helper（Q9）。
- `src/sop/mod.rs:12-30` —— 模块清单与 re-export：新增 `review_gate` 模块与新符号的导出。
- `src/config/manifest.rs:610-618` —— change coding agent 角色提示词的声明块：补「放在回复末尾」（Q13）。
- `src/config/manifest.rs:697` —— change test agent 角色提示词的回注声明：补「用 `<issues>` 块逐条列 + 放末尾」（Q13）。
- `src/config/manifest.rs:687` —— change test agent 纪律条「回注只写失败现象与摘要，不猜文件位置」：与「逐条 `<issues>` 明细」并列（保留「不猜位置」这条既有纪律，只把载体说清）。
- `src/config/manifest.rs:1334-1374` —— change 六件套 e2e 夹具的生成器与漂移守卫（改了角色提示词 → 夹具必须重生成）。
- `src/server/tests/mod.rs:524-540` —— 「协议违例上限不得走 System」的源码守卫：违例文案迁到新模块后本守卫会**空转**，必须扩到新模块并钉住新分层（违例上限→Quality、实例失败上限→System）。
- `src/change/plan_audit.rs:711-730` / `:735-757` —— 读台账与 agents-change.md 的守卫（本靶要改这两份文档 → 追加式改动不得碰既有锚点）。
- `src/change/coding_gate.rs:490-502` —— 读台账的守卫（追加 D026 不得删改 D023 的登记字样）。
- `tests/change_ask_ring.rs:31` / `:34-45` / `:604-628` —— 硬编码基线与白名单的守卫：本场提交的文件一条都不在旧白名单里 → 必须刷新常量。
- `tests/change_ask_ring.rs:528-546` —— e2e 夹具漂移守卫（改提示词 → 夹具重生成后才绿）。
- `tests/change_failure_context/main.rs:57-58` / `:746-800` —— 锁着「占位文案逐字送达」的既有断言：被本靶推翻 → 按新口径改写。
- `src/change/session.rs:906-931` —— `the_placeholder_issue_text_is_delivered_verbatim`：同上（就地单测）。
- `src/sop/planning.rs:315-328` —— `missing_verdict_is_conservative_fail_then_remediation`：被 Q2「机制统一」推翻 → 按新口径改写。
- `src/sop/output.rs:119-123` / `:125-135` —— `fail_without_issues_block_is_empty_list` / `missing_or_unknown_verdict_is_conservative_fail`：同上。
- `docs/plantree/plans/009-change-sop/decisions/README.md:167-180` —— 决策台账末尾（D025 之后）：追加 `## D026 ·`。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:210` —— 「测试 review FAIL → `<route>coding</route>`」= 与代码不一致的旧口径（事实 #31）+ 本靶要写的新口径落点。
- `docs/plantree/plans/009-change-sop/topics/vs-mvp.md:34-35` —— 差异四「连续 3 次无标记 → 判基础设施故障」（事实 #32）与「质量循环仍无上限」= 本靶之后不再准确。
- `docs/plantree/plans/009-change-sop/roadmap.md:65-73` —— Phase C 段：追加本场条目。
- `tests/e2e/fixtures/change_agents.json:1` —— change 六件套 e2e 夹具：随提示词改动重生成（漂移守卫的两端）。
- `src/server/change_engine.rs:180-189` —— 生产装配点的 `ProdChangeSession { … }` **字面量构造**：Q12 给结构体加 `plan_review_instances` 字段（`src/change/session.rs:54-64`）后，这里必须补 `plan_review_instances: 0` 一行，**否则仓内编译不过**（原 §4 白名单漏了这个文件 → coding 阶段第 1 次往返申报，证据 `src/server/change_engine.rs:180`）。**只补这一行**，不动该文件其余装配逻辑。

### 2.2 调用链（谁调用它 / 它调用谁）

- `src/server/change_engine.rs:66-240` `run_change` → `src/change/orchestrator.rs:119` `run` → `src/change/orchestrator.rs:173` `session.run_coding(...)` → `src/change/session.rs:315` `ProdChangeSession::run_coding` → `src/change/session.rs:348` `review_once` → **共享 helper** → `src/sop/planning.rs:66-70` `ReviewSessionFactory::open` → `src/sop/rpc_bridge.rs:139-145` `RpcReviewFactory::open` → `src/engine/client.rs:56-63` `RpcClient::spawn` → pi 子进程（`change-coding-reviews/review-N`）。
- `src/change/orchestrator.rs:243-250` `session.run_testing(ui)?` → `src/change/session.rs:371` `run_testing` → `src/change/session.rs:417` `review_once` → 同一 helper → `src/server/change_engine.rs:173-178` 的 `change-test-review` 工厂 → `sessions/change-test-reviews/review-N`。
- `src/change/orchestrator.rs:136-160` plan 段（`run_plan` / `fix_plan` → `src/change/session.rs:231` `run_plan_with` → `:250-254` 审查）→ 同一 helper → `src/server/change_engine.rs:150-158` 的 `change-plan-review` 工厂。
- **空箱追补链（本靶新增）**：`src/change/session.rs:372` 判定路由 → `:376-381` 发现 issues 为空 → **再次** `self.test_agent.converse(追补消息, ui)`（同一个长驻 test agent 会话，`src/sop/rpc_bridge.rs:24-65` `RpcPlanSession`）→ 回到 `:372` 重新判定（最多 3 次追补）→ 仍不带 → Quality。
- **共享 helper 的复用面（本靶新增）**：`src/sop/review_gate.rs` 的 helper 被 5 处调用——change 三角色（`src/change/session.rs:250`/`:348`/`:417`）、MVP plan-review（`src/sop/planning.rs:110-114`）、MVP coding（`src/sop/coding.rs:146`）、MVP testing（`src/sop/testing.rs:185`）——**一处实现、一处上限常量**。
- 共享解析器 `src/sop/coding_protocol.rs:94` 的调用方：`src/change/session.rs:227`（经 `review_once`）、`src/sop/coding.rs:153`（MVP coding 循环）、`src/sop/planning.rs` 不用它（用 `src/sop/output.rs:40` 的 `parse_verdict`）、`src/sop/testing.rs:190` 用的是 `src/sop/testing_protocol.rs:174` 的 `parse_test_review_report`（**另一个协议族，本靶不动**，登记在 §6 R8）。
- 判负结果的下游（**不改**）：`src/change/session.rs:358-367`（coding 判负 → Quality）、`:426-435`（test 判负 → Quality）、`src/change/orchestrator.rs:250-267`（RouteCoding 回注）、`:268-276`（RoutePlan 回注）——本靶只改「判负之前的通道分类」，判负之后的处置一律不动。
- 审查轮次计数的下游：`src/change/session.rs:362`/`:429`/`:438-440` → `src/change/orchestrator.rs:150`/`:182`/`:211`/`:244`（`state.review_rounds`）→ `src/change/orchestrator.rs:304` → `src/change/report.rs`（报告的「审查轮次」）——Q12 改后这条链的数值会随重审变化（既有单测 `src/change/orchestrator.rs:477`/`:923`/`:1024` 的期望值在无违例时不变）。
- 夹具重生成链：`src/config/manifest.rs:1334-1352`（生成器）→ `tests/e2e/fixtures/change_agents.json` → `src/config/manifest.rs:1354-1374`（守卫）+ `tests/change_ask_ring.rs:528-546`（孪生守卫）——改角色提示词后两端必须同时绿。
- **装配链（Q12 新字段的编译耦合点）**：`src/server/change_engine.rs:180` 的 `ProdChangeSession { … }` 字面量 → 结构体定义 `src/change/session.rs:54-64`（新增 `plan_review_instances`）→ 字段写入 `src/change/session.rs:274` → 读取 `src/change/session.rs:445-447`（`plan_reviews()`）→ `src/change/orchestrator.rs:150`/`:182`（`state.review_rounds`）→ `src/change/report.rs`（「审查轮次」）。**字面量构造点漏改 = 编译期就红**（不是运行期行为差异），所以它属于「必须跟着改」的那一类。

### 2.3 隐式耦合（grep 几乎拿不到的那类）

- **Quality 与 System 的分岔决定「会不会被自动重启」**：`src/engine/retry.rs:63-68` 的 `is_infra_failure` **只认 `DevforgeError::System`**；`src/server/mod.rs:1002-1015` 的 `map_intervention` 把 Quality 变成明确的 `RunOutcome::Failed`（不重启），其余错误上抛给重试层 → 重启（`src/server/mod.rs:1243-1270`，最多 4 次尝试、退避 10s/30s/90s）。**同一句「判定失败」文案，错误种类选错 = 一个 Quality 变成一次整 run 重启**（用户裁决 Q3 的落点）。
- **「实例失败」有三种来源、三种错误种类**：空回复 → `System`（`src/sop/rpc_bridge.rs:82-88`）、子进程早退 → `System`（`src/engine/client.rs:147-153`）、prompt 失败（模型通道错误）→ `Agent`（`src/engine/client.rs:283-292`）。**`Agent` 类现在既不重启也不换实例**（`is_infra_failure` 不认它）→ 死在 run 上。共享 helper 必须把**三种都收进「实例失败」通道**（否则模型通道错误这一种仍会绕过新机制）。
- **「违例」与「判负」在类型上原本没有区分**：MVP 侧靠 `ReviewReport.protocol_violation`（`src/sop/coding_protocol.rs:80-84`）这一个 bool 区分；change 侧只是**看到 bool 就 Quality**（`src/change/session.rs:256`/`:352`/`:420`），于是「判负」与「违例」在 change 侧被压成同一条终止路径。共享 helper 的三通道分类（判负 / 违例 / 实例失败）是**类型纪律**上的必要升级（`assets/skills/coding-discipline/SKILL.md` 第 7 条：让类型承担约束）。
- **「FAIL 无 issues」在两个协议族里各有一份解析器**：`src/sop/coding_protocol.rs:94`（change 三角色 + MVP coding 用）与 `src/sop/output.rs:40`（MVP plan-review 用）；第三个协议族 `src/sop/testing_protocol.rs:174`（MVP test-review 用）**不判**这一条（它的 FAIL 走 `reroute` 路由，issues 为空时同样会回注一条空清单给被测 agent）——本靶按裁决 C 的范围**记遗留不修**（§6 R8）。
- **`src/server/tests/mod.rs:524-540` 的源码守卫会因文案迁移而空转**：它逐字比对 `src/sop/coding.rs` / `src/sop/testing.rs` / `src/sop/doc_from_zero.rs` 里的旧文案（“…判定为基础设施故障”），本靶把违例文案迁到 `src/sop/review_gate.rs` 之后，那三个文件里就不再有这段字样 → 守卫「绿但没守任何东西」。**必须把守卫扩到新模块**（钉住「违例上限→Quality、实例失败上限→System」）。
- **提示词 ↔ e2e 夹具的漂移守卫**：`src/config/manifest.rs:536`/`:577`/`:634`/`:664`/`:701` 五个 change 角色提示词常量 → `tests/e2e/fixtures/change_agents.json` → 漂移守卫（`src/config/manifest.rs:1354-1374`、`tests/change_ask_ring.rs:528-546`）。本靶**改其中两个**（coding / test agent）→ 夹具**必须**重生成（不重生成 = 直接红）。
- **决策台账的号段与既有守卫**：`src/change/plan_audit.rs:711-730` 与孪生 `tests/change_ask_ring.rs:556-575` 断言「`## D024 ·`」**恰好出现一次**；`src/change/coding_gate.rs:490-502` 断言台账仍含 D023 的三个登记点字样 → 本靶新增的标题必须是 `## D026 ·`（写成 D024/D025 都会让计数变 2 直接红；D025 已被上一场占用）。
- **topic 文档的守卫**：`src/change/plan_audit.rs:735-757` 与孪生 `tests/change_ask_ring.rs:578-597` 断言 agents-change.md 仍含问询环的答复载体 / 供给 / 审点锚点（`[必须裁决]`/`[自决]`/`用户裁决：`、「禁止|不许」、「ask-user-question」、「留痕核对」、「分类恰当性」）→ 本靶改 `agents-change.md:210` 时这些锚点一个都不能碰。
- **两层「白名单」口径不同、不可互相替代**：引擎的越界门（`src/change/orchestrator.rs:183` → `src/change/boundary.rs:102` `classify`）拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188`，= **本变更全部提交文件**）比**本计划 §4 白名单**；而 `tests/change_ask_ring.rs:34-45` 的 `WHITELIST` 常量守的是**coding 步提交**（测试阶段提交走 `tests/change_ask_ring.rs:627` 的 else 分支，只要求 `tests/` 前缀）→ §4 必须覆盖整个提交集合（含新增的 `tests/change_review_protocol/main.rs` 与重生成的夹具），守卫常量只覆盖 coding 步文件。
- **plan 文本里的验收标准必须是列表行**：下游对账引擎只认 `- ` 列表行（`src/change/plan_audit.rs:280-290` 的验收标准解析），写成表格 = 对账永远对不齐（本计划 §5 已按列表行写）。
- **未跟踪产物**：`git status` 里的 `.pi/goals/archived/*.md`、`docs/plantree/changes/`（含本计划）、`tests/e2e/results/*.json` → 不进 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188` `changed_files` → `src/change/orchestrator.rs:183` 越界门，不触发），但会被 `git add -A` 卷进提交 → 白名单外的这些路径一律不许 `git add`（上一场的 a17 就在盯这条：`tests/change_failure_context/main.rs:1082-1100`）。
- **日志面是这条链唯一的取证面**：review 实例的空回复 / 早退会落进 `src/engine/client.rs:88-116` 的 stderr 伴读线程（写 `pi-stderr.log`）与 `tracing::warn!`（落 `$HOME/.devforge/logs/devforge.log.jsonl`，`src/main.rs:143-146`）→ 本靶新增的三条通道各自的日志文案要能区分（「协议违例」「实例失败」「空箱拒收」三类字样互不相同），否则事后无法归因（本靶不扩到 F6 的可观测性体系，只保证**不混淆**）。
- **`src/change/session.rs:371` 的格式残留**：`fn run_testing(&mut self, ui: &mut dyn UserLink) -> Result<TestingTurn> {        let reply = …`（签名与函数体同一行，上一场遗留）→ 本靶要动这个函数，但**不顺手格式化**（最小 diff，§6 R12）。
- **「给结构体加字段」是编译期耦合，白名单要按「全仓构造点」清点**：Rust 的结构体字面量要求**每个**构造点都补上新字段（生产装配 `src/server/change_engine.rs:180`、仓内单测 `src/change/session.rs:625` 起的一批、`tests/change_failure_context/main.rs:771`、`tests/change_ask_ring.rs:179`）→ 原 §4 白名单只列了 `src/change/session.rs` 与两个测试文件，**漏了生产装配点** `src/server/change_engine.rs`。这类缺口在计划阶段**不体现为行为差异**，只在编译期爆 → 白名单清点必须沿「结构体字段变更」反查构造点（§6 R18）。

---

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `src/sop/review_gate.rs`（**新增**） | ①**两个上限常量**（协议违例 3、审查实例失败 3）——单一事实源，`src/sop/coding.rs:81` / `src/sop/testing.rs:75` 改为引用此处；②**「预算」对象**（违例计数 / 实例失败计数），由调用方决定生命周期（Q9）；③**三通道分类**（判负 / 协议违例 / 实例失败）的类型表达（纪律第 7 条）；④**换新实例重审 helper**：每轮 `factory.open()` 开**新**实例 → 审 → 分类：违例 → 计数、>上限 → `DevforgeError::Quality`（超限才 Quality，裁决 A）；实例失败（`Err` 或空回复）→ 计数、>上限 → `DevforgeError::System`（基础设施语义，裁决 Q3）→ 触发 run 级自动重启；判负 → 交出并返回实际开过的实例数（Q12） | 需求 1、2、3；裁决 A、Q3、Q9 |
| `src/sop/mod.rs` | ①`pub mod review_gate;`；②re-export 新常量与共享符号（对外形状随本靶口径变化） | 需求 1、3 |
| `src/sop/coding_protocol.rs` | **Q4 的落点**：`parse_review_report`（`:94-153`）新增判定——**FAIL 且 issues 为空 → `protocol_violation = true`**（违例原因进 `issues` 字段供错误文本用，但**不得**当被审对象的问题回注；参照 `src/sop/doc_protocol.rs:116-124`）。**两处 FAIL 分支都要判**：缺 `<next>` 的分支（`:110-120`）与正常分支（`:145-153`） | 需求 1；裁决 A、Q4 |
| `src/sop/output.rs` | ①`parse_verdict`（`:40-61`）：**FAIL 无 issues → 判违例**（不再产出 `Verdict::Fail { issues: [] }` 空清单）；②`conservative_fail`（`:63-70`）：缺/未知 verdict 归入**同一**违例通道（Q2「机制也统一」）；③`Verdict` 的类型形态要能表达三态（判负 / 违例）——**类型纪律**：不允许用「空清单」兼表「违例」 | 需求 4；裁决 C、Q2、Q4 |
| `src/sop/planning.rs` | MVP plan-review 循环（`:105-133`）改口径：**协议违例 → 换全新实例重审**（≤3）→ 超限 **Quality**；**判负（FAIL + 非空 issues）→ 照旧回注规划 agent**（逐条原文，零校验自动重查，`:118-127` 不动）。既有单测 `:315-328` 的期望值按新口径改写（`issues_total` 不再记违例、规划 agent 不再收到违例回注） | 需求 4；裁决 C、Q2 |
| `src/sop/coding.rs` | ①审查轮（`:146-170`）改用共享 helper（违例重审 / 实例失败重试的机制与上限只有一份）；②`MAX_PROTOCOL_VIOLATIONS`（`:81`）改为引用共享常量；③`review_rounds` 计数语义随 helper 返回的实例数（既有语义「重审也算一轮」保持） | 需求 1；裁决 A、Q9 |
| `src/sop/testing.rs` | ①审查轮（`:185-215`）改用共享 helper；②`MAX_TEST_PROTOCOL_VIOLATIONS`（`:75`）改为引用共享常量 | 需求 1；裁决 A、Q9 |
| `src/change/session.rs` | ①`review_once`（`:214-228`）→ 改用共享 helper（三角色共用一处，**不复制三份**），空回复检查收敛到 helper 一处；②三处「协议违例 → Quality」（`:256-262`/`:352-357`/`:420-425`）改为走 helper 的「换新实例重审」；③`run_testing`（`:371-395`）的 route 分支：**空 issues → 拒收该回注 + 追一条「请补齐失败明细后重发」**（≤3 次追补，第 4 次仍不带 → Quality），**删掉两处占位文案合成**（`:376-381`、`:386-391`）；④审查轮次计数（`:362`/`:429`/`:438-440`）改为实际开过的实例数（Q12）；⑤`coding_kickoff`（`:142-165`）/ `testing_kickoff`（`:167-180`）补生产者侧协议契约（Q13）；⑥就地单测：占位文案那条（`:906-931`）按新口径改写 + 新增各裁决分支的单测 | 需求 1、2、3；裁决 A、B、Q1、Q6、Q12、Q13 |
| `src/change/boundary.rs` | `parse_route`（`:40-45`）改为**只认尾部协议位**（Q1、Q5）：route 块独占一行、其后只允许 `<issues>`/`<impact-gap>` 块与空白；散文提及不算。既有单测（`:266-267`）按新口径改写 + 新增「散文提及不触发」「尾部独立行触发」两条 | 需求 4；裁决 Q1 |
| `src/config/manifest.rs` | ①`CHANGE_CODING_AGENT_PROMPT`（`:610-618`）：声明块写明「放在回复末尾」；②`CHANGE_TEST_AGENT_PROMPT`（`:687`/`:697`）：回注必须用 `<issues>` 块逐条列失败明细、路由块与明细放在回复末尾（保留「不猜文件位置」这条既有纪律）；③夹具生成器/漂移守卫（`:1334-1374`）**不动**（但夹具文件要重生成，见下） | 需求 2；裁决 B、Q13 |
| `src/server/change_engine.rs` | **编译期补字段**：生产装配点的 `ProdChangeSession` 字面量（`:180-189`）必须补 `plan_review_instances: 0` —— Q12 给结构体加了字段（`src/change/session.rs:63`），Rust 要求所有字面量构造点同步补齐，否则**仓内编译不过**（coding 阶段第 1 次往返申报，证据 `src/server/change_engine.rs:180`）。**只补这一行**，不改该文件的装配逻辑与单测 | 需求 3；Q12 |
| `src/server/tests/mod.rs` | 源码守卫（`:524-540`）扩到新模块：钉住「协议违例上限 → `DevforgeError::Quality`」「审查实例失败上限 → `DevforgeError::System`」，并保留对旧文案的逐字比对（防止旧三处把违例上限写成 System 又回来） | 需求 3；裁决 A、Q3 |
| `tests/change_review_protocol/main.rs`（**新增**） | 本靶的**验收对账测试**（黑盒口径，仿 `tests/change_failure_context/main.rs` 的风格：只经公开 API 观察，每条用例首行写「错实现形态」）：逐条对账 §5 验收标准 1-32 | 需求 4 |
| `tests/change_failure_context/main.rs` | ①a13（`:746-800`）与 `PLACEHOLDER_ISSUE` 常量（`:57-58`）锁着**被本靶推翻**的口径（占位文案逐字送达）→ 按新口径改写（占位文案不再产生；空箱走拒收追补）；②**刷新本文件自己的 `BASELINE`（`:32`）与 `WHITELIST`（`:35`）常量**——a16（`:958-1005`）与 a17（`:1030-1100`）把**守卫文件**（`tests/change_ask_ring.rs`）的常量与本文件的常量逐字比对、并扫 `BASELINE..HEAD` 的提交内容 → 本场刷新了守卫常量，这两处常量不跟着刷新就**必红**（a16 比对失败 / a17 把本场提交判成越界）；③a17 的「白名单里的每个文件都要被本场 **coding 步**提交改到」→ 本场 coding 白名单 = §4 去掉唯一那个测试阶段文件（`tests/change_review_protocol/main.rs`）= **19 个**（含本回合补入的 `src/server/change_engine.rs`） | 需求 2、4；裁决 B |
| `tests/change_ask_ring.rs` | 刷新白名单守卫的硬编码常量：`BASELINE`（`:31`）改成本场基线 sha、`WHITELIST`（`:34-45`）改成本场 **coding 步白名单**（= §4 白名单去掉唯一那个测试阶段文件 `tests/change_review_protocol/main.rs`，共 **19 个**：原 18 个 + 本回合补入的 `src/server/change_engine.rs`）——不刷新则 `the_change_commits_stay_inside_the_whitelist`（`:604-628`）把本场提交判成越界 → 必红；`:530` 的夹具漂移守卫与 `:558`/`:578` 的台账/文档守卫**一字不动** | 需求 4 |
| `tests/e2e/fixtures/change_agents.json` | change 六件套 e2e 夹具：随 `src/config/manifest.rs` 两个角色提示词的改动**重生成**（生成器 `src/config/manifest.rs:1334-1352`；漂移守卫 `src/config/manifest.rs:1354-1374` + `tests/change_ask_ring.rs:528-546`） | 需求 2；Q13 |
| `docs/plantree/plans/009-change-sop/decisions/README.md` | 追加 `## D026 ·`（Q14）：登记「审查输出协议硬化」——三条裁决（违例可重审 ≤3、空箱必拒收 + 追补 ≤3、实例失败 ≠ 判负）+ **三通道分层**（判负 / 协议违例 / 实例失败）+ **尾部协议位**路由规则 + **遗留清单**（Q10、Q15）。**只追加、不动既有条目**（`src/change/plan_audit.rs:711-730`、`:735-757`、`src/change/coding_gate.rs:490-502` 及其孪生守卫盯着） | 需求 1、2、3、4 |
| `docs/plantree/plans/009-change-sop/topics/agents-change.md` | ①`:210` 的「测试 review FAIL → `<route>coding</route>`」= **与代码不一致的旧口径**（事实 #31）→ 改成代码的真实语义（test review 判负 → Quality 终止；**违例** → 换新实例重审）；②补本靶的新口径（空箱拒收、尾部协议位）。**问询环锚点一个都不能碰**（`src/change/plan_audit.rs:735-757`） | 需求 1、2、4 |
| `docs/plantree/plans/009-change-sop/topics/vs-mvp.md` | 差异四（`:34-35`）：「连续 3 次无标记 → 判**基础设施故障**」= 与代码不一致（事实 #32，代码判**质量失败**）→ 改正；「质量循环（审查打回、修复轮次）仍无上限」→ 补上「**审查输出违例**的重审有界（≤3）」 | 需求 1、4 |
| `docs/plantree/plans/009-change-sop/roadmap.md` | Phase C 段（`:65-73`）**追加**一条本场次条目（协议硬化三裁决 + 尾部协议位）；**不改**既有条目与历史证据行 | 需求 1、2、3、4 |

### 3.2 受影响不改

| 文件 / 位置 | 为什么可以不改 |
| --- | --- |
| `src/sop/testing_protocol.rs:174-250`（MVP test-review 解析器） | 裁决 C 的范围只点了 change 三角色 + MVP plan-review；MVP test-review 的「FAIL 空清单 → 空回注」是**同款黑箱但不在本靶范围** → 记遗留（§6 R8），本靶不动它的解析与循环 |
| `src/sop/testing_protocol.rs:272-304`（doc 单元审查解析器，`:292` 附近不判违例） | 裁决 C 明确「doc 侧记遗留不修」→ 只作参照，不改 |
| `src/sop/doc_protocol.rs:82-130` | 已判「FAIL 无 issues → 违例」（`:116-124`）= **参照实现**；doc SOP 的重审上限/实例失败口径（`src/sop/doc_protocol.rs:13`/`:20`）与本靶不同 → 记遗留，不改 |
| `src/sop/doc_from_zero.rs:314-360` | doc 侧「实例失败 → 换新实例 → 耗尽后 System」的**既有参照实现**：本靶复用它的语义，不改它的代码（doc 侧不动是裁决 C 的边界） |
| `src/sop/doc_fix.rs:279` 附近 | doc-fix 的协议违例上限（Quality 口径）= 既有实现，本靶不动 |
| `src/sop/rpc_bridge.rs:76-90` | 「空回复 = 基础设施故障」是**传输层**契约（空串不得当审查结论），本靶在会话层再加一层有界重试，两层不冲突；改它会动 MVP 侧的公共面 |
| `src/engine/client.rs:141-155`/`:283-292` | 子进程早退 / prompt 失败的错误种类由传输层给；本靶在 helper 里把 `Err` 统一收进「实例失败」通道，不改传输层 |
| `src/engine/retry.rs:63-68`、`src/server/mod.rs:1002-1015`/`:1243-1270` | run 级重启与 Quality/System 的分岔是既有机制；本靶只**选对**错误种类，不改机制 |
| `src/change/orchestrator.rs:250-267`（RouteCoding 回注）、`:268-276`（RoutePlan 回注） | 回注**之后**的处置（计数、上限、失败上下文写入、Quality 文案）全不变；本靶只保证「进到这两条分支的 failures 必非空」 |
| `src/change/orchestrator.rs:21` `MAX_TEST_REROUTES`、`:252-259` 超限分支 | 回注次数上限与超限语义与本靶无关（空箱追补发生在**进分支之前**） |
| `src/change/testing_gate.rs:115-140`（`route_back_message` / `strip_locations`） | D025 已定的三出口同源口径（c5 已落地）——本靶只保证送进去的 failures 非空，**不动渲染** |
| `src/change/state.rs:40-62`（`ChangeState`） | 拒收计数不落断点（Q8）→ 状态结构不动；`TestFailureContext` 语义不变 |
| `src/change/plan_audit.rs`（审查指令与计划门） | 计划轮的门与指令与本靶无关；但它的两条守卫单测（`:711-730`/`:735-757`）**读的是本靶要改的文档** → 本靶不改 `plan_audit.rs` 本身，只保证那两条守卫仍绿（§3.3） |
| `src/change/coding_gate.rs:87-105`（基线门）、`:188`（`changed_files`）、`:490-502`（台账守卫） | 基线门与回注可达性是 F6/F7 的地盘（上一场已登记为 R1）；`changed_files` 的用法不变；台账守卫只读不改 |
| `src/change/report.rs` | 报告字段口径不变；只有「审查轮次」的**数值**会随 Q12 变化（既有单测在无违例时不变） |
| `src/change/mod.rs` | 本靶不改 change 侧的 re-export 面：新的拒收上限常量放在 `src/change/session.rs`（`pub const`），`devforge::change::session::…` 即可达，不动 `src/change/mod.rs` |
| `src/change/boundary.rs:47-66`（`parse_impact_gaps`） | 见 Q10：`<impact-gap>` 块的定位不锚到尾部（锚了会让 coding agent 的中段声明失效 → 范围膨胀），记遗留 |
| `src/change/session.rs:197-212`（`issues_of`） | 见 Q10：`<issues>` 块定位不锚到尾部（同一理由 + 与 `parse_impact_gaps` 保持同一口径），记遗留 |
| `src/sop/planning.rs:118-127`（判负回注规划 agent） | 判负通道不变（Q11）：只有**违例**不再回注 |
| `src/sop/coding.rs:187-193`、`src/sop/testing.rs:215-224` | 判负回注被审 agent 的通道不变（Q11） |
| `src/config/manifest.rs:536-576`/`:634-663`/`:701-760`（三个 change **审查**角色提示词） | 审查侧提示词**已经**要求「FAIL 时逐条列 issues」（事实 #25）→ 新判定是给既有契约装牙齿，不改提示词（也就不用重生成夹具里这三项） |
| `src/config/manifest.rs:1334-1374`（夹具生成器 + 漂移守卫） | 生成器与守卫本身不动（只重生成产物） |
| `assets/skills/coding-discipline/SKILL.md` | 本变更**遵守**它，不改它（约束写明） |
| `docs/plantree/ideas/selfhost-findings-20261021.md` | 该文件按其自述「由操作员（江晚棠会话）维护」→ 本靶不写；F5 的状态更新与遗留登记由操作员在本场 Settled 后做（Q14） |
| `docs/plantree/plans/010-run-resume/**`、010 台账 | 与本靶无关（续跑语义沿用 010 D002/D003） |
| `tests/e2e/**`（除夹具） | 本靶不新增 e2e 脚本：真链是否回注/是否违例取决于模型行为，断言押在模型行为上 = 假红（证明面全落仓内 Rust 单测 + 仓外集成测试） |

### 3.3 必须回归验证

| 模块 | 为什么风险在这里 |
| --- | --- |
| `src/sop/coding_protocol.rs` 的既有单测（`mod tests` 起于 `:196`） | 解析器是**全仓共用**的（change 三角色 + MVP coding 循环）：`report_fail_fix_with_issues`（`:236`）、`report_fail_missing_next_defaults_fix`（`:270`）、`report_missing_or_unclosed_verdict_is_conservative`（`:216`）、`report_semantic_contradictions_are_conservative`（`:296`）等**既有期望值**（尤其「FAIL 缺 next → issues 为空、protocol_violation = false」）会因新判定而变——改写时必须逐条对齐新口径，且 `protocol_violation` 的三态不能让 PASS 路径变形 |
| `src/sop/output.rs` 的既有单测（`:99-137`） | `verdict_fail_with_issues`（`:107`）不得回归；`fail_without_issues_block_is_empty_list`（`:119`）与 `missing_or_unknown_verdict_is_conservative_fail`（`:125`）锁的是**被本靶推翻**的口径 → 按新口径改写（不是放松断言） |
| `src/sop/planning.rs` 的既有单测（`:246-330`） | `fail_then_remediation_then_pass`（`:271`）、`zero_validation_flow_no_user_ask_during_loop`（`:286`）、`approved_first_round`（`:253`）不得回归；`missing_verdict_is_conservative_fail_then_remediation`（`:315`）按新口径改写（违例 → 换新实例，不再回注规划 agent） |
| `src/sop/coding.rs:434-467` 的两条违例单测 | `protocol_violation_retries_review_not_agent`（`:435`）、`repeated_protocol_violations_fail_fast`（`:450`）是本靶要对齐的**参照行为**；改用共享 helper 后这两条必须仍然绿（`review_rounds == 2`、`issues_total == 0`、超限报错含「协议违例」） |
| `src/sop/testing.rs:492-520` 的两条违例单测 | `protocol_violation_retries_review_not_agent`（`:492`）、`repeated_protocol_violations_fail_fast`（`:511`）：同上，且**不得**触发 doc-fix 移交（`doc_fix_handoffs == 0`） |
| `src/sop/coding.rs`/`:testing.rs` 的其余既有单测（阶段推进、打回、移交恢复、连续无标记上限） | 审查轮改成 helper 后，`review_rounds` / `issues_total` / `stages_completed` 的既有语义不得回归（尤其 `src/sop/testing.rs:454`/`:464`/`:488` 的「账本轮次跨移交累计」） |
| `src/change/session.rs` 的既有就地单测（`mod tests` 起于 `:462`） | `plan_pass_runs_audit_and_review_with_audit_injected`（`:718`）、`plan_fail_carries_issues_back`（`:743`）、`coding_review_failure_fails_the_stage`（`:1075`）、`testing_review_is_invoked_and_can_fail`（`:1100`）、`testing_route_coding_returns_failures`（`:1150`）、`the_placeholder_issue_text_is_delivered_verbatim`（`:906`）等：前五条**不得回归**（判负通道不变），最后一条**按新口径改写**（占位文案不再产生） |
| `src/change/session.rs:1042-1118`（coding 门与审查） | `coding_runs_gates_and_returns_changed_files`、基线红拦截、自证门等机械门行为不得回归（本靶在 `run_coding` 里只动审查调用点） |
| `src/change/boundary.rs:255-300` 的既有单测 | `parse_route`（`:266-267`）与 `classify`（越界二分）的既有期望值按新口径改写；**越界二分的判定**（白名单外无申报 = 范围膨胀）不得回归 |
| `src/change/orchestrator.rs` 的既有单测（`mod tests` 起于 `:330`） | 编排层不动，但 `state.review_rounds` 的数值来源变了（Q12）→ `happy_path_plan_coding_testing_report`（`:470`，断言 `review_rounds == 2`）、`plan_reviews_are_counted_in_report`（`:915`，断言 `== 3`）、`plan_fix_loop_recovers_after_one_rejection`（`:1010`，断言 `== 4`）在**无违例**时数值不变 → 这三条是 Q12 的回归闸门 |
| `src/change/orchestrator.rs:250-267` 的回注分支 | 空箱追补后进分支的 failures 必非空 → 失败上下文（`src/change/state.rs`）的写入内容不再可能是占位文案 |
| `src/server/tests/mod.rs:524-540` | 源码守卫：本靶把违例文案迁到新模块后必须**扩写**它（否则绿但没守任何东西） |
| `src/server/change_engine.rs:180-189`（生产装配点）+ 全仓 `ProdChangeSession` 字面量 | Q12 给结构体加了 `plan_review_instances` 字段 → **每个**字面量构造点都要补（生产装配 `src/server/change_engine.rs:188`、仓内单测 `src/change/session.rs:625` 起的一批、`tests/change_failure_context/main.rs:779`、`tests/change_ask_ring.rs:187`）→ `cargo build` 必须通过（本靶最容易漏的一处编译期耦合）；该文件自带单测（`:280-380`：`ctx_paths_point_at_repo_plan_dir_and_sop_blackboard`（`:312`）、`finalize_writes_report_with_usage_and_wall_clock`（`:346`）、`session_dirs_cover_all_six_profiles`（`:361`））不得回归 |
| `src/server/tests/mod.rs` 的 `quality_failure_path_is_not_retried_end_to_end`（`:493`）/ `quality_error_becomes_failed_and_is_not_retried`（`:566`） | 违例上限 → Quality（不自动重启）、实例失败上限 → System（可重启）这条分岔是本靶的核心语义，必须端到端仍成立 |
| `src/change/plan_audit.rs:711-730`（台账 D024）+ `:735-757`（agents-change.md 锚点） | 读本靶要改的两份文档 → 追加式改动不得碰既有锚点 |
| `src/change/coding_gate.rs:490-502`（台账 D023） | 同上 |
| `tests/change_ask_ring.rs:528-546`（夹具漂移）、`:556-575`（台账）、`:578-597`（文档锚点）、`:604-628`（白名单守卫） | 四条都是**读本靶要改的文件**的守卫：夹具必须重生成、台账必须追加 D026、白名单常量必须刷新 |
| `src/config/manifest.rs:1354-1374`（夹具漂移守卫，孪生） | 同上 |
| `tests/change_failure_context/main.rs`（a1-a17） | 本靶**改写** a13（占位文案那条）+ 刷新本文件的 `BASELINE`/`WHITELIST` 常量；a16（`:958-1005`）与 a17（`:1030-1100`）随之改口径（它们比对的就是守卫文件的常量）——**这两条是本靶最容易漏的红**：守卫常量刷新了而这里没刷新 = 必红。其余十六条不得回归（尤其 a16/a17 锁白名单两层口径、a14 锁台账 D025） |
| `tests/change_review_protocol/main.rs`（本靶新增） | 逐条对账 §5 验收标准 1-32；每条用例首行写「错实现形态」 |
| 仓内单测（`cargo test`） | 本靶的新增/改写单测全在白名单文件的 `#[cfg(test)]` 模块与新增的 `tests/change_review_protocol/main.rs` 内 |
| `tests/e2e/test_26_change_e2e.py`、`tests/e2e/test_27_change_ask_ring.py` | 真链：改了引擎（审查协议 + 路由判定）后 change run 仍要 Settled（本靶不新增 e2e 断言，但要绿） |
| 引擎越界门（`src/change/boundary.rs:102` + `src/change/orchestrator.rs:183`） | §4 白名单必须覆盖**整个提交集合**（含新增的 `tests/change_review_protocol/main.rs` 与重生成的 `tests/e2e/fixtures/change_agents.json`），少写一个 = 范围膨胀 Quality |
| 部署侧（不在仓内） | `~/.devforge/manifest.json` 缓存与 profile 重渲染（`src/main.rs:152`）→ 仓内改完不等于用户态生效（本靶**改了** change 角色提示词 → profile 面影响面比上一场大，须重装/重渲染） |

---

## 4 白名单

- `src/sop/review_gate.rs`（**新增**）
- `src/sop/mod.rs`
- `src/sop/coding_protocol.rs`
- `src/sop/output.rs`
- `src/sop/planning.rs`
- `src/sop/coding.rs`
- `src/sop/testing.rs`
- `src/change/session.rs`
- `src/change/boundary.rs`
- `src/config/manifest.rs`
- `src/server/change_engine.rs`
- `src/server/tests/mod.rs`
- `tests/change_review_protocol/main.rs`（**新增**）
- `tests/change_failure_context/main.rs`
- `tests/change_ask_ring.rs`
- `tests/e2e/fixtures/change_agents.json`
- `docs/plantree/plans/009-change-sop/decisions/README.md`
- `docs/plantree/plans/009-change-sop/topics/agents-change.md`
- `docs/plantree/plans/009-change-sop/topics/vs-mvp.md`
- `docs/plantree/plans/009-change-sop/roadmap.md`

白名单共 **20 个文件** = **coding 步的 19 个** + **测试阶段的 1 个**（`tests/change_review_protocol/main.rs`）。三层「白名单」口径不同、**不可互相替代**：

- **引擎的越界门**（`src/change/orchestrator.rs:183` → `src/change/boundary.rs:102` `classify`）拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188` `changed_files` = 本变更**全部**提交文件，含测试阶段）比**本节清单** → 本节必须覆盖**整个提交集合**（20 个），少写一个 = 差集非空 + 无 `<impact-gap>` 申报 = 判「越界改动未申报（范围膨胀）」直接 Quality。
- **提交内容守卫的 `WHITELIST` 常量**（`tests/change_ask_ring.rs:34-45`，孪生 `tests/change_failure_context/main.rs:35`）守的是**coding 步提交**：提交信息含 ` step ` 的提交里每个文件都必须在常量内；**不含** ` step ` 的提交（测试阶段自己的提交）只许是 `tests/` 前缀的文件（`tests/change_ask_ring.rs:626-632`）→ 本场常量列 **19 个 coding 步文件**（= 本节 20 个去掉 `tests/change_review_protocol/main.rs`），**不加**测试阶段文件（加了就把 a17「白名单里的每个文件都要被本场 coding 步提交改到」打红）。**注意**：`tests/e2e/fixtures/change_agents.json` 随提示词在 **coding 步**重生成 → 属 coding 步文件，**要**在常量里；`src/server/change_engine.rs`（Q12 补字段）同样属 coding 步文件，**要**在常量里。
- **上一场验收测试文件的常量也在这条链上**：`tests/change_failure_context/main.rs:32`/`:35` 的 `BASELINE`/`WHITELIST` 与 a16（`:958-1005`）/a17（`:1030-1100`）比对的就是守卫文件的常量 → 本场刷新守卫常量时**必须**同步刷新它（否则 a16 比对失败、a17 把本场提交判成越界）。

精确清单见上（20 个文件；其中 `tests/change_review_protocol/main.rs` 是**测试阶段**唯一的产出文件，其余 19 个属 coding 步）。

白名单之外不得有任何改动。本计划文件本身（计划阶段的交付物）与工作区里的未跟踪产物（`.pi/goals/archived/*.md`、`docs/plantree/changes/`、`tests/e2e/results/*.json`）都不属 coding 阶段的改动面，一律不许 `git add`。

---

## 5 验收标准

- 给定 change 侧 plan 审查实例第一次返回**缺 `<stage-verdict>`** 的文本、第二次返回 PASS → 引擎**开过两个**审查实例（第二个是全新实例），plan 轮判 Pass，**不**因第一次的违例终止 run。
- 给定 change 侧 coding 审查第一次返回 `<stage-verdict>FAIL</stage-verdict>` + `<next>fix</next>` 且**不带** `<issues>` → 引擎**不**产生「变更编码审查未通过」的终止，而是**开第二个全新审查实例**重审（违例 ≠ 判负，**不**回注 coding agent）；第二个实例返回 PASS → coding 轮正常推进。
- 给定 change 侧 test 审查第一次返回 FAIL 空箱、第二次返回 PASS → 同样开两个实例、testing 轮判 Pass（不产生「变更测试审查未通过」）。
- 给定 change 侧任一审查实例**连续 4 次**返回协议违例 → run 判**质量失败**（`Quality`，不自动重启），错误文本含「协议违例」与轮次计数，且**不**把违例原因回注被审 agent。
- 给定 change 侧审查实例第一次返回**空回复**（进程正常结束但未产出一字）→ 引擎**开第二个全新审查实例**（空回复不得被当成判负），第二个实例返回 PASS → 正常推进。
- 给定 change 侧审查实例**连续 4 次**返回空回复 → run 判**基础设施故障**（`System` → 触发 run 级自动重启，从断点继续），错误文本含「基础设施故障」与轮次计数。
- 给定 change 侧审查实例第一次返回**模型通道错误**（审查调用返回 `Err`）→ 与空回复同口径：换新实例重审，**不**判负、**不**直接判死 run。
- 给定 change 侧 test 审查返回 **FAIL + 非空 issues**（真判负）→ 仍判「变更测试审查未通过」并 Quality 终止（判负通道不因本靶改变），错误文本**逐字含** issues 原文。
- 给定 MVP coding 审查实例返回 **FAIL 空箱** → 引擎**开第二个全新实例**重审，**不**把「审查未通过」回注给 coding agent，且 `issues_total` **不**累计。
- 给定 MVP coding 审查实例**连续 4 次**返回 FAIL 空箱 → 判**质量失败**（不自动重启），错误文本含「协议违例」。
- 给定 `parse_verdict` 的输入是 `<verdict>FAIL</verdict>` 且**无** `<issues>` → 输出**协议违例**（不是 `Fail { issues: [] }` 空清单）。
- 给定 MVP plan-review 的输入是**缺 `<verdict>` 块**的文本 → 同样判**协议违例**；plan-review 循环**开第二个全新审查实例**重审，**不**把违例原因当 issue 回注规划 agent，`issues_total` **不**累计。
- 给定 MVP plan-review **连续 4 次**违例 → 判**质量失败**（不自动重启），错误文本含「协议违例」。
- 给定 MVP plan-review 返回 **FAIL + 非空 issues** → 仍按现状回注规划 agent（逐条原文），规划 agent 收到的清单**非空**。
- 给定 test agent 回复**尾部**是 `<route>coding</route>` 但**不带** `<issues>` → 引擎**拒收**该回注（不产生 `RouteCoding`、**不**合成占位文案「测试阶段判定失败（未给出 issues 明细）」），并向 test agent 追一条**含「请补齐失败明细后重发」**的消息。
- 给定 test agent **连续 4 次**回 `<route>coding</route>` 且都不带 `<issues>` → 第 1、2、3 次各追补一次，第 4 次判**质量失败**（错误文本含「未给出 issues 明细」与连续计数），且**全程没有**出现过占位文案「测试阶段判定失败（未给出 issues 明细）」。
- 给定 test agent 第一次回 `<route>coding</route>` 不带 `<issues>`、第二次回同样的路由但**带** `<issues>`（两条明细）→ 引擎接受该回注，`RouteCoding` 的 failures **逐字等于**两条明细原文（无占位文案、无引擎合成文案）。
- 给定 test agent 回复的**正文中间**出现散文提及 `<route>coding</route>`、**末尾**是 `<stage-complete>1</stage-complete>` → 引擎**不**判回退（不产生 `RouteCoding`），按通过路径继续（进 test review）。
- 给定 test agent 回复**末尾**是 `<route>coding</route>` + `<issues>` → 引擎判回退（`RouteCoding`）——与上一条对称。
- 给定 coding agent 回复**末尾**是 `<route>plan</route>` + `<impact-gap>`（三条目）→ 越界申报照旧被识别（行为不变，仍打回 plan 补边界）。
- 给定 coding agent 回复**中间**出现 `<route>plan</route>` 提及、**末尾**是 `<stage-complete>1</stage-complete>` → 引擎**不**判越界申报（路由只认尾部协议位）。
- 给定 `src/sop/coding_protocol.rs` 的解析器输入是 `<stage-verdict>FAIL</stage-verdict>` + `<next>fix</next>`（无 issues）→ 输出 `protocol_violation = true` 且 `issues` 非空（违例原因）。
- 给定 `src/sop/output.rs` 的解析器输入是 `<verdict>FAIL</verdict>`（无 issues）→ 输出**违例**（不是空清单的 FAIL）。
- 给定 `src/config/manifest.rs` 的 change coding / change test 两个角色提示词 → 都写明「路由块放在回复末尾」，且 test agent 提示词写明「回注必须用 `<issues>` 块逐条列失败明细」；给定 `tests/e2e/fixtures/change_agents.json` → 与 builtin **逐字段一致**（漂移守卫绿）。
- 给定 `src/server/tests/mod.rs` 的源码守卫 → 协议违例上限仍判 `DevforgeError::Quality`、审查实例失败上限判 `DevforgeError::System`（新模块的文案不得回退成「协议违例走 System」）。
- 给定 change 侧审查发生过一次违例重审 → 报告里的「审查轮次」**计入**这次重审（实际开过的实例数），不是恒为 1。
- 给定 `docs/plantree/plans/009-change-sop/decisions/README.md` → 含 `## D026 ·` 一节，写明三条裁决（违例可重审 ≤3 / 空箱必拒收 + 追补 ≤3 / 实例失败 ≠ 判负）、**三通道分层**、**尾部协议位**路由规则与**遗留清单**；既有 `## D024 ·`/`## D025 ·` 条目与问询环锚点**一字不动**。
- 给定 `docs/plantree/plans/009-change-sop/topics/agents-change.md` → `:210` 不再写「测试 review FAIL → `<route>coding</route>`」（与代码不符的旧口径），并写明本靶的新口径；给定 `docs/plantree/plans/009-change-sop/topics/vs-mvp.md` → 差异四里「判基础设施故障」改成「判质量失败」，并补「审查输出违例的重审有界（≤3）」。
- 给定 `tests/change_ask_ring.rs` 的两个常量 → `BASELINE` 等于本场基线 sha `ad485549e0981b402bd53d64f2dc7e07c79e47b7`、`WHITELIST` 逐个列出本场 **coding 白名单的 19 个文件**（= §4 白名单去掉唯一那个测试阶段文件 `tests/change_review_protocol/main.rs`；含新增的 `src/sop/review_gate.rs` 与本回合补入的 `src/server/change_engine.rs`；**不**含 `tests/change_review_protocol/main.rs`），且 `the_change_commits_stay_inside_the_whitelist` 对 `BASELINE..HEAD` 的扫描通过。
- 给定 `tests/change_failure_context/main.rs` 的两个常量 → 与 `tests/change_ask_ring.rs` 的两个常量**逐字一致**（同一基线、同一 19 个文件的清单），且该文件的 a16、a17 两条用例**绿**（a16 比对的是守卫常量，a17 扫的是本场提交内容）。
- 给定本变更的提交内容 → §4 白名单（**20 个文件**）之外无任何改动；未跟踪产物一律不 `git add`。
- 给定 `ProdChangeSession` 新增了 `plan_review_instances` 字段 → 仓内**全部**字面量构造点（`src/server/change_engine.rs:180`/`:188`、`src/change/session.rs:625` 起的一批单测构造点、`tests/change_failure_context/main.rs:771`、`tests/change_ask_ring.rs:179`）**都补了该字段**，`cargo build` 编译通过（缺任一处 = 编译不过）。

## 6 风险

- **R1 · 路由收紧会引入一条新的终止路径（本靶最需要盯的一条）** —— 路由块改成「只认尾部协议位」后，**coding / test agent 若把声明块写在回复中间**（现状提示词没要求位置，事实 #24），引擎会**看不见**这次声明：coding 侧 → 落进「越界改动未申报（范围膨胀）」（`src/change/boundary.rs:114-116` → `src/change/orchestrator.rs:186-191`）Quality；test 侧 → 落进「未输出 `<stage-complete>`」（`src/change/session.rs:187-195`）Quality。缓解：①提示词补「放末尾」（Q13，在白名单内）；②`coding_kickoff`/`testing_kickoff` 也补同一句（每轮运行时提示词，`:142-165`/`:167-180`）；③验收标准 22/23 钉住提示词锚点。**残留**：change 侧**没有**「无标记重推环」（MVP 有 `src/sop/coding.rs:60-64` 的 `CONTINUE_NUDGE`），agent 放错位置 = 一次 Quality，而不是被追一次。补重推环属另一次口径（与 F6/F7 相邻），本靶**只登记不实施**。
- **R2 · 尾部协议位规则把「合法但形状不同」的回复判成「无路由」** —— 合法形态有两种（route + `<issues>`；route + `<impact-gap>`），本靶的口径覆盖这两种；若 agent 在协议块**之后**又写了散文总结，则该回复不再被认成尾部协议位 → 同 R1 的后果。缓解同 R1（提示词写明「其后不要再写别的」）。验收标准 19/20 只覆盖两种合法形态，不覆盖「协议块后有散文」——该形态按「不是尾部协议位」处理（这是本口径的**定义**，不是漏测）。
- **R3 · 「FAIL 无 issues → 违例」是给既有契约装牙齿，但模型可能真的会输出空 FAIL** —— 三个 change 审查角色的提示词已经要求「FAIL 时逐条列 issues」（事实 #25），但模型仍可能偶发空 FAIL → 本靶的处置是**换新实例重审 ≤3 → Quality**（不是死循环、不是回注空清单）。若某角色频繁空 FAIL，本靶会把 run 从「静默黑箱」变成「明确的质量失败」——**死亡原因从「不可归因」变成「可归因」是本靶的核心收益**，但**这类 run 会死**（不自动重试）。观测面：`src/change/report.rs` 的「审查轮次」按 Q12 计入重审次数。
- **R4 · 审查实例失败 → System → **整 run** 自动重启** —— 裁决 Q3 的落点：实例失败耗尽后是 `System` → `src/engine/retry.rs:63-68` 认它 → 触发 run 级自动重启（`src/server/mod.rs:1243-1270`，最多 4 次尝试、退避 10s/30s/90s）→ **重跑整个 testing 段**（含回归套件，代价大）。这是「基础设施语义」的既有代价（doc 侧同款，`src/sop/doc_from_zero.rs:329-331`）。缓解：共享 helper 在**实例级**先换新实例重试 ≤3（把大多数瞬时故障挡在 run 级重启之前），只有连续失败才升级到 run 级。残留：真·持续性故障（模型渠道长期挂）仍会耗尽 4 次 run 级尝试。
- **R5 · 共享 helper 动了三个既有循环，回归面比「只改 change 侧」大** —— MVP coding / testing 循环改用共享 helper（Q9/Q10）会碰它们已有的违例重审逻辑（`src/sop/coding.rs:146-170`、`src/sop/testing.rs:185-215`）与计数语义（`review_rounds` / `issues_total` / 跨移交累计）。既有四条违例单测（`src/sop/coding.rs:435`/`:450`、`src/sop/testing.rs:492`/`:511`）是回归闸门。缓解：helper 的「预算」对象让调用方决定计数生命周期（MVP = 整个 run 一个，保留跨阶段累计语义；change = 每次审查调用一个）。**若 review 指出 MVP 侧改动过大**，退路是：MVP 两循环只引用共享常量、保留自己的循环（机制不统一但行为零变化）——本计划按「机制统一」写（用户裁决 Q2 的精神）。
- **R6 · `src/sop/output.rs` 的 `Verdict` 形态变了 = MVP plan-review 的公共面变了** —— `parse_verdict` 的返回类型要能表达「违例」（Q2「机制也统一」），这是 `src/sop/output.rs:19-24` 的**公开枚举**（`src/sop/mod.rs` 有 re-export）→ 仓外若有匹配者会编译不过。本仓是应用不是库（仓内匹配点：`src/sop/planning.rs:114-127` 与其单测）→ 记一笔，不阻塞。替代方案（用 `issues` 空 + 单独 bool 兼表违例）被否：那是「用空清单兼表违例」，正是本靶要消灭的形状（类型纪律）。
- **R7 · `src/server/tests/mod.rs:524-540` 的守卫会因文案迁移而空转** —— 违例上限的文案从三个文件迁到 `src/sop/review_gate.rs` 之后，那三个文件里不再有旧字样 → 守卫「绿但没守任何东西」。本靶**必须**把守卫扩到新模块（钉住 Quality / System 分层），否则等于撤防。
- **R8 · 遗留清单（本靶**明确不修**，登记在 D026）** —— ①`src/sop/testing_protocol.rs:174-250`（MVP test-review 解析器）不判「FAIL 无 issues」→ MVP testing 侧仍有同款空清单回注；②`src/sop/testing_protocol.rs:272-304`（doc 单元审查解析器，`:292` 附近）不判违例；③`src/sop/doc_protocol.rs:82-130` 已判但 doc 侧的重审上限/实例失败口径与本靶不同（两套常量 `src/sop/doc_protocol.rs:13`/`:20`）；④`<issues>`（`src/change/session.rs:197-212`）与 `<impact-gap>`（`src/change/boundary.rs:47-66`）的块定位**未**锚到尾部（散文里提到这两个块名仍会被当成真块解析）——本靶只锚 route（Q5/Q10）；⑤change 侧**无标记重推环**（R1）；⑥`src/change/session.rs:371` 的格式残留（签名与函数体同一行）本靶**不顺手格式化**（最小 diff）。
- **R9 · 拒收计数是 session-local，run 重启后清零（Q8）** —— 追补发生在**同一次 `run_testing` 调用内**，引擎不会在追补途中死掉 → 计数天然有界；但若 run 因别的原因（基础设施重启）重启，计数清零 → 同一类空箱可被反复追补（每次 3 次）。要跨 run 累计就得动 `src/change/state.rs:40-62`（新增断点字段 + 缺键兼容面），本靶**不做**（收益为零、代价是状态面变更）。
- **R10 · 台账撞号 + 三道守卫读着本靶要改的文档** —— 本靶新增 `## D026 ·`（D025 已被上一场占用）；`src/change/plan_audit.rs:711-730` 与孪生 `tests/change_ask_ring.rs:556-575` 断言「`## D024 ·`」**恰好出现一次** → 新标题写成 D024/D025 都会直接红；`src/change/coding_gate.rs:490-502` 断言台账仍含 D023 的三个登记点字样；`src/change/plan_audit.rs:735-757` 与孪生 `tests/change_ask_ring.rs:578-597` 断言 agents-change.md 仍含问询环锚点 → 本靶**只追加、不删改**这些字样。
- **R11 · 提示词改动 → e2e 夹具必须重生成（否则漂移守卫直接红）** —— 本靶改 `src/config/manifest.rs:610-618`/`:687`/`:697` 两个 change 角色提示词 → `tests/e2e/fixtures/change_agents.json` 必须随生成器（`src/config/manifest.rs:1334-1352`）重生成，漂移守卫（`src/config/manifest.rs:1354-1374` + `tests/change_ask_ring.rs:528-546`）才绿。**手改夹具 = 直接红**（它是 builtin 的序列化）。
- **R12 · 「白名单」有三个数目、三个口径** —— §4（20 个，覆盖**整个提交集合**）≠ 提交内容守卫的 `WHITELIST` 常量（19 个，守 coding 步提交）≠ 上一场验收测试文件里那份常量（19 个，被 a16 逐字比对）→ 写错任一层 = 红。逐条见 **R16**（含三层的具体行号）；验收标准 29/30/31/32 钉住两层常量的具体内容。
- **R13 · 缓存/构建产物** —— `target/` 不入白名单；`~/.devforge/manifest.json` 缓存与 profile 重渲染（`src/main.rs:152`）→ 仓内改完不等于用户态生效；本靶**改了** change 角色提示词 → 用户态 profile 必须重渲染才带上新契约。
- **R14 · 既有单测锁着被本靶推翻的三条口径** —— `src/change/session.rs:906-931`（占位文案逐字送达）、`tests/change_failure_context/main.rs:57-58`/`:746-800`（同一条口径的仓外孪生）、`src/sop/planning.rs:315-328`（违例回注规划 agent）、`src/sop/output.rs:119-135`（空清单 / 保守 FAIL）→ 必须按**新口径改写**（不是放松断言、不是删测试），并在提交信息里写清是口径变更；写成「不检查」等于撤防。
- **R16 · 白名单守卫是**三层**耦合的（最容易漏红的一处）** —— ①引擎越界门比的是**计划 §4**（20 个，覆盖全部提交文件）；②提交内容守卫的 `WHITELIST` 常量守的是**coding 步提交**（19 个）——`tests/change_ask_ring.rs:604-628` 与孪生 `tests/change_failure_context/main.rs:1030-1100`（a17）；③**上一场**的验收测试文件 `tests/change_failure_context/main.rs:32`/`:35` 另有一份自己的 `BASELINE`/`WHITELIST` 常量，a16（`:958-1005`）拿它与**守卫文件的**常量逐字比对 → 本场刷新了守卫常量（②）却没刷新③ → a16 必红；刷新了②但把测试阶段文件也列进去 → a17 的「白名单里的每个文件都要被本场 coding 步提交改到」必红。三层数目不同（20 / 19 / 19）且口径不同，写错任一层 = 红。
- **R17 · 本靶的证据面是仓内单测，**真链不覆盖** —— 真链是否违例/是否空箱取决于模型行为 → 断言押在模型行为上 = 假红。验收标准 1-32 全部落在「解析器输出 / 实例开启次数 / 错误种类 / 错误文本 / 提示词锚点 / 文档内容 / 编译通过」这些可机械观测的面上；真链（`tests/e2e/test_26_change_e2e.py`）只作「不回归」的旁证。
- **R18 · 计划阶段的「白名单按需求点清点」漏掉了编译期耦合点（本次往返申报的根因）** —— Q12 要给 `ProdChangeSession` 加 `plan_review_instances` 字段（`src/change/session.rs:63`），Rust 的**字面量构造**要求每个构造点同步补齐：生产装配点 `src/server/change_engine.rs:180-189`、仓内单测构造点（`src/change/session.rs:625` 起的一批）、`tests/change_failure_context/main.rs:771`、`tests/change_ask_ring.rs:179`。原 §4 白名单只按「需求点 → 文件」清点，**只列了 `src/change/session.rs` 与两个测试文件**，漏了生产装配点 → coding 阶段编译不过，只能按「不默默改越界文件」的规矩申报。**本回合已补**：§4 白名单 19 → **20**、coding 步常量 18 → **19**、§3.1 新增该文件、§2.1/§2.2/§2.3 补影响面（事实 #33）、验收标准新增第 32 条钉住「所有构造点都补齐且 `cargo build` 通过」。**残留**：①coding 阶段已落在工作区的那一行改动（`src/server/change_engine.rs` 的 `plan_review_instances: 0`）必须在补白名单后的**同一个 change** 里提交，否则越界门（`src/change/orchestrator.rs:183` → `src/change/boundary.rs:102` `classify`）判「越界改动未申报（范围膨胀）」Quality；②两层守卫常量（`tests/change_ask_ring.rs:34-45` 与孪生 `tests/change_failure_context/main.rs:35`）**必须同步加上** `src/server/change_engine.rs`，否则 a17「白名单里的每个文件都要被本场 coding 步提交改到」必红（R12/R16 的三层数目同步 20/19/19）；③更一般的教训：**凡本靶新增结构体字段 / 枚举变体，白名单必须沿「构造点」反查全仓**——这类缺口在计划阶段**没有任何行为差异可供检视**，只在编译期爆（R4/R7 类的「文档-代码不一致」检视法在这里完全无效）。
