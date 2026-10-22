# 变更计划 · change-20261021-change-sop-path-line-src-config-manifest

目标仓：`/home/trtyr/code-work/devforge`
基线：`30208734c035145ede001958a10ee8a5ea5cd633`（`docs/plantree/changes/change-20261021-change-sop-path-line-src-config-manifest/baseline.txt`）
上一场：`change-20261020-backnotprop-pstack-github-ai-skills-home`（已 Settled，HEAD=`3020873`）
同题参考（**未执行**）：`docs/plantree/changes/change-20261021-2026-10-20-change-sop-devforge-settled-e/change-plan.md:1` —— 那是三靶合并版（问询环 + 死亡诊断 + 续跑稳健化）；本计划只取其「问询环」一靶，并按本靶口径**删掉**问询记录层（见 Q5）
文档档位：none（计划阶段照常读已有文档；不跑文档流水线）

---

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

上一场让 devforge 用 change SOP 改它自己，两场自举（2026-10-20）跑下来 `questions_total=0`——**change plan agent 从来没跟用户说过一句话**：它把自己标成「必须用户裁决」的歧义全部按默认值自决放行，写进计划、过审、进 coding。调用方要的「它会跟你沟通，你再跟我沟通」这条环**从未闭合**。

这一场是**单靶**：只修「问询环失效」，不碰死亡诊断、不碰续跑稳健化（那是后续场次）。要修的是四件事：

1. **供给**：给 `change-plan-agent` 装上 ask 包（与 MVP `plan-agent` 同源，`src/config/manifest.rs:22`），并同步修正把缺陷锁死的单测断言（`src/config/manifest.rs:1097`）。现在这个 agent 结构上就没有能向用户提问的工具。
2. **排查**：其余五个 `change-*` agent 逐个过一遍——谁在职责上需要问用户/报偏差，对照 MVP 侧同类 agent 的配置，结论逐个落地（钉进单测），不是「看一眼觉得没问题」。
3. **行为收口（双保险）**：提示词侧写死「必须用户裁决」的条目**不许带默认值放行**，要么用 ask-user-question 真问出去（走 `ui.request` 链路），要么在计划审查那一层被**打回**；审查侧新增判据（留痕核对 + 分类恰当性），引擎侧补一道**确定性硬门**（判「结构与留痕」这种能机械判的事），审查官判「分类对不对」这种语义事——分工与仓内既有约定一致（`src/change/plan_audit.rs:3`「引擎算确定性的东西，审查官只做语义判断」）。
4. **真链证明**：修完后在真链上要**观察到 `ui.request` 真实发出**（用可控的 e2e 夹具植入一处「必须用户裁决」的歧义来触发）。同时**先把 e2e 的装配耦合修对**：`tests/e2e/test_26_change_e2e.py:138`-`:143` 的 `have`/`extend` 在宿主清单已含 `change-*` 时会变成空操作、`extra_packages` 全清会连 ask 包一起清掉——不先修这个，e2e 会「跑通了但根本没有问询环」（假绿）。

明确不做：不引入新的外部依赖（ask 包是仓内已在用的同一个包）；不改 MVP 路径的既有行为；不做死亡诊断/续跑稳健化；不写实施顺序与回归命令。

### 1.2 根因定位（代码级）

**问询环在供给层就不存在，机械层也没有兜底**：

- `src/config/manifest.rs:899`：`change-plan-agent` 的 `extra_packages: vec![]`——agent 手里**没有**能发 `extension_ui_request` 的工具。MVP 的 `plan-agent` 装了 ask 包（`src/config/manifest.rs:22` 的 `PLAN_AGENT_EXTRA`，用在 `src/config/manifest.rs:785`）。
- `src/config/manifest.rs:1097`：`assert!(cpa.extra_packages.is_empty());` 把「空包」**钉成了契约**——缺陷被锁进测试，谁想修都得先改这条断言。
- **通道侧是好的**：agent 侧发问 → `src/sop/rpc_bridge.rs:24` 的 `RpcPlanSession::converse` 遇 `extension_ui_request` 回调 `ui.ask`（`src/sop/planning.rs:38`）→ `src/server/mod.rs:181` 的 `RelayUserLink` → `src/server/mod.rs:126` 的 `ask_blocking` → `src/server/mod.rs:135` 广播 `ui.request` → SSE。change 引擎也已经把它交给编排器（`src/server/change_engine.rs:221`）。**缺的只是「Agent 侧有工具」+「计划里必须问」+「问不到不许放行」三件事。**
- **产出侧没写死**：`src/config/manifest.rs:473` 的 `CHANGE_PLAN_AGENT_PROMPT` 第 1 块只说「列出无法从需求/代码确定、必须问用户的点（**问了就把答案写进来**）」——既没说「必须问」，也没给写法与载体（怎么标、答复写哪）。
- **审查侧看不见**：`src/change/plan_audit.rs:61` 的机械审计里**没有**任何「必须裁决项是否被问过/答过」的检查；`src/change/plan_audit.rs:209` 的审查指令只把既有四条机械审计结果注入给审查官——审查官也就没拿到「这条被自决了」的事实。
- **结构性绕过（上一场审查补出的洞，必须堵）**：若判定只靠标题/措辞的模糊匹配（`src/change/plan_audit.rs:142` 的 `section_lines` 按标题串找块），那么「结构没被识别」与「确实没有裁决项」会得到**同样的 0 条问题** → 门空转 = 静默自决原样复发。所以「必须裁决」必须是**显式标记 + 留痕字段**的结构契约，且提示词与门**共用同一批字面量**（Q2）。

### 1.3 歧义与本计划采用的默认

