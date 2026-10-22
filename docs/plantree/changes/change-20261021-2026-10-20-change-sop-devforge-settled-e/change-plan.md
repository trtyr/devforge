# 变更计划 · change-20261021-2026-10-20-change-sop-devforge-settled-e

目标仓：`/home/trtyr/code-work/devforge`
基线：`30208734c035145ede001958a10ee8a5ea5cd633`（`docs/plantree/changes/change-20261021-2026-10-20-change-sop-devforge-settled-e/baseline.txt`）
上一场：`change-20261020-backnotprop-pstack-github-ai-skills-home`（已 Settled，提交范围 `edbf48d..3020873`）
文档档位：none（计划阶段照常读已有文档；不跑文档流水线）

---

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

上一场让 devforge 用 change SOP 改它自己，第一场跑完了（Settled），但过程中暴露了三个**真实**缺陷。现在按优先级让 devforge 修自己，三件事：

**（1）问询环失效（最高优先）**：第一场 plan agent 在计划里自己列了 6 条「必须用户裁决」的歧义（上一场计划文件的「歧义」小节，A1–A6，每条都带了一个默认值），**一条都没真问用户**，全部按默认值自决放行，整场 `questions_total=0`。调用方的原始诉求是「它会跟你沟通，你再跟我沟通」——**必须用户裁决的条目不许静默自决**。我采用**双层**做法（两层都要，缺一层都还会复发）：

- **供给层（让问询环存在）**：`change-plan-agent` 的 profile 现在**没有** ask-user-question 包（`src/config/manifest.rs:899` 的 `extra_packages: vec![]`），agent 手里根本没有能发 `extension_ui_request` 的工具 → 一条 `ui.request` 都发不出。给 `change-plan-agent` 装上 `npm:@juicesharp/rpiv-ask-user-question`（与 MVP `plan-agent` 同包，`src/config/manifest.rs:22`），并在 change plan 提示词里把口径改成硬约束：**每个「必须裁决」项必须逐条用 ask-user-question 问用户，并把用户答复逐字写进该条目的 `用户裁决：` 字段**。
- **机械层（让没问过的计划过不了审）**：新增一条**确定性**判定（与六块审计同族，落在 `src/change/plan_audit.rs`），三件事同时成立才算过：
  1. **结构契约**：计划必须有「歧义」块，且块内每条列表行都带**分类标记**（`[必须裁决]` 或 `[自决]`）；结构不符 → 直接报问题（**不是 0 条**）。这堵的是「结构没被识别 = 静默自决复发」这个洞。
  2. **问了没**：`[必须裁决]` 项必须带非空 `用户裁决：`，且该答复必须能在**本轮问询记录**里逐字找到（问询记录由引擎侧记录器写进黑板）。
  3. **落点**：判定不过 → 计划轮判 Fail（复用既有打回-修复回路 `src/change/orchestrator.rs:24` 的 `MAX_PLAN_FIXES`，不新增上限），打回消息自带「先用 ask-user-question 问用户」的指令。
  「分类恰当性」（把本该用户裁决的条目标成 `[自决]`）机械门判不了 → 归审查官（见 §1.3 的 Q15）。

**（2）死亡诊断缺失**：第一场夜里 devforge 服务器在 change run 的 testing 阶段两次**静默死亡**——进程直接消失，stderr 无 panic、内核无 OOM 记录、journal 无痕迹，**完全无法归因**。给 devforge 加崩溃取证能力，三件套：

- **panic hook**：把 panic 原文（payload + 位置）写到 **HOME 下固定路径**（`~/.devforge/forensics/panic-<ts>-<run_id>.log`），并带上**当时的阶段/步骤**（进程内运行上下文）。
- **run 心跳/阶段定期落盘**：`~/.devforge/runs/<run_id>/run.json` 记 `{run_id, sop, project, task, pid, attempt, phase, stage, step, started_at_ms, last_beat_ms, status}`，心跳定期刷新、阶段/步骤变更即写 → 异常退出后能知道**死在哪个阶段哪一步**。字段来源见 §1.3 的 Q16（`pid` 记 **server 进程**，不是 pi 子进程；`phase` 由编排器写；`stage` 由会话层写；`step` 取已提交的最大步骤号；`attempt` 取断点里的重试计数）。**登记点在门 0 成功之后**（`src/server/change_engine.rs:111` 之后），门 0 之前的早退**不产生**心跳记录（见 Q17）。
- **非正常退出留验尸文件**：下次启动（或下次 change run 开跑）收割「状态仍是 running 且进程已不在 / 心跳陈旧」的 run 记录，写 `~/.devforge/forensics/autopsy-<run_id>.md`（含 run 身份、最后阶段/步骤、心跳时间、死因判定、相关 panic 文件名）。**设计内的失败（质量失败 / 取消 / 超时）不算异常**——心跳在**登记之后的全部退出路径**都被置为 finished（RAII guard 兜底，见 §1.3 的 Q17）。

**（3）续跑稳健化**：第一场实测暴露「崩溃后续跑时 `ensure_baseline` 把 `baseline.txt` 重记成当前 HEAD，已提交步骤从门禁视野消失」的自锁路径，已由操作员热修一步（step 8：`src/change/gate0.rs`，含单测）。本次做两件事：**审计这条修复是否完备**（记录缺失 / 为空 / 损坏 / 记录的 sha 不在当前历史 / 不是 HEAD 祖先），并**把 change SOP 续跑路径上同类稳健性洞一次补齐、配齐单测**（陈旧证据跨轮沿用、change-id 标记丢失即换号、断点损坏被静默当冷启动、计划文件缺失报成「范围膨胀」、续跑重编已提交步骤号）。

明确不做：不换模型、不换 harness、不引入新的外部依赖（panic hook / 心跳 / 验尸全部用标准库 + 已有依赖）、不改 MVP（planning / coding）路径的既有行为、不写实施顺序与回归命令。

### 1.2 根因定位（第一场为什么失败；代码级）

**问询环失效**——不是「模型忘了问」，是**问询环在供给层就不存在**，机械层也没有兜底：

- `src/config/manifest.rs:899`：`change-plan-agent` 的 `extra_packages: vec![]`；`src/config/manifest.rs:1097` 甚至把「change-plan-agent 不装任何包」钉成了单测契约。只有 MVP 的 `plan-agent` 装了 ask 包（`src/config/manifest.rs:22` 的 `PLAN_AGENT_EXTRA`，用在 `src/config/manifest.rs:785`）。→ change plan agent **没有能发 `extension_ui_request` 的工具**。
- 通道侧是好的、不是坏的：`src/sop/rpc_bridge.rs:24` 的 `RpcPlanSession::converse` 遇 `extension_ui_request` 会回调 `ui.ask`；`src/server/mod.rs:177` 的 `RelayUserLink` 把 `ask` 转成 SSE 的 `ui.request`（`src/server/mod.rs:135`）；change 引擎已经把它交给编排器（`src/server/change_engine.rs:221`）。**缺的只是「Agent 侧有工具」+「计划里必须问」+「机械门兜底」三件事。**
- `CHANGE_PLAN_AGENT_PROMPT`（`src/config/manifest.rs:473`）第 1 块只写「列出无法从需求/代码确定、必须问用户的点（**问了就把答案写进来**）」——既没说「必须问」，也没给写法与载体；`src/change/plan_audit.rs:61` 的机械审计里**没有任何**「必须裁决项是否被问过/答过」的检查 → 第一场那份 6 条 A1–A6 带着默认值一路过审（`src/change/plan_audit.rs:209` 的审查指令只把机械审计结果注入给审查官，审查官也就没看到这条）。
- **结构性绕过（第一轮审查补出）**：即使加了「必须裁决」检查，只要判定依赖**标题/措辞的模糊匹配**（`src/change/plan_audit.rs:142` 的 `section_lines` 按标题串找块），那么「结构没被识别」与「确实没有裁决项」会得到**同样的 0 条问题** → 门空转 = 上一场静默自决原样复发。所以判定必须改成**结构契约**（块必须存在 + 每条必须带分类标记），且提示词与门**共用同一套标记常量**（§1.3 的 Q2）。

**死亡诊断缺失**——取证面四处皆空：

- 进程内**没有** panic hook：`src/main.rs:144`-`:146` 只初始化 tracing；全仓没有 `std::panic::set_hook` 调用。panic 只会走默认 hook 打到 stderr（进程一死，stderr 未必留得住；第一场现场就「stderr 无 panic」）。
- 阶段**只在阶段切换时**落盘：`src/change/orchestrator.rs:152`（plan→coding）、`:177`（coding→testing）、`:247`（testing→done）三处写 `state.phase` 并 `save`（`:153`/`:178`/`:248`；回退路径另有 `:263`/`:271` 两处，见 §2.2）；run 中途死掉，盘上只有**上一个**阶段 → 不知道死在哪个阶段哪一步。
- 没有「上次跑是否正常收尾」的标记 → 异常退出与正常结束在盘上不可区分 → 没法自动留验尸文件。
- 子进程死亡是有判的（`src/engine/client.rs:134` 的 `next_line` → 「子进程已退出…按基础设施故障处理」），但**服务器自己**的死亡没有任何落盘面。

**续跑自锁**——step 8 热修（`src/change/gate0.rs:66`，复用逻辑在 `src/change/gate0.rs:73`-`:82`，单测在 `src/change/gate0.rs:222`）只堵住了一个洞：**记录存在且非空**时复用。剩下五个洞全在：

- `src/change/gate0.rs:76`：只判 `!sha.is_empty()` → 内容是垃圾（非 sha）也照样当基线返回。
- `src/change/gate0.rs:77`：无条件返回记录里的 sha，**不验证它在不在当前仓库历史里** → 目标仓被重建/历史被改写后，diff 锚点直接失效（后续 `git diff <baseline>` 失败，见 `src/change/coding_gate.rs:188`）。
- `src/change/gate0.rs:74`：记录读不到（缺失/空/损坏）就往下走 → **重记当前 HEAD → 自锁路径原样复发**（已提交步骤从 `git log <baseline>..HEAD` 的门禁视野里消失，见 `src/change/coding_gate.rs:256` 的步骤↔提交对账）。
- `src/change/gate0.rs:173`：`write_baseline` 直接写目标文件（非原子）→ 崩溃在写一半时留下截断文件。
- 同类洞（本次一并补）：陈旧证据跨轮沿用（`src/change/coding_gate.rs:16`/`:18`、`src/change/testing_gate.rs:16` 的证据文件黑板永不清理）、`current` 标记丢失即换号（`src/change/change_id.rs:55`）、断点损坏被静默当冷启动（`src/change/state.rs:127`）、计划文件缺失时报成「范围膨胀」（`src/server/change_engine.rs:124` 读不到计划 → 白名单为空 → 每个文件都算越界）。

### 1.3 歧义与本计划采用的默认

