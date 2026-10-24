# 变更计划 · change-20261021-coding-agent-c4-path-line-coding-src-cha

目标仓：`/home/trtyr/code-work/devforge`
基线：`5d7bfd023f50fd6463d796cad5bd9c479f03d7c3`（`docs/plantree/changes/change-20261021-coding-agent-c4-path-line-coding-src-cha/baseline.txt`）
上一场：`change-20261021-change-sop-path-line-src-config-manifest`（已 Settled，HEAD=`5d7bfd0`）
文档档位：none（计划阶段照常读已有文档；不跑文档流水线）
本场是**自举第四场·单靶**：只治「回注消息**没有送达通道**」——把测试失败明细真正送到 coding agent 的输入面；**不做**协议硬化（下一场）、不做 MVP 侧同款黑箱、不做 doc 侧排查。

---

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

上一场（c3）的终审发现了一条硬事实：**「让 coding 方能定位」在当前数据流下不可能成立**。回注消息只有两个出口——`src/change/orchestrator.rs:266` 的 Quality 错误文本、`:262` 的 WARN 日志——两者都是**运维/日志面**，不是 coding agent 的**输入面**：

- `src/change/state.rs:40-62` 的 `ChangeState` 没有任何字段存上一轮失败（`:121` 的 `bump_test_reroute` 只计数）；
- `src/change/session.rs:136-152` 的 `coding_kickoff` 无失败上下文；`:302` 的 `run_coding` 也不携带。

本靶要把「送达通道」这条管子接上，具体三件事：

1. **持久化失败上下文**：`ChangeState` 增加字段，存**上一轮测试失败的 issues 逐条原文** + **失败发生的轮次**；测试阶段回注 coding 时写入断点（黑板 `state.json`）。
2. **注入送达**：回注轮（存在上一轮失败上下文）的 coding kickoff 里注入该上下文——标题行「上一轮测试失败明细如下（逐条原文）」+ issues 逐条原文。按 2026-10-21 ui-1 裁决：**保留 `path:line`、不剥位置、不写「请自己定位原因」类话术**。
3. **清除语义**：失败上下文何时清除由本计划设计，但必须**显式登记为决策**（台账 D025），且**断点续跑时表现一致**（续跑要么照样送达同一份上下文，要么照样没有——不能出现「看运气」）。
4. **证明**：单测覆盖「回注写入 → 重跑 kickoff 含原文 → 清除语义」三段；验收标准逐条可机械对账。

**三出口同源**（2026-10-21 用户裁决）：回注内容在三个出口同源——coding kickoff 送达面、`src/change/orchestrator.rs:262` 的 WARN 日志、`:266` 的 Quality 失败文本——都由**同一份 issues 原文**渲染，不再出现「送达面带行号、日志面被剥成 `<位置略>`」的两套内容。因此 `src/change/testing_gate.rs:115` 的 `route_back_message` 也要改口径（不剥定位、删零信息话术），这是本靶口径翻转的**唯一**代码落点。

**明确不做（边界）**：

- 「FAIL 不附 issues 判协议违例 + 有界重审×3 / 拒收追补×3」= **下一场（协议硬化）**的靶；本靶只负责「把**已有的** issues 文本送达」，**空 issues 时引擎合成占位文案的现状保持不动**（`src/change/session.rs:355-360`）。
- MVP 侧 `src/sop/output.rs:60` 的同款黑箱、doc 侧排查，本靶不碰。
- 生产者侧提示词（`src/config/manifest.rs:687`）**不改**：本靶只搬运 test agent **已经给出**的 issues 文本，不改「test agent 该产出什么」。
- 不引入新的外部依赖；遵守 `assets/skills/coding-discipline/SKILL.md`（本变更**遵守**它，不改它）。

### 1.2 代码事实（定位到行）

| # | 事实 | 位置 |
| --- | --- | --- |
| 1 | 回注消息的三个出口：Quality 错误文本、WARN 日志、Quality 里的 `msg` 副本 | `src/change/orchestrator.rs:262`、`:266`、`:261` |
| 2 | 回注消息模板写死「请自己定位原因（本消息不提供文件位置）」并把所有 `path:line` 剥成 `<位置略>` | `src/change/testing_gate.rs:115-124`、`:126-140` |
| 3 | 断点结构里**没有**失败上下文字段；`bump_test_reroute` 只加计数 | `src/change/state.rs:40-62`、`:121-123` |
| 4 | 断点反序列化失败会**静默冷启动**（`.ok()` → `unwrap_or_else(new)`）；真实向量是**非 Option 且无 `#[serde(default)]`** 的既有字段（`phase` / `baseline_sha`）——**缺键的 `Option<T>` 返回 `None`（不报错）**，未知字段则被忽略 | `src/change/state.rs:79-85`、`:41`、`:43`（serde 行为见 serde 1.0.229 运行时的 `private/de.rs:22-61`） |
| 5 | coding kickoff 的正文里没有任何失败上下文 | `src/change/session.rs:136-152` |
| 6 | `run_coding` 每轮先跑「基线轮」再发 coding kickoff | `src/change/session.rs:302-310`、`:84-95` |
| 7 | 编排层调 `run_coding` 时手里只有 `state`（失败上下文只可能住在那里） | `src/change/orchestrator.rs:173` |
| 8 | 测试失败 → 回注 coding 的编排分支（写上下文的地方） | `src/change/orchestrator.rs:250-267` |
| 9 | coding 成功 → 转 testing 的分支（清除点之一） | `src/change/orchestrator.rs:176-179` |
| 10 | testing 通过 → done 的分支（清除点之二） | `src/change/orchestrator.rs:247-248` |
| 11 | issues 的**唯一来源**是 test agent 回复里的 `<issues>` 块（原文，不剥位置） | `src/change/session.rs:184-196`、`:355-360` |
| 12 | 回注上限分支（超限即 Quality，**不投递**） | `src/change/orchestrator.rs:252-259`、`src/change/orchestrator.rs:21` |
| 13 | 既有两条断言锁着旧口径「回注不得带定位」 | `src/change/testing_gate.rs:209-222`、`src/change/orchestrator.rs:602` |
| 14 | 台账 D018「回归失败直接丢回 coding，**不做定位**」= 旧口径的仓内事实源 | `docs/plantree/plans/009-change-sop/decisions/README.md:123` |
| 15 | topic 文档**三处**复述同一旧口径 | `docs/plantree/plans/009-change-sop/topics/agents-change.md:172`、`:211`、`docs/plantree/plans/009-change-sop/topics/vs-mvp.md:24` |
| 16 | test agent 提示词仍要求「回注只写失败现象与摘要，**不猜文件位置**」= 生产者侧旧口径（本靶不改） | `src/config/manifest.rs:687`（常量 `:664`） |
| 17 | 上一场留下的**白名单守卫**用硬编码基线与白名单扫 `BASELINE..HEAD`——本场不刷新它就必红 | `tests/change_ask_ring.rs:31`（`BASELINE`）、`:34-45`（`WHITELIST`）、`:604`（`the_change_commits_stay_inside_the_whitelist`） |

**回注轮的可达性事实（本靶只登记，见 §6 R1）**：`run_coding` 的第一段是「基线轮」，`src/change/coding_gate.rs:87-105` 的 `check_baseline_log` 要求 `evidence/baseline-test.log` 的退出码为 0（「基线不是绿的——不许在红基线上动代码」）。回注轮里测试是红的：agent 若如实覆盖写基线证据（基线轮提示词 `src/change/session.rs:84-95` 要求「跑一遍既有全套」并写 `exit_code: N`），基线门会在 coding kickoff **之前**就把 run 判死 → 送达面根本走不到。送达通道本靶只保证「**kickoff 文本里带着失败明细**」，**可达性**属另一靶。

### 1.3 歧义

- [必须裁决] **Q1 · 回注消息三个出口（coding kickoff 送达面 / `src/change/orchestrator.rs:262` WARN 日志 / `:266` Quality 失败文本）要不要统一口径** —— 现状 `src/change/testing_gate.rs:115` 的 `route_back_message` 会剥掉所有 `path:line` 并写死「请自己定位原因（本消息不提供文件位置）」；若只改送达面，同一个失败会出现两套内容（送达面带行号、日志面被剥成 `<位置略>` + 零信息话术），运维看到的与 coding 拿到的对不上。影响验收口径（回注内容在三个出口分别长什么样、既有两条断言改不改）。
  备选：A 只改送达面（`route_back_message` 与 WARN/Quality 文本保持现状）／B 三出口同源（改 `route_back_message`：不剥定位、删零信息话术，coding 与 plan 两个目标同改）／C 只有 coding 目标改 `route_back_message`（plan 目标仍剥定位）。
  用户裁决：三出口同源（推荐）
- [必须裁决] **Q2 · 本靶决策的登记落点** —— 「失败上下文的清除语义」按需求要**显式登记为决策**；仓内的决策事实源是 `docs/plantree/plans/009-change-sop/decisions/README.md`（现登记至 D024，`:167-174`），而**未实施的 c3 场计划已把 D025 号用在自己那条决策上**（判负必须可诊断）。影响验收口径（台账里有没有本靶这条决策、号段怎么排、既有断言 `tests/change_ask_ring.rs:557` 断言「`## D024 ·` 恰好出现一次」不能被碰坏）。
  备选：A 台账新增 `## D025 ·`（本场决策正式登记）+ 给 D018 加旁注，下一场从 D026 起／B 只在本计划内登记（不动台账文件），D025 号留给下一场。
  用户裁决：用 D025（推荐）