- [自决] **Q1 · 问询环的修法做几层**：只装包、只改提示词、还是装包 + 提示词 + 机械门 + 审查？——（默认：**四层都做**：装包（`src/config/manifest.rs:899`）+ 产出侧提示词契约（`src/config/manifest.rs:473`）+ 引擎侧确定性硬门（`src/change/plan_audit.rs` + `src/change/session.rs:216`）+ 审查侧语义审点（`src/config/manifest.rs:523`）。理由：只装包 = 把纪律寄托在模型自觉上——上一场已经证明模型会静默自决；只改提示词同理。硬门只判「结构与留痕」这种能机械判的事，分类恰当性交审查官——与 `src/change/plan_audit.rs:3` 的既有分工一致，不新增编排状态机。）
- [自决] **Q2 · 判定契约靠语义、靠措辞扫描、还是靠结构契约**：——（默认：**结构契约**。标记常量集中定义在 `src/change/plan_audit.rs`（`[必须裁决]` / `[自决]` / `用户裁决：`），`src/config/manifest.rs` 的提示词锚点单测**断言提示词含这些常量**（双向钉住，防提示词与门漂移；体例同 `src/config/manifest.rs:1019` 对 `PLAN_BLOCKS` 的引用）。歧义块取**最后一个**含「歧义」的标题（`src/change/plan_audit.rs:142` 的既有语义：后出现的标题覆盖先前的）；「顶层列表行」= 行首无缩进且以 `- ` / `* ` 开头（**带空格**）——分隔线 `---` 与散文行都不算列表行。理由：措辞扫描会让「结构未识别」与「无裁决项」同为 0 条问题 → 门空转。）
- [自决] **Q3 · 硬门判到哪一层**：只判「标了 `[必须裁决]` 却没留痕」，还是连「歧义块缺失 / 条目未分类 / 顶层散文里偷偷声明必须裁决」一起判硬失败？——（默认：**只判前者为硬失败**（返回 `PlanTurn::Fail` → 走既有打回-修复回路）；后者作为**确定性事实**注入审查指令，由审查官按新审点判 FAIL。理由：硬门误伤（模型第一次没按结构写就打回）会白烧掉 3 轮修复上限里的一轮；而「条目没分类」这件事本质是语义判断，机械门判不准，硬判只会制造假阳性。）
- [自决] **Q4 · 用户裁决的载体写法**：——（默认：同一条目内 `用户裁决：<答复原文>`，答复**逐字**抄（比较时忽略空白差异）。不引入额外编号体系、不引入问询记录文件。理由：载体要能被审查官一眼看见；编号体系会多出一处要漂移的事实源。）
- [自决] **Q5 · 要不要引擎侧的「问询记录」（黑板留痕 + 批次隔离）**：——（默认：**本靶不引入**。上一场三靶计划（`docs/plantree/changes/change-20261021-2026-10-20-change-sop-devforge-settled-e/change-plan.md:71` 的 Q4）设计了黑板问询记录 + 跨 run 批次隔离，用来证明「答复真的问过、不是编的」；本靶按需求口径只做「提示词 + 计划审查」双保险，那一层属于机械层的扩展，**留给后续场次**。本靶的「真问过」证据落在**真链 e2e**：SSE 上的 `ui.request` 事件 + autopilot 记录的答复原文与计划里 `用户裁决：` 字段的逐字交叉核对。已知代价：单条计划里「编造一条 `用户裁决：`」机械判不出来（归审查官的分类审点 + e2e 的真链见证兜底）。）
- [自决] **Q6 · 机械门不过时的动作**：直接判 Quality 死掉，还是打回去让 agent 问？——（默认：**打回**（`PlanTurn::Fail`），复用既有修复回路（`src/change/orchestrator.rs:24` 的 `MAX_PLAN_FIXES = 3`），打回消息自带「先用 ask-user-question 问用户」的指令。直接判死会让「第一次没问」＝整场失败，太脆。）
- [自决] **Q7 · 其余五个 `change-*` agent 逐个过完的结论要不要顺带修供给**：——（默认：**只登记不改**。逐个结论见 §3.2；两个非问询环的供给差异（`change-coding-review` 少 `pi-lens`、`change-test-agent` 多 `pi-lens`）**不在本靶范围**（本靶只治问询环），登记在 009 台账 D024 的「遗留」小节，留后续场次决策。理由：改供给会牵动夹具与漂移守卫，属于另一靶的边界。）
- [自决] **Q8 · e2e 真链的「必问场景」怎么造**：靠改需求文本植入歧义，还是靠别的方式？——（默认：**改需求文本**：在 `tests/e2e/test_26_change_e2e.py:97` 的 `REQUIREMENT` 里加一条**需求与代码都定不下来、且影响验收口径**的口径（复合词算一个词还是两个词），并写明「此条由需求方定」。理由：这是唯一能端到端触发「必须问」的入口（agent 侧没有别的触发器），且是「可控的测试夹具」——需求里已存在的模糊点被模型静默自决，才是这一场要证伪的行为。）
- [自决] **Q9 · 答复原文的逐字核对要不要做硬断言**：——（默认：**做**，但比较时忽略空白差异；失败信息必须能区分三种失败模式（没问 / 问了没抄 / 抄的不是答复原文）。理由：这是「问询环真的闭合」的唯一端到端证据；只断言「有 `ui.request`」会漏掉「问了但没落进计划」。风险与缓解见 §6 的 R8。）

---

## 2 影响面

### 2.1 直接命中