以下都是需求/代码里定不下来的点；括号内是本计划采用的默认（用户若不认同，对应条目在 §5 里有可机械验证的验收，改默认只需改这一处 + 对应验收）：

- [自决] **Q1 问询环的修法**：只改模板、只装包、还是双层？——（默认：**双层**：装包 + 提示词硬约束 + 确定性机械门。理由：只装包等于把纪律寄托在模型自觉上——第一场已经证明模型会静默自决；只改模板同理。）
- [自决] **Q2 判定契约（提示词与门共用同一份）**：靠语义理解、靠措辞扫描、还是靠结构契约？——（默认：**结构契约**，标记常量集中定义在 `src/change/plan_audit.rs`，`src/config/manifest.rs` 的提示词锚点单测**断言提示词含这些常量**（双向钉住，防止提示词与门漂移）：
  - 计划必须有一个**标题含「歧义」**的块（没有 → 结构问题）；**块解析取最后一个含「歧义」的标题**（既有 `section_lines` 的语义：后出现的标题覆盖先前的，`src/change/plan_audit.rs:142`）——计划里出现多个含「歧义」的标题时（如「## 1 需求复述与歧义」与「### 1.3 歧义与…」并存），**只有最后一个标题的块内容参与判定**，更外层那个块里的列表行不判结构问题；
  - 该块内**每条顶层列表行**（行首无缩进）必须以分类标记开头：`- [必须裁决] ` 或 `- [自决] `（既无标记、又不是「无歧义」这类显式声明 → 结构问题，**非 0 条**）；**缩进的子行**是上一条的展开说明，不受任何约束；
  - 块内**顶层**（行首无缩进）且**不是**列表行的散文行，不得出现「必须裁决」「必须用户裁决」「需用户确认」等措辞——那是在散文里声明必须裁决又不给载体 → 判结构问题；
  - `[必须裁决]` 项另需 `用户裁决：<答复原文>`，且答复能在本轮问询记录里逐字找到。
  理由：措辞扫描会让「结构未识别」与「无裁决项」同为 0 条问题 → 门空转 = 静默自决复发（第一轮审查补出的洞）。）
- [自决] **Q3 用户裁决的载体写法**：——（默认：同一行内 `用户裁决：<答复原文>`；答复必须能在问询记录里**逐字**找到（忽略空白差异）。不引入额外编号体系。）
- [自决] **Q4 问询记录落哪、怎么隔离跨 run 的陈旧答复**：——（默认：黑板内一份**按批次追加**的问询记录（运行期文件，文件名见 §2.1）；**每轮 change run 的门 0 追加一个批次头**（`src/change/prepare.rs:68` 那一层），**不清空**旧批次；判定只认**当前批次**的答复。理由：计划文件在目标仓跨 run 留存（`src/server/change_engine.rs:124` 读它取白名单/验收标准），若记录按 run 清空，续跑回到 plan 阶段时「答复不在本轮记录」必 Fail 且无处可解；按批次追加则既隔断跨 run 的陈旧答复，又保留上一场的记录供事后查。同一场内的多轮打回共享同一批次。）
- [自决] **Q5 机械门不过时的动作**：直接判 Quality 死掉，还是打回让 agent 去问？——（默认：**打回**（`PlanTurn::Fail`），复用既有修复回路 `src/change/orchestrator.rs:24`（上限 3 轮），打回消息自带「先用 ask-user-question 问用户」的指令。直接判死会让「第一次没问」＝整场失败，太脆。）
- [自决] **Q6 取证落点**：——（默认：`~/.devforge/forensics/`（panic 文件 + 验尸文件）与 `~/.devforge/runs/<run_id>/run.json`（心跳）。都在 HOME 下的固定路径，跨 run、跨会话可查。）
- [自决] **Q7 「异常退出」怎么判**：——（默认：心跳记录 `status == running` **且**（pid 已不存在 **或** 心跳陈旧超过阈值）→ 判异常，写验尸文件；`status == finished` 一律不写。阈值取一个明显大于心跳间隔的值（心跳间隔 5s → 陈旧阈值 60s 量级），具体数值由 coding 定并在单测里钉住。）
- [自决] **Q8 panic hook 会不会改变现有 stderr 行为**：——（默认：**不改变**：新 hook 链上原 hook（stderr 仍打印 panic），额外落盘；`set_hook` 是进程全局、只能设一次 → 落盘逻辑必须拆成纯函数（可单测），装 hook 只是薄封装。）
- [自决] **Q9 基线记录缺失/损坏时怎么办**：静默重记（现状）还是硬失败？——（默认：**硬失败**，且错误文本必须写清「为什么不能重记」与「怎么恢复」。理由：静默重记就是自锁本身；而且此时**已经无法从盘上推出原基线**，任何自动猜测都是在赌。）
- [自决] **Q10 基线的第二份落点放哪**：——（默认：黑板 `change/baseline.txt`（与断点同层，`src/change/state.rs:127` 的 `change/state.json` 同目录）。计划目录那份是交付物，黑板那份是运行资产——两份互为备份，谁在谁不在都能恢复。）
- [自决] **Q11 陈旧证据怎么处置**：删还是留？——（默认：**轮转**为 `*.prev`（保留一份上一轮内容供事后查），当前文件清空/不存在 → 新一轮的证据门拿不到旧证据，但上一轮内容不丢。）
- [自决] **Q12 `fresh` 重跑与「基线不得重记」的张力**：——（默认：**不特殊照顾 fresh**：只要这个 change 已经开跑过（有断点或已有计划文件），基线记录缺失就是硬失败，错误文本指引操作员「恢复记录」或「连计划文件与基线记录一起清掉重开」。fresh 的语义（`src/runstate/resume.rs:119`）是「忽略断点重做单元」，不是「重定基线」；重定基线会让旧提交永远留在 diff 里。）
- [自决] **Q13 取证/心跳要不要覆盖 MVP 的 planning / coding SOP**：——（默认：**接口通用、只接 change**：心跳/验尸模块不绑定 SOP，但本次只有 change 路径接线（`src/server/change_engine.rs`）。MVP 路径的死亡取证留给下一场，避免本次 diff 膨胀。）
- [自决] **Q14 问询门要不要也给 MVP 规划路径加**：——（默认：**不加**。MVP `plan-agent` 本来就有 ask 包与「结束前必须问完成确认」（`src/sop/planning.rs:21`-`:22`），它的问询环是通的；本场要治的是 change SOP 这条没有问询环的路径。）
- [自决] **Q15 「分类恰当性」谁管**：机械门只能判「结构齐不齐、问了没」，判不了「该不该问」——若 agent 把所有歧义都标成 `[自决]`，机械门无话可说。——（默认：**归审查官**：`CHANGE_PLAN_REVIEW_PROMPT`（`src/config/manifest.rs:523`）新增一条审点「必须裁决项与自决项的分类是否恰当：把必须用户裁决的条目标成自决 = FAIL」。机械门与审查官分工：机械门管**结构与问答留痕**，审查官管**语义分类**。）
- [自决] **Q16 心跳各字段的来源**（第一轮审查补出）：——（默认）：
  - `pid` = **server 进程 pid**（`std::process::id()`，在心跳登记时取一次）——**不是** pi 子进程：pi 子进程每阶段正常退出，若记它就会「进程已不在」误报异常。
  - `phase` = **编排器**是唯一真源（`src/change/orchestrator.rs` 的**六个**同步点，见 §2.2：断点载入点 `:124` + 五个阶段迁移点）；会话层不写 phase。**初值** = 断点里的 `ChangeState.phase`（`src/change/state.rs:40` 的字段、`src/change/state.rs:127` 的 `load`；冷启动时 `ChangeState::new` 默认为 Plan）→ 登记时（`src/server/change_engine.rs:111` 之后）由 change 侧读**同一份**断点取得，编排器在 `src/change/orchestrator.rs:124` 载入后再同步一次 → **续跑直接进 coding/testing 时 phase 也着地**（否则 phase 永不着地，与「stage 首段必须等于当前 phase」矛盾）。
  - `stage` = **会话层**写（`src/change/session.rs` 的七个轮次入口），统一词表：`plan/author`、`plan/review`、`coding/baseline`、`coding/impl`、`coding/review`、`testing/agent`、`testing/review`（stage 的首段必须等于当前 phase）。
  - `step` = **已提交的最大步骤号**：由会话层在 coding 轮从 `git log <baseline>..HEAD` 的提交信息解析（`src/change/coding_gate.rs:209` 的 `commits_since` + `src/change/coding_gate.rs:230` 的 `step_numbers_in`），无提交时为 0。
  - `attempt` = **当前尝试号** = 断点里的 `RunState.attempt`（`src/runstate/mod.rs:69`）+ 1：断点里存的是「已重试次数」（重试层在 `src/server/mod.rs:1260` 写 `rs.attempt = attempt`，只在 attempt>1 时写），首次跑断点里是 0 → 当前尝试号 1。change 侧用自己的 `StateStore` 读**同一份** `state.json`（重试层与 change 用的是同一个 SOP 键）→ **不需要改 `src/server/mod.rs`**。）
- [自决] **Q17 心跳的终态口径**（第一轮审查补出、第二轮审查补细）：——（默认：
  - `status` 只有两个取值 `running` / `finished`；收割器**只**对 `status == running` 的记录写验尸 → 设计内失败不会被写成「未正常收尾」。
  - **登记点 = 门 0 成功之后**（`src/server/change_engine.rs:111` 之后）：门 0 之前的 6 条早退（`src/server/change_engine.rs:73` 无工作区、`src/server/change_engine.rs:79` profile 缺失、`src/server/change_engine.rs:85` 建黑板目录失败、`src/server/change_engine.rs:93` 缺 requirement、`src/server/change_engine.rs:95` requirement 为空、`src/server/change_engine.rs:103` 门 0 失败含本次新引入的基线硬失败）**根本不产生心跳记录** → 收割器无对象可写，不会把门 0 前的设计内失败写成异常。
  - **收口 = 登记后立刻拿到的 RAII guard**（`Drop` 里把心跳置为 finished）：覆盖登记之后的**全部**退出路径——包括 watchdog 之前的会话 spawn 失败（`src/server/change_engine.rs` 里三处 `RpcClient::spawn`）、watchdog 内的成功 / 质量失败 / 取消 / 超时 / 技术故障，以及 `src/server/change_engine.rs:239` 的 `map_intervention` 之前。显式的收口调用仍保留在 `:239` 之前（让正常路径的收口不依赖 Drop）。）
- [自决] **Q18 续跑回到 plan 阶段的用户成本**：门 0 每轮 run 开新批次（Q4），所以续跑回到 plan 阶段时**必须重新问一次用户**（用户不在线 → run 阻塞在 `ui.ask` 的 `rx.recv()`，`src/server/mod.rs:140`）。——（默认：**接受这个成本**：这正是「不许沿用上一场的静默自决」的代价；打回回路有 3 轮，agent 在下一轮补问即可。）