- [自决] **Q3 · 失败上下文里「轮次」的语义** —— 取「第几轮回注」（与 `state.test_reroutes` 同值，首次回注 = 1），不取「第几次 coding 往返」（`state.roundtrips`）。理由：回注轮次才是失败上下文所属的那一轮；`roundtrips` 记的是 plan↔coding 边界往返，两者混用会让「这一轮是第几回注」对不上账。
- [自决] **Q4 · 清除语义（三条规则，见 §5 的对账口径）** —— ①**覆盖**：新一次回注失败**覆盖**旧上下文（不追加）；②**消费即清**：coding 轮成功（转 testing，`src/change/orchestrator.rs:176-179`）时清除；③**终态清**：run 到 done（`src/change/orchestrator.rs:247-248`）时清除。**例外**：回注超限分支（`src/change/orchestrator.rs:252-259`）**既不写也不清**（`:253` 的落盘不碰这个字段）→ 超限前有上下文就原样保留（该分支没有下一次 coding 轮，写了也送不出去）。理由：①test agent 每轮重跑全套，它报出来的就是当前还红的东西，追加只会把已修好的旧失败继续喂给 agent；②上下文是「给下一轮 coding 看的」，coding 一旦过门它就过期了（留着会在 plan 回退重进 coding 时把过期失败再喂一遍）；③收尾不留过期状态。**续跑一致性**：上下文住在断点里，崩溃/重启后照样在同一轮送达同一份（幂等），清除点也照样在同一条状态迁移上生效。
- [自决] **Q5 · 失败上下文怎么从编排层传到提示词层** —— 走 `ChangeSession::run_coding` 的**新增参数** `Option<&TestFailureContext>`（`src/change/orchestrator.rs:78`），而不是往 `ChangeCtx`（`src/change/session.rs:27-40`）塞字段。理由：`ChangeCtx` 在 `src/server/change_engine.rs:245-255` 装配时构造，那时还没有失败上下文（它只在 run 中途产生）；参数是唯一能把「运行期状态」送进提示词层的干净通道，且**类型上不可能忘传**（签名强制）。
- [自决] **Q6 · 送达块的文本形态** —— 固定两段：一行标题「上一轮测试失败明细如下（逐条原文）」+ 每条 issue 一行（`- ` + **原文**，不剥、不改写、不截断）；轮次数字**可以**附在标题行或独立行，验收不依赖它。理由：需求逐字给了标题，标题**逐字可查**才是机械对账的前提；轮次是给人和后续排查看的，钉死它的呈现位置没有收益。
- [自决] **Q7 · 注入条件** —— 「存在失败上下文」才注入；需求原文的「`test_reroutes > 0`」是**必要非充分**条件（回注过但上下文已被清除时不再注入——那是设计上的清除语义，不是漏投）。理由：注入条件与「上下文在不在」一一对应，才能让清除语义可对账。
- [自决] **Q8 · 既有两条断言怎么处理** —— `src/change/testing_gate.rs:209` 与 `src/change/orchestrator.rs:602` 按**新口径改写**（不是放松断言：改成「必须带行号、必须不含零信息话术」），并在提交信息里写清是口径变更。理由：这两条锁的是被本靶推翻的旧口径；留着必红，放松成「不检查」等于把防线撤了。
- [自决] **Q9 · 生产者侧提示词改不改** —— 不改（`src/config/manifest.rs:687` 保持原样）。**生产者侧有两处旧表述，本靶都只登记不改**：①`src/config/manifest.rs:687`（test-agent 系统提示词的纪律条「回注只写失败现象与摘要，不猜文件位置——定位是 coding 的事」）；②`src/change/session.rs:162`（`testing_kickoff` 给 test agent 的「失败时输出 `<route>coding</route>`……并附失败摘要」——这是**每轮运行时提示词**，不是角色提示词）。理由：需求边界写死「本靶只负责把**已有的** issues 文本送达」；这两处管的是 **test agent 该产出什么**，放开它们属生产者侧/协议硬化场（下一场：FAIL 无 issues 判协议违例 + 有界重审×3）。代价：实际送达的 issues 可能**没有** `path:line`（test agent 按现行纪律不猜位置）——这是「不剥位置」的**上限**，不是本靶能补的，登记在 §6 R6。
- [自决] **Q10 · 证明面落在哪** —— 全部落在**仓内 Rust 单测**（`src/change/state.rs` / `src/change/testing_gate.rs` / `src/change/session.rs` / `src/change/orchestrator.rs` 各自的 `#[cfg(test)]` 模块），**不新增 e2e 脚本**。理由：真链是否回注取决于模型行为，把断言押在模型行为上 = 假红；三段证明（写入 / 送达 / 清除）都能在仓内确定性复现。
- [自决] **Q11 · 基线门在回注轮挡住 coding 轮怎么办** —— **本靶只登记不实施**（§1.2 的可达性事实 + §6 R1）。理由：需求「要修什么」四条里没有它，单靶纪律要求范围外只登记；顺带修它会把「基线轮要不要在回注轮跳过」这类口径问题带进来（又要一轮裁决），且与下一场的协议硬化场次撞车。
- [自决] **Q12 · D025 撞号怎么处理** —— 本靶占 `## D025 ·`（用户裁决 Q2），c3 场那条未实施计划里的 D025 在其**实施时**改用 D026。理由：台账号必须唯一且递增；c3 场计划尚未实施，其号段是纸面占位，本靶先落地成事实。
- [必须裁决] **Q13 · 本变更的测试阶段提交文件（`tests/change_failure_context/main.rs`）算不算「白名单」** —— 本变更的提交集合里含测试阶段产出的验收对账测试（步骤 7 的提交 `ebf5d1c`）；计划 §4 白名单原先只列 10 个 coding 步文件 → ①引擎越界门（`src/change/boundary.rs:102` `classify`）拿 `git diff --name-only <baseline>`（**含全部提交文件**）比计划白名单，差集非空且无 `<impact-gap>` 申报 → 判「越界改动未申报（范围膨胀）」直接 Quality（run 判死）；②§5 验收第 17 条按「本变更的提交」对账，白名单不覆盖该提交 → **该条验收本身不成立**。但 `tests/change_ask_ring.rs` 的 `WHITELIST` 常量（`:34-45`）守的是**coding 步提交**（测试阶段提交走 `:627` 的 else 分支，只要求 `tests/` 前缀），且 a16 逐个点名「本场白名单的 10 个文件」→ 那里加第 11 个就把 a16 打红。影响验收口径（白名单到底几个文件、哪一层的白名单覆盖哪些提交）。
  备选：A 测试阶段文件**不进**任何白名单（现状：越界门直接判死）／B 进 §4 计划白名单（11 个）但**不进** `tests/change_ask_ring.rs` 的 `WHITELIST` 常量（仍 10 个 coding 文件）／C 两处都进（11 个，a16 直接红）。
  用户裁决：测试阶段文件是本变更提交的一部分，计入 §4 但不计入 coding 白名单常量 —— 否则会破坏 a16「守卫的 WHITELIST 必须逐个列出本场白名单的 10 个文件」

---

## 2 影响面

### 2.1 直接命中

