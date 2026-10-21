# change · 决策记录

> 只记**已拍板**的东西；每条写「决定 / 为什么 / 谁拍的 / 影响」。
> 待定项在 `../open-questions.md`，关闭后搬到这里。

## D001 · 无 git 时由引擎建基线

**决定**：门 0 检测到无 `.git` → `git init` + 一次全量提交（信息 `devforge: baseline`），基线 sha 落 `docs/plantree/<change-id>/baseline.txt`。
**为什么**：没有基线就没有 diff、没有回滚、没有“只改了该改的”的证据；靠人肉记忆不可审查。
**拍板**：用户 2026-10-20。
**影响**：目标仓会被写入 `.git`——这是对 P008「目标仓只读」红线的**显式例外**，仅限 `.git` 与 `docs/`。

## D002 · 文档完整度不由引擎探测

**决定**：引擎不判断“有没有文档”，只认调用方传的 `doc_level`（`none` / `essential` / `complete`），默认 `none`。
**为什么**：文档该不该补是人的决定；程序猜错一次就会在错误的仓库上跑几十分钟文档。
**拍板**：用户 2026-10-20。
**影响**：`essential` 复用 `doc-from-zero` 轻量档；`complete` 由人显式触发。

## D003 · 计划落目标仓 `docs/plantree/`

**决定**：`change-plan.md`（六块）、影响面、边界都写进目标仓 `docs/plantree/`；运行证据留黑板。
**为什么**：计划要能随改动提交、被审查、以后回查；放黑板等于工程侧看不见。
**拍板**：用户 2026-10-20。

## D004 · plan 阶段独立 profile

**决定**：新建 `change-plan-agent` / `change-plan-review`（以及后续 `change-coding-*` / `change-test-*`），与 MVP 的 `plan-agent` / `plan-review` 平行。
**为什么**：变更计划的纪律（影响面证据、白名单、验收命令）与从零规划的纪律不同，混在一个提示词里两边都会漂；名字用 `change-<阶段>-<角色>`，避免“change-review”被误读成“代码审查”。
**拍板**：用户 2026-10-20（含命名口径）。

## D005 · 越界改动分两类：范围膨胀 vs 计划缺口

**决定**：coding 完成后引擎算 `git diff --name-only <baseline>` 对白名单：
- 全部在白名单内 → 进 testing；
- 越界且 agent **已输出 `<route>plan</route>` + `<impact-gap>`（含文件/理由/证据）** → 打回 plan 补边界，边界增量过审查后重进 coding；
- 越界且无申报 → **FAIL（范围膨胀）**，一票否决。
**为什么**：两种越界在 diff 上无法区分，硬 FAIL 会把“计划漏了必需文件”逼成欺骗或偷改；
判据选「有没有先回 plan」—— 边界是计划的一部分，扩边界必须回计划留痕并过审。
**拍板**：用户 2026-10-20（提出二分），机制由本 plan 定。
**影响**：change 的编排器不能直接照搬 `run_coding`：多了基线 diff、plan 产物与**coding→plan 反向回退**（MVP 只走正向交接）；详见 `topics/boundary-protocol.md`。

## D006 · 回归优先

**决定**：testing 阶段先复跑既有套件，再补变更测试；回归失败直接阻塞。
**为什么**：在既有项目上，“新测试过了”不代表“没弄坏别的”。

## D007 · 改动必须有计划依据（不追求“最小改动”）

**决定**：判据是**每一处改动能指回计划里的一条依据**，不是 diff 越小越好。
- 结构性改动（抽接口、改数据结构、拆模块）**允许**，但必须写进 plan 的「要改的」表并过审查；
- 禁止的是**无依据改动**：顺手格式化、顺手改命名、删无关代码、无申报的越界。
**为什么**：“最小改动”容易退化成打补钉——该重构时硬压成最小，代价是后面的人还债；
真正要防的是“没人知道为什么改了这行”，而不是“改了多少行”。
**拍板**：用户 2026-10-20（否定了“最小改动”这个口径）。
**影响**：审查不拿 diff 大小当信号，只拿“越界文件有没有依据、依据成不成文”当判据。