---

## 2 影响面

### 2.1 直接命中

- `src/config/manifest.rs:899` —— `change-plan-agent` 的 `AgentManifest`：`extra_packages: vec![]` → 改为一装 `PLAN_AGENT_EXTRA`（`src/config/manifest.rs:22`，与 `plan-agent` 同包）；**这是「问询环在供给层不存在」的根因行**。
- `src/config/manifest.rs:1097` —— 既有单测 `assert!(cpa.extra_packages.is_empty());` 把「空包」钉成契约 → 必须同步改为断言含 ask 包（否则改动即红）。
- `src/config/manifest.rs:473` —— `CHANGE_PLAN_AGENT_PROMPT` 的第 1 块（`需求复述与歧义`）：把「列出来（问了就把答案写进来）」改成硬约束——**歧义块必须存在、每条必须带分类标记、`[必须裁决]` 项必须逐条用 ask-user-question 问用户、答复逐字写进 `用户裁决：`**；提示词里出现的分类标记必须与 `src/change/plan_audit.rs` 的常量一致（由提示词锚点单测双向钉住，Q2）。
- `src/config/manifest.rs:523` —— `CHANGE_PLAN_REVIEW_PROMPT`：新增一条审点「必须裁决项与自决项的分类是否恰当」（Q15：机械门管语义管不了的那一半）。
- `src/config/manifest.rs:1251` / `src/config/manifest.rs:1600` —— change 六件套 e2e 夹具与 builtin 的逐字段漂移守卫：提示词/包一改就红 → 夹具必须**用生成器重生成**（生成器 `fn dump_change_agents_fixture` 在 `src/config/manifest.rs:1233`，说明在 `:1229`），不许手改。
- `src/change/plan_audit.rs:13` —— `PLAN_BLOCKS`（六块清单）：「歧义」块是「需求复述」这一块的子结构 → 判定所需的**结构契约常量**（分类标记、裁决字段名、块标题关键词）定义在本文件，供提示词与门共用（Q2）。
- `src/change/plan_audit.rs:61` —— `audit()`：机械审计入口，**签名不变**（只做 repo 内可判的检查）；新增的裁决判定是**独立纯函数**（比对源不在 repo 内，见 §2.3），不塞进 `audit()`。
- `src/change/plan_audit.rs:142` —— `section_lines()`：现有块解析器（「歧义」块内容行抽取复用它；注意它**跳过标题行**，所以「歧义」块是否存在要另扫标题行）。
- `src/change/plan_audit.rs:236` —— `audit_plan_file()`：读计划并审计的入口，**签名不变**；裁决判定不走它（它只拿到 repo 与计划路径，拿不到黑板里的问询记录）。
- `src/change/mod.rs:36` —— `pub use plan_audit::{PlanAudit, audit as audit_plan, review_instruction};`：新增裁决判定纯函数的 re-export（保持 `src/change` 对外导出面一致）→ **本文件进白名单**（原计划把它写成「若需导出再改」，与白名单纪律冲突，已定为必改）。
- `src/change/session.rs:23` —— `UserLink` 的引入处（`use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};`）：`run_plan_with` 里用记录器包一层 `ui`（记录器实现 `UserLink::ask`，`src/sop/planning.rs:38`），把问答写进黑板里的问询记录文件（运行期文件，批次头由 `src/change/prepare.rs:68` 那一层追加）。
- `src/change/session.rs:216` —— `ProdChangeSession::run_plan_with`：计划轮的**唯一收口点**（plan / amend / fix 三条路都走它）→ 问询记录器与裁决门接在这里；门不过 → 返回 `PlanTurn::Fail`（**不进审查轮**：这是确定性判定，不该交给审查官重判）。
- `src/change/session.rs:269` —— `fix_plan`：打回消息的组装处 → 裁决门的问题清单要带「先用 ask-user-question 问用户」的指令（由门的问题文本自带即可，此处只保证问题原样回注）。
- `src/change/session.rs:290` / `src/change/session.rs:339` —— `run_coding` / `run_testing`：每轮开始处**轮转上一轮的证据文件**（`evidence/baseline-test.log`、`evidence/self-check.log`、`evidence/regression.log`、`testing/acceptance.md`）→ 新一轮拿不到旧证据。
- `src/change/session.rs:292` / `src/change/session.rs:298` / `src/change/session.rs:316` / `src/change/session.rs:339` / `src/change/session.rs:384` —— 会话层的**七个 stage 写点**（Q16 的统一词表）：`coding/baseline`、`coding/impl`、`coding/review`、`testing/agent`、`testing/review`；`plan/author`（`src/change/session.rs:217`）与 `plan/review`（`src/change/session.rs:229`）同理。
- `src/change/session.rs:134` —— `coding_kickoff`：续跑约束（「不得重编已提交步骤的编号」）写在这里。
- `src/change/session.rs:64` —— `plan_kickoff`：计划轮的开工语 → 补一句结构契约与「有必须裁决项先用 ask-user-question 问用户」（与提示词同口径，提示词是主、kickoff 是补）。
- `src/change/orchestrator.rs:152` / `:177` / `:247` / `:263` / `:271` —— **phase 的唯一真源**（五个阶段迁移点：plan→coding、coding→testing、testing→done、testing→coding 回退、testing→plan 回退；对应 `save` 在 `:153`/`:178`/`:248`/`:264`/`:272`，另有两处只更新计数器的 `save` 在 `:209`/`:253`）→ 这五处各同步一次心跳的 phase（Q16）。
- `src/change/orchestrator.rs:124` —— **phase 的初值同步点**：`let mut state = load(run.store, run.change_id, run.baseline_sha);` 之后立刻把断点里的 phase 同步进心跳 → 续跑直接进 coding/testing（不经任何迁移点）时 phase 也能着地（Q16）。
- `src/change/gate0.rs:66` —— `ensure_baseline`：本次审计与加固的主战场（复用逻辑 `:73`-`:82`、写盘 `:173`、常量 `:171`）。
- `src/change/gate0.rs:222` —— 既有单测（`existing_baseline_record_is_reused_even_after_head_advances`）→ 不得回归，且本次要补齐「缺失/为空/损坏/sha 不在历史/非祖先」的用例。
- `src/change/prepare.rs:65` —— `ensure_baseline` 的唯一调用点（门 0）→ 传入「黑板备份路径」与「是否已在跑（断点/计划文件存在）」两个新事实；同时在此写基线备份。
- `src/change/prepare.rs:68` —— 黑板骨架目录清单（`evidence` / `testing/reports` / `coding/reports`）→ 追加问询记录所在的 `plan` 目录；**并在此追加本轮的批次头**（Q4：不清空旧批次）。
- `src/change/change_id.rs:55` —— `assign_change_id`：`current` 标记丢失时按黑板里已有的 `<change-id>/` 目录恢复同一个 id（不换号）。
- `src/change/state.rs:127` —— `change::state::load`：`Loaded::Unusable`（断点损坏）当前被静默当冷启动 → 必须留下可观测痕迹（reason 进日志）。
- `src/forensics.rs` —— **本变更新增**（模块登记点见 `src/lib.rs:9`）：panic hook（落盘）+ run 心跳 + 异常退出验尸 + 收割器 + **心跳登记的 RAII guard**（Drop 里收口为 finished）。纯标准库，不引新依赖。
- `src/lib.rs:9` —— 模块清单（该行是 `pub mod change;`；`pub mod forensics;` 是本次**新增行**，与 `pub mod change;` 同级）→ 注册 `forensics`。
- `src/main.rs:146` —— `logging::init` 之后：装 panic hook（HOME 固定路径）+ 启动时收割陈旧 run（写验尸文件）。
- `src/server/change_engine.rs:66` —— `run_change` 入口：门 0 之前有 6 条早退（`src/server/change_engine.rs:73` 任务无工作区、`src/server/change_engine.rs:79` profile 缺失、`src/server/change_engine.rs:85` 建黑板目录失败、`src/server/change_engine.rs:93` 缺 requirement 文件、`src/server/change_engine.rs:95` requirement 为空、`src/server/change_engine.rs:103` 门 0 失败——含本次新引入的基线硬失败）→ **这 6 条早退不产生心跳记录**（Q17 的登记点在门 0 之后），收割器无对象可写。
- `src/server/change_engine.rs:111` —— **心跳登记点**（门 0 成功、`change-id` 与基线已定之后）：登记 run/sop/project/task/pid/attempt，随后立即拿到 RAII guard 兜住后续全部退出路径。
- `src/server/change_engine.rs:103` —— 门 0 调用点（`let prep = prepare(`）：问询记录批次头与基线备份都在这一层落；门 0 失败（基线硬失败，Q9/R12）→ 无心跳记录、无验尸文件。
- `src/server/change_engine.rs:124` —— 读计划文件取 `plan_facts`（验收标准 / 白名单）处：计划文件缺失且已在 coding/testing 阶段 → 快速失败并说清「计划文件缺失」（而不是让白名单为空 → 报成「范围膨胀」）。
- `src/server/change_engine.rs:221` —— `RelayUserLink` 交给编排器处 → 问询链路的终点（`src/server/mod.rs:177`）。
- `src/server/change_engine.rs:239` —— `map_intervention` 之前的显式收口点：失败/取消/超时时把心跳置为 finished（Q17），否则收割器会把**设计内失败**写成「未正常收尾」；RAII guard 是同一动作的兜底（覆盖 watchdog 之前的早退）。
- `tests/e2e/fixtures/change_agents.json` —— change 六件套 e2e 夹具：`change-plan-agent` 条目的 `role_prompt` / `extra_packages` 必须与 builtin 逐字段一致（漂移守卫 `src/config/manifest.rs:1251`）。
- `tests/e2e/test_26_change_e2e.py:130` —— e2e 从夹具装载 change 六件套并写进 manifest → 夹具带 ask 包，e2e 里的 change plan agent 才真的能问用户。
- `tests/e2e/test_26_change_e2e.py:194` —— e2e 的 autopilot 启动（`UiAutoPilot(server, sse, default_answer="继续")`）：**「继续」只是非 select 形状请求的兜底答复**，而 ask-user-question 发的是 **select 形状**请求（回放脚本见 `src/sop/rpc_bridge.rs` 末尾的 `method":"select"`）→ 实际答复是**选项标签**（`tests/e2e/common.py:389`-`:396`），而选项标签由 agent 自拟 → 计划里的 `用户裁决：` 必须逐字等于那个标签（见风险 R18 与 §5 的双向一致性验收）。
- `tests/e2e/common.py:365` —— `UiAutoPilot.answered` 的 append（列表定义在 `:345`）：**只记 `id`/`status`/`question`，不记实际 POST 出去的 value** → 答复原文在 e2e 侧无处可查 → 本次改这个文件让 `answered` 记下 value（白名单内，见 §3.1）。
- `src/runstate/mod.rs:69` —— `RunState.attempt`（重试次数的落盘字段；写入方是重试层 `src/server/mod.rs:1260` 的 `rs.attempt = attempt`）→ 心跳的 `attempt` 字段由 change 侧用自己的 `StateStore` 读同一份文件得到（Q16），**不需要改重试层**。
- `docs/plantree/plans/009-change-sop/decisions/README.md:159` —— 决策台账（现登记至 D023）→ 登记本次三条决策（问询门 / 崩溃取证 / 续跑加固）；**并因本次改供给而取代 D009**（`:64` 标题、`:66` 决定行「仅 codegraph MCP」）→ D024 标题显式写「取代 D009 的供给部分」，D009 标题加旁注（体例同 `:152` 的 D022a 取代 D022，两边都标）。
- `docs/plantree/plans/009-change-sop/roadmap.md:24` —— T004 的供给描述（「deepseek-flash + codegraph MCP + `plan-tree` 技能」）→ 因本次改供给而失真，随本次同步。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:20` —— 「问到的答案写进『需求复述』」：本次**改载体**（答复进歧义块的 `用户裁决：` 字段）→ 该行失真，必须同步。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:60` —— 「`change-plan-agent`：…**仅 codegraph MCP**」：本次**改供给**（加 ask-user-question 包）→ 该行失真，必须同步。