- `src/change/state.rs:40-62` —— `ChangeState` 结构：新增失败上下文字段（`Option<TestFailureContext>`，`#[serde(default)]`）；这是「持久化上一轮失败」的唯一落点。
- `src/change/state.rs:65-76` —— `ChangeState::new`：新字段初始化为「无上下文」。
- `src/change/state.rs:79-85` —— `ChangeState::from_run`：`serde_json::from_value(...).ok()` 失败即**静默冷启动**（往返计数、测试回注计数、已完成步骤、已对账验收项全部归零）。**准确机制**（已查 serde 1.0.229 运行时源码 `private/de.rs:22-61`：`missing_field` 的注释写着「If the missing field is of type `Option<T>` then treat is as `None`, otherwise it is an error」，其 `deserialize_option` 直接 `visitor.visit_none()`）：**缺键的 `Option<T>` → `None`（不报错）**；**缺键的非 Option 字段**（且无 `#[serde(default)]`）→ `missing_field` 错误 → 被 `.ok()` 吞掉；**未知字段被忽略**（无 `deny_unknown_fields`）→ 新代码写出的新键不会让旧代码读不懂。→ 本靶的新字段是 `Option<TestFailureContext>`，**缺键安全**（旧断点文件照样加载成功、上下文为「无上下文」）；真正的冷启动向量在**既有**的非 Option 字段 `src/change/state.rs:41`（`phase`）与 `:43`（`baseline_sha`）——**本靶不引入、也不修**（见 §6 R2）。
- `src/change/state.rs:88-100` —— `write_into`：新字段随整个结构落进 `state.json`（跨会话保留的载体）。
- `src/change/state.rs:121-123` —— `bump_test_reroute` 旁边：新字段的「写入 / 清除」helper 落在这里（与既有计数 helper 同处）。
- `src/change/orchestrator.rs:250-267` —— `TestingTurn::RouteCoding` 分支：**写失败上下文**的地方（`:251` 计数之后、`:252-259` 上限检查之后、`:264` 落盘之前）。
- `src/change/orchestrator.rs:173` —— 编排层调 `session.run_coding(...)`：**把 `state` 里的失败上下文传下去**的唯一调用点。
- `src/change/orchestrator.rs:78` —— `ChangeSession::run_coding` 的 trait 签名：新增失败上下文参数（送达通道的入口）。
- `src/change/orchestrator.rs:176-179` —— coding 成功转 testing 的分支：**清除点之一**（消费即清）。
- `src/change/orchestrator.rs:247-248` —— testing 通过转 done 的分支：**清除点之二**（终态清）。
- `src/change/orchestrator.rs:329-341` —— `Scripted` 假 session 的结构体与其 `new()`：**新增「本轮收到的失败上下文」字段**的落点（字段块 `:330-337`），供「写入 → 送达 → 清除」单测断言编排层确实把上下文传了下去。
- `src/change/orchestrator.rs:383-393` —— `Scripted` 假 session 的 `run_coding` 实现：签名跟着改；并记录收到的上下文，供「写入 → 送达 → 清除」的单测断言。
- `src/change/orchestrator.rs:53` —— `TestingTurn::RouteCoding` 的文档注释「回归/测试失败 → 回注 coding（**不给定位**）」= 旧口径落点，随口径翻转同步。
- `src/change/orchestrator.rs:260` —— 行内注释「回注只带摘要（不给 path:line 定位）——测试 agent 读行为不读代码」= 旧口径落点（紧贴被改代码，不改就是当场可见的「文档与代码不一致」）。
- `src/change/session.rs:136-152` —— `coding_kickoff`：**送达面**——新增失败上下文参数，并把送达块拼进 kickoff 正文。
- `src/change/session.rs:302-310` —— `ProdChangeSession::run_coding`：把收到的失败上下文透传给 `coding_kickoff`（`:310` 是实际发送点）。
- `src/change/session.rs:184-196` —— `issues_of`：失败明细的**唯一来源**（从 `<issues>` 块取原文，**不剥位置**）→ 本靶不改，但送达内容的「逐条原文」性质由它保证。
- `src/change/session.rs:154-170` —— `testing_kickoff`：`:162` 的「失败时输出 `<route>coding</route>`（计划问题用 `<route>plan</route>`）并**附失败摘要**」= **生产者侧**旧表述（管的是 test agent 该产出什么、产出多细）→ 本靶**只登记不改**（Q9、R6、§3.2）；它与 `src/change/session.rs:355-360` 的占位文案同属下一场协议硬化。
- `src/change/session.rs:355-360` —— test agent `<route>coding</route>` 分支：空 issues 时合成占位文案的现状 —— **本靶保持不动**（边界：属下一场协议硬化）。
- `src/change/testing_gate.rs:115-124` —— `route_back_message`：三出口同源后**不再剥 `path:line`**、**删掉「请自己定位原因（本消息不提供文件位置）」**。
- `src/change/testing_gate.rs:126-140` —— `strip_locations`：剥定位的实现 —— 本靶**删掉**（减法优先；剥了就等于把失败明细二次阉割）。
- `src/change/testing_gate.rs:1-10` —— 模块头注释「**回注不带定位**：bug 打回 coding/plan 时只给失败摘要，**不给文件行号**」= 旧口径的仓内落点，随口径翻转同步。
- `src/change/testing_gate.rs:112-114` —— `route_back_message` 的文档注释（「只给失败摘要，不带文件行号定位」/「不改变『不给定位』这条硬约束」）= 旧口径落点。
- `src/change/testing_gate.rs:209-222` —— 既有单测 `route_back_message_carries_no_location`：锁的是旧口径 → 按新口径改写。
- `src/change/mod.rs:33-36` —— `pub use orchestrator::{ChangeOutcome, ChangeRun, ChangeSession, CodingTurn, PlanTurn, TestingTurn, run as run_change}`：`ChangeSession` 是**对外 re-export 的 trait**，其 `run_coding` 签名变更 = 对仓外实现者的破坏性变更（本仓是应用不是库；仓内实现点只有 `src/change/session.rs:258` 与 `src/change/orchestrator.rs:355` 两处）。
- `src/change/mod.rs:42` —— `pub use state::{CHANGE_SOP, ChangePhase, ChangeState}`：新增类型要一并导出。
- `src/change/mod.rs:43-46` —— `pub use testing_gate::{...}`：新增的送达块渲染函数要一并导出。
- `docs/plantree/plans/009-change-sop/decisions/README.md:123` —— D018「回归失败直接丢回 coding，**不做定位**」：加「定位口径由 D025 改写」的旁注（两边都标，只标一边等于旧决策看着仍有效）。
- `docs/plantree/plans/009-change-sop/decisions/README.md:167-174` —— 台账末尾（D024 之后）：新增 `## D025 ·`（送达通道 + 清除语义 + 回注消息不再剥定位）。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:172` —— 「与 MVP testing 的差别」表第 4 行「测试 Agent 不看代码、**不做定位**」= 旧口径第二落点。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:211` —— 「回归红 → `<route>coding</route>`，**不定位、不分析原因**」= 旧口径第三落点（同块兄弟行 `:210`「测试 review FAIL → `<route>coding</route>`（默认）」说的是**路由**口径，不含定位表述，本靶不动它）。
- `docs/plantree/plans/009-change-sop/topics/vs-mvp.md:24` —— 「**回归先行**：先跑全套既有测试，红 → 直接 `<route>coding</route>`，**不做定位**」= 旧口径的**第 8 处**落点（与 MVP 的差异对照表）。
- `tests/change_ask_ring.rs:31` / `:34-45` / `:604` —— 上一场留下的白名单守卫：`BASELINE`（`:31`）与 `WHITELIST`（`:34-45`）是**硬编码的上一场常量**，`the_change_commits_stay_inside_the_whitelist`（`:604`）扫 `BASELINE..HEAD` → 本场提交的文件（5 个 `src/change/*.rs` + 2 个 topic 文档 + 1 个测试文件）**一条都不在旧白名单** → 本场提交一落地该用例**必红** → 必须随本场刷新常量（本靶把该文件纳入白名单，见 §3.1）。
- `tests/change_failure_context/main.rs:1` —— **测试阶段产出的验收对账测试**（步骤 7 的提交 `ebf5d1c`；a1–a17 十七条用例，逐条对账 §5 验收标准 1–17）：它是**本变更提交集合的一部分** → 必须在 §4 白名单里（否则引擎越界门 `src/change/boundary.rs:102` `classify` 判「越界改动未申报（范围膨胀）」直接 Quality，见 §6 R17），但**不进** `tests/change_ask_ring.rs` 的 `WHITELIST` 常量（那里守的是 coding 步提交，a16 逐个点名 10 个文件）。
- `docs/plantree/plans/009-change-sop/roadmap.md:65-73` —— Phase C 段：**追加**一条本场次条目（不改既有条目与历史证据行）。
- `src/change/coding_gate.rs:87-105` —— `check_baseline_log`（「基线不是绿的——不许在红基线上动代码」）：回注轮可达性的拦路门 —— **本靶只登记不改**（§1.2、§6 R1）。

### 2.2 调用链（谁调用它 / 它调用谁）

- `src/server/change_engine.rs:66` `run_change` → `src/change/orchestrator.rs:119` `run` → `src/change/orchestrator.rs:173` `session.run_coding(...)` → `src/change/session.rs:302` `ProdChangeSession::run_coding` → `src/change/session.rs:310` `converse(coding_kickoff(...))` → **coding agent 的输入面**（送达终点）。
- `src/change/orchestrator.rs:173`（新参数）→ `src/change/session.rs:302`（新参数）→ `src/change/session.rs:136` `coding_kickoff`（新参数）—— 失败上下文的**单向传递链**，全程只读引用，不复制状态。
- `run_coding` 的**生产**调用点**只有** `src/change/orchestrator.rs:173` 一处（`Scripted` 假件在 `src/change/orchestrator.rs:329`、其 `impl ChangeSession for Scripted` 在 `:355`；`src/server/mod.rs:324` 的 `run_coding` 是 **MVP** 的同名方法，与本靶无关）→ trait 签名变更的编译面 = 1 处生产调用 + 2 处实现（`src/change/session.rs:258`、`:355`）+ 6 处单测调用（见上条），共 9 个改动点。
- `src/change/orchestrator.rs:250-267`（写）→ `src/change/state.rs:88-100` `write_into` → `src/change/state.rs:135-143` `save` → 黑板 `state.json` → 下一次 `start` 的 `src/change/state.rs:127-133` `load` → `src/change/state.rs:79-85` `from_run` → `src/change/orchestrator.rs:173`（再送达）—— **跨会话的完整回路**。
- `src/change/orchestrator.rs:261` `route_back_message` → `src/change/testing_gate.rs:115` → 消息进 `src/change/orchestrator.rs:262` 的 WARN 与 `:266` 的 Quality 文本 —— 改 `route_back_message` 后，**日志面与失败原因文本**同时变成原文。
- `src/change/testing_gate.rs:115` `route_back_message` 的调用方：**生产**两处 `src/change/orchestrator.rs:261`（coding 路由）与 `:270`（plan 路由）；**其自身单测**两处 `src/change/testing_gate.rs:210` 与 `:226` —— 改它同时影响 coding 与 plan 两个路由的消息与这两条单测（用户裁决：三出口同源 = 同一条渲染，两个目标同口径）。
- `src/change/session.rs:136` `coding_kickoff` 的调用方**只有** `src/change/session.rs:310`；`src/sop/coding.rs:31` 的同名函数是 **MVP** 的另一个东西（本靶不碰）。
- `src/change/session.rs:302` `run_coding` 的调用方：**生产**只有 `src/change/orchestrator.rs:173`；**单测**另有 6 处调用点 `src/change/session.rs:833`/`:872`/`:899`/`:924`/`:948`/`:970`（对应测试函数**定义**在 `:816`/`:852`/`:882`/`:904`/`:930`/`:954`，调用形态 `session.run_coding(&mut NoUser, 0)`）→ 签名变更要同步这 **6 处调用点**（`:816` 是函数定义行，不是调用点）。