- `src/config/manifest.rs:899` —— `change-plan-agent` 的 `AgentManifest`：`extra_packages: vec![]` → 装 `PLAN_AGENT_EXTRA`（`src/config/manifest.rs:22`，与 `plan-agent` 同包）。**这是「问询环在供给层不存在」的根因行。**
- `src/config/manifest.rs:1097` —— 既有单测 `assert!(cpa.extra_packages.is_empty());` 把「空包」钉成契约 → 必须同步改为断言含 ask 包（不改则改动即红）。
- `src/config/manifest.rs:473` —— `CHANGE_PLAN_AGENT_PROMPT`：产出侧契约（歧义块单独成小节 + 每条顶层列表行带 `[必须裁决]`/`[自决]` 标记 + `[必须裁决]` 必须用 ask-user-question 问用户 + 答复逐字写进 `用户裁决：` + **禁止**留默认值自决放行 + 无歧义也要显式写「无歧义」）。
- `src/config/manifest.rs:523` —— `CHANGE_PLAN_REVIEW_PROMPT`：新增审点（`[必须裁决]` 项必须带非空 `用户裁决：`；分类恰当性；歧义块缺失/条目未分类 = FAIL）。
- `src/config/manifest.rs:1092`-`:1126` —— change 六件套的逐 Agent 供给断言：需求 2 的落点（六个 agent 逐个钉住「有没有 ask 包」，改一处不能误伤别处）。
- `src/config/manifest.rs:1016`-`:1038` —— 既有提示词锚点单测 `change_plan_prompts_carry_six_blocks_and_forbidden_items`（已引用 `crate::change::plan_audit::PLAN_BLOCKS`，`:1019`）→ 新锚点按同体例挂在这里。
- `src/config/manifest.rs:1232`-`:1247` —— 夹具生成器 `dump_change_agents_fixture`：提示词/包一改就必须**重跑生成器**（`cargo test --lib dump_change_agents_fixture -- --ignored`），不许手改夹具。
- `src/config/manifest.rs:1251` / `src/config/manifest.rs:1600` —— 夹具与 builtin 的两处漂移守卫（`:1600` 逐字段比对 `extra_packages`）。
- `src/change/plan_audit.rs:13` —— `PLAN_BLOCKS`（六块清单）：「歧义」是「需求复述」这一块的**子结构** → 契约常量（`[必须裁决]`/`[自决]`/`用户裁决：`）定义在本文件，供提示词与门共用（Q2）。
- `src/change/plan_audit.rs:61` —— `audit()`：机械审计入口，**签名与既有四条检查不变**；新的裁决判定是**独立纯函数**（不进 `audit()`），避免动到既有审计行为。
- `src/change/plan_audit.rs:142` —— `section_lines()`：歧义块解析复用它（注意它**跳过标题行**、后出现的同名标题覆盖先前的 → 「歧义」块取**最后一个**含「歧义」的标题）。
- `src/change/plan_audit.rs:209` —— `review_instruction()`：确定性事实（歧义块结构、未分类条目、散文声明）与新审点的注入点。
- `src/change/plan_audit.rs:236` —— `audit_plan_file()`：签名不变；裁决判定不走它（它只拿得到 repo 与计划路径）。
- `src/change/session.rs:64` —— `plan_kickoff()`：计划轮开工语 → 补一句问询契约（有 `[必须裁决]` 就先问用户、答复逐字写进 `用户裁决：`、不许带默认值自决放行）。
- `src/change/session.rs:216` —— `ProdChangeSession::run_plan_with()`：计划轮的**唯一收口点**（plan / amend / fix 三条路都走它）→ 硬门接在这里；门不过 → `PlanTurn::Fail`（不进审查轮：确定性判定不该交给审查官重判）。
- `src/change/session.rs:269` —— `fix_plan()`：打回消息的组装处 → 必须带「先用 ask-user-question 问用户」的指令。
- `src/change/session.rs:498` —— 既有单测夹具 `plan_text()`：**必须同步到新契约**（补歧义块 + 标记），否则 `src/change/session.rs:599` 与 `src/change/session.rs:625` 两条既有单测会被新门打红。
- `src/change/session.rs:599` / `src/change/session.rs:625` / `src/change/session.rs:643` —— 既有三条 plan 轮单测（Pass / Fail 回注 / 无标记）：**回归面**，不得变红。
- `tests/e2e/fixtures/change_agents.json:3` —— `change-plan-agent` 条目：`extra_packages` 为 `[]` → 必须随 builtin 重生成。
- `tests/e2e/test_26_change_e2e.py:138`-`:143` —— e2e 的 manifest 装配：`have`/`extend` 在宿主清单已含 `change-*` 时**变成空操作**，且 `extra_packages` 全清会把 ask 包一起清掉 → 改成**按名 upsert**（夹具条目覆盖宿主同名条目；change 六件套的 `extra_packages` 不清空）。
- `tests/e2e/test_26_change_e2e.py:97` —— `REQUIREMENT` 夹具文本：植入一处「需求与代码都定不下来、影响验收口径」的歧义（Q8）。
- `tests/e2e/test_26_change_e2e.py:194` —— e2e 的 autopilot 启动（`default_answer="继续"`）：select 形状的请求实际答复是**选项标签**（`tests/e2e/common.py:389`-`:396`），不是 `default_answer`。
- `tests/e2e/common.py:345` —— `UiAutoPilot.answered` 列表定义。
- `tests/e2e/common.py:365` —— `answered` 的 append：只记 `id`/`status`/`question`，**不记实际 POST 出去的 value** → 答复原文在 e2e 侧无处可查 → 本次补 `value`（只加字段，不改已有键语义）。
- `tests/e2e/common.py:372`-`:396` —— `_smart_answer()`：select 形状取选项标签（答复原文的真实来源）。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:20` —— 1.0 需求澄清：「用 `ask-user-question` 问人（与 MVP 同一条 UI 链路）」——**文档早就写了这套设计，代码却没给工具**（文档与代码不一致，见 §6 的 R10）；本次把载体口径同步成「`用户裁决：` 字段 + 禁默认放行」。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:60` —— 1.6 供给：`change-plan-agent` 的供给清单（本次补 ask 包）。
- `docs/plantree/plans/009-change-sop/topics/agents-change.md:65`-`:72` —— 1.7 审点表：新增「留痕核对 + 分类恰当性」一行。
- `docs/plantree/plans/009-change-sop/decisions/README.md:64` / `:66` —— D009「change-plan 只给 codegraph」：本次改的是 **npm 包供给、MCP 供给不变** → D024 要写清这层关系，D009 标题加旁注（两边都标）。
- `docs/plantree/plans/009-change-sop/decisions/README.md:159` —— 台账现登记至 D023 → 本次登记 **D024**。

### 2.2 调用链（谁调用它 / 它调用谁）

- `src/change/session.rs:248` —— `impl ChangeSession for ProdChangeSession`：`run_plan`（`:249`）/ `amend_plan`（`:254`）/ `fix_plan`（`:269`）三条计划路径全部汇到 `run_plan_with`（`:216`）→ **硬门只需接一处**（含 coding→plan 回退与审查打回两条路）。
- `src/change/orchestrator.rs:133` —— plan 阶段的打回-修复循环（`MAX_PLAN_FIXES = 3`，`:24`）：`PlanTurn::Fail` → 回注问题 → `fix_plan` 再来一轮 → 超限才 Quality（`:159`）→ 硬门复用这条既有回路，**不新增上限、不新增状态机**。
- `src/change/orchestrator.rs:199` —— `amend_plan` 的调用点（coding 阶段发现计划缺口 → 回 plan 补边界）：补边界的指令已写明「**只补边界，不要重写整份计划**」（`src/change/boundary.rs:151`）→ 结构不会被 amend 打乱，新门不会误伤这条路径。
- `src/change/session.rs:217` → `src/sop/rpc_bridge.rs:24` → `src/sop/planning.rs:38` → `src/server/mod.rs:181` → `src/server/mod.rs:126` → `src/server/mod.rs:135` —— 问询链路：agent 发 `extension_ui_request` → `ui.ask` → `RelayUserLink` → `UiRelay::ask_blocking` → SSE `ui.request`（既有单测 `src/sop/rpc_bridge.rs:208`）。**本次不改这条链路**，只补「Agent 侧有工具」。
- `src/server/change_engine.rs:136` —— `change-plan-agent` 的 profile 装配点：profile 里的包清单来自 manifest → 渲染器 `src/config/render.rs:182` 把 `extra_packages` 合并进 `settings.json` 的 `packages`（`src/config/render.rs:185` 判幂等跳过）→ 「装没装 ask 包」的真实判据在 profile，不在 manifest。
- `src/server/change_engine.rs:221` —— `RelayUserLink` 交给编排器处 → 问询链路的终点。
- `src/change/plan_audit.rs:187` —— `plan_facts()`：验收标准/白名单的读取口（`src/server/change_engine.rs:124` 用它取白名单）→ 本变更**不影响**它的抽取口径。
- `src/config/render.rs:182` / `:185` / `:186` —— 渲染器通用分发：新包走既有分支（双记录 = `settings.json` 的 `packages` + `npm/package.json` 的 dependencies），不改渲染逻辑。

### 2.3 隐式耦合（grep 几乎拿不到的那类）