### 2.2 调用链（谁调用它 / 它调用谁）

- `src/change/session.rs:248` —— `impl ChangeSession for ProdChangeSession`：`run_plan`（`:249`）/ `amend_plan`（`:254`）/ `fix_plan`（`:269`）三条计划路径全部汇到 `run_plan_with`（`:216`）→ **问询门只需接一处**（三条路径都被覆盖，含 coding→plan 回退与审查打回）。
- `src/change/orchestrator.rs:133` —— plan 阶段的打回-修复循环（`MAX_PLAN_FIXES = 3`，`:24`）：`PlanTurn::Fail` → 回注问题 → `fix_plan` 再来一轮 → 超限才 Quality（`:159`）→ 问询门复用这条既有回路，**不新增上限、不新增状态机**。
- `src/change/orchestrator.rs:124`/`:152`/`:177`/`:247`/`:263`/`:271` —— **phase 的六个写点**（Q16 的唯一真源）：`:124` 是**断点载入后的初值同步**（续跑直入 coding/testing 的唯一着地点），`:152`/`:177`/`:247` 是正常推进（plan→coding→testing→done），`:263`/`:271` 是**回退**（testing→coding、testing→plan）。心跳的 phase 在这六处同步；`:209`/`:253` 两处 `save` 只更新计数器，心跳只刷新不换 phase。
- `src/change/session.rs:217`/`:229`/`:292`/`:298`/`:316`/`:339`/`:384` —— **stage 的七个写点**（Q16）：会话层只写 stage，不写 phase；stage 首段必须等于当前 phase，否则心跳记录里标为不一致（供验尸文件提示）。
- `src/change/prepare.rs:30` —— `prepare()` 是门 0：`src/server/change_engine.rs:103` 调用 → 基线、change-id、黑板骨架都在这里备好；「是否已在跑」的事实、以及问询记录的批次头也只能在这里取/落。门 0 失败（`:103` 的 `?`）→ **不产生心跳记录**（Q17 的登记点在 `:111` 之后）。
- `src/change/gate0.rs:142` —— `head_sha`（`git rev-parse HEAD`）：基线记录的有效性校验（sha 是否存在、是否 HEAD 祖先）要用同类 git 调用，形状与 `src/change/coding_gate.rs:209` 的 `commits_since` 一致。
- `src/change/coding_gate.rs:256` —— `check_step_commits`（`git log <baseline>..HEAD` 对账步骤↔提交）：**基线被重记就会让已提交步骤消失**（自锁的受害面）→ 基线加固的直接受益者。
- `src/change/coding_gate.rs:188` —— `changed_files`（`git diff --name-only <baseline>`）：基线 sha 不在历史时这里直接失败 → 校验的价值点。
- `src/change/coding_gate.rs:209` + `:230` —— `commits_since` + `step_numbers_in`：心跳 `step` 字段的取值来源（Q16）——已提交的最大步骤号。
- `src/change/boundary.rs:102` —— `classify`（越界二分）：白名单来自 `plan_facts`（`src/change/plan_audit.rs:187`）→ 计划文件缺失导致白名单为空时，每个文件都判越界 → 「计划文件缺失」必须提前快速失败。
- `src/change/state.rs:135` —— `change::state::save`：`store.load()` 失败时用新状态覆盖（断点损坏的证据被抹）→ 至少要留痕。
- `src/runstate/mod.rs:118` / `src/runstate/resume.rs:119` —— 断点读（`Loaded::Unusable` 带原因）与 `start_gate`（fresh 清断点）→ change 侧的 `change::state::load`（`src/change/state.rs:127`）把 `Unusable` 吞了，本次补上痕迹；`fresh` 与「基线不得重记」的张力见 Q12。
- `src/sop/rpc_bridge.rs:24` —— `RpcPlanSession::converse`：`extension_ui_request` → `ui.ask`（`src/sop/planning.rs:38` 的 `UserLink`）→ **本次不改**（通道已通；缺的只是 Agent 侧有工具 + 机械门）。
- `src/server/mod.rs:135` —— `UiRelay::ask_blocking` 广播 `ui.request`（`src/server/mod.rs:109`）→ `src/server/mod.rs:181` 的 `RelayUserLink` 实现 `UserLink::ask` → 记录器包在它外面（记录 + 转发）。
- `src/engine/client.rs:134` —— `next_line` 的子进程死亡检测（「子进程已退出…按基础设施故障处理」）→ 已有对「子进程死」的判据，**不复用也不重造**（取证针对的是服务器自己）。
- `src/server/mod.rs:1361` —— `run_with_watchdog`（300ms 一跳的看门狗线程）→ 心跳的落盘**不靠它**（心跳由 `src/forensics.rs` 自带打点线程负责，`src/server/mod.rs` 零改动）。
- `src/server/mod.rs:1249` —— `run_with_retry`：基础设施故障会重跑同一 run（最多 4 次尝试）→ 每次尝试都是一次 `run_change` → 心跳按 run_id 一份，重试时把 `status` 重置为 running；`attempt` 由重试层写进断点（`src/server/mod.rs:1260`）→ change 侧从自己的 `StateStore` 读同一个字段（Q16），重试层**不改**。
- `src/change/report.rs:49` —— `ChangeReport::render`（写 `docs/plantree/changes/<id>/report.md`）→ 本次不改字段（取证文件路径是 HOME 下的固定路径，可直接找）。

### 2.3 隐式耦合（grep 几乎拿不到的那类）

- **运行时清单缓存**：`src/main.rs:152`（`if manifest_path.exists()`）——`~/.devforge/manifest.json` 存在即用落盘清单（`src/main.rs:153` 的 `ConfigManifest::load`，不再用 `builtin()`）→ 仓内给 `change-plan-agent` 加了包，若不重存清单并重渲染 profile，**ask 包不会进 profile**，问询环在用户态依然不存在（仓内改完 ≠ 用户态生效；属部署侧动作）。
- **profile 的包安装面**：`src/config/render.rs:182`-`:189` —— `base_packages + extra_packages` 写进 profile 的 `settings.json` 的 `packages` → 「装没装 ask 包」的真实判据在 profile 的 settings.json，不在 manifest 里。
- **e2e 的 manifest 拼装**：`tests/e2e/test_26_change_e2e.py:140`-`:143` —— e2e 把 `base_packages` 清空、把宿主 agent 的 `extra_packages` 清空，再把**夹具里的** change 六件套 extend 进 manifest → 夹具里 `change-plan-agent` 带不带 ask 包，决定了 e2e 真链里问询环存不存在；**夹具不重生成 = e2e 假绿**。
- **pi 的 skill / 包加载契约**（目标仓外，pi 包内）：`npm:@juicesharp/rpiv-ask-user-question` 是**扩展包**（提供 `extension_ui_request` 能力），与 skill 的 frontmatter 契约是两回事——本次只加包、不加 skill，不触碰 skill 契约（仓内侧的对应口径是 `src/config/render.rs:182` 的包清单与 `src/config/skill_assets.rs:14` 的内置资产注册，二者互不相干）。
- **提示词与门必须共用一份契约**：分类标记定义在 `src/change/plan_audit.rs`，提示词（`src/config/manifest.rs:473`）里出现的是同一批字面量 → 靠 `src/config/manifest.rs` 的提示词锚点单测双向钉住（提示词含门用的标记常量 = 契约没漂）。否则改了一边、另一边还在 → 门空转（Q2 的根因）。
- **裁决比对的源不在 repo 内**：问询记录是**黑板**里的运行期文件（不是目标仓里的文件）→ `src/change/plan_audit.rs:61` 的 `audit()`（只看 repo）与 `:236` 的 `audit_plan_file()`（只拿路径）都拿不到它 → 裁决判定必须是**独立纯函数**（入参含问询记录文本），由会话层读文件后传入（§2.1 的 `src/change/session.rs:216`）；引擎侧 Fail 的入口因此与 `audit().issues`（只进审查指令，`src/change/plan_audit.rs:209`）**分开**。
- **`std::panic::set_hook` 是进程全局、只能设一次**（安装点：`src/main.rs:146` 之后）：装 hook 的代码必须是**幂等且只装一次**的薄封装；落盘逻辑必须拆成纯函数（否则单测装第二次 hook 会互相污染）。
- **取证目录的归属**：`~/.devforge` 由 `src/main.rs:144` 约定（`logs` / `manifest.json` / `profiles` / `plans` / `sessions`）；引擎侧只拿得到 `plans_root`（`~/.devforge/plans`）→ 取证根的推导必须两处一致（`plans_root` 的父目录），并由单测钉住，否则 panic 文件会落错地方。
- **子进程环境继承**：`src/engine/process.rs:54`（`build_spawn_command` 定义）里的 `src/engine/process.rs:63`-`src/engine/process.rs:65` 只注入 `PI_CODING_AGENT_DIR` / `DEVFORGE_MANAGED` / `DEVFORGE_ROLE` → 取证/心跳路径**不能靠 env 传给子进程**，只能靠 HOME 约定 + 盘上文件。
- **孤儿清场会先于取证发生**：`src/engine/ledger.rs:119`（`scan_and_kill_run` 定义，`:137` 是那一行 SIGKILL）在取消/超时/失活时 SIGKILL 自家 run 进程 → 验尸不能依赖子进程还活着，只能依赖盘上的心跳记录（心跳记的是 server 进程的 pid，见 Q16）。
- **e2e 里「答复原文」只有两个来源**（§5 的 e2e 断言要靠它们）：①**autopilot 记录**（`tests/e2e/common.py:345` 的 `answered` 列表）——现状在 `:365` **不记 value** → 本次改 `tests/e2e/common.py` 记下实际 POST 的 value；②**引擎侧黑板问询记录**（本次新增的记录器写的问答文件）——e2e 能从 `server.home` 下读到，是与 autopilot **相互独立的第二来源**（两者应一致，可交叉核对）。`src/sop/rpc_bridge.rs:24` 的 `ui.ask` 回调拿得到答复但**当前不落盘**（答复直接回填 pi 的 stdin），所以仓内现成可查的答复来源只有这两处 —— 这也是 e2e 断言必须改 `tests/e2e/common.py` 的原因。
- **日志会被清理/轮转**：`src/logging.rs:33` 的 `~/.devforge/logs/<component>.log.jsonl` 是滚动日志 → 取证文件必须**独立目录、独立文件**，不能只靠日志。
- **断点被覆盖的时机**：`src/change/state.rs:135` 的 `save` 先 `store.load()`，不可用时用新状态覆盖 → 损坏断点的原始内容会被抹掉；补痕迹要趁覆盖前。
- **`change/state.json` 与计划目录分离**（010 D022a，断点读写见 `src/change/state.rs:127`、基线落点见 `src/change/prepare.rs:65`）：断点在黑板、交付物在目标仓 → 基线备份放**黑板**（Q10）、计划目录那份仍是交付物，两份互为备份；同理，问询记录也放黑板（运行资产），而计划（交付物）在目标仓跨 run 留存 → 记录按批次隔离（Q4）。
- **工作区当前不干净**（见风险 R1）：`git status` 显示 `src/change/coding_gate.rs`、`src/change/session.rs`、`src/config/manifest.rs`、`src/config/skill_assets.rs` 四个文件有**未提交**改动（第一场测试阶段产物，+880 行）→ 相对本变更基线 `3020873` 的 `git diff --name-only` 会把这四个文件算成「本次改动」→ 越界门（`src/change/boundary.rs:102`）会判范围膨胀。**开工前必须先由操作员处置（提交或 stash）**。
- **untracked 文件不进 diff**：`src/change/coding_gate.rs:188` 的 `git diff --name-only <baseline>` 不含未跟踪文件 → `.pi/goals/archived/*.md`、`docs/plantree/changes/`、`tests/e2e/results/*.json` 这些未跟踪文件不会触发越界门，但会污染提交（白名单外改动）。