### 2.3 隐式耦合（grep 几乎拿不到的那类）

- **断点反序列化失败 = 静默冷启动（机制已查证，但**不是**本靶新字段带来的）**：`src/change/state.rs:79-85` 的 `.ok()` 会把**反序列化错误**当成「断点不存在」→ `ChangeState::new` → **往返计数、测试回注计数、已完成步骤、已对账验收项全部归零**。**准确机制**（serde 1.0.229 运行时 `private/de.rs:22-61`：`missing_field` 的注释「If the missing field is of type `Option<T>` then treat is as `None`, otherwise it is an error」，`deserialize_option` 走 `visitor.visit_none()`）：**缺键的 `Option<T>` → `None`（不报错）**；**缺键的非 Option 字段**（且无 `#[serde(default)]`）→ `missing_field` 错误 → 被 `.ok()` 吞掉；**未知字段被忽略**（无 `deny_unknown_fields`）。→ ①本靶的新字段 `Option<TestFailureContext>` **缺键安全**：`#[serde(default)]` 对它是语义 no-op（保留只为与既有字段风格一致、意图显式），**不会**造成冷启动；②真实向量是**既有**的 `src/change/state.rs:41`（`phase: ChangePhase`）与 `:43`（`baseline_sha: String`）——这两个字段没有 `#[serde(default)]`，断点文件缺它们就冷启动（`phase` 连 `Default` 都没派；`baseline_sha` 空值倒有兜底：`src/change/orchestrator.rs:124-126` 会用 run 的基线回填）→ **本靶只登记不修**（§6 R2）。
- **两个「轮次」不能混用**：`state.test_reroutes`（测试→coding 回注次数，跨 run 累计，上限 `src/change/orchestrator.rs:21` 的 3）与 `state.roundtrips`（plan↔coding 边界往返次数，上限 3）是两个计数器；失败上下文里的「轮次」取前者（Q3）。
- **回注上限分支既不写也不清**：`src/change/orchestrator.rs:252-259` 在超限时**先落盘再报错**，且 `state.phase` 仍停在 testing → 该分支**不得**写失败上下文（写了就是「没有下一次 coding 轮」的上下文，白占断点、还会被下一轮误送），也**不得**清除既有上下文（`:253` 的落盘不碰这个字段）→ **超限前有上下文就原样保留**（这是 Q4 三条清除规则之外的例外分支）。
- **失败上下文的消费者只有一个**：`src/change/session.rs:136` 的 `coding_kickoff`（coding 轮）。plan 轮（`src/change/session.rs:64` `plan_kickoff`）、testing 轮（`:154` `testing_kickoff`）**不消费**它 → 上下文不进 plan 回注、不进 testing 回注（这两个是另一条路由的失败原因，形态不同、不能混）。
- **WARN 日志是真链的取证面**：`src/change/orchestrator.rs:262` 的 `tracing::warn!` 落在 `$HOME/.devforge/logs/devforge.log.jsonl`（`src/main.rs:143-146` 以 `$HOME` 定位日志目录）→ 口径翻转后，运维从日志里第一次能直接看到 `path:line`（这是本靶口径翻转的**额外收益**，也是 e2e 将来能条件式断言的面）。
- **Quality 失败文本是 run 的失败原因**：`src/change/orchestrator.rs:266` 的文本进 run 状态 → 口径翻转后，run 失败原因里带行号，运维不必翻日志。
- **基线门在送达链前面**：`src/change/session.rs:304-307`（基线轮）→ `src/change/coding_gate.rs:87-105`（退出码必须为 0）→ **之后**才是 `src/change/session.rs:310`（coding kickoff）→ 送达点在基线门**之后**（可达性风险 R1）。
- **test agent 现行纪律禁止它给位置（生产者侧两处旧表述，本靶只登记不改）**：`src/config/manifest.rs:687`「回注只写失败现象与摘要，不猜文件位置——定位是 coding 的事」（角色提示词）＋ `src/change/session.rs:162`「……并附失败摘要」（每轮运行时提示词）→ 送达通道不剥位置，但**上游可能根本没给**位置、甚至只给了「摘要」而非逐条明细（Q9、§6 R6）。这两条纪律与「删掉零信息话术」分处**生产侧/消费侧**，不冲突，但读起来像两套口径 → 风险面已登记。
- **决策台账的号段与既有守卫**：`tests/change_ask_ring.rs:557` 断言台账里「`## D024 ·`」**恰好出现一次** → 本靶新增的标题必须是 `## D025 ·`（写成 D024 会让计数变 2 直接红）。
- **本靶要改的三份文档，各有仓内守卫盯着**（grep 只看得到一个孪生，漏了就会变红）：
  - `src/change/plan_audit.rs:711-730` `decision_ledger_registers_d024_and_annotates_d009`：读 `docs/plantree/plans/009-change-sop/decisions/README.md`，断言「`## D024 ·`」恰好 1 次 + D009 标题含「D024」旁注 → 本靶**追加** D025 不碰这两处即可；
  - `src/change/coding_gate.rs:490-502` `decision_ledger_registers_d023`：同一份台账，断言它仍含「D023」「coding-discipline」「evidence/self-check.log」「只落 change 路径」→ 追加 D025 时**不得删改**这些字样；
  - `src/change/plan_audit.rs:735-757` `agents_change_doc_matches_the_implemented_contract`：读 `docs/plantree/plans/009-change-sop/topics/agents-change.md`，断言它仍含答复载体三个锚点（`[必须裁决]`/`[自决]`/`用户裁决：`）、「禁止|不许」、「ask-user-question」、「留痕核对」、「分类恰当性」→ 本靶改 `:172`/`:211` 时**只改那两行**，问询环的锚点一个都不能动；
  - 另有两道**孪生守卫**在 `tests/change_ask_ring.rs:556-575`（台账 D024）与 `:578-597`（agents-change 验收 25）—— 与上面两条断言内容相同、只是住在集成测试里。