## D008 · 基线不做体积守卫

**决定**：`git init` 后全量 `git add -A` + 提交，不按大小/文件数做豁免。
**为什么**：会被豁免的往往正是关键文件；漏登一个文件，后面 diff 就看不见它。真正可能有 git 的大仓也走不到这条分支。
**拍板**：用户 2026-10-20。

## D009 · change-plan 只给 codegraph（npm 包供给由 D024 增补）

**决定**：`change-plan` profile = deepseek-flash + **仅 codegraph MCP**；不给 context7，不给 `doc-contract`。
**为什么**：改既有代码靠结构信息（谁调它、它依赖谁）；库文档帮助有限但会稀释注意力。`doc-contract` 是实体文档的契约，计划文档套它会制造无意义的必填节。
**拍板**：用户 2026-10-20。
**增补（2026-10-21）**：本条只约束 **MCP 供给**（仍然只给 codegraph）；**npm 包供给**由 D024 增补（`change-plan-agent` 装 ask 包，用来真问用户）。两层互不冲突——只标一边等于旧决策看着仍有效。

## D010 · plan 不跑测试

**决定**：plan 阶段不跑测试、不产回归命令；基线由 **coding 前置门**自己判断怎么跑并取证。
**为什么**：三个阶段解耦——plan 面对的是需求与代码结构，跑测试是 coding/testing 的本职；硬塞进 plan 就是越权。
**影响**：原 D010（允许跑只读命令）作废；plan 的输入只有代码、文档与需求。
**拍板**：用户 2026-10-20（推翻了前一版）。

## D011 · 有文档时先读文档再 codegraph 核实

**决定**：`docs/contexts/` 存在 → 先读文档建地图，再用 codegraph 核实与补漏；冲突以代码为准，并记入风险。
**为什么**：文档是别人（或过去我们）已经付过的理解成本，不读就是重复劳动；但文档可能过期，不能当真理。
**拍板**：用户 2026-10-20。

## D012 · plan 阶段的审查不执行验收命令

**决定**：`change-plan-review` 只读计划与基线证据，不执行验收命令；执行归 `change-plan-agent`（基线证据）与 testing 阶段。
**为什么**：验收命令验证的是「改完之后」的状态（新测试、新行为），在 plan 阶段本来就跑不通；让审查官去跑只会把「命令没写错」和「功能没做完」混为一谈。
**拍板**：用户 2026-10-20（质疑了「为什么 plan 阶段要跑验收」）。

## D013 · plan 只写验收标准

**决定**：六块里的第 5 块只写**验收标准**（输入→期望输出）；回归命令与新功能的测试命令都不写。
**为什么**：跑测试是 coding / testing 的本职，agent 自己知道这个项目怎么跑（README/CI/包管理器），用不着 plan 教；plan 写了反而是越权与冗余。
**影响**：plan 审查只核验收标准可观测，不评命令怎么写、不拿测试结果当证据。
**拍板**：用户 2026-10-20（第二次纠正）。

## D014 · coding 阶段写单测，不写端到端

**决定**：沿用 MVP 口径——coding 只写单测（每个改动点至少一条），**不写端到端**；测试文件必须列在白名单里，否则写测试本身算越界。
**为什么**：单测是“改对了”的自证，e2e 是“整体没坏”的验证，后者慢且归 testing；白名单不包含测试文件的话，写测试会被自己的越界机制拦住。
**拍板**：用户 2026-10-20。

## D015 · 实施顺序归 coding，不进 plan

**决定**：plan 只出六块（需求/影响面/三张表/白名单/验收标准/风险）；实施步骤由 coding 阶段自己产出 `impl-steps.md` 并按步提交。
**为什么**：plan 是**参考与目标**，不是实现指令。plan agent 不是编码专家，它排的顺序没有权威；硬写进去只会在执行时多一层“计划写错了”的扯皮。
**影响**：审查对账时同时看两边——plan 的影响面/边界（范围对不对）与 coding 自己的 impl-steps（步骤自洽不自洽）。
**拍板**：用户 2026-10-20（纠正了实施顺序归属）。