---

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `src/config/manifest.rs` | ①`change-plan-agent` 装 `PLAN_AGENT_EXTRA`（`:899`），问询环才有工具；②`CHANGE_PLAN_AGENT_PROMPT`（`:473`）把结构契约（歧义块 + 分类标记 + `用户裁决：` 载体）写成硬约束，标记字面量与门共用，并要求**「歧义」单独成块**（避免多个同名标题让块解析歧义）；③`CHANGE_PLAN_REVIEW_PROMPT`（`:523`）加「分类恰当性」审点；④同步 `:1097` 的「空包」断言与提示词锚点单测 | 需求 1（供给层 + 契约） |
| `src/change/plan_audit.rs` | 新增**结构契约 + 裁决判定**的确定性纯函数：歧义块必须存在、每条列表行必须带分类标记（结构不符 → 非 0 条问题）；`[必须裁决]` 项必须带 `用户裁决：` 且答复能在本轮问询记录里逐字找到。契约常量集中在本文件供提示词共用；`audit()`（`:61`）与 `audit_plan_file()`（`:236`）签名不变 | 需求 1（机械层） |
| `src/change/session.rs` | ①`run_plan_with`（`:216`）接入裁决门（读黑板问询记录 → 调纯函数 → 不过则 `PlanTurn::Fail`，不进审查轮）；②用记录器包 `ui`，把问答写进黑板问询记录；③`run_coding`/`run_testing`（`:290`/`:339`）轮转上一轮证据；④`coding_kickoff`（`:134`）加续跑约束；⑤七个 stage 写点（`:217`/`:229`/`:292`/`:298`/`:316`/`:339`/`:384`）写心跳 stage 与 step | 需求 1、2、3 |
| `src/change/orchestrator.rs` | **phase 的唯一真源**：断点载入点（`:124`，续跑直入 coding/testing 时的初值着地）+ 五个阶段迁移点（`:152`/`:177`/`:247`/`:263`/`:271`，含两条回退路径）各同步一次心跳 phase；回退路径（testing→coding、testing→plan）此前不在任何影响面里，本次补进 | 需求 2 |
| `src/change/gate0.rs` | `ensure_baseline`（`:66`）完备化：记录有效性与仓库历史双向校验（存在 + 是 HEAD 祖先）、损坏/缺失不得静默重记（冷启动 vs 在跑分开）、基线备份（黑板）互为备份、写盘原子化（`:173`）；补齐缺失/为空/损坏/sha 不在历史/非祖先的单测（不得回归 `:222`） | 需求 3 |
| `src/change/prepare.rs` | 门 0 传入「黑板备份路径」与「是否已在跑」；写基线备份；黑板骨架加问询记录目录；**每轮 run 追加一个问询批次头**（不清空旧批次，Q4） | 需求 1、3 |
| `src/change/change_id.rs` | `assign_change_id`（`:55`）：`current` 标记丢失时按黑板里已有的 `<change-id>/` 目录恢复同一个 id（不换号 → 不换基线） | 需求 3 |
| `src/change/state.rs` | `load`（`:127`）遇到 `Loaded::Unusable` 不再静默：留下可观测痕迹（reason 进日志），让「断点损坏被当冷启动」不再无声 | 需求 3 |
| `src/change/mod.rs` | `:36` 的 re-export 增补裁决判定纯函数，保持 `src/change` 对外导出面一致（不再是「若需导出再改」） | 需求 1 |
| `src/forensics.rs` | **新增**：panic hook 落盘（HOME 固定路径，带当时阶段/步骤）、run 心跳（HOME 下 runs 目录，按 run_id 一份 JSON 记录：run/sop/project/task/pid/attempt/phase/stage/step/时间戳/status）、**心跳登记的 RAII guard**（Drop 里把 status 收口为 finished，覆盖登记之后的全部退出路径）、异常退出验尸文件（HOME 下 forensics 目录）、陈旧 run 收割器（只认 status=running）。纯标准库 | 需求 2 |
| `src/lib.rs` | 注册 `forensics` 模块 | 需求 2 |
| `src/main.rs` | 启动时装 panic hook（`:146` 之后）+ 收割陈旧 run 写验尸文件 | 需求 2 |
| `src/server/change_engine.rs` | change run 的取证接线：开跑前收割陈旧 run；**心跳只在门 0 成功之后登记**（`src/server/change_engine.rs:111` 之后；`src/server/change_engine.rs:73`/`:79`/`:85`/`:93`/`:95`/`:103` 六条门 0 前的早退**不产生**记录）；登记后立即拿到 RAII guard 兜住全部退出路径，另有 `src/server/change_engine.rs:239` 之前的显式收口；`attempt` 从自己的 `StateStore` 读断点里的重试计数（`src/runstate/mod.rs:69`）；计划文件缺失且已在 coding/testing → 快速失败说清「计划文件缺失」 | 需求 2、3 |
| `tests/e2e/fixtures/change_agents.json` | 提示词与包变更后与 builtin 逐字段同步（漂移守卫 `src/config/manifest.rs:1251`/`:1600`）——必须用生成器重生成 | 需求 1 |
| `tests/e2e/test_26_change_e2e.py` | 加机械断言：①change 六件套 profile 的包清单（settings 里的 packages 数组）含 ask 包；②**问答 ↔ 计划双向一致性**：真链若发出过 `ui.request`，其**实际答复原文**（取自 autopilot 记录的 value，或引擎侧黑板问询记录，两者应一致）必须逐字出现在计划的 `用户裁决：` 行；若一个都没问，计划里不得出现 `[必须裁决]` 条目 | 需求 1 |
| `tests/e2e/common.py` | `UiAutoPilot` 记录**实际 POST 出去的答复原文**：现状 `:365` 的 append 只记 `id`/`status`/`question` → 改成同时记 `value`（答复文本取自 `:372` 的 `_smart_answer` 返回值）→ e2e 断言才有「答复原文」可对账（§2.3 的两个来源之一） | 需求 1 |
| `docs/plantree/plans/009-change-sop/decisions/README.md` | 登记 D024（问询门：装包 + 结构契约 + 确定性门 + 批次隔离）、D025（崩溃取证三件套、字段来源与落点）、D026（续跑加固：基线不可重记 + 同类洞清单 + 遗留小节），并标注与 010 D002/D003 的关系；**D024 显式标注取代 D009 的供给部分**（体例同 D022a 取代 D022，见同文件 `:152`），同时在 D009 标题（`:64`）加「供给部分于 2026-10-21 由 D024 取代」的旁注（**两边都标**） | 需求 1、2、3 |
| `docs/plantree/plans/009-change-sop/topics/agents-change.md` | `:20`（答复载体从「需求复述」改为歧义块的 `用户裁决：` 字段）与 `:60`（change-plan-agent 供给从「仅 codegraph MCP」改为 codegraph + ask-user-question 包）两处口径随本次改动同步 | 需求 1 |
| `docs/plantree/plans/009-change-sop/roadmap.md` | `:24` 的 T004 供给描述（「deepseek-flash + codegraph MCP + `plan-tree` 技能」）因本次**改供给**（增补 ask-user-question 包）而失真 → 该行同步为新供给口径 | 需求 1 |

### 3.2 受影响不改