- **运行时清单缓存**：`src/main.rs:152` —— `~/.devforge/manifest.json` 存在即用落盘清单（`src/main.rs:153`），不再用 `builtin()` → 仓内给 `change-plan-agent` 加了包，**不重存清单并重渲染 profile，用户态的问询环依然不存在**（部署侧动作，仓内单测验不到）。
- **渲染幂等面**：`src/config/render.rs:185`-`:186` —— 判定标准刻意保守（清单新增了包 → 一律重装全量）→ 改了 `extra_packages` 的 profile 会被整包重装（e2e 首次渲染要多装一个包，见 R7）。
- **pi 扩展包 vs skill 的契约是两回事**：`npm:@juicesharp/rpiv-ask-user-question` 是**扩展包**（提供 `extension_ui_request` 能力），与 skill 的 frontmatter 契约无关（内置 skill 注册在 `src/config/skill_assets.rs:14` 一侧）→ 本次只加包、不加 skill，不碰 skill 契约。
- **提示词与门必须共用同一批字面量**：标记常量在 `src/change/plan_audit.rs`，提示词（`src/config/manifest.rs:473`）里出现的是同一批字面量 → 靠锚点单测双向钉住（Q2）；否则改了一边、另一边还在 → 门空转（= 静默自决复发）。
- **裁决比对的源不在 repo 内**：真正的「用户答复」来自 SSE 通道（运行期：`src/server/mod.rs:135` 广播的事件里只有 id/question/options，**没有答复**），仓内单测拿不到 → 本靶的仓内判定只做**结构与留痕存在性**（「答复字段在不在、是不是空」，落在 `src/change/plan_audit.rs:61` 的新纯函数上），「答复是不是真问出来的」由真链 e2e 见证（Q5 的已知代价）。
- **e2e 的装配耦合**：`tests/e2e/test_26_change_e2e.py:138`-`:143` —— 宿主清单已含 `change-*` 时 `extend` 空操作 + `extra_packages` 全清 → 真链会「跑通但没有问询环」；宿主清单是否已含 `change-*` 取决于用户态有没有重存过清单（`src/main.rs:152`）→ **测试不能依赖宿主状态**，必须按名 upsert。
- **e2e 的答复原文只有两个来源**：①autopilot 记录（`tests/e2e/common.py:365`，本次补 `value`）；②SSE 事件里的 `ui.request`（只有 id/question/options，**没有答复**）→ 逐字核对必须靠 ①。
- **工作区当前不干净**（阻断项，见 R1）：`git status` 显示 `src/change/coding_gate.rs`、`src/change/session.rs`、`src/config/manifest.rs`、`src/config/skill_assets.rs` 四个文件有**未提交**改动（上一场的验收测试补录，+880 行，全是 `#[cfg(test)]` 模块）→ 相对本变更基线 `3020873` 的 `git diff --name-only` 会把它们算成「本次改动」；其中两个**不在本白名单** → 越界门（`src/change/boundary.rs:102`）会判范围膨胀；另两个虽在白名单，也会让「每块 diff 都要有计划依据」的审查判据（`src/config/manifest.rs:632`）误红。
- **untracked 文件不进 diff**：`src/change/coding_gate.rs:188` 的 `git diff --name-only <baseline>` 不含未跟踪文件 → 本计划文件、`tests/e2e/results/*.json` 这类未跟踪产物不触发越界门，但会污染提交（白名单外改动）。
- **计划文件跨 run 留存**：计划在目标仓（`src/change/prepare.rs:57`）、断点在黑板 → 续跑回到 plan 阶段时硬门会重跑（既有 fix 回路承接），不新增状态。
- **审查子进程的 cwd 是黑板，计划在目标仓**：`src/change/plan_audit.rs:209` 的审查指令必须附计划全文（既有行为）→ 新增的确定性事实随指令一起过去，审查官不需要自己去目标仓读。

---

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `src/config/manifest.rs` | ①`:899` 给 `change-plan-agent` 装 `PLAN_AGENT_EXTRA`（问询环才有工具）；②`:473` 产出侧提示词加歧义条目契约（分类标记 + 必须问 + 逐字留痕 + 禁默认放行 + 无歧义也要显式写）；③`:523` 审查提示词加「留痕核对 + 分类恰当性 + 结构缺失」审点；④`:1097` 的「空包」断言改成「含 ask 包」；⑤`:1092`-`:1126` 逐 Agent 供给断言补齐（需求 2 的落点：六个 change agent 逐个钉住）；⑥`:1016` 提示词锚点单测补新锚点（与门常量双向钉住） | 需求 1、2、3 |
| `src/change/plan_audit.rs` | 新增歧义块的**确定性判定**（独立纯函数 + 契约常量）：`[必须裁决]` 条目缺非空 `用户裁决：` → 硬门问题；歧义块结构事实（未分类条目、顶层散文声明、计数）→ 注入审查指令；常量与提示词共用；`audit()`（`:61`）与 `audit_plan_file()`（`:236`）签名不变；附台账 D024 登记守卫与文档口径守卫单测（体例同 `src/change/coding_gate.rs:740`） | 需求 3 |
| `src/change/session.rs` | ①`:64` kickoff 加问询契约一句；②`:216` `run_plan_with` 接入硬门（不通过 → `PlanTurn::Fail`，走既有打回回路）；③`:269` `fix_plan` 打回消息带「先用 ask-user-question 问用户」；④`:498` 既有单测夹具 `plan_text()` 同步到新契约（否则 `:599`/`:625` 两条既有单测变红） | 需求 3 |
| `tests/e2e/fixtures/change_agents.json` | 提示词与包随 builtin 同步 → **必须用生成器重生成**（`src/config/manifest.rs:1232`），不许手改（漂移守卫 `src/config/manifest.rs:1251`/`:1600` 会红） | 需求 1、3 |
| `tests/e2e/test_26_change_e2e.py` | ①`:138`-`:143` 装配改成**按名 upsert**（宿主清单已含 `change-*` 时夹具仍生效；change 六件套的 `extra_packages` 不清空）；②`:97` 的 `REQUIREMENT` 植入一处「必须用户裁决」的歧义；③新增断言：渲染出的 change-plan-agent profile 的包清单（渲染记录的 `packages` 数组）含 ask 包、SSE 出现 `ui.request` 且应答数对齐、计划里有 `[必须裁决]` 条目带非空 `用户裁决：`、答复原文逐字落在 `用户裁决：` 行 | 需求 4 |
| `tests/e2e/common.py` | `:365` 的 `answered` 记录补 `value`（实际 POST 出去的答复原文）→ e2e 才能做「答复 ↔ 计划」逐字核对；只加字段、不改已有键语义（`test_08`/`test_14`/`test_19`/`test_20` 都吃这条记录） | 需求 4 |
| `docs/plantree/plans/009-change-sop/decisions/README.md` | 登记 **D024**（问询环：供给 + 结构契约 + 硬门 + 审查审点 + 遗留发现），并给 D009（`:64`/`:66`）加「供给部分由 D024 增补」的旁注（两边都标，只标一边等于旧决策看着仍有效） | 需求 1、2、3 |
| `docs/plantree/plans/009-change-sop/topics/agents-change.md` | `:20` 答复载体（改成「`用户裁决：` 字段 + 禁默认放行」）、`:60` 供给（+ ask 包）、`:65`-`:72` 审点表（加留痕核对与分类恰当性）三处同步 | 需求 1、3 |
| `docs/plantree/plans/009-change-sop/roadmap.md` | Phase A **追加**一条新条目（T008c · 问询环修复：供给 + 结构契约 + 硬门 + 真链实证）；**不改** T004 等既有条目的历史描述与证据行 | 需求 1 |