## D016 · change-coding-agent 给 codegraph + context7

**决定**：`change-coding-agent` = deepseek-flash + codegraph + context7 + 四语言 best-practices，与 MVP `coding-agent` 同款供给。
`change-coding-review` 只给 codegraph（审查靠对账与读 diff，不靠写实现）。
**为什么**：它在真写代码，改的又常常是别人库的调用；不给 context7 就只能猜 API 形状。
**拍板**：用户 2026-10-20。

## D017 · 回归由 Agent 跑，引擎不代劳

**决定**：回归命令由 test agent 自己判断并执行（或 coding 前置自己跑），引擎不跑回归、也不重跑；引擎只核证据（日志里退出码与自述是否矛盾）。
**为什么**：引擎根本不知道这个项目的测试怎么跑（pytest / go test / npm / make），凭什么是它来指挥；跑测试是 agent 的本职。
**拍板**：用户 2026-10-20。

## D018 · 回归失败直接丢回 coding（定位口径由 D025 改写）

**决定**：testing 阶段回归红 → 直接 `<route>coding</route>` 带失败摘要；不要求 test agent 分析“是谁弄坏的”。
**为什么**：test agent 只看行为不看代码，定位不了；定位是实现方的事。
**拍板**：用户 2026-10-20。
**旁注（2026-10-21）**：本条的「不做定位」口径由 D025 改写——回注改为带**失败明细逐条原文**（含 `path:line`）；「不要求 test agent 分析是谁弄坏的」这半条不变。

## D019 · testing 不给 codegraph

**决定**：`change-test-agent` / `change-test-review` 不给 codegraph（沿 MVP 黑盒纪律）。
**为什么**：给了就会写出贴着内部实现的测试，改一次就碎；测试只看行为。
**拍板**：沿用 MVP 口径（2026-10-20 确认）。

## D020 · 完成标记沿用 MVP，但无标记重推有界

**决定**：标记一律 `<stage-complete>N</stage-complete>`（不另造）；**无标记重推上限 3 次**，第 4 次判基础设施故障并带上最近一次回复尾巴。
**为什么**：标记确实由 AI 生成，它可能一直不发——MVP 的「零上限、模型自然收敛」在这里就是无限打转的漏洞（用户 2026-10-20 指出）。
**影响**：与 MVP 的行为差异必须显式登记；审查侧协议违例的上限仍为 3。**MVP 同步改**：005 decisions/015 与 007 decisions/D013 同口径（进度循环设界，质量循环不设）。

## D021 · 没有独立文档阶段，文档由 coding agent 写

**决定**：change 不设文档阶段；改动涉及的 `docs/contexts/` 由 coding agent 同步更新，`doc-lint` 退出码必须为 0（与 MVP coding 同款门）。收尾只剩引擎写 `report.md`。
**为什么**：文档是改动的交付物之一，不是另一个 agent 的后手修；拆出去就会出现“代码改了文档没改”的空窗。
**拍板**：用户 2026-10-20。

## D022 · 检查点由引擎写（位置于 2026-10-20 由 D022a 取代）

**决定**：检查点（阶段、基线 sha、往返次数、已完成步骤/验收项、审查轮次）由引擎写，agent 不碰。
**为什么**：检查点是编排状态，不是 agent 的笔记；agent 写就会出现“自述完成”与事实不符。
**拍板**：用户 2026-10-20（要求做续跑）。

## D022a · 检查点改住黑板（取代 D022 的位置部分，2026-10-20）

**决定**：checkpoint 从 `docs/plantree/changes/<id>/state.json` 改为**引擎级黑板** `plans/<project>/<task>/change/state.json`（plan 010 D001/D005）。`docs/plantree/changes/<id>/` 只留交付物：`change-plan.md` / `history.md` / `report.md`。
**为什么**：checkpoint 是**运行进度**，不是交付物；跟着目标仓走会让“同一 change 换会话接着做”失真，也会被清理产物的操作连带删掉（文档 SOP 已实测踩过一次）。
**影响**：P009 T011b（checkpoint + resume）的落点改为黑板；续跑语义、证据复核、故障重启统一按 plan 010 执行，本 SOP 只声明自己的状态扩展段与 done 证据。
**拍板**：用户 2026-10-20。