| 文件 | 为什么可以不改 |
| --- | --- |
| `src/sop/planning.rs` | `UserLink`（`:38`）与 MVP 规划循环不变；change 路径复用同一 trait，记录器作为它的实现塞进去即可，不改 trait 形状 |
| `src/sop/rpc_bridge.rs` | `RpcPlanSession::converse`（`:24`）的 `extension_ui_request` → `ui.ask` 通道已通且有单测（`ui_request_goes_to_user_and_answer_flows_back`），本次不动 |
| `src/server/mod.rs` | `UiRelay`（`:109`）/`RelayUserLink`（`:177`）/`run_with_watchdog`（`:1361`）都不动：问询经 `RelayUserLink` 照旧上行 SSE；心跳由 `src/forensics.rs` 自带打点，不塞进看门狗。`attempt` 也**不需要**改它：重试层已经把重试计数写进同一份断点（`:1260`），change 侧用自己的 `StateStore` 读 `src/runstate/mod.rs:69` 的同名字段即可 |
| `src/runstate/mod.rs`、`src/runstate/resume.rs` | 断点通用机制（原子写、身份自证、fresh 语义）不变；change 侧只在 `src/change/state.rs:127` 补一条「不可用」的痕迹 |
| `src/engine/client.rs` | 子进程死亡已有判据（`:134`）；取证针对「服务器自己死」，两者不重叠，不重造轮子 |
| `src/engine/ledger.rs` | 孤儿清场与进程身份红线不动；取证不依赖子进程存活 |
| `src/engine/process.rs` | spawn 环境标记不动；取证路径走 HOME 约定，不靠 env |
| `src/change/coding_gate.rs` | 证据**解析**与门（`:87` 基线门、`:113` 自证门、`:256` 步骤↔提交门）一律不动；陈旧证据的**轮转**在会话层做（轮转 = 改文件名，不改门）；`commits_since`（`:209`）与 `step_numbers_in`（`:230`）只被**读**（取心跳的 step），不改 |
| `src/change/testing_gate.rs` | 回归门（`:43`）与验收对账（`:75`）不动；验收对账表的轮转同样在会话层做 |
| `src/change/boundary.rs` | 越界判定（`:102`）不动；「计划文件缺失」在更早的地方（引擎层）快速失败，不改判定逻辑 |
| `src/change/report.rs` | 报告字段不动；取证文件在 HOME 固定路径，可直接找 |
| `src/change/docs.rs`、`src/change/history.rs` | 文档档位与往返历史与本次三件事无关 |
| `src/config/render.rs` | 渲染器按 `agent.skills`/`extra_packages` 通用分发（`:182`），新包走既有分支；「装没装」的断言放在 manifest 单测与 e2e，不改渲染逻辑 |
| `src/config/skill_assets.rs`、`assets/skills/**` | 本次不新增 skill；`coding-discipline` 等供给一字不动 |
| `docs/plantree/plans/010-run-resume/decisions/README.md` | 010 的决策口径（断点真源 / 默认续跑 / done 自校验 / 引擎与 SOP 分工）不变；本次只是把 010 D003「done 必须自校验」的精神落到 change SOP 的具体点，登记在 009 台账并交叉引用 |
| `tests/e2e/test_08_e2e_planning.py`、`tests/e2e/test_14_coding_full_chain.py`、`tests/e2e/test_17_testing_config.py` | MVP 路径的 e2e（问询、完成确认、skills 矩阵）不受本次改动影响；`test_17` 断言的 skills 矩阵本次不动（不加 skill） |

### 3.3 必须回归验证

| 模块 | 为什么风险在哪里 |
| --- | --- |
| `src/change/plan_audit.rs` 的既有审计 | 新增的裁决判定是**独立纯函数**（不改 `audit()`），但常量与解析要共用既有块解析器（`:142`）→ 既有四条（path 存在性 / 白名单存在 / 要改的 ⊆ 白名单 / 验收可观测）行为不能变 |
| `src/change/session.rs` 的计划/编码/测试轮次 | 裁决门接在 `run_plan_with`（`:216`）正中，三条计划路径共用 → 接错会把「amend/fix」也拦下或全都不拦；门序（require_marker → 审计 → 门 → 审查）不能变 |
| `src/change/orchestrator.rs` 的五个阶段迁移点 | phase 同步写错一处 → 验尸文件里的阶段与真实阶段不符；回退路径（`:263`/`:271`）此前无人碰过，最易漏 |
| `src/change/gate0.rs` 的基线复用单测（`:222`） | 第一场 step 8 的热修不得回归（续跑必须复用已记录基线） |
| `src/change/coding_gate.rs` 的 `parse_baseline_log` 单参签名与「回归日志不可解析」文案 | 第一场 step 7 钉过的两条守卫，基线加固极易顺手改到它（`src/change/coding_gate.rs` 的解析被 testing 门复用） |
| `src/config/manifest.rs:1097` 的包断言 | 「change-plan-agent 空包」是旧契约，本次要改它——不改则改动即红；改错则会误给别的 Agent 加包 |
| 提示词 ↔ 门 的契约锚点单测 | 分类标记在提示词与门里各有一份字面量 → 锚点单测（提示词含门用的常量）是唯一防漂移手段，删了它契约就散 |
| 夹具漂移守卫（`src/config/manifest.rs:1251`/`:1600`） | 提示词/包一字之差即红；必须重生成而不是手改 |
| 009 文档三处口径（`docs/plantree/plans/009-change-sop/decisions/README.md:64`/`:66`/`:159`、`docs/plantree/plans/009-change-sop/topics/agents-change.md:20`/`:60`、`docs/plantree/plans/009-change-sop/roadmap.md:24`） | 改供给/改载体后这三处同时失真；**取代标注要两边都标**（D024 标取代 D009 + D009 标题旁注），漏一边会让旧决策看着仍有效 |
| e2e skills/packages 矩阵（`tests/e2e/test_11_coding_config.py`、`tests/e2e/test_17_testing_config.py`） | 误给 doc-/test-/plan- 家族加包或加 skill 会在此暴露 |
| e2e change 真链（`tests/e2e/test_26_change_e2e.py`） | 问询包 + 提示词 + 门在真链上的端到端实证：真 plan agent 是否真会发 `ui.request`、答复是否逐字进计划、问答与计划是否双向一致 |
| `src/forensics.rs` 的新模块单测 | panic 落盘、心跳、验尸、收割、**RAII guard 收口**五条行为都要有单测（纯函数可测，不依赖真实崩溃） |
| e2e 的 autopilot 记录（`tests/e2e/common.py`） | 改 `answered` 的记录形状会影响**所有**用 autopilot 的 e2e（`tests/e2e/test_08_e2e_planning.py`、`tests/e2e/test_14_coding_full_chain.py`、`tests/e2e/test_20_testing_fixture.py`）→ 只能**加字段**、不能改已有键的语义 |
| 仓内单测（`cargo test`） | 新增/改动的单测（契约判定、记录器、基线校验、取证、阶段同步、状态痕迹、change-id 恢复）全在白名单文件内 |
| 部署侧（不在仓内） | `~/.devforge/manifest.json` 缓存与 profile 重渲染：仓内改完不等于用户态生效（`src/main.rs:152`） |

---

## 4 白名单

- `src/config/manifest.rs`
- `src/change/plan_audit.rs`
- `src/change/session.rs`
- `src/change/orchestrator.rs`
- `src/change/gate0.rs`
- `src/change/prepare.rs`
- `src/change/change_id.rs`
- `src/change/state.rs`
- `src/change/mod.rs`
- `src/forensics.rs`（**本变更新增**；待建文件 → 机械审计会报「白名单里的文件不存在」，属**已知误报**，消解路径见 R21）
- `src/lib.rs`
- `src/main.rs`
- `src/server/change_engine.rs`
- `tests/e2e/fixtures/change_agents.json`
- `tests/e2e/test_26_change_e2e.py`
- `tests/e2e/common.py`
- `docs/plantree/plans/009-change-sop/decisions/README.md`
- `docs/plantree/plans/009-change-sop/topics/agents-change.md`
- `docs/plantree/plans/009-change-sop/roadmap.md`

新增测试文件：**无**。本仓 Rust 单测内联在被改文件的 `#[cfg(test)]` 模块（新增的 `src/forensics.rs` 自带 `#[cfg(test)]`）；e2e 断言追加进已有的 `tests/e2e/test_26_change_e2e.py`，不新增 e2e 脚本。

白名单之外不得有任何改动。注意：本计划文件（`docs/plantree/changes/` 目录下本次 change-id 的那一份）与基线记录是**计划阶段**的产物，由计划阶段写入，不在 coding 白名单内；`.pi/goals/`、`tests/e2e/results/` 等未跟踪产物同样不得进入本变更的提交。

---

## 5 验收标准