- **提示词 ↔ e2e 夹具的漂移守卫**：`src/config/manifest.rs:536`/`:634`/`:701`/`:664` 的提示词常量 → `tests/e2e/fixtures/change_agents.json`（`src/config/manifest.rs:1338` 生成器产出）→ 漂移守卫（`src/config/manifest.rs:1356`、`tests/change_ask_ring.rs:529`）。本靶**不改任何提示词常量** → 夹具**不需要**重生成（若顺手动夹具 = 直接变红）。
- **未跟踪产物**：`git status` 里的 `.pi/goals/archived/*.md`、`tests/e2e/results/*.json`、`docs/plantree/changes/`（含本计划）→ 不进 `git diff --name-only <baseline>`（不触发越界门），但会被 `git add -A` 卷进提交 → 白名单外的这些路径一律不许 `git add`。
- **越界门的白名单与守卫的白名单是两层、口径不同（本轮缺口就出在这儿）**：引擎的越界门（`src/change/orchestrator.rs:183` → `src/change/boundary.rs:102` `classify`）用的是**计划文件 §4 白名单**（经 `src/change/plan_audit.rs:379` `plan_facts` → `section_paths` 从计划文本抽），比对的 `changed` 是 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188` `changed_files`）= **本变更的全部提交文件**（coding 步 + 测试阶段）；而 `tests/change_ask_ring.rs:34-45` 的 `WHITELIST` 常量守的是**coding 步提交**（测试阶段提交走 `:627` 的 else 分支，只要求 `tests/` 前缀）→ 计划 §4 必须覆盖**整个提交集合**（11 个文件），守卫常量只覆盖 coding 步（10 个文件）：两个口径都叫「白名单」、数目不同，别混（§6 R17）。
- **`ChangeSession` 是对外 re-export 的 trait**：`src/change/mod.rs:33-36` 导出 `ChangeSession` → trait 签名变更对仓外使用者是破坏性变更（本仓是应用不是库，仓内匹配点只有 `src/change/session.rs:258` 与 `src/change/orchestrator.rs:355` 两处实现）。

---

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `src/change/state.rs` | ①`ChangeState`（`:40-62`）新增失败上下文字段 `Option<TestFailureContext>`——**必须是 `Option`**（serde 对**缺键的 `Option<T>` 返回 `None`**：运行时 `private/de.rs:22-61` 的 `missing_field` 助手对 Option 走 `visit_none()`，注释写着「If the missing field is of type `Option<T>` then treat is as `None`」）→ 旧断点文件缺这个键时照样加载成功、上下文为「无上下文」；`#[serde(default)]` 对 Option **语义上是 no-op**，保留只为与既有字段风格一致、意图显式（**不是**「防冷启动」的开关）——冷启动的真实向量是既有的非 Option 字段 `src/change/state.rs:41`（`phase`）/`:43`（`baseline_sha`），本靶只登记不修（§6 R2）；②新增可序列化结构 `TestFailureContext { round: usize, issues: Vec<String> }`（issues = 逐条原文，含 `path:line`；round = 第几轮回注，与 `state.test_reroutes` 同值），**必须派生 `Debug`/`Clone`/`PartialEq`/`Eq`**（`src/change/state.rs:39` 的 `ChangeState` 已派生这四项；嵌套类型不带派生 → `src/change/state.rs:171` 的 `assert_eq!(got, st)` **编译不过**）；③`ChangeState::new`（`:65-76`）初始化为「无上下文」；④在 `bump_test_reroute`（`:121-123`）旁新增「写入上下文 / 清除上下文」helper（状态迁移只走 helper，不散落字段直改）；⑤补单测：序列化往返、旧断点文件（无该键）加载后计数不丢、写入 / 清除语义 | 需求 1、3 |
| `src/change/testing_gate.rs` | ①`route_back_message`（`:115-124`）：**删掉**「请自己定位原因（本消息不提供文件位置）」话术、**不再剥 `path:line`**（三出口同源，用户裁决 Q1）；②**删掉** `strip_locations`（`:126-140`，减法优先——留着就是二次阉割）；③新增**共用的失败明细渲染**（issues 逐条原文，一行一条）供回注消息与 coding 送达块共用，避免同一内容两处渲染漂移；④新增 coding 送达块渲染（标题行「上一轮测试失败明细如下（逐条原文）」+ 明细）；⑤模块头注释（`:1-10`）与 `route_back_message` 文档注释（`:112-114`）同步新口径；⑥既有单测 `:209 route_back_message_carries_no_location` 按新口径改写（必须带行号 + 必须不含零信息话术），并补「失败明细逐条原文」「无零信息话术」的单测 | 需求 2、3、4 |
| `src/change/session.rs` | ①`coding_kickoff`（`:136-152`）：新增失败上下文参数，无上下文时**正文逐字不变**（不注入任何空块）；有上下文时把送达块拼进正文（标题行 + issues 逐条原文，不剥、不改写、不截断）；②`ProdChangeSession::run_coding`（`:302-349`）把收到的上下文透传给 kickoff（`:310`）；③`ChangeSession` 实现（`:302`）的签名跟着改；④补单测：kickoff 文本逐字含 issues 原文（含 `path:line`）、含标题行、不含「请自己定位原因」/「本消息不提供文件位置」/「<位置略>」；无上下文时与改动前逐字一致（既有 `coding_ready`（`:777`）与 `Agent.seen` 假件可直接复用） | 需求 2、4 |
| `src/change/orchestrator.rs` | ①`ChangeSession::run_coding` 签名（`:78`）新增失败上下文参数（送达通道入口）；②调用点（`:173`）把 `state` 里的上下文传下去；③`RouteCoding` 分支（`:250-267`）：在上限检查（`:252-259`）**之后**、落盘（`:264`）**之前**写入失败上下文（round = `state.test_reroutes`，issues = 原文）——**超限分支不写**（没有下一次 coding 轮）；④`InBoundary` 分支（`:176-179`，coding 成功转 testing）：**清除**上下文（消费即清）；⑤`Done` 分支（`:247-248`）：**清除**上下文（终态清）；⑥`Scripted` 假 session（`:383-393`）签名跟着改 + 记录收到的上下文；⑦两处旧口径注释（`:53`、`:260`）同步新口径；⑧既有单测 `:602`（`assert!(!msg.contains("test_a.py:12"))`「回注不得带定位」）按新口径改写（改成「必须带行号」），并补「回注写入 → 重跑送达 → 清除语义 → 续跑一致」的单测 | 需求 1、2、3、4 |
| `src/change/mod.rs` | ①`pub use state::{...}`（`:42`）补 `TestFailureContext`；②`pub use testing_gate::{...}`（`:43-46`）补新的送达块渲染函数（对外形状随本靶口径变化） | 需求 1、2 |
| `docs/plantree/plans/009-change-sop/decisions/README.md` | ①新增 `## D025 ·`（**用户裁决 Q2**：本靶占 D025）：登记「回注失败上下文送达 coding agent」——失败上下文的字段形态（issues 逐条原文 + 轮次）、写入点、**清除语义三条规则**（覆盖 / 消费即清 / 终态清）、续跑一致性、以及「回注消息不再剥定位、不写零信息话术」的口径翻转（用户裁决 Q1）；②给 D018（`:123`「回归失败直接丢回 coding，不做定位」）加**旁注**「定位口径由 D025 改写」（两边都标，只标一边等于旧决策看着仍有效）；③**只追加、不动既有条目**：`src/change/plan_audit.rs:711-730`（D024 恰好一次 + D009 旁注）、`src/change/coding_gate.rs:490-502`（D023 + 三个登记点字样）与它们的孪生 `tests/change_ask_ring.rs:556-575` 都盯着这份文件 | 需求 2、3 |
| `docs/plantree/plans/009-change-sop/topics/agents-change.md` | **两处**旧口径同步为新口径：`:172`（「与 MVP testing 的差别」表第 4 行「测试 Agent 不看代码、**不做定位**」）、`:211`（「回归红 → `<route>coding</route>`，不定位、不分析原因」）—— 改成「回注带失败明细原文（含 `path:line`），失败明细逐条送达 coding agent」；**只改这两行**：问询环的答复载体 / 供给 / 审点锚点（`[必须裁决]`/`[自决]`/`用户裁决：`、「禁止|不许」、「ask-user-question」、「留痕核对」、「分类恰当性」）一个都不能动——`src/change/plan_audit.rs:735-757` 与孪生 `tests/change_ask_ring.rs:578-597` 盯着它们 | 需求 2、3 |
| `docs/plantree/plans/009-change-sop/topics/vs-mvp.md` | `:24`（「**回归先行**：先跑全套既有测试，红 → 直接 `<route>coding</route>`，**不做定位**」）= 与 MVP 差异对照表里的旧口径第 8 处 → 同步为「回注带失败明细原文（含 `path:line`）」 | 需求 2、3 |
| `tests/change_ask_ring.rs` | ①刷新白名单守卫的硬编码常量：`BASELINE`（`:31`）改成本场基线 sha、`WHITELIST`（`:34-45`）改成本场白名单（逐个文件）—— 不刷新则 `the_change_commits_stay_inside_the_whitelist`（`:604`）扫 `BASELINE..HEAD` 时把本场改的文件判成越界 → **必红**；②`:294`/`:529`/`:557` 三条问询环契约断言**一字不动**；③`repo_root()`/`git()` 辅助函数不动 | 需求 4 |
| `tests/change_failure_context/main.rs` | **测试阶段产出的验收对账测试**（步骤 7 的提交 `ebf5d1c`；a1–a17 十七条用例，逐条对账 §5 验收标准 1–17）：它是**本变更提交集合的一部分** → 必须进 §4 白名单，否则 `src/change/boundary.rs:102` `classify`（调用点 `src/change/orchestrator.rs:183`）拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188`，**含全部提交文件**）比计划白名单时差集非空 + 无申报 → 判「越界改动未申报（范围膨胀）」直接 Quality（§6 R17）。**本轮不改它的内容**（文件已定稿；coding 步提交的文件集合本轮未变——步骤 7 只改提交信息，`git diff --name-only <baseline>` 改前改后逐字相同） | 需求 4 |
| `docs/plantree/plans/009-change-sop/roadmap.md` | Phase C 段（`:65-73`）**追加**一条本场次条目（送达通道 + 清除语义 + 口径翻转）；**不改**既有条目与历史证据行（改写历史 = 篡改历史） | 需求 2、3 |

### 3.2 受影响不改

| 文件 / 位置 | 为什么可以不改 |
| --- | --- |
| `src/change/session.rs:355-360` | 空 issues 时合成占位文案的现状 —— 需求边界写死「本靶只负责把**已有的** issues 文本送达」，占位文案属下一场（协议硬化）；本靶的送达通道会**原样搬运**这条占位文案（算作一条 issue 送达），不判它对错 |
| `src/change/session.rs:184-196` `issues_of` | issues 原文的唯一来源，它**本来就不剥位置**；本靶只搬运，不改解析口径 |
| `src/change/session.rs:64` `plan_kickoff` | **消费面**理由：失败上下文的消费者只有 coding 轮（`src/change/session.rs:136` 的 `coding_kickoff`），plan 轮的提示词与失败上下文无关 → 不改 |
| `src/change/session.rs:154-170` `testing_kickoff`（**生产者侧**） | `:162` 的「失败时输出 `<route>coding</route>`（计划问题用 `<route>plan</route>`）并附失败摘要」是**给 test agent 的产出契约**（test agent 该产出什么、产出多细）→ 属下一场协议硬化（c3 计划 Q3：route 无 issues → 拒收 + 追补 ×3），**本靶只登记不改**（Q9、R6）。本靶只保证「**已有的** issues 原文被原样送达」；该文件在白名单内但**只改** `coding_kickoff`（送达面）与 `run_coding`（透传），`:154-170` 一行不动 |
| `src/change/session.rs:84-95` `baseline_kickoff` | 基线轮是「只取证不改代码」的取证轮，不是失败明细的送达面；顺带注入会让取证轮去改代码（与基线轮职责冲突） |
| `src/change/coding_gate.rs:87-105` `check_baseline_log` | 回注轮可达性的拦路门 —— 本靶只登记（§1.2、§6 R1、Q11），改它要另立口径（基线轮在回注轮要不要跳过），属范围外 |
| `src/change/orchestrator.rs:21` `MAX_TEST_REROUTES`、`:252-259` 超限分支 | 回注上限语义与本靶无关；超限分支的**行为**保持（仍 Quality 终止），它**既不写也不清**失败上下文（`:253` 的落盘不碰这个字段）——保持超限前原样 |
| `src/change/orchestrator.rs:269-276` `RoutePlan` 分支 | plan 路由的回注内容随 `route_back_message` 的改动自动同源（用户裁决 Q1：两个目标同口径），但 plan 路由**不写**失败上下文（上下文只服务于 coding 送达面） |
| `src/change/report.rs` | 变更报告（`ChangeReport`）的字段口径不变；失败上下文不进报告（报告是交付物，失败上下文是运行期状态） |
| `src/change/boundary.rs`、`src/change/coding_gate.rs`（其余）、`src/change/history.rs`、`src/change/docs.rs`、`src/change/gate0.rs`、`src/change/prepare.rs`、`src/change/change_id.rs` | 与送达通道 / 失败上下文无关，本靶不扩围 |
| `src/change/plan_audit.rs` | 计划轮的门与审查指令与失败上下文无关（**注意**：`src/change/plan_audit.rs:409` 的收尾块属 c3 场未实施计划的范围，本靶不碰）。但它的两条守卫单测（`:711-730` 读台账、`:735-757` 读 agents-change.md）**读的是本靶要改的文件** → 本靶**不改** `plan_audit.rs` 本身，只保证那两条守卫仍绿（§3.3 必回归） |
| `src/config/manifest.rs`（含 `:664`/`:687` 的 test-agent 提示词、`:536`/`:634`/`:701` 的 review 提示词） | 生产者侧与提示词面本靶不改（Q9）：只搬运**已有的** issues 文本；因此 e2e 夹具 `tests/e2e/fixtures/change_agents.json` **不需要**重生成（动了反而触发漂移守卫变红） |
| `src/sop/**`（`output.rs` / `planning.rs` / `coding.rs` / `testing.rs` / `doc_fix.rs` / `coding_protocol.rs`） | MVP 侧同款黑箱与协议硬化场次，本靶不碰 |
| `src/server/change_engine.rs:66`/`:180`/`:208-209` | 只做装配（构造 `ProdChangeSession`、调编排器），失败上下文经 trait 参数流动，不改这里 |
| `src/runstate/**` | 断点机制本身（原子写、跨会话）不变；本靶只加字段，不改存储 |
| `tests/change_ask_ring.rs` 的**问询环契约断言**（`:294` 审点锚点、`:529` 夹具漂移、`:557` D024 台账计数） | 该文件**要改**（只改 `:31`/`:34-45` 两个常量，见 §3.1），但这三条断言**一字不动**：本靶新增 `## D025 ·` 不影响「`## D024 ·` 恰好出现一次」；本靶不改任何提示词常量 → 夹具不重生成 → `:529` 仍绿 |
| `tests/e2e/**` | 本靶不新增 e2e 脚本（Q10）：真链是否回注取决于模型行为，断言押在模型行为上 = 假红 |
| `assets/skills/coding-discipline/SKILL.md` | 本变更**遵守**它，不改它（约束写明） |
| `docs/plantree/plans/010-run-resume/**`、010 台账 | 与本靶无关（续跑语义沿用 010 D002/D003，不改其登记） |
| `docs/plantree/plans/009-change-sop/roadmap.md` 的既有条目 | 历史交付记录（含证据 commit），改写 = 篡改历史 → 以**追加**方式登记 |

### 3.3 必须回归验证

| 模块 | 为什么风险在这里 |
| --- | --- |
| `src/change/state.rs:146-246` 的既有单测（`mod tests` 起于 `:146`） | `ChangeState` 加了字段：`roundtrip_persists_phase_baseline_and_counters`（`:158` 是**函数定义行**，整结构相等断言在 `:171`）等断言整结构相等（`assert_eq!(got, st)`）→ 新字段初值必须一致，且新类型必须派生 `PartialEq`/`Eq`（否则**编译不过**）；`resume_keeps_roundtrips_across_sessions`（`:201`）是续跑语义的地基 |
| `src/change/state.rs:79-85` `from_run` 的失败分支 | 旧断点文件兼容 = 本靶最危险的静默失败面（丢计数）；必须专门覆盖 |
| `src/change/testing_gate.rs:209-222` `route_back_message_carries_no_location` | **与本靶口径直接冲突**（锁的是「回注不得带定位」）→ 按新口径改写（是口径变更，不是回归红） |
| `src/change/testing_gate.rs` 其余既有单测（`:150` 起的回归/对账/验收用例） | 本模块只动消息渲染与两处注释，既有机械门行为不得回归 |
| `src/change/orchestrator.rs:587` `testing_failure_routes_back_and_is_bounded` | 回注路径的编排行为（回注计数、回到 coding）不得回归；其中 `:602` 的断言按新口径改写 |
| `src/change/orchestrator.rs:637-660` `test_reroutes_are_bounded_across_sessions` | 超限分支不得回归（且新口径下该分支**既不写也不清**失败上下文：超限前有上下文就原样保留） |
| `src/change/orchestrator.rs:466-560` 的其余既有单测 | 阶段推进、越界二分、往返上限、报告口径都不得回归（本靶在 `:173`/`:176-179`/`:247-248`/`:250-267` 四处动了状态迁移） |
| `src/change/session.rs:815-850` `coding_runs_gates_and_returns_changed_files` 等既有单测 | `run_coding` 签名变了（调用点 `:816` 等要跟着改）；既有门行为（基线门 / 自证门 / 步骤对账 / 审查）不得回归 |
| `src/change/session.rs:1037` `testing_route_coding_returns_failures` | 失败明细的**来源**路径不得回归（送达通道的上游） |
| `src/change/mod.rs` 的 re-export 面 | 新增导出不得与既有名字冲突（编译期即错，无需额外验证） |
| `tests/change_ask_ring.rs:604` `the_change_commits_stay_inside_the_whitelist` | **必红守卫**：它用硬编码 `BASELINE`（`:31`）+ `WHITELIST`（`:34-45`）扫 `BASELINE..HEAD`，断言每个「coding 步提交」的文件都在白名单内 → 本场提交的文件一条都不在旧白名单 → 不刷新常量就红；刷新后它守的是「本场 coding 步提交的每个文件 ⊆ 本场白名单」 |
| `src/change/plan_audit.rs:711-730` `decision_ledger_registers_d024_and_annotates_d009` | **读本靶要改的台账**：断言「`## D024 ·`」恰好 1 次 + D009 标题含「D024」旁注 → 本靶追加 D025 时不得动这两处 |
| `src/change/plan_audit.rs:735-757` `agents_change_doc_matches_the_implemented_contract` | **读本靶要改的 agents-change.md**：断言答复载体三锚点 + 「禁止|不许」+ 「ask-user-question」+ 「留痕核对」+ 「分类恰当性」仍在 → 本靶只改 `:172`/`:211` 两行，问询环锚点一个都不能碰 |
| `src/change/coding_gate.rs:490-502` `decision_ledger_registers_d023` | **读本靶要改的台账**：断言台账仍含「D023」「coding-discipline」「evidence/self-check.log」「只落 change 路径」→ 追加 D025 时不得删改这些字样 |
| `tests/change_ask_ring.rs:557` `decision_ledger_registers_d024_and_annotates_d009` | 读本靶要写的台账，断言「`## D024 ·`」**恰好出现一次** → 本靶新增的标题必须是 `## D025 ·`（写成 D024 直接红）；它与 `src/change/plan_audit.rs:711-730` 是**同一断言的孪生**（一个在集成测试、一个在仓内单测），两边都要绿 |
| `tests/change_ask_ring.rs:578-597` `agents_change_doc_matches_the_implemented_contract` | 与 `src/change/plan_audit.rs:735-757` 同断言的孪生（读 agents-change.md 的问询环锚点）→ 本靶改 `:172`/`:211` 时不得碰那些锚点 |
| `tests/change_ask_ring.rs:529` `e2e_fixture_matches_builtin_and_carries_the_ask_package` | 提示词/夹具漂移守卫：本靶**不改**任何提示词常量 → 夹具不得重生成（重生成即越界） |
| `tests/e2e/test_26_change_e2e.py`、`tests/e2e/test_27_change_ask_ring.py` | 真链：改了引擎（回注口径 + 送达面）后 change run 仍要 Settled（本靶不新增 e2e 断言，但要绿） |
| `tests/change_failure_context/main.rs`（a1–a17，十七条用例） | 本变更的验收对账测试：逐条对账 §5 验收标准 1–17。a16 锁 `tests/change_ask_ring.rs` 的两个常量（`WHITELIST` 逐个列出 **coding 白名单的 10 个文件**、不多不少），a17 锁「coding 步提交 ⊆ coding 白名单 + 测试阶段提交只许 `tests/` 前缀」。**两层白名单数目不同（§4 = 11 / 守卫常量 = 10）**，改任一侧都会把另一侧的断言打红 → 本轮两侧都不动 |
| 引擎越界门（`src/change/boundary.rs:102` `classify` + 调用点 `src/change/orchestrator.rs:183`） | 越界门拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188`，**含本变更全部提交文件**，含测试阶段的 `tests/change_failure_context/main.rs`）比**计划 §4 白名单** → §4 少写这个文件 = 差集非空 = 「越界改动未申报（范围膨胀）」Quality（§6 R17） |
| 仓内单测（`cargo test`） | 本靶的新增/改写单测全在白名单文件的 `#[cfg(test)]` 模块内（无新增测试文件） |
| 部署侧（不在仓内） | `~/.devforge/manifest.json` 缓存与 profile 重渲染（`src/main.rs:152`）→ 仓内改完不等于用户态生效（本靶不改提示词，缓存影响面小，但口径翻转涉及引擎二进制，须重装才生效） |

---

## 4 白名单

- `src/change/state.rs`
- `src/change/testing_gate.rs`
- `src/change/session.rs`
- `src/change/orchestrator.rs`
- `src/change/mod.rs`
- `tests/change_ask_ring.rs`
- `tests/change_failure_context/main.rs`
- `docs/plantree/plans/009-change-sop/decisions/README.md`
- `docs/plantree/plans/009-change-sop/topics/agents-change.md`
- `docs/plantree/plans/009-change-sop/topics/vs-mvp.md`
- `docs/plantree/plans/009-change-sop/roadmap.md`

白名单共 **11 个文件** = **coding 步的 10 个** + **测试阶段的 1 个**（`tests/change_failure_context/main.rs`，步骤 7 的提交 `ebf5d1c`）。两层「白名单」口径不同、**不可互相替代**：

- **引擎的越界门**（`src/change/orchestrator.rs:183` → `src/change/boundary.rs:102` `classify`）拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188` `changed_files` = 本变更**全部**提交文件，含测试阶段）比**本节清单** → 本节必须覆盖**整个提交集合**（11 个），少写一个 = 差集非空 + 无 `<impact-gap>` 申报 = 判「越界改动未申报（范围膨胀）」直接 Quality（§6 R17）。
- **`tests/change_ask_ring.rs` 的 `WHITELIST` 常量**（`:34-45`）守的是**coding 步提交**（测试阶段提交走 `:627` 的 else 分支，只要求 `tests/` 前缀）→ 那里**仍列 10 个 coding 白名单文件**、**不**加测试阶段文件（加了就把 a16「守卫的 WHITELIST 必须逐个列出本场白名单的 10 个文件」打红）。

证明面：coding 步的证明全落在白名单文件内既有的 `#[cfg(test)]` 模块（`src/change/state.rs` / `src/change/testing_gate.rs` / `src/change/session.rs` / `src/change/orchestrator.rs`）；测试阶段的证明落在 `tests/change_failure_context/main.rs`（a1–a17 十七条用例，对账 §5 验收标准 1–17）。

白名单之外不得有任何改动（白名单共 11 个文件，上面的清单就是全部）。本计划文件本身（计划阶段的交付物）与工作区里的未跟踪产物都不属 coding 阶段的改动面，一律不许 `git add`（未跟踪产物清单与理由见 §2.3、§6 R12）。

---

## 5 验收标准

- 给定一个新建的 `ChangeState` → 失败上下文字段为「无上下文」；给定 `coding_kickoff(ctx, None)` → 文本**逐字**等于改动前的 kickoff（不含标题行、不含任何空块）。
- 给定编排器收到 `TestingTurn::RouteCoding`，其 `failures` 为两条（一条含 `tests/change_ask_ring.rs:557`，另一条是「接口返回 500」）→ 落盘断点里的失败上下文存在、**轮次为 1**、**issues 逐字等于**这两条原文（含 `tests/change_ask_ring.rs:557`），且回注计数为 1。
- 给定上一条写出的断点文件（换一个会话重新加载同一黑板路径）→ 失败上下文仍在、轮次仍为 1、两条 issues 逐字不变（**跨会话保留**）。
- 给定 coding 轮被调用且传入的上下文是「轮次 1 + 两条 issues」→ 发给 coding agent 的 kickoff 文本含标题行「上一轮测试失败明细如下（逐条原文）」，且**逐字**含两条 issue 原文（含 `tests/change_ask_ring.rs:557`）。
- 给定带失败上下文的 kickoff 文本 → **不含**「请自己定位原因」、**不含**「本消息不提供文件位置」、**不含**「<位置略>」。
- 给定 `route_back_message` 的入参是「coding」+ 一条含 `tests/change_ask_ring.rs:557` 的失败明细 → 文本**逐字**含 `tests/change_ask_ring.rs:557`，且**不含**上述三处零信息/阉割痕迹（送达面与日志面**同源**）。
- 给定 `route_back_message` 的入参是「plan」+ 同一条含 `tests/change_ask_ring.rs:557` 的失败明细 → 同样逐字含 `tests/change_ask_ring.rs:557`、同样不含零信息话术（两个路由目标同口径）。
- 给定 coding 轮成功（改动全在白名单内 → 阶段转 testing）→ 落盘断点里的失败上下文为「无上下文」（**消费即清**）。
- 给定第二次回注（新的 failures 列表）→ 失败上下文的 issues **等于新列表原文**、**不含旧 issues**（**覆盖**语义），轮次为 2。
- 给定测试通过（阶段转 done）→ 落盘断点里的失败上下文为「无上下文」（**终态清**）。
- 给定回注次数已达上限（计数 > 上限 3）→ 报错文本仍含「上限 3」，且失败上下文**既没有被写入新失败、也没有被清除**——落盘断点里的失败上下文与超限前**逐字相同**（超限前没有上下文就仍为「无上下文」，超限前有就原样保留）。
- 给定一份**旧版**断点文件（`change` 段里**没有**失败上下文这个键）→ 加载**成功**、失败上下文为「无上下文」，且往返计数 / 回注计数 / 已完成步骤 / 已对账验收项**逐字不变**（验证的是「新字段是 `Option` → **缺键安全**」：serde 对缺键的 `Option<T>` 返回 `None` 而不是 `missing_field` 错误；这条**不**证明「`#[serde(default)]` 防住了冷启动」——冷启动的真实向量在既有的 `phase`/`baseline_sha`，见 §6 R2）。
- 给定 test agent 回复 `<route>coding</route>` 且**不带** `<issues>` → 引擎仍合成占位文案「测试阶段判定失败（未给出 issues 明细）」，该占位文案作为**一条** issue 原样进入失败上下文，并被**逐字**送进 kickoff（现状保持不动，属下一场协议硬化）。
- 给定 `docs/plantree/plans/009-change-sop/decisions/README.md` → 含 `## D025 ·` 一节，且该节写明失败上下文的字段形态、写入点、**清除语义三条规则**与续跑一致性；D018（`:123`）带「定位口径由 D025 改写」的旁注。
- 给定 `docs/plantree/plans/009-change-sop/topics/agents-change.md` → `:172` 与 `:211` 两处不再写「不做定位 / 不定位、不分析原因」；给定 `docs/plantree/plans/009-change-sop/topics/vs-mvp.md` → `:24` 不再写「不做定位」；三处统一改为「失败明细逐条原文送达 coding agent」。
- 给定 `tests/change_ask_ring.rs` 的两个常量 → `BASELINE` 等于本场基线 sha、`WHITELIST` 逐个列出本场 **coding 白名单的 10 个文件**（= §4 白名单去掉测试阶段文件 `tests/change_failure_context/main.rs`，与 §4 的 coding 部分逐字一致；**不**含测试阶段文件），且 `the_change_commits_stay_inside_the_whitelist` 对 `BASELINE..HEAD` 的扫描通过（本场 coding 步提交的每个文件都在这 10 个文件内；测试阶段提交只许是 `tests/` 下的文件）。
- 给定本变更的提交内容 → §4 白名单（**11 个文件** = coding 步 10 个 + 测试阶段 1 个）之外无任何改动；其中 coding 步提交的文件 ⊆ 那 10 个 coding 白名单文件、测试阶段提交的文件只许是 `tests/` 下的文件；未跟踪产物一律不 `git add`。

---

## 6 风险

- **R1 · 送达通道的「可达性」不归本靶，但会让本靶看起来没生效（最需要盯的一条）**：`run_coding` 的第一段是基线轮（`src/change/session.rs:304-307`），`src/change/coding_gate.rs:87-105` 的 `check_baseline_log` 要求 `evidence/baseline-test.log` 退出码为 0（「基线不是绿的——不许在红基线上动代码」）。回注轮里测试是红的：agent 若如实覆盖写基线证据（基线轮提示词 `src/change/session.rs:84-95` 要求跑一遍既有全套并写真实退出码），基线门会在 **coding kickoff 之前**把 run 判死 → 送达面走不到，表现成「改了也没用」。本靶只保证「kickoff 文本里带着失败明细」；**基线门在回注轮的口径**（跳过？只跑既有套件？）属另一靶（Q11 + §1.2）。缓解：验收标准全部落在「kickoff 文本 + 断点字段」两个可机械观测的面上，不押真链行为。
- **R2 · 冷启动的真实向量在**既有**非 Option 字段（本靶不引入、只登记不修）**：`src/change/state.rs:79-85` 的 `.ok()` → `unwrap_or_else(|| Self::new(...))` 会把**反序列化错误**当成「断点不存在」→ 往返计数、测试回注计数、已完成步骤、已对账验收项**全部归零**。**准确机制**（serde 1.0.229 运行时 `private/de.rs:22-61`：`missing_field` 的注释写着「If the missing field is of type `Option<T>` then treat is as `None`, otherwise it is an error」，`deserialize_option` 走 `visitor.visit_none()`）：**缺键的 `Option<T>` → `None`（不报错）**；**缺键的非 Option 字段**（且无 `#[serde(default)]`）→ `missing_field` 错误 → 被 `.ok()` 吞掉；**未知字段被忽略**（无 `deny_unknown_fields`）。→ ①**本靶的新字段 `Option<TestFailureContext>` 缺键安全**：`#[serde(default)]` 对它是语义 no-op（保留只为风格与意图显式），**不会**造成冷启动 → 上一版计划里「新字段没加 default 就会把在跑的 run 静默重置」的说法**作废**；②真实向量是**既有**的 `src/change/state.rs:41`（`phase: ChangePhase`，连 `Default` 都没派）与 `:43`（`baseline_sha: String`）——断点文件缺这两个键就冷启动；`baseline_sha` 空值有兜底（`src/change/orchestrator.rs:124-126` 用 run 的基线回填），`phase` 没有 → **本靶只登记不修**（给它们加兜底是另一个靶的事：给 `phase` 派 `Default` 等于把「阶段不明」静默当成「plan 阶段」）。
- **R3 · 口径翻转在仓内有多处旧口径落点，只改一处 = 文档与代码不一致**：**口径陈述**落点共 **8 处**——`src/change/testing_gate.rs:7-9`（模块头）、`:112-114`（函数文档注释）、`src/change/orchestrator.rs:53`（枚举文档注释）、`:260`（行内注释）、`docs/plantree/plans/009-change-sop/decisions/README.md:123`（D018 标题）、`docs/plantree/plans/009-change-sop/topics/agents-change.md:172`、`docs/plantree/plans/009-change-sop/topics/agents-change.md:211`、`docs/plantree/plans/009-change-sop/topics/vs-mvp.md:24`——8 处**全部在本靶白名单内**，必须**一起**同步；另 `docs/plantree/plans/009-change-sop/roadmap.md:69`（「回注不带定位」）是**历史证据行**（描述 commit `c862b5f` 当时的口径）→ 按历史不改写，由 D025 旁注消歧。**代码/断言**落点（`src/change/testing_gate.rs:117` 零信息话术、`:126-140` 剥定位实现、`:216` 旧口径断言、`src/change/orchestrator.rs:602` 旧口径断言）已在 §3.1/§3.3 列出；其中 `src/change/testing_gate.rs:112-114` 与 `src/change/orchestrator.rs:260` 紧贴被改代码，不改就是当场可见的不一致。
- **R4 · 两条既有断言锁着被推翻的旧口径**：`src/change/testing_gate.rs:209` `route_back_message_carries_no_location`、`:602`（`assert!(!msg.contains("test_a.py:12"), "回注不得带定位")`）→ 必须按新口径**改写**（改成「必须带行号」+「必须不含零信息话术」），并在提交信息里写清是口径变更；写成「不检查」等于撤防。
- **R5 · 决策台账撞号 + 台账/文档各有守卫盯着**：未实施的 c3 场计划已把 D025 用在自己那条决策上（判负必须可诊断）；本靶按用户裁决占 `## D025 ·`（Q2、Q12），c3 那条在其实施时改用 D026。既有守卫 `tests/change_ask_ring.rs:557`（孪生 `src/change/plan_audit.rs:711-730`）断言「`## D024 ·`」恰好出现一次 → 新标题写成 D024 会让计数变 2 直接红。另有三道守卫读本靶要改的文档：`src/change/coding_gate.rs:490-502`（台账须留 D023 的三个登记点字样）、`src/change/plan_audit.rs:735-757` 与孪生 `tests/change_ask_ring.rs:578-597`（agents-change.md 须留问询环的答复载体 / 供给 / 审点锚点）→ 本靶**只追加、不删改**这些字样。
- **R6 · 生产者侧仍禁止给位置 / 只要求「失败摘要」→ 送达的 issues 可能根本没有 `path:line`，甚至可能没有明细**：两处生产者侧旧表述（**本靶都只登记不改**，见 Q9）：①`src/config/manifest.rs:687`（test-agent 角色提示词纪律条「回注只写失败现象与摘要，**不猜文件位置**——定位是 coding 的事」）；②`src/change/session.rs:162`（`testing_kickoff` 的「失败时输出 `<route>coding</route>`……并附失败摘要」——「摘要」不等于「issues 逐条原文」）。本靶只保证「**不剥**已有内容」，不制造位置、也不把「摘要」升级成「逐条明细」→ 实际送达的 issues 可能**没有** `path:line`。ui-1 裁决的收益在真链上要等生产者侧放开才完全兑现（届时无需改本靶代码）。
- **R7 · 失败上下文的体积无上限**：issues 逐条原文进断点文件（`src/change/state.rs:88-100` 整体序列化），长回归输出可能让 `state.json` 变大。本靶**不截断**（截断 = 丢信息 = 回到黑箱）；风险是断点文件变大，可接受。
- **R8 · 续跑时同一份上下文被重复送达（幂等但可能被重复处理）**：上下文住在断点里，崩溃/重启后照样送达同一份 → agent 可能重复处理同一批失败。理由：宁重复不丢（丢了就回到本靶要治的黑箱）；agent 的实施步骤与提交幂等由既有门（`src/change/coding_gate.rs:256` `check_step_commits`）兜。
- **R9 · `ChangeSession` 是对外 re-export 的 trait，签名变更 = 破坏性变更**：`src/change/mod.rs:33-36` 导出 `ChangeSession`；`run_coding` 加参数后，仓外若有实现者会编译不过。本仓是应用不是库（仓内匹配点只有 `src/change/session.rs:258` 与 `src/change/orchestrator.rs:355` 两处实现）→ 记一笔，不算阻塞。
- **R10 · 同一个失败在三个出口的措辞不完全相同（内容同源 ≠ 文本逐字相同）**：送达面有独立标题行「上一轮测试失败明细如下（逐条原文）」，日志面/失败原因文本走 `route_back_message` 的措辞；**issues 明细本身同源同原文**。若验收要求三处文本逐字相同，则口径要再收一次——本计划按「**明细同源**、措辞各按出口职责」定（用户裁决 Q1 的落点）。
- **R11 · 缓存/构建产物**：`target/` 不入白名单；`~/.devforge/manifest.json` 缓存与 profile 重渲染（`src/main.rs:152`）→ 仓内改完不等于用户态生效（本靶不改提示词，profile 面影响小，但引擎二进制须重装）。
- **R12 · 未跟踪产物污染提交**：`git status` 里的 `.pi/goals/archived/*.md`、`tests/e2e/results/*.json`、`docs/plantree/changes/`（含本计划）→ 未跟踪文件不进 `git diff --name-only <baseline>`（不触发越界门），但会被 `git add -A` 卷进提交 → 提交前逐个核对（本变更按步提交，不许 `-A`）。
- **R13 · e2e 真链的模型不确定性（不押断言）**：真链是否回注取决于模型行为 → 本靶不新增 e2e 断言（Q10），证明全在仓内单测；将来若要 e2e 见证，只能做**条件式**日志守卫（读 e2e 服务器**隔离 HOME** 下的 `.devforge/logs/devforge.log.jsonl`，不是宿主 `~/.devforge/...`），否则恒不触发（假绿）。
- **R14 · 「两个 3」与「两个轮次」不能混用**：`src/change/orchestrator.rs:21` `MAX_TEST_REROUTES = 3`（测试→coding 回注上限，跨 run）与 `src/change/boundary.rs` 的 plan↔coding 往返上限 3 是两个计数器；失败上下文里的「轮次」取前者（Q3）——验收标准第 2/9 条按「轮次 1 / 轮次 2」对账，别与往返次数混。
- **R15 · 本靶不解决的相邻问题（登记不实施）**：①基线门在回注轮的可达性（R1）；②route 无 issues 的拒收/追补（下一场协议硬化，现状占位文案保持）；③MVP 侧 `src/sop/output.rs` 同款黑箱与 doc 侧排查；④生产者侧 test-agent 位置纪律（R6）。四者都不在本靶白名单内，任何「顺手改」都算越界。
- **R16 · 上一场留下的白名单守卫会因本场提交而**必红**（本靶必须随场刷新）**：`tests/change_ask_ring.rs:604` `the_change_commits_stay_inside_the_whitelist` 用硬编码 `BASELINE`（`:31`）+ `WHITELIST`（`:34-45`）扫 `BASELINE..HEAD`，断言每个「coding 步提交」的文件都在白名单内 → 本场提交的文件（5 个 `src/change/*.rs` + 2 个 topic 文档 + 1 个测试文件）**一条都不在旧白名单** → 不刷新常量就变红；且旧 `BASELINE` 指向更早的提交（`3020873`），扫描范围横跨好几场，别的场次一旦越界也会把它点爆。本靶把该文件纳入白名单并刷新两个常量（§3.1），并列入 §3.3 必回归；刷新后它守的是「本场 coding 步提交的每个文件 ⊆ 本场白名单」，而「上一场遗留未提交改动」那两条检查（`src/change/coding_gate.rs`/`src/config/skill_assets.rs`）随 `BASELINE` 前移而变成对本场的断言（本场不改这两个文件 → 仍绿）。
- **R17 · 计划白名单漏写测试阶段产出的文件 → 引擎越界门直接判死（本轮计划缺口，已补）**：本变更的提交集合含测试阶段产出的 `tests/change_failure_context/main.rs`（步骤 7 的提交 `ebf5d1c`），而计划 §4 白名单原先只列 10 个 coding 步文件 → ①**引擎越界门**（`src/change/orchestrator.rs:183` → `src/change/boundary.rs:102` `classify`）拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188` `changed_files`，**含全部提交文件**：coding 步 + 测试阶段）比计划白名单，差集 = `tests/change_failure_context/main.rs` 非空且无 `<impact-gap>` 申报 → 判「越界改动未申报（范围膨胀）」直接 Quality（run 判死）；②**§5 验收第 17 条**「白名单之外无任何改动」按「本变更的提交」对账，白名单不覆盖该提交 → **该条验收本身不成立**。**处置**：§4 补列该文件（白名单 11 个）、§3.1 补一行、§2 影响面与 §6 各记一笔、§5 第 16/17 条口径同步；**但** `tests/change_ask_ring.rs` 的 `WHITELIST` 常量（`:34-45`）**仍列 10 个 coding 文件**（它守的是 coding 步提交；测试阶段提交走 `:627` 的 else 分支，只要求 `tests/` 前缀；a16 逐个点名「本场白名单的 10 个文件」，多列一个即红）。两个口径都叫「白名单」、数目不同（11 vs 10），是本轮缺口的结构性成因 → 下一场起：白名单分两层写（coding 白名单 / 全变更白名单），别再混用。