### 3.2 受影响不改

| 文件 / 位置 | 为什么可以不改 |
| --- | --- |
| `src/change/session.rs:1092` 起的五个 change agent 供给现状 | **需求 2 的逐个结论**（对照 MVP 侧同类 agent）：①`change-plan-agent`（`:899`）——**缺 ask 包，唯一的问询环缺口，本靶修**；②`change-plan-review`（`:909`）——无包，**正确**：审查提示词明确「不与用户交互」，MVP `plan-review`（`:794`）同样无包；③`change-coding-agent`（`:920`）——已是 `CODING_AGENT_EXTRA`（含 ask），**与 MVP `coding-agent`（`:805`）同源**，其偏差协议走 `<route>plan</route>` 引擎回路（不直接问用户），保留不动；④`change-coding-review`（`:936`）——无 ask 包，**正确**（审查不与用户交互，MVP `coding-review`（`:821`）也没有 ask，只有 `pi-lens`）；⑤`change-test-agent`（`:947`）——含 ask，**无缺口**；⑥`change-test-review`（`:956`）——无包，**正确**（MVP `test-review`（`:847`）同样无包）。**结论：问询环缺口只有 `change-plan-agent` 一个**，其余五个按职责都不需要问用户工具 |
| `src/config/render.rs:182`-`:189` | 渲染器按 `base_packages + extra_packages` 通用分发，新包走既有分支；「装没装」的断言放在 manifest 单测与 e2e，不改渲染逻辑 |
| `src/sop/planning.rs:38`、`src/sop/rpc_bridge.rs:24` | `UserLink` 抽象与 `extension_ui_request → ui.ask` 通道已通（既有单测 `src/sop/rpc_bridge.rs:208`），本次只补「Agent 侧有工具 + 计划里必须问 + 门兜底」 |
| `src/server/mod.rs:126`/`:135`/`:181` | `UiRelay` / SSE `ui.request` / `RelayUserLink` 都不动：问询照旧上行 SSE |
| `src/change/orchestrator.rs:24`/`:133`/`:159` | 打回-修复循环与上限复用既有实现，不新增上限、不新增状态机 |
| `src/change/boundary.rs:102`/`:149` | 越界二分与「只补边界」的补边界指令都不动 |
| `src/change/plan_audit.rs:61`/`:236` | `audit()` 的既有四条检查与 `audit_plan_file()` 签名不变（新判定是独立纯函数，不塞进 `audit()`） |
| `src/change/mod.rs:36` | 新判定用 `crate::change::plan_audit::` 全路径调用即可（既有 `src/change/session.rs:224` 就是这么写的）→ 不改 re-export 面 |
| `src/change/coding_gate.rs`、`src/change/testing_gate.rs`、`src/change/state.rs`、`src/change/history.rs`、`src/change/report.rs`、`src/change/docs.rs`、`src/change/gate0.rs`、`src/change/prepare.rs`、`src/change/change_id.rs` | 与问询环无关；**本靶不扩到死亡诊断/续跑稳健化**（那是后续场次） |
| `src/config/skill_assets.rs`、`assets/skills/**` | 本次不新增 skill；工程纪律 skill 的供给口径一字不动 |
| `src/runstate/**`、`src/engine/**` | 断点机制与子进程机制不变 |
| MVP 侧八个 Agent（`plan-agent`/`plan-review`/`coding-agent`/`coding-review`/`test-agent`/`test-review`/`doc-fix`/`doc-agent`/`doc-review`） | 供给逐字段不变（回归面，`src/config/manifest.rs:1049`-`:1090` 的既有断言守着） |
| `docs/plantree/plans/009-change-sop/roadmap.md` 的既有条目（T004 等） | 历史交付记录（含证据 commit），改写等于篡改历史 → 本次以**追加**条目的方式登记 |
| `docs/plantree/plans/010-run-resume/**` | 010 决策链与本靶无关 |
| `tests/e2e/test_08_e2e_planning.py` 等 MVP e2e | MVP 问询环本来是通的（`plan-agent` 有 ask 包），本次不改也不回归其行为；`tests/e2e/common.py` 的改动只加字段 |

### 3.3 必须回归验证

| 模块 | 为什么风险在这里 |
| --- | --- |
| `src/change/session.rs:599`/`:625`/`:643` 三条既有 plan 轮单测 | 硬门是新的 Fail 点；夹具 `plan_text()`（`:498`）不同步到新契约，这三条会直接变红（它们是「既有行为不回归」的锚） |
| `src/change/plan_audit.rs:61` 的既有四条审计 | 新判定共用块解析器（`:142`）→ path 存在性 / 白名单存在 / 「要改的」⊆白名单 / 验收可观测 四条行为一个字都不能变 |
| `src/config/manifest.rs:1049`-`:1139` 的十五 Agent 供给断言 | 改 `:899` 与 `:1097` 时最容易误伤别的 Agent（尤其 `:1108`/`:1114`/`:1121` 那几条「与 MVP 同档」断言） |
| `src/config/manifest.rs:1016` 的提示词锚点单测 | 提示词一改，锚点单测是唯一的「提示词与门没漂移」防线 |
| 夹具漂移守卫 `src/config/manifest.rs:1251`/`:1600` | 提示词/包一字之差即红；必须重生成而不是手改 |
| `src/config/render.rs:362`-`:380` 的双记录断言 | 渲染器不改，但「四件套 + 1」的形状若被误改会红 |
| e2e 里所有用 autopilot 的脚本（`tests/e2e/test_08_e2e_planning.py`、`tests/e2e/test_14_coding_full_chain.py`、`tests/e2e/test_19_testing_full_chain.py`、`tests/e2e/test_20_testing_fixture.py`） | `tests/e2e/common.py` 的 `answered` 记录形状被改 → 只能**加字段**、不能改已有键的语义 |
| e2e change 真链 `tests/e2e/test_26_change_e2e.py` | 问询环的端到端实证：真 plan agent 是否真发 `ui.request`、答复是否落进计划、问答与计划是否双向一致、装配是否还受宿主清单影响 |
| e2e MVP 规划链 `tests/e2e/test_08_e2e_planning.py:190` | MVP `plan-agent` 早有 ask 包（既有机制见证）→ 本次不得回归 |
| 仓内单测（`cargo test`） | 新增/改动的单测全在白名单文件内（无新增文件） |
| 部署侧（不在仓内） | `~/.devforge/manifest.json` 缓存与 profile 重渲染（`src/main.rs:152`）→ 仓内改完不等于用户态生效 |