- 给定 `ConfigManifest::builtin()` 的 `change-plan-agent` → `extra_packages` 含 `npm:@juicesharp/rpiv-ask-user-question`（当前为空 → 问询环在供给层不存在）。
- 给定渲染出的 `change-plan-agent` profile 的 `settings.json` → `packages` 含 `npm:@juicesharp/rpiv-ask-user-question`。
- 给定 `ConfigManifest::builtin()` 的其余十四个 Agent → `extra_packages` 与 `skills` 与本次改动前逐字段相同（不误给别的 Agent 加包或加 skill）。
- 给定 `CHANGE_PLAN_AGENT_PROMPT` → 同时含「必须裁决」「自决」「ask-user-question」「用户裁决：」四个锚点，且含的分类标记字面量与 `src/change/plan_audit.rs` 的契约常量**逐字相同**（提示词锚点单测钉住，防契约漂移）。
- 给定 `CHANGE_PLAN_REVIEW_PROMPT` → 含「分类」审点（把必须用户裁决的条目标成自决 = FAIL）。
- 给定一个**没有**「歧义」块的计划文本 → 裁决判定返回 ≥1 条**结构**问题（非 0 条：结构未识别不得等于「无裁决项」）。
- 给定一个「歧义」块里有条目**不带**任何分类标记的计划文本 → 裁决判定返回 ≥1 条结构问题（带该行的行号或原文）。
- 给定一个「歧义」块里有**顶层散文行**写着「必须裁决」但没有对应条目与载体 → 裁决判定返回 ≥1 条结构问题（不许在散文里偷偷声明必须裁决又不给载体）。
- 给定一个「歧义」块里所有条目都带 `[自决]` 标记、没有 `[必须裁决]` 项的计划 → 裁决判定返回 0 条问题（自决项不触发问询要求；其恰当性归审查官）。
- 给定一个计划里有**两个**含「歧义」的标题（「## 1 需求复述与歧义」与「### 1.3 歧义与…」）→ 判定只作用于**最后一个**标题的块；更外层那个块里的无标记列表行**不**产生结构问题（块解析取最后一个命中，`src/change/plan_audit.rs:142` 的既有语义）。
- 给定一个歧义块里有 2 条 `[必须裁决]` 项、且两条都**没有** `用户裁决：` 的计划文本 → 裁决判定返回 ≥1 条问题，文本含「必须裁决」。
- 给定同一个计划、两条都带 `用户裁决：<答复>` 且两处答复都能在**当前批次**的问询记录里逐字找到 → 裁决判定返回 0 条问题。
- 给定一个歧义块里有 2 条 `[必须裁决]` 项、但当前批次的问询记录里只有 1 条已答复 → 裁决判定返回问题，文本指明「问过 1 条」与「列了 2 条」的数量差。
- 给定一条 `用户裁决：<答复>` 但答复文本在**当前批次**的问询记录里找不到（只在上一批次里有） → 裁决判定返回问题，文本含「问询记录」与「本轮」。
- 给定「块标题含『必须用户裁决』、条目本身不带分类标记」的歧义块 → 仍判为结构问题（复现第一场 A1–A6 的形态：没有标记就等于没分类，不许靠措辞蒙过去）。
- 给定记录器包住一个脚本化 `UserLink` 并问一次 → 黑板问询记录里有 1 条问答（问题原文与答复原文都在），且计数为 1。
- 给定记录器包住的 `UserLink::ask` 返回错误（用户没应答）→ 该条**不计**为已答复（答复为空）。
- 给定门 0 连续跑两轮（同一 change）→ 问询记录里出现**两个批次头**，第一轮的问答仍留在文件里（不清空）。
- 给定当前批次的问询记录里没有任何问答、但计划里有 `[必须裁决]` 项 → 裁决判定返回 ≥1 条问题（跨 run 的旧答复不得充数）。
- 给定 change plan 轮：脚本化 agent 写出带 `[必须裁决]` 项（无 `用户裁决：`）的计划且全程不调 `ui.ask` → `run_plan` 返回 `PlanTurn::Fail`（不是 `Pass`），问题清单含「必须裁决」。
- 给定 change plan 轮：脚本化 agent 先调 `ui.ask` 拿到答复、再把答复逐字写进计划的 `用户裁决：` → `run_plan` 返回 `PlanTurn::Pass`，且审查轮次为 1。
- 给定 change plan 轮：裁决门判 Fail → 打回消息里含「ask-user-question」与「用户裁决」两个锚点（agent 知道下一步该干什么）。
- 给定 change 六件套 e2e 夹具 `tests/e2e/fixtures/change_agents.json` → `change-plan-agent` 条目的 `extra_packages` 含 ask 包，且整个夹具与 builtin 逐字段一致（漂移守卫转绿）。
- 给定 e2e 渲染出的 `change-plan-agent` profile 的 `settings.json` → `packages` 含 ask 包。
- 给定 e2e 真链跑完：若 SSE 时间线里出现过 `ui.request` → 每个已答复的问题的**实际答复原文**（取自 autopilot 记录的 value，见 `tests/e2e/common.py:365`；或引擎侧黑板问询记录，两者应一致）都逐字出现在计划的 `用户裁决：` 行里；若一次都没问 → 计划里没有任何 `[必须裁决]` 条目（问答与计划双向一致）。
- 给定 autopilot 应答了一个 select 形状的请求 → 它记录的 `value` 等于 `_smart_answer` 实际选中的**选项标签**（`tests/e2e/common.py:389`-`:396`），而不是 `default_answer`（select 形状下后者不生效）。
- 给定 `forensics::record_panic(root, "boom-xyz", Some("src/change/session.rs:216"))` → `root/forensics/` 下出现一个 `panic-` 开头的文件，正文含 `boom-xyz` 与 `src/change/session.rs:216`。
- 给定当前运行上下文为 `phase=testing`、`stage=testing/agent`、`step=3` → panic 记录文件正文含 `testing`、`testing/agent` 与 `3`（当时阶段与步骤可见）。
- 给定 panic hook 已安装 → `catch_unwind` 一次 `panic!("boom-xyz")` → HOME 固定路径下出现 panic 文件且正文含 `boom-xyz`。
- 给定 panic hook 已安装 → stderr 仍能拿到 panic 信息（新 hook 链上原 hook，不吞 stderr）。
- 给定心跳记录里的 `pid` → 等于 server 进程 pid（不是任何 pi 子进程的 pid；子进程正常退出不得触发「进程已不在」）。
- 给定 change run 的 coding 阶段推进（基线轮 → coding 轮 → 审查轮） → 心跳记录里的 `stage` 依次为 `coding/baseline`、`coding/impl`、`coding/review`，且 `phase` 为 `coding`。
- 给定冷启动（无断点）→ 心跳记录的 `phase` **初值**为 `plan`（不是空值）。
- 给定续跑时断点里的 `phase=coding`（直接进 coding 阶段、不经任何阶段迁移）→ 心跳记录的 `phase` 为 `coding`（不是空、也不是 plan）——即 `src/change/orchestrator.rs:124` 的初值同步生效。
- 给定 change run 在 coding 阶段已有按步提交（提交信息含 `step N`） → 心跳记录里的 `step` 等于已提交的最大步骤号。
- 给定一个 `status=running` 且进程已不在的 run 记录 → 收割器写出验尸文件，正文含 run_id、phase、stage、step 与「未正常收尾」字样。
- 给定一个 `status=finished` 的 run 记录 → 收割器不写任何验尸文件。
- 给定一个 `status=running` 且心跳陈旧的 run 记录 → 收割器写出验尸文件，正文含最后心跳时间与「心跳陈旧」字样。
- 给定 change run 以**质量失败**结束（审查打回 / 验收不过） → 心跳记录的 `status` 为 `finished`（收割器不会给它写验尸）。
- 给定 change run 被**取消或超时** → 心跳记录的 `status` 为 `finished`（同上）。
- 给定门 0 因基线记录缺失/损坏而硬失败（`src/server/change_engine.rs:103`）→ 盘上**没有**该 run 的心跳记录（不留 `status=running`），下次收割也**不**写验尸文件（门 0 前的其余早退同理）。
- 给定门 0 成功、但登记之后在 watchdog 之前就失败（会话 spawn 失败）→ 心跳记录被 RAII guard 收口为 `finished`，收割器不写验尸文件。
- 给定心跳记录的 `attempt` → 等于断点里的 `RunState.attempt`（`src/runstate/mod.rs:69`）+ 1（首次跑断点里是 0 → 当前尝试号 1；重试后为 2、3…）。
- 给定 `src/main.rs` 源码 → 同时含 panic hook 的安装调用与「启动时收割陈旧 run」的调用（`include_str!` 守卫，防接线被悄悄删掉）。
- 给定 `src/lib.rs` 源码 → 含 `pub mod forensics;`。
- 给定 `src/change/mod.rs` 源码 → 含裁决判定纯函数的 re-export（对外导出面与既有风格一致）。
- 给定 `ensure_baseline` 在「记录存在且非空且是当前仓库的祖先提交」时 → 返回该记录里的 sha，且**不**改写记录文件（第一场 step 8 的行为不得回归）。
- 给定 `ensure_baseline` 在「计划目录的记录缺失、但黑板备份存在且有效」时 → 返回备份里的 sha，并把计划目录的记录补写回去。
- 给定 `ensure_baseline` 在「记录存在但内容不是合法 sha（损坏）」时 → 返回错误，错误文本含「基线」与「损坏」，且记录文件**不被改写**。
- 给定 `ensure_baseline` 在「记录里的 sha 不在当前仓库历史中（仓库被重建）」时 → 返回错误，错误文本含该 sha 与「历史」，且记录文件**不被改写**。
- 给定 `ensure_baseline` 在「记录里的 sha 存在但不是 HEAD 的祖先」时 → 返回错误，错误文本含「祖先」。
- 给定冷启动（无断点、无计划文件、无任何基线记录） → `ensure_baseline` 记录当前 HEAD 并正常返回（首跑不误伤）。
- 给定「这个 change 已在跑」（有断点或已有计划文件）且基线记录与备份都缺失 → `ensure_baseline` 返回错误，错误文本含「基线」与「重记」字样，且**不**把当前 HEAD 写成新基线（自锁路径被堵死）。
- 给定基线写入过程被打断 → 目标目录不留 `.tmp` 残留文件，且记录文件要么是旧的完整内容、要么是新的完整内容（原子写）。
- 给定 coding 轮开始时黑板里已有上一轮的自证证据 → 旧文件被轮转为 `.prev`，当前文件不存在（新证据门拿不到旧证据）。
- 给定 testing 轮开始时黑板里已有上一轮的回归证据与验收对账表 → 两者同样被轮转为 `.prev`（当前文件不存在）。
- 给定黑板 `change/current` 标记文件丢失、但 `change/<change-id>/` 目录仍在 → `assign_change_id` 返回**同一个** id（不换号、不换基线）。
- 给定黑板 `change/state.json` 内容损坏 → `change::state::load` 仍返回冷启动状态，且留下可观测痕迹（reason 出现在日志里，不静默吞掉）。
- 给定 change run 处在 coding/testing 阶段、但目标仓的计划文件缺失 → run 快速失败，错误文本含「计划文件」与「缺失」（而不是报成「范围膨胀」）。
- 给定 coding 轮的开工指令 → 含「不得重编已提交步骤编号」的约束（续跑不得改号）。
- 给定 `docs/plantree/plans/009-change-sop/decisions/README.md` → 含 D024（问询门）、D025（崩溃取证）、D026（续跑加固）三条决策，每条写清「决定 / 为什么 / 影响」。
- 给定 `docs/plantree/plans/009-change-sop/topics/agents-change.md` → `:20` 行的答复载体与 `:60` 行的 change-plan-agent 供给，均与本次改动后的实际一致（载体=歧义块的 `用户裁决：` 字段；供给=codegraph + ask-user-question 包）。

## 6 风险