## D023 · 工程纪律 skill 的供给口径 + change 路径的「先证后报」机械门

**决定**：①pstack 的 24 条 principle **蒸馏成一份**工程纪律 skill（`assets/skills/coding-discipline/SKILL.md`，采纳 12 条，每条含触发时机 / 规则 / 反例 / 证据形态），随 profile 分发给**四个** coding/review Agent（`coding-agent` / `coding-review` / `change-coding-agent` / `change-coding-review`），其余十一个 Agent 不给；②change 路径新增一道「先证后报」机械门（黑板 `evidence/self-check.log` 存在 + 退出码为 0，形状与既有 baseline 门一致，失败即 Quality、不自动重问 Agent）；③机械门**只落 change 路径**，MVP 黑板路径不接门。
**为什么**：①纪律只有一份才不漂移——原样 vendor 24 份会让 profile 膨胀，且 pstack 的 principle skill 靠 `poteto-mode` 按名路由，本仓 coding agent 没有这层路由（skill 必须留在系统提示的技能清单里才能被按需加载：frontmatter 要有非空 `description`，且**不设** `disable-model-invocation: true`）；②纪律光写没人查等于白写——「先证后报」里能机械判的那部分（证据存在 + 退出码为 0）下沉成门，与仓内「厚 Agent、薄编排」一致；③MVP 的 `CodingSopLoop::run` 未进黑板路径（`src/sop/coding.rs:113`），加门要动编排 API，本次只治理 live 的 change 路径。
**影响**：四份 coding/review 提示词按各自现状分别改（只引用 skill，不复述既有条款，避免口径冲突）；change coding 轮多一道机械门（kickoff 与审查指令写清证据路径与写法，审查侧核对证据与本轮 diff）；新文件 `assets/skills/coding-discipline/SKILL.md` 是 `src/config/skill_assets.rs` 的编译期依赖，必须与注册改动同一次提交落地。
**拍板**：计划 `change-20261020-backnotprop-pstack-github-ai-skills-home`（2026-10-20）。

## D024 · 问询环：供给 + 结构契约 + 硬门 + 审查审点

**决定**：①**供给**——`change-plan-agent` 装 ask 包（`npm:@juicesharp/rpiv-ask-user-question`，与 MVP `plan-agent` 同包）：问询环此前在**供给层就不存在**（agent 手里没有能发 `extension_ui_request` 的工具）；②**产出侧结构契约**——歧义单独成小节（标题含「歧义」），每条写成顶层列表行并带分类标记：`[必须裁决]`（需求与代码都定不下来、且影响验收口径）必须先用 `ask-user-question` 真问用户、把答复**逐字**写进同条目的 `用户裁决：` 字段，**禁止**带默认值自决放行；`[自决]`（不涉及验收口径）由 agent 按判断定；无歧义也要显式写「无歧义」；③**引擎侧确定性硬门**（`change::plan_audit::audit_verdicts`）——歧义块里标了 `[必须裁决]` 却缺**非空** `用户裁决：` → 计划轮直接打回（`PlanTurn::Fail`，不进审查轮，走既有 3 轮修复回路）；④**审查侧语义审点**——新增**留痕核对**与**分类恰当性**两条，并把裁决门的确定性事实（未分类条目、散文里的必须裁决声明、计数）注入审查指令。
**为什么**：上一场自举实测 `questions_total=0`——change plan agent 从来没跟用户说过一句话：它把「必须用户裁决」的歧义全按默认值自决放行，写进计划、过审、进 coding。上一场已证明「只改提示词」不够（模型会静默自决），故四层叠加：供给（有没有工具）+ 产出契约（怎么写）+ 硬门（写没留痕，机械判）+ 审查审点（该不该问，语义判）。分工与仓内既有约定一致（D012：引擎算确定性的东西，审查官只做语义判断）。
**与 D009 的关系**：D009 只约束 **MCP 供给**（change-plan 仍只给 codegraph，不给 context7）；D024 增补的是 **npm 包供给**（ask 包）。
**已知能力边界**：硬门只判「标了必须裁决却没留痕」，判不了「该不该问」——把该问的标成 `[自决]` 是绕过，归审查官的分类恰当性审点 + 真链 e2e 见证兜底。
**遗留（本靶只登记，不改）**：①`change-coding-review` 无包，比 MVP `coding-review` 少一个诊断包 `pi-lens`；②`change-test-agent` 比 MVP `test-agent` 多一个 `pi-lens`（与 MVP 黑盒纪律相悖）；③**testing 阶段的验收测试从不提交**——上一场遗留的 880 行未提交补录让下一场 coding 继承脏工作区，越界门（`git diff --name-only <baseline>`）判范围膨胀；本场已用 `git stash` 归位（内容可恢复）并登记。
**拍板**：计划 `change-20261021-change-sop-path-line-src-config-manifest`（2026-10-21）。