---

## 4 白名单

- `src/config/manifest.rs`
- `src/change/plan_audit.rs`
- `src/change/session.rs`
- `tests/e2e/fixtures/change_agents.json`
- `tests/e2e/test_26_change_e2e.py`
- `tests/e2e/common.py`
- `docs/plantree/plans/009-change-sop/decisions/README.md`
- `docs/plantree/plans/009-change-sop/topics/agents-change.md`
- `docs/plantree/plans/009-change-sop/roadmap.md`

新增测试文件：**无**。Rust 单测内联在被改文件的 `#[cfg(test)]` 模块；e2e 断言追加进已有的 `tests/e2e/test_26_change_e2e.py`，不新增 e2e 脚本。

白名单之外不得有任何改动。特别说明：上一场遗留的两个未提交文件**不在**白名单 → 开工前必须由操作员先把那批未提交改动单独提交（见 §6 的 R1），否则越界门会判范围膨胀。本计划文件与基线记录是**计划阶段**的产物（写在目标仓 `docs/plantree/changes/` 下），由计划阶段写入，不在 coding 白名单内。

---

## 5 验收标准

- 给定 `ConfigManifest::builtin()` 的 `change-plan-agent` → `extra_packages` 含 `npm:@juicesharp/rpiv-ask-user-question`（改动前是空数组）。
- 给定 builtin 里其余十四个 Agent → `extra_packages` 与本次改动前逐字段相同（不得顺手给别的 Agent 加包或删包）。
- 给定 change 六件套各自的职责 → 逐个有断言钉住：`change-plan-agent` 有 ask 包；`change-plan-review`、`change-coding-review`、`change-test-review` 无包；`change-coding-agent`、`change-test-agent` 保留与 MVP 同源的 ask 包。
- 给定 `CHANGE_PLAN_AGENT_PROMPT` → 同时含「必须裁决」「自决」「ask-user-question」「用户裁决：」四个锚点，且其中的标记字面量与 `src/change/plan_audit.rs` 的契约常量**逐字相同**（防漂移）。
- 给定 `CHANGE_PLAN_REVIEW_PROMPT` → 含「分类恰当性」审点（把必须用户裁决的条目标成自决 = FAIL）与「留痕核对」审点（`[必须裁决]` 条目缺 `用户裁决：` = FAIL）。
- 给定一个带 `[必须裁决]` 条目但该条目没有 `用户裁决：` 字段的计划 → 裁决门返回 ≥1 条问题，问题文本同时含「必须裁决」与「ask-user-question」。
- 给定一个带 `[必须裁决]` 条目且该条目有非空 `用户裁决：` 字段的计划 → 裁决门返回 0 条问题。
- 给定一个带 `[必须裁决]` 条目且该条目的 `用户裁决：` 为空的计划 → 裁决门返回 ≥1 条问题。
- 给定一个歧义块里全是 `[自决]` 条目（没有必须裁决项）的计划 → 裁决门返回 0 条问题（自决项不触发问询要求，其恰当性归审查官）。
- 给定一个歧义块里有**未带分类标记**的顶层列表行的计划 → 裁决门不判失败（0 条），但注入审查指令的确定性事实里列出该未分类行。
- 给定一个歧义块里有顶层散文行写着「必须用户裁决」却没有对应条目的计划 → 裁决门不判失败，但注入审查指令的确定性事实里列出该散文行。
- 给定 change plan 轮：计划里有 `[必须裁决]` 条目且无 `用户裁决：` → `run_plan` 返回 `PlanTurn::Fail`（不是 `Pass`），问题清单含「必须裁决」。
- 给定 change plan 轮：计划合规（歧义块齐、必须裁决项有留痕）→ `run_plan` 返回 `PlanTurn::Pass`，且审查轮次为 1。
- 给定 change plan 轮：审查 FAIL → `run_plan` 返回 `PlanTurn::Fail` 且问题来自审查（裁决门不误伤合规计划）。
- 给定 plan 轮的打回指令（`fix_plan` 的消息）→ 含「ask-user-question」锚点（agent 知道下一步该做什么）。
- 给定 plan 轮的开工指令（`plan_kickoff` 的消息）→ 含「ask-user-question」与「用户裁决」两个锚点。
- 给定 change 六件套 e2e 夹具 `tests/e2e/fixtures/change_agents.json` → `change-plan-agent` 条目的 `extra_packages` 含 ask 包，且整个夹具与 builtin 逐字段一致（漂移守卫转绿）。
- 给定 e2e 拼装的 manifest（宿主清单里**已含** `change-*` 的情形）→ change 六件套按名覆盖宿主同名条目（夹具的包与提示词生效），且 change 六件套的 `extra_packages` 不被清空。
- 给定 e2e 渲染出的 `change-plan-agent` profile 的包清单（渲染记录的 `packages` 数组）→ 含 `npm:@juicesharp/rpiv-ask-user-question`。
- 给定 e2e 真链跑完 → SSE 时间线出现 ≥1 个 `ui.request`，且 autopilot 的应答数与请求数对齐。
- 给定 e2e 真链跑完 → 计划里有 `[必须裁决]` 条目，且该条目带非空 `用户裁决：` 字段（问询环闭合的产物）。
- 给定 e2e 真链跑完 → autopilot 记录的答复原文（去空白后）逐字出现在计划的 `用户裁决：` 字段里。
- 给定 autopilot 应答了一个 select 形状的请求 → `autopilot.answered` 里记下实际 POST 出去的 `value`，且该 `value` 等于选项标签（不是 `default_answer`）。
- 给定 `docs/plantree/plans/009-change-sop/decisions/README.md` → 含 D024 一节（问询环：供给 + 结构契约 + 硬门 + 审查审点 + 遗留发现），台账里 D024 只出现一次，且 D009 标题带「供给部分由 D024 增补」的旁注。
- 给定 `docs/plantree/plans/009-change-sop/topics/agents-change.md` → 需求澄清一节的答复载体口径、供给一节的 ask 包、审点表的留痕核对与分类恰当性三处，都与本次改动后的实际一致。
- 给定本变更的提交内容 → diff 里不含上一场遗留的未提交改动（那 880 行验收测试补录已由操作员在 coding 轮开始前单独提交），白名单外无任何改动。

## 6 风险