- **R1 · 工作区当前不干净（开工前必须处置）**：`git status` 显示 `src/change/coding_gate.rs`、`src/change/session.rs`、`src/config/manifest.rs`、`src/config/skill_assets.rs` 四个文件有**未提交**改动（第一场测试阶段的产物，+880 行）。相对本变更基线 `3020873`，`git diff --name-only` 会把这四个文件算成「本次改动」→ 越界门（`src/change/boundary.rs:102`）会判「范围膨胀」，且步骤↔提交门（`src/change/coding_gate.rs:256`）会因多出提交而红。**处置：开工前由操作员把这批改动提交（单独一次提交）或 stash；本白名单不含「顺手提交」这四个文件的改动。**（本计划的基线记录已经把 `3020873` 记为锚点，重记基线会把这个洞再踩一次。）
- **R2 · 裁决门误伤（假阳性）**：契约要求「歧义」块的每条列表行都带分类标记——若 agent 漏标一行 → 结构问题 → 打回重写（最多 3 轮，`src/change/orchestrator.rs:24`）。这是**设计意图**（不许有「没分类的歧义」），但代价是第一次交审常被打回。缓解：提示词把契约写死 + 门的问题文本指出具体是哪一行缺标记 + 契约标记在提示词与门之间有锚点单测防漂移。
- **R3 · 问询门把 run 逼进死路**：门不过 → 打回 → 最多 3 轮 → 仍不过则 Quality（`src/change/orchestrator.rs:159`）→ 整场失败。这是设计意图（「不许静默自决」），但若用户不在线（`ui.ask` 阻塞等应答，`src/server/mod.rs:140` 的 `rx.recv()`），run 会**卡在等应答**上。缓解：门的问题文本必须让 agent 知道「问不到人就如实写明无法裁决，不要硬编答复」——本计划**不**引入「问不到人自动放行」的口子（那正是第一场的病），但要在风险里明说：**用户不在线时，change 的 plan 阶段会阻塞等待**。
- **R4 · 装包不等于能问**：包装上只保证 pi 侧有 `extension_ui_request` 能力；真发不发得出来还看 agent 行为（提示词纪律）与 pi 版本。缓解：机械门兜底（没问过 → 计划过不了），所以「装了但模型不问」不会静默通过；e2e 真链可实证。
- **R5 · 运行时清单缓存导致「改了不生效」**：`src/main.rs:152` —— `~/.devforge/manifest.json` 存在即用落盘清单 → 必须重存清单（PUT `/api/v1/config/manifest`）并重渲染 profile，ask 包才会进 profile。仓内无法验证这一段，属部署侧动作。
- **R6 · e2e 夹具漂移导致真链假绿**：`tests/e2e/fixtures/change_agents.json` 不重生成 → e2e 里的 change plan agent 仍无 ask 包 → 真链「跑通了」但问询环不存在。漂移守卫（`src/config/manifest.rs:1251`/`:1600`）会红，但只在仓内单测里；e2e 用的是夹具文件本身 → 必须重生成 + 重跑 e2e 断言。
- **R7 · 证据轮转可能丢内容**：轮转只保留一份 `.prev`，再下一轮会覆盖 → 跨多轮的证据历史只留最近两轮。这是「减法优先」的取舍；取证需求由问题 2 的独立目录承担，不靠黑板证据。
- **R8 · 心跳字段口径漂移**：**单一真源**已钉死——`phase` 只由编排器写（`src/change/orchestrator.rs` 的五个阶段迁移点：`:152`/`:177`/`:247`/`:263`/`:271`，含两条回退路径），`stage` 只由会话层写（`src/change/session.rs` 的七个写点，词表见 Q16），`step` 只从已提交历史取（`src/change/coding_gate.rs:209`+`:230`），`pid` 只记 server 进程（Q16）。会话层**不**写 phase，编排器**不**写 stage → 两处不会互相覆盖。缓解：stage 首段必须等于当前 phase，不一致时心跳记录里显式标出（供验尸文件提示），并由单测钉住映射关系。
- **R9 · 取证根推导不一致**：`src/main.rs:144` 用 `HOME/.devforge`，引擎侧只拿得到 `plans_root` → 两处推导必须一致（`plans_root` 的父目录），否则 panic 文件落错地方、收割器扫不到。缓解：提供单一推导函数 + 单测钉住（§5 的 `src/lib.rs` 守卫同款思路）。
- **R10 · panic hook 装不上或装两次**：`std::panic::set_hook` 是进程全局、只能设一次 → 装 hook 必须幂等（重复调用不报错、不覆盖）；落盘逻辑拆成纯函数（`record_panic`）以便单测。缓解：纯函数单测 + `include_str!` 守卫钉住接线。
- **R11 · 验尸文件误报（阈值噪音）**：心跳陈旧阈值过短 → 正常的长步骤也会被判「异常」并写验尸。缓解：阈值取明显大于心跳间隔的量级（Q7）。
- **R12 · 基线硬失败会挡住正常续跑**（Q12）：基线记录缺失时改为硬失败 → 若某场 run 的基线确实丢了（且黑板备份也没有），该 change 就得人工清理重开，而不是自动恢复。这是刻意的取舍（自动重记 = 自锁），错误文本必须写清恢复步骤，否则操作员会以为系统坏了。
- **R13 · 「非祖先」判据可能过严**：目标仓被 rebase / 历史被改写后，原基线可能存在但不是 HEAD 祖先 → 硬失败。取舍：宁可停下叫人（010 D001「跑错比白跑贵」），不做自动猜测。风险是偶发的合法场景被拦，错误文本必须指明可执行的下一步。
- **R14 · 文档与代码不一致（三处，不止台账）**：①`docs/plantree/plans/009-change-sop/decisions/README.md:159` 现登记至 D023，本次新增 D024–D026，而 `src/config/manifest.rs:73` 的注释写明提示词「勿手改——改决策先改 plan tree」→ 提示词与包的改动必须与台账登记**同一次提交**落地；**且 D009（`docs/plantree/plans/009-change-sop/decisions/README.md:64` 标题、`:66` 决定行的「仅 codegraph MCP」）因本次改供给而失真** → D024 必须**显式标注取代 D009 的供给部分**（体例同 D022a 取代 D022，见同文件 `:152`），且 D009 标题也要加「供给部分于 2026-10-21 由 D024 取代」的旁注（**两边都标**，只标一边等于旧决策仍看着有效）；②`docs/plantree/plans/009-change-sop/topics/agents-change.md:20`（答复写进「需求复述」）与 `:60`（change-plan-agent 仅 codegraph MCP）因本次**改载体 + 改供给**而失真 → 已进白名单，本次同步这两行；③`docs/plantree/plans/009-change-sop/roadmap.md:24`（T004 的供给描述）同样因改供给而失真 → 已进白名单，本次同步该行。**遗留**：agents-change.md 里检查点路径那段（写作 `docs/plantree/changes/<id>/state.json`）与 D022a 已定的黑板口径不一致，属**本变更之前**的陈旧漂移，不在本次范围（不顺手改），登记在 009 台账 D026 的「遗留」小节。
- **R15 · 与 010 决策链的关系**：本次收紧的是 change SOP 的续跑行为（基线不可重记、证据轮转、id 恢复），不改变 010 的通用口径（断点真源 / 默认续跑 / done 自校验）→ 010 台账不动，只在 009 台账交叉引用；若将来要给「引擎通用层」加同样的加固（如 `src/runstate/mod.rs` 的 `Unusable` 处理），必须先回 010 决策。
- **R16 · 无新依赖的约束**：panic hook / 心跳 / 验尸全部用标准库（`std::panic`、`std::fs`、`std::thread`、`std::time`）→ 不得引入 `panic` crate 之类的第三方；也不得为了取证去读 `/proc` 之外的敏感面。
- **R17 · 死因仍是未知**：本次交付的是**取证能力**，不是死因结论。已知候选（供后续排查参考，不在本次改动范围）：子进程继承 `DEVFORGE_MANAGED` 后被清场逻辑波及（`src/engine/ledger.rs:119` 的 `scan_and_kill_run`，SIGKILL 在 `src/engine/ledger.rs:137`）、watchdog 的失活收割（`src/server/mod.rs:1393`）、宿主机层面的外部 kill。验尸文件上线后才有数据可判。
- **R18 · e2e 真链里答复原文不由 e2e 决定（真实答复是 agent 自拟的选项标签）**：`tests/e2e/test_26_change_e2e.py:194` 只是把 `default_answer="继续"` 传给 autopilot；而 ask-user-question 发的是 **select 形状**请求（回放脚本见 `src/sop/rpc_bridge.rs` 末尾的 `method":"select"`）→ `tests/e2e/common.py:372` 的 `_smart_answer` 对 select 会改取**选项标签**（`:389`-`:396`：标题含「完成/是/确认/已/通过」时取命中的标签，否则取 `labels[0]`），而**选项标签是 agent 自己拟的** → 真实答复既不是「继续」，也不由 e2e 固定。真实失败模式是：**agent 抄的是自己拟的选项标签，而门要求逐字等于实际答复**——只要 agent 改写了措辞（如写「采用第一个选项」而不是标签原文），裁决门就判 Fail → 打回 → 3 轮耗尽 → Quality → e2e 变红。缓解：①门的问题文本要求逐字抄答复原文；②`tests/e2e/common.py:365` 改为记录实际 POST 出去的 value（本次改，见 §3.1）→ 断言拿得到真值；③断言做成**双向一致性**（问过就必须逐字出现在计划里；没问过就不许有 `[必须裁决]` 条目），失败时能一眼分出「问了没抄」「抄的不是标签」「没问却标了」；④黑板问询记录（引擎侧）留有同一份答复原文，可与 autopilot 记录交叉核对。
- **R19 · 「全标自决」的绕过**：机械门只能判「结构齐不齐、问了没」，判不了「该不该问」——agent 若把 6 条必须裁决项全标成 `[自决]`，门无话可说。缓解：把「分类恰当性」写进审查官判据（`CHANGE_PLAN_REVIEW_PROMPT`，Q15），由审查官对着需求与代码判断；机械门保证「每条都显式分类过」，不保证「分类正确」。这是本设计的**已知能力边界**（厚 Agent、薄编排的取舍），不是遗漏。
- **R20 · 续跑回到 plan 阶段要重新问一次**：门 0 每轮 run 开新批次（Q4），所以续跑回到 plan 阶段时必须重新问用户（用户不在线 → 阻塞在 `src/server/mod.rs:140` 的 `rx.recv()`）。取舍：宁可多问一次，也不许沿用上一场的静默自决（Q18）。缓解：打回回路有 3 轮，agent 下一轮补问即可；记录按批次追加，上一场的答复仍在盘上可查。
- **R21 · `src/forensics.rs` 是待建文件（已知误报）**：机械审计的白名单存在性检查（`src/change/plan_audit.rs:89`）会把尚未创建的新文件报为「白名单里的文件不存在」——**沿用上一场 A4 的口径：这是待建文件误报，不是边界问题**，消解路径是 coding 阶段把该文件建出来即消解。**硬约束**：`src/forensics.rs` 是 `src/lib.rs` 里 `pub mod forensics;` 的编译期依赖（模块清单在 `src/lib.rs:9`，新增行与其同级），两者必须**同一次提交**落地，否则 `cargo build` / `cargo test` 编不过。
- **R22 · 新结构契约会作废本计划自身（自举影响）**：Q2 要求「歧义」块每条顶层列表行以 `[必须裁决]`/`[自决]` 开头，而本计划 §1.3 原本是 `- **Q1 …**` 形态（无标记）→ 变更落地后，同一 change 续跑回到 plan 阶段（`src/change/orchestrator.rs:271` 的 testing→plan 回退）时，**这份计划会被自己的结构门打回**。处置：①**块解析规则已钉死**（Q2：取**最后一个**含「歧义」的标题，既有 `section_lines` 语义，`src/change/plan_audit.rs:142`）→ 本计划有两个含「歧义」的标题（`## 1 需求复述与歧义` 与 `### 1.3 歧义与本计划采用的默认`），但**判定只作用于后者** → §1.1/§1.2 里的无标记列表行不在块内、不判结构问题（这一点是 R22 处置成立的前提，已按 Q2 复核）；②**本计划已按新契约自修**（§1.3 的 18 条全部带 `[自决]` 标记；§1.3 唯一的顶层散文行不含「必须裁决」「必须用户裁决」「需用户确认」等措辞）→ 本 change 自己续跑回 plan 阶段不会被打回；③一般情形：变更前写下的旧计划文件若在续跑时进入 plan 阶段，plan agent 会按新提示词重写计划（打回-修复回路有 3 轮，缺标记的行被打回后补上即可）→ 暴露面仅限「被回退到 plan 阶段」的那一次；④非 plan 阶段不受影响（白名单/验收标准的读取口 `src/server/change_engine.rs:124` 不跑结构门）。