## D025 · 回注失败上下文送达 coding agent（失败明细逐条原文）

**决定**：①**送达通道**——测试阶段回注 coding 时，把**上一轮测试失败的明细逐条原文**（含 `path:line`）写进断点的失败上下文（`TestFailureContext { round, issues }`：`round` = 第几轮回注，与 `test_reroutes` 同值；`issues` = test agent 给的原文，不剥不改写不截断），并在**下一轮 coding 的开工语**里以送达块送达（标题行「上一轮测试失败明细如下（逐条原文）」+ 明细逐条一行）；**无上下文时开工语逐字不变**（不注入任何空块）。②**三出口同源**——coding 开工语、WARN 日志、Quality 失败原因文本的失败明细**同源同原文**；回注消息**不再剥 `path:line`**、**不再写「请自己定位原因」类零信息话术**（D018/D019 的定位口径由本条改写）。③**清除语义三条规则**——**覆盖**（新一次回注的 issues 覆盖旧的，不追加）、**消费即清**（coding 轮过门转 testing 时清）、**终态清**（run 到 done 时清）；**例外**：回注超限分支**既不写也不清**（超限前有上下文就原样保留——该分支没有下一次 coding 轮）。④**续跑一致性**——上下文住在断点里，崩溃/重启后照样在同一轮送达同一份（幂等），清除点也照样在同一条状态迁移上生效。
**为什么**：上一场终审查出一条硬事实——回注消息此前只有两个出口（Quality 错误文本、WARN 日志），都是**运维/日志面**，不是 coding agent 的**输入面**；`ChangeState` 没有任何字段存上一轮失败 → 「让 coding 方能定位」在当前数据流下不可能成立。不剥定位、不写零信息话术，是因为同一失败在三个出口出现两套内容时，运维看到的与 coding 拿到的对不上。
**影响**：改动落在 `src/change/state.rs`（失败上下文 + 断点字段 + 写入/清除 helper）、`src/change/testing_gate.rs`（回注渲染 + 共用明细渲染）、`src/change/session.rs`（开工语送达块）、`src/change/orchestrator.rs`（写入/清除三处状态迁移 + `ChangeSession::run_coding` 增上下文参数）、`src/change/mod.rs`（re-export）；`ChangeSession` 是对外 re-export 的 trait，签名变更属破坏性变更（本仓是应用不是库，仓内只有两处实现）。**本条只登记不解决**：①回注轮的**可达性**——基线门要求 `evidence/baseline-test.log` 退出码为 0，而回注轮里测试是红的，基线门可能在 coding 开工语之前就把 run 判死；②test agent 侧「不猜文件位置」的生产者侧纪律（`src/config/manifest.rs`）本场不放开 → 实际送达的 issues 可能没有 `path:line`；③`route` 无 issues 时的拒收/追补（下一场协议硬化）。
**拍板**：计划 `change-20261021-coding-agent-c4-path-line-coding-src-cha`（2026-10-21；用户裁决 Q1 三出口同源、Q2 台账占 D025）。