- **R1 · 工作区不干净（阻断项，开工前必须处置）**：`git status` 显示 `src/change/coding_gate.rs`、`src/change/session.rs`、`src/config/manifest.rs`、`src/config/skill_assets.rs` 四个文件有**未提交**改动（上一场 `change-20261020-backnotprop-pstack-github-ai-skills-home` 的验收测试补录，+880 行，全是 `#[cfg(test)]` 模块）。相对本变更基线 `3020873`，`git diff --name-only` 会把它们算成「本次改动」：其中 `src/change/coding_gate.rs` 与 `src/config/skill_assets.rs` **不在白名单** → 越界门（`src/change/boundary.rs:102`）会判「范围膨胀」；另两个虽在白名单，也会让审查判据「每块 diff 都要有计划依据」（`src/config/manifest.rs:632`）误红。**处置：coding 轮开始前由操作员把这批改动单独提交（一次提交，与本变更分开）**；本白名单不含「顺手提交别人的改动」。
- **R2 · 硬门误伤（假阳性）**：硬门判「标了 `[必须裁决]` 却没留痕」——若模型第一次没按新契约写（没标或标了没问），第一轮会被打回。3 轮上限（`src/change/orchestrator.rs:24`）用完就 Quality（`:159`）→ 整场失败。缓解：契约写进提示词（`src/config/manifest.rs:473`）**且**写进 kickoff（`src/change/session.rs:64`）**且**打回消息自带「先用 ask-user-question 问用户」；违规文本必须点名是哪一条、缺什么字段。
- **R3 · 问询环会把 run 阻塞住**：问询经 `ui.ask` 阻塞等应答（`src/server/mod.rs:126` 的 `rx.recv()`）→ 用户不在线时，change 的 plan 阶段会**卡住**等答案。这是「不许静默自决」的代价，本靶**不**引入「问不到人自动放行」的口子（那正是要治的病）。缓解：e2e 侧有 autopilot 替用户应答；真链侧由调用方保证有人在线。
- **R4 · 装包 ≠ 真会问**：包装上只保证 pi 侧有 `extension_ui_request` 能力，真发不发得出来还看 agent 行为。缓解：提示词契约 + 硬门 + 审查审点 + e2e 真链见证四层叠加，「装了但模型不问」不会静默通过（最多打成 Quality，不会假绿）。
- **R5 · 运行时清单缓存导致「改了不生效」**：`src/main.rs:152` —— `~/.devforge/manifest.json` 存在即用落盘清单 → 必须重存清单（PUT `/api/v1/config/manifest`）并重渲染 profile，ask 包才会进 profile。**仓内无法验证这一段，属部署侧动作**。
- **R6 · e2e 夹具漂移导致真链假绿**：不重生成夹具 → e2e 里的 change plan agent 仍无 ask 包 → 真链「跑通了」但问询环不存在。漂移守卫（`src/config/manifest.rs:1251`/`:1600`）会红，但只在仓内单测里；e2e 用的是夹具文件本身 → **必须重生成 + 重跑 e2e 断言**。
- **R7 · e2e 首次渲染新增 npm 依赖**：e2e 现在要为 `change-plan-agent` 装 `npm:@juicesharp/rpiv-ask-user-question`（`src/config/render.rs:182` 合并 → 触发安装）→ e2e 首次运行需要 npm 可达（先例：`tests/e2e/test_08_e2e_planning.py` 本来就要装 MVP 的包）。若 CI 环境无 npm 网络，e2e 会在渲染阶段红（不是本变更的逻辑缺陷，但会表现为 e2e 失败）。
- **R8 · e2e 真链里答复原文不由 e2e 决定**：ask-user-question 发的是 **select 形状**请求（`tests/e2e/common.py:372` 的 `_smart_answer`），答复是**选项标签**（`:389`-`:396`），而选项标签是 **agent 自己拟的** → 真实答复既不是 `default_answer="继续"`，也不由 e2e 固定。真实失败模式：agent 抄的是自己拟的标签，门要求逐字一致 → 抄错就红。缓解：①提示词要求逐字抄答复；②`tests/e2e/common.py:365` 记录实际 POST 出去的 `value`（本变更改）；③断言做成双向一致（问过就必须逐字出现在计划里；没问过就不许有 `[必须裁决]` 条目），失败信息要能一眼分出「没问 / 问了没抄 / 抄的不是原文」三种模式。
- **R9 · 「全标自决」的绕过**：硬门只判「标了必须裁决却没留痕」，判不了「该不该问」——agent 若把必须用户裁决的条目标成 `[自决]`，门无话可说。缓解：归审查官（`src/config/manifest.rs:523` 的分类恰当性审点 + 注入的确定性事实）。这是本设计的**已知能力边界**（厚 Agent、薄编排的取舍），不是遗漏。
- **R10 · 文档与代码不一致（三处）**：①`docs/plantree/plans/009-change-sop/topics/agents-change.md:20` 早就在说「用 `ask-user-question` 问人（与 MVP 同一条 UI 链路）」，而代码（`src/config/manifest.rs:899`）**没给工具** → 本次以代码为准修齐，并把载体口径同步成「`用户裁决：` 字段 + 禁默认放行」；②`docs/plantree/plans/009-change-sop/topics/agents-change.md:199` 声称 change 测试双 Agent 与 MVP `test-agent`/`test-review`「同款」，而代码给 `change-test-agent` 的是 `CODING_AGENT_EXTRA`（含 `pi-lens`，`src/config/manifest.rs:947`）、MVP `test-agent` 是不含 `pi-lens` 的 `TEST_AGENT_EXTRA`（`src/config/manifest.rs:838`）→ 文档与代码不一致 + 与 MVP 黑盒纪律相悖，**本靶不改**，登记在 D024 的「遗留」；③`docs/plantree/plans/009-change-sop/decisions/README.md:64`/`:66` 的 D009 标题「change-plan 只给 codegraph」——本次改的是 **npm 包**供给、MCP 供给不变 → D024 要写清这层关系并给 D009 加旁注（两边都标）。
- **R11 · 遗留发现（本靶不改，但必须登记）**：①`change-coding-review`（`src/config/manifest.rs:936`）无包，而 MVP `coding-review`（`:821`）有 `pi-lens`（诊断层）→ 相对 MVP 同类少一个诊断包；②`change-test-agent`（`:947`）比 MVP `test-agent`（`:838`）多一个 `pi-lens`（黑盒纪律：MVP 明确不给）。两者都**不是问询环缺口**，本靶只登记（D024 的「遗留」小节 + `docs/plantree/plans/009-change-sop/topics/agents-change.md` 的供给节），留给后续场次决策。**不许在本靶顺手改**（会牵动夹具与漂移守卫，属另一靶的边界）。
- **R12 · 新契约会作废本计划自身（自举影响）**：新结构契约要求「歧义」块的每条顶层列表行带分类标记，而本计划 §1.3 原本是 `- **Q1 …**` 形态（无标记）→ 变更落地后，同一 change 续跑回到 plan 阶段时**这份计划会被自己的门打回**。处置：①块解析规则取**最后一个**含「歧义」的标题（`src/change/plan_audit.rs:142` 的既有语义）→ 本计划有两个含「歧义」的标题（`## 1 需求复述与歧义` 与 `### 1.3 歧义与本计划采用的默认`），**判定只作用于后者** → §1.1/§1.2 的无标记列表行不在块内；②**本计划已按新契约自修**（§1.3 的九条全部带 `[自决]` 标记，且无顶层散文行声明必须裁决）→ 本 change 自己续跑回 plan 阶段不会被打回；③一般情形：变更前写下的旧计划若在续跑时进入 plan 阶段，plan agent 会按新提示词重写（打回-修复回路 3 轮，缺标记的行被打回后补上即可）→ 暴露面仅限「被回退到 plan 阶段」的那一次。
- **R13 · 「什么算必须用户裁决」的边界模糊**：判据写在提示词里（「需求原文与代码都定不下来、且影响验收口径」）→ 主观成分无法完全机械化。缓解：判据写进提示词与审查审点（两处同口径）；e2e 夹具用「复合词算一个词还是两个词」这种**明显无法从代码推出、且直接影响测试期望值**的口径来触发。
- **R14 · 提示词与门漂移**：分类标记在提示词与门里各有一份字面量 → 锚点单测（提示词含门用的常量）是唯一防漂移手段，删了它契约就散（`src/config/manifest.rs:1016` 那条单测就是它的位置）。
- **R15 · e2e 断言依赖模型行为（可能假红）**：「SSE 出现 ≥1 个 `ui.request`」取决于模型真的去问 → 模型不配合时 e2e 会红。缓解：失败信息要能区分三种原因（包没装 → profile 断言先红 / 模型没问 → 计划里没有 `[必须裁决]` 条目 / 问了没抄 → 逐字核对失败），不要让三种原因混成一条红。
- **R16 · 续跑回到 plan 阶段要重新问一次**：计划文件在目标仓跨 run 留存、硬门每轮重跑 → 续跑回到 plan 阶段时，带 `[必须裁决]` 的计划要重新问用户（用户不在线就阻塞）。取舍：宁可多问一次，也不许沿用上一场的静默自决；打回回路有 3 轮，agent 下一轮补问即可。
- **R17 · 无新依赖的约束**：本次只加仓内已在用的同一个 npm 包（`src/config/manifest.rs:22`），不引入任何新的第三方 crate/包；结构契约与判定全部用标准库实现。
- **R18 · e2e 装配的宿主依赖**：`tests/e2e/test_26_change_e2e.py:138` 的 `have` 来自宿主清单（`~/.devforge/manifest.json`，内容随用户态变化）→ 修复后必须**按名 upsert**而不是「只在缺失时 extend」，否则宿主清单一旦含 `change-*`，e2e 就测的不是夹具里的那套供给（真链假绿）。

---

## 7 验收对账映射（criterion → test path:line）

> 行号是**改动前**的锚点（`path:line` 供定位）；实际对账行号以改完后的真实位置为准（下游测试阶段逐条回填到验收对照表）。

| # | 验收标准（§5 序号） | 证据位置 |
| --- | --- | --- |
| 1 | change-plan-agent 装上 ask 包 | `src/config/manifest.rs:899`（改后同条断言在 `src/config/manifest.rs:1097` 附近） |
| 2 | 其余十四个 Agent 供给不变 | `src/config/manifest.rs:1049`-`:1090`（既有逐 Agent 断言） |
| 3 | change 六件套逐个钉住 | `src/config/manifest.rs:1092`-`:1126`（改后的逐 Agent 断言） |
| 4 | 产出侧提示词四锚点 + 与门常量同字面量 | `src/config/manifest.rs:1016`-`:1038`（提示词锚点单测） + `src/change/plan_audit.rs`（常量锚点单测） |
| 5 | 审查提示词新增两条审点 | `src/config/manifest.rs:1032`（审查侧锚点断言处） |
| 6 | `[必须裁决]` 缺留痕 → ≥1 条问题 | `src/change/plan_audit.rs`（新增裁决判定单测） |
| 7 | `[必须裁决]` 有留痕 → 0 条问题 | `src/change/plan_audit.rs`（新增裁决判定单测） |
| 8 | `用户裁决：` 为空 → ≥1 条问题 | `src/change/plan_audit.rs`（新增裁决判定单测） |
| 9 | 全 `[自决]` → 0 条问题 | `src/change/plan_audit.rs`（新增裁决判定单测） |
| 10 | 未分类条目 → 不判失败但进事实 | `src/change/plan_audit.rs`（审查指令事实单测） |
| 11 | 散文声明必须裁决 → 不判失败但进事实 | `src/change/plan_audit.rs`（审查指令事实单测） |
| 12 | plan 轮不合约 → `PlanTurn::Fail` | `src/change/session.rs`（新增硬门单测，夹具见 `src/change/session.rs:498`） |
| 13 | plan 轮合规 → `PlanTurn::Pass` + 审查 1 轮 | `src/change/session.rs:599`（既有单测，夹具同步后） |
| 14 | 审查 FAIL 的问题原样回注 | `src/change/session.rs:625`（既有单测） |
| 15 | 打回消息带 ask-user-question | `src/change/session.rs:269`（新增/扩展单测） |
| 16 | kickoff 带 ask + 用户裁决 | `src/change/session.rs:618`（既有 kickoff 断言处） |
| 17 | 夹具与 builtin 逐字段一致 | `src/config/manifest.rs:1251` + `src/config/manifest.rs:1600`（既有漂移守卫） |
| 18 | e2e 装配按名 upsert | `tests/e2e/test_26_change_e2e.py:138`-`:143`（改后新增装配断言） |
| 19 | profile 含 ask 包 | `tests/e2e/test_26_change_e2e.py`（新增 profile 断言） |
| 20 | 真链出现 `ui.request` 且应答数对齐 | `tests/e2e/test_26_change_e2e.py`（新增 SSE 断言） |
| 21 | 计划里有 `[必须裁决]` + 非空 `用户裁决：` | `tests/e2e/test_26_change_e2e.py`（新增计划断言） |
| 22 | 答复原文逐字落在 `用户裁决：` | `tests/e2e/test_26_change_e2e.py` + `tests/e2e/common.py:365`（记录 value） |
| 23 | autopilot 记录实际答复值 | `tests/e2e/common.py:365`（新增 value 记录） + `tests/e2e/common.py:389`（选项标签来源） |
| 24 | D024 登记 + D009 旁注 | `docs/plantree/plans/009-change-sop/decisions/README.md:159`（登记点） + `src/change/plan_audit.rs`（新增台账守卫单测，体例同 `src/change/coding_gate.rs:740`） |
| 25 | agents-change 三处口径同步 | `docs/plantree/plans/009-change-sop/topics/agents-change.md:20`/`:60`/`:65` + `src/change/plan_audit.rs`（新增文档口径守卫单测） |
| 26 | 提交内容不含上一场遗留未提交改动 | 开工前 `git status --porcelain` 为空（操作员一次性动作）+ 本次提交 `git show --stat` 只含白名单文件 |
