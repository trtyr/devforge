# 变更计划 · change-20261020-backnotprop-pstack-github-ai-skills-home

目标仓：`/home/trtyr/code-work/devforge`
参考项目：`/home/trtyr/code-work/pstack`（backnotprop/pstack 镜像，MIT，© 2026 Lauren Tan，上游 `cursor/plugins/pstack`）
基线：`edbf48d748a64737bacb6c64155ab0142d639772`（见 `docs/plantree/changes/change-20261020-backnotprop-pstack-github-ai-skills-home/baseline.txt`）

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

用户给了 pstack 这个「AI 工程纪律 skills 合集」，要求：**参考它来优化 devforge 目前的 coding 流程 —— 准确说，是优化我们的 coding agent，让它能更好地写代码**。

我把需求读成一件可交付的事：**把 pstack 的工程纪律落进 devforge 的 coding agent（提示词 + 规则供给 + 可机械判的门），让「写代码」这件事从「模型自觉」变成「有规则、有证据、有门」**。具体三件：

1. **规则供给**：把 pstack 的 24 条 principle 蒸馏成 devforge 语境下的**一份**工程纪律规则（唯一事实源，形如 `assets/skills/coding-discipline/SKILL.md`），随 profile 分发给「写代码 + 审代码」四个 Agent。
2. **提示词对齐**：现状不是「没有纪律」，而是**两边的预审不对齐**——MVP 侧 `CODING_AGENT_PROMPT` 的「提交前预审」有四条（`src/config/manifest.rs:112` 文档即契约、`:113` 断言必须能失败、`:114` 三条自查、`:117` doc-lint 门禁；`:121` 自述「预审四条全过」），其中 `:113` 已有行为断言的**前半段**（换个错实现还绿吗）、`:116`+`:117` 已有先证后报的**前半段**（命令原文 + 退出码 + 摘要落 `coding/stages.md`）；而 change 侧 `CHANGE_CODING_AGENT_PROMPT`（`src/config/manifest.rs:553`–`:599`）**没有预审清单**，只在 `:565` 要求基线证据「命令 + 退出码 + 摘要」。因此本次净增量只有三样：①纪律条目本体只此一份（放进 skill）；②MVP 侧把 `:113` 补齐成完整的行为断言判据（补「只改内部重构不得让它变红」这半边）、把 `:116`+`:117` 的证据口径指向 skill 的「先证后报」条目；③change 侧新建一份与 MVP 对齐的预审清单（**只引用 skill，不复述 `:112`/`:114`/`:117` 已有条款**，避免重复与口径冲突，见风险 R5）。审查侧同步加一条对应审查面（规则写了没人查 = 白写）。
3. **机械下沉**：「先证后报」里能机械判的那部分（证据文件存在 + 退出码为 0）下沉成**一道机械门**，形状与既有 baseline 门完全一致（引擎不自己跑测试，只验证据）；其余判断类纪律留在 Agent 侧——这与本仓架构原则 §6「厚 Agent、薄编排」一致（`docs/plantree/baseline/architecture-principles.md`）。

明确不做：不换模型、不换 harness、不引入子代理/多模型编排（pstack 的 arena/swarm/poteto-mode）、不改 MVP 旧 coding 剧本的编排结构、不把 pstack 24 份 skill 原样塞进 profile、不动 plan 阶段。

### 1.2 pstack 24 条原则的取舍（蒸馏结果）

采纳 12 条（进 skill，每条含「触发时机 / 规则 / 反例 / 证据形态」四要素）：

- 行为断言（`principle-test-behavior-not-implementation`）、先证后报（`principle-prove-it-works`）、减法优先（`principle-subtract-before-you-add`）、最小 diff（`principle-laziness-protocol`）、根因优先（`principle-fix-root-causes`）、边界收口（`principle-boundary-discipline`）、类型纪律（`principle-type-system-discipline`）、读者负担（`principle-minimize-reader-load`）、上下文守护（`principle-guard-the-context-window`）、不阻塞人类（`principle-never-block-on-the-human`，限定版）、可验证单元（`principle-sequence-verifiable-units`）、幂等收敛（`principle-make-operations-idempotent`）。

落地约束（pi skill frontmatter 契约，见 §2.3）：新 skill 的 frontmatter 必须有**非空** `description`（缺了 pi 根本不加载该 skill，profile 里落着文件也等于没有）；且**不设** `disable-model-invocation: true`——pstack 的 principle skills 设了它、靠 `poteto-mode` 按名路由，而 devforge 的 coding agent 没有这层路由，skill 必须留在系统提示的技能清单里才能被按需加载。

不单列 12 条及理由（避免规则表膨胀、避免与 devforge 既有条款打架）：

- `principle-build-the-lever` / `principle-encode-lessons-in-structure` —— devforge 已有机械门体系（`devforge doc-lint` + `src/change/coding_gate.rs`），杠杆已由门承担；**本次变更本身就是 encode-lessons-in-structure 的一次应用**（纪律 → skill → 门），故不另立条目。
- `principle-explain-the-number`（性能数字）、`principle-experience-first`、`principle-exhaust-the-design-space`、`principle-redesign-from-first-principles`、`principle-outcome-oriented-execution` —— 属规划/产品/性能决策，不在 coding 纪律范围内。
- `principle-foundational-thinking`（数据结构先行）—— 并入「类型纪律」。
- `principle-model-the-domain` —— 与「类型纪律」重叠，且 devforge 提示词已有「架构 AI-friendly / 模块边界」条款。
- `principle-migrate-callers-then-delete-legacy-apis` —— devforge 无外部兼容包袱，改 API 直接改；不单列。
- `principle-separate-before-serializing-shared-state` —— devforge 已按「每个 Agent 独占自己的产物」设计；不单列。
- `principle-attack-the-premise` —— devforge 的审查打回是零校验自动重审，没有「前提」概念；待出现「同一原因连续打回」再提炼。

### 1.3 歧义（必须用户裁决；括号内为本计划采用的默认）

- **A1 作用范围**：只治理 change SOP（当前 live 路径）还是两条 coding 路径都治理？——（默认：提示词与 skill 覆盖两条路径的 coding 双 Agent + change coding 双 Agent；**机械门只加在 change 路径**，因为 MVP 黑板路径没进 `CodingSopLoop::run`（`src/sop/coding.rs:113`），加门要动编排 API。）
- **A2 引入方式**：pstack 24 份 skill 原样 vendor，还是蒸馏成一份？——（默认：**蒸馏成一份** `coding-discipline`。理由：原样塞 24 份会让 profile 膨胀（24 个 skill 目录 + 清单噪音），且 pstack 的 principle skill 带 `disable-model-invocation: true`、靠 `poteto-mode` 按名路由，本仓的 coding agent 没有这层路由（取值决定见 §1.2 落地约束）；蒸馏时保留 MIT 归属声明。）
- **A3 与既有纪律的冲突**：pstack「不阻塞人类」要求可逆决策自己定，而 devforge 现有「plan 偏差协议」要求存疑必须问用户（`src/config/manifest.rs:99` 的偏差协议小节，正文在 `:99`–`:106`）。——（默认：**不动偏差协议**（它是决策链 D005 的产物），纪律条目只写「白名单内、可逆的小决策自决并留痕；越界/不可逆才问」。）
- **A4 白名单里的新文件怎么办**：——（**已落定：选 (a)，保留新增文件**。`assets/skills/coding-discipline/SKILL.md` 是本计划唯一新文件，白名单其余全是既有文件。`src/change/plan_audit.rs:88` 的存在性检查会把它报为「不存在」，这是**待建文件的误报**（该检查的原意见 `src/change/plan_audit.rs:290` 的测试注释「按『新增文件』豁免」），coding 阶段把该文件建出来即消解。硬约束：该文件是 `src/config/skill_assets.rs` 里 `include_str!` 的编译期依赖——它不在工作区，`cargo build`/`cargo test` 就编不过，因此它必须与 `src/config/skill_assets.rs` 的注册改动**同一次提交**落地。不选 (b)（正文内联为 Rust 常量）的原因：house style 规定仓内真源是 `assets/skills/<skill>/`（`docs/plantree/plans/005-coding-stage/evidence/skills-sources.md:1`），长 markdown 塞进 Rust 常量会破坏可读性与可维护性。）
- **A5 证据门失败要不要自动重问 Agent**：——（默认：**不重问**，与既有 baseline 门同策略：失败即 Quality，不自动重启，见 `src/change/coding_gate.rs:1`。）
- **A6 是否也给 plan / test / doc 阶段套这套纪律**：——（默认：不做。需求点名 coding agent；plan 阶段有自己的机械审计 `src/change/plan_audit.rs:14`，test 阶段已有「反向验证」纪律。）

## 2 影响面

### 2.1 直接命中

- `src/config/manifest.rs:74` —— MVP Coding Agent 提示词（写代码方）：在既有「提交前预审」（`:112`/`:113`/`:114`/`:117`）之后新增一条「工程纪律」小节，只做三件事——引用 `coding-discipline` skill、把 `:113` 的「断言必须能失败」补齐为完整行为断言判据（补「只改内部重构不得让它变红」这半边）、把 `:116`+`:117` 的证据口径指向 skill 的「先证后报」条目；**不复述** `:112`/`:114`/`:117` 已有条款（避免重复与口径冲突，见风险 R5）。
- `src/config/manifest.rs:135` —— MVP Coding Review 提示词：新增一条审查面「核对自证证据是否真实存在且可复现」（对照 skill 的「先证后报」与「行为断言」条目），不复述被审方条款。
- `src/config/manifest.rs:553` —— Change Coding Agent 提示词（live 路径的写代码方）：**该提示词没有预审清单**（`:553`–`:599`，只在 `:565` 要求基线证据「命令 + 退出码 + 摘要」），因此新增一份与 MVP 对齐的「提交前预审」小节——同样只引用 skill + 两条自证项，不复述 `:565` 已有条款。
- `src/config/manifest.rs:602` —— Change Coding Review 提示词：新增一条与 `:135` 同口径的「自证核对」审查面。
- `src/config/manifest.rs:790` —— `coding-agent` 的 skills 列表：加 `coding-discipline`。
- `src/config/manifest.rs:805` —— `coding-review` 的 skills 列表：加 `coding-discipline`。
- `src/config/manifest.rs:903` —— `change-coding-agent` 的 skills 列表：加 `coding-discipline`。
- `src/config/manifest.rs:918` —— `change-coding-review` 的 skills 列表：现为空 `vec![]`，改为含 `coding-discipline`。
- `src/config/skill_assets.rs:14` —— `builtin_skill_files` 的 match：注册新内置资产（无 `skills_source` 时也必须落真内容，理由见同文件头注释 P2-ISSUE-05）。
- `assets/skills/coding-discipline/SKILL.md` —— **本变更新增**：纪律条目唯一事实源（12 条 + 未纳入 12 条的理由 + 来源与许可登记）。
- `src/change/coding_gate.rs:16` —— 证据文件常量区：加 `SELF_CHECK_LOG`（与 `BASELINE_LOG` 同形）。
- `src/change/coding_gate.rs:32` —— `parse_baseline_log`：**签名与文案都不动**（`fn(text: &str) -> Result<BaselineEvidence>`）——它有第二个调用方 `src/change/testing_gate.rs:27`（回归日志解析），改签名就得连带改 `src/change/testing_gate.rs`，而该文件不在本计划白名单内（改了会被 `src/change/boundary.rs:102` 的白名单比对判成越界，`:115` 判 `OutOfBoundary` 回退 plan）。做法：在本文件内另起**参数化私有解析**（文件名与错误文案作为参数传入），`parse_baseline_log` 降级为薄封装（行为、文案、签名全部不变），新门只调私有解析。
- `src/change/coding_gate.rs:66` —— 现有「缺退出码」报错把 `evidence/baseline-test.log` 硬编码进文案（该文案对 baseline 门与回归路径保持原样不动）。
- `src/change/coding_gate.rs:75` —— `check_baseline_log` 之后：新增 `check_self_check`（存在 + 退出码为 0，否则 Quality），走参数化私有解析，文件名与错误文案指向 `evidence/self-check.log`。
- `src/change/session.rs:130` —— `coding_kickoff`：告知自证证据文件路径与写法（与 `baseline_kickoff`（`src/change/session.rs:80`）同款显式指令）。
- `src/change/session.rs:301` —— **新门唯一接线锚点**：`run_coding` 里 `require_marker`（`:301`）之后、`read_steps`（`:303`）之前，也就是 ② coding 轮之内；**不接**在基线轮（`check_baseline_log` 在 `src/change/session.rs:289`）之后——那时 `evidence/self-check.log` 还不存在，会把每次 change run 拦死。
- `src/change/mod.rs:29` —— re-export 新门函数（与 `check_baseline_log` 并列）。
- `tests/e2e/fixtures/change_agents.json` —— change 六件套 e2e 夹具：提示词与 skills 变更后必须与 builtin 同步。
- `docs/plantree/plans/009-change-sop/decisions/README.md` —— 决策台账：登记 D023（工程纪律 skill 的供给口径、change 路径新增「先证后报」机械门、机械门只落 change 路径的不对称）。依据：`src/config/manifest.rs:73` 的「勿手改——改决策先改 plan tree」，change 侧提示词挂在 P009 决策链上（`src/config/manifest.rs:552`、`:601`）。

### 2.2 调用链（谁调用它 / 它调用谁）

- `src/config/render.rs:101` —— 渲染器按 `agent.skills` 逐个分发：新 skill 走 `src/config/render.rs:112` 的 builtin 分支落盘到 `profiles/<agent>/skills/coding-discipline/SKILL.md`（渲染器本身不改）。
- `src/config/manifest.rs:1162` —— 夹具漂移守卫：change 六件套逐字段比对 builtin → 提示词/skills 一改就红（生成器在 `src/config/manifest.rs:1144`）。
- `src/change/orchestrator.rs:119` —— 编排器 `run` 调 `session.run_coding` → 命中新门；门失败以 Quality 冒泡，阶段不前进（编排层不感知具体门）。
- `src/change/session.rs:284` —— `run_coding` 的真实顺序：基线轮 `src/change/session.rs:286`–`:289`（`check_baseline_log` 在 `:289`）→ coding 轮 `:292`–`:301`（`require_marker` 在 `:301`）→ **新门**（接线点 `:301` 之后、`:303` 之前）→ `read_steps`（`:303`）+ `check_step_commits`（`:304`）→ coding review（`:307` 起）。
- `src/change/session.rs:94` —— `coding_review_instruction`：审查指令要能核「自证证据」，需与新证据文件对齐（属本文件改动范围）。
- `src/sop/coding.rs:113` —— MVP `CodingSopLoop::run`：**本次不动**（见 A1），其黑板路径未入参。
- `src/server/mod.rs:377` —— MVP `CodingSopLoop` 调用点：**本次不动**（随之不动）。
- `src/config/manifest.rs:438` —— `AgentManifest.skills` 字段：类型不变（`Vec<String>`），只加元素。

### 2.3 隐式耦合（grep 拿不到的那类）

- **运行时清单缓存**：`src/main.rs:150` —— `~/.devforge/manifest.json` 存在时用落盘清单（不再用 `builtin()`）→ 改了 builtin 不会自动生效，须重存清单并重渲染 profile，否则新 skill 根本不会进 profile（仓内改完 ≠ 用户态生效）。
- **pi skill frontmatter 契约**（目标仓外，pi 包内 `dist/core/skills.js`）：① frontmatter 缺 `description` → pi **不加载**该 skill（返回 `skill: null`），profile 里落着文件也等于没有；② frontmatter 写 `disable-model-invocation: true` → 该 skill 被**排除出系统提示的技能清单**（只能靠 `/skill:name` 显式调用），coding agent 不会知道它存在。本仓的 coding agent 没有 pstack 那样的 mode 路由 → 新 skill 必须有非空 `description` 且**不设**该字段（取值决定见 §1.2，验收见 §5）。
- **profile 是渲染产物**：`~/.devforge/profiles/<agent>/skills/` 是生成物；仓内真源是 `assets/skills/`（`docs/plantree/plans/005-coding-stage/evidence/skills-sources.md:1` 记录了这一分工）。
- **e2e skills 矩阵断言**：`tests/e2e/test_17_testing_config.py:83`（doc-fix 必须 4 个 skill）、`tests/e2e/test_17_testing_config.py:60`（test-agent 恰 1 个）、`tests/e2e/test_11_coding_config.py:118`（plan-agent 恰 `plan-tree`）—— 误给 doc-*/test-*/plan-* 加 skill 会直接红。
- **仓内单测断言**：`src/config/manifest.rs:1093` 断言 `cca.skills.len() == 4` —— 加第 5 个 skill 后必须同步改（同文件内）。
- **审计口径**：`src/change/plan_audit.rs:14` 六块标题、`src/change/plan_audit.rs:88` 白名单存在性、`src/change/plan_audit.rs:104` 验收措辞 —— 计划文本写法受它约束（见风险 R1）。
- **许可与归属**：pstack 为 MIT（Copyright 2026 Lauren Tan），上游 `cursor/plugins/pstack`；本地镜像是 `main` 分支 + 手工改写（`/home/trtyr/code-work/pstack/MIRROR.md`）。蒸馏改写必须保留归属。
- **来源台账口径**：`docs/plantree/plans/005-coding-stage/evidence/skills-sources.md:1` 的入库纪律是「原样 vendor，不经手重写」；本变更是蒸馏改写，**不入该台账**（见风险 R4）。
- **黑板布局**：`src/sop/coding_protocol.rs:17` 定的黑板常量（`coding/stages.md` 等）不变；新证据文件只落在 change 路径黑板（`evidence/` 子树，`src/change/prepare.rs:68` 已建）。

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `assets/skills/coding-discipline/SKILL.md` | 纪律唯一事实源：12 条条目（每条含触发时机/规则/反例/证据形态）+ 未纳入 12 条的理由 + 来源与许可登记；它是 `src/config/skill_assets.rs` 里 `include_str!` 的编译期依赖，必须与该文件的注册改动同一次提交落地（见 §1.3 A4） | 需求 1（规则供给） |
| `src/config/skill_assets.rs` | 注册内置资产，使无 `skills_source` 时 profile 也拿到真内容（否则退回占位目录，重现 P2-ISSUE-05 类问题） | 需求 1 |
| `src/config/manifest.rs` | 四份提示词按各自现状分别改：MVP 侧（`:74`/`:135`）在既有四条预审之上补两半判据 + 审查面；change 侧（`:553`/`:602`）新建预审清单 + 审查面（一律只引用 skill，不复述 `:112`/`:114`/`:117`/`:565` 已有条款）；四个 Agent 的 skills 增项；同步 `cca.skills.len()` 断言并补纪律锚点单测 | 需求 1、2 |
| `src/change/coding_gate.rs` | 新增「先证后报」机械门：证据文件 + 退出码，形状与既有 baseline 门一致；解析抽成参数化私有函数，`parse_baseline_log`（`:32`）的签名与文案不动，回归路径（`src/change/testing_gate.rs:27`）零影响 | 需求 3 |
| `src/change/session.rs` | 接线新门；kickoff 与审查指令显式写清证据文件路径与格式 | 需求 3 |
| `src/change/mod.rs` | re-export 新门函数，保持 `src/change` 对外导出面一致 | 需求 3 |
| `tests/e2e/fixtures/change_agents.json` | 提示词与 skills 变更后与 builtin 逐字段同步（漂移守卫 `src/config/manifest.rs:1162`） | 需求 1、2 |
| `docs/plantree/plans/009-change-sop/decisions/README.md` | 登记 D023：①工程纪律 skill 的供给口径（唯一事实源 + 四个 coding/review Agent 获得）；②change 路径新增「先证后报」机械门（证据文件 + 退出码）；③机械门只落 change 路径的不对称决策及其理由（MVP 黑板路径未进 loop）。依据 `src/config/manifest.rs:73`「改决策先改 plan tree」，台账现登记至 D022a | 需求 1、3 |

### 3.2 受影响不改

| 文件 | 为什么可以不改 |
| --- | --- |
| `src/sop/coding.rs` | MVP 编码剧本：机械门需要黑板路径，而 `run` 签名没带（`src/sop/coding.rs:113`）；本次只治理 live 的 change 路径，提示词侧纪律两边都已覆盖 |
| `src/server/mod.rs` | MVP `CodingSopLoop` 调用点随上一条不动 |
| `src/config/render.rs` | 渲染器按 `agent.skills` 通用分发，新 skill 走既有 builtin 分支（`src/config/render.rs:112`），无需改渲染逻辑；本次也不新增渲染级用例（同款落盘路径已由既有 `src/config/render.rs:344` 的 doc-contract 用例覆盖），「落盘」验收改在 `src/config/skill_assets.rs` 层判定（见 §5） |
| `src/change/orchestrator.rs` | 编排层不感知具体门；门失败以 Quality 冒泡，阶段不前进——已有处理路径 |
| `src/sop/coding_protocol.rs` | 黑板常量与协议标签不变；新证据文件不新增回复协议标签（避免给模型加新协议面） |
| `src/change/plan_audit.rs` | 审计口径本次不改；新文件存在性检查产生的发现是待建文件误报（见 §1.3 A4 与风险 R1），coding 阶段建出文件即消解 |
| `assets/skills/doc-contract/SKILL.md` | 文档契约（写文档用）与工程纪律（写代码用）是两件事，不混在一份规则里 |
| `docs/plantree/plans/005-coding-stage/evidence/skills-sources.md` | 该台账是 P005 收官证据、只登记「原样 vendor」的四语言 skills；pstack 是蒸馏改写，来源登记写在 skill 文件内（差异见风险 R4） |
| `tests/e2e/test_11_coding_config.py` | 只断言 plan-agent 的 skills 恰为 `plan-tree`、coding 双 profile 的四语言 skill 真内容；coding profile 多一个 skill 不影响它 |
| `tests/e2e/test_17_testing_config.py` | 断言的是 test-/doc-/review- 家族的 skills 矩阵；本次不给这些 Agent 加 skill |
| `tests/e2e/test_26_change_e2e.py` | change 真链 e2e 脚本不改，作为回归项使用（新增门在真链里由真 Agent 写证据） |
| `src/change/testing_gate.rs` | 测试阶段黑盒纪律（回归优先 / 对账 / 反向验证）已有对应条款，不重复加；它还是 `parse_baseline_log` 的第二个调用方（`:27`），本计划不改该解析的签名与文案（见 §2.1 与风险 R12），因此无需改动 |

### 3.3 必须回归验证

| 模块 | 为什么风险在这里 |
| --- | --- |
| `src/config`（manifest / render / skill_assets） | 15 个 Agent 的 profile 渲染都从这里出；skills 增项 + 提示词改写一旦漏改，四份提示词与四份夹具会互相漂移 |
| `src/change/coding_gate.rs` + `src/change/session.rs` | 新门第一次在真链上拦人：解析形状错会把红证据放行，或把合规证据误杀成 Quality |
| 夹具漂移守卫 `src/config/manifest.rs:1162` | 提示词改一个字就红；必须重生成夹具而不是手改 |
| e2e skills 矩阵（`tests/e2e/test_11_coding_config.py`、`tests/e2e/test_17_testing_config.py`） | 误给 doc-/test-/plan- 家族加 skill 会在此暴露 |
| e2e change 真链（`tests/e2e/test_26_change_e2e.py`） | 新门 + 新提示词的端到端实证：真 Agent 是否会写 `evidence/self-check.log`、真链是否仍能走到 Settled |
| e2e coding 全链（`tests/e2e/test_14_coding_full_chain.py`） | MVP coding profile 渲染与提示词锚点（`tests/e2e/test_14_coding_full_chain.py:69` 断言各 profile 的 AGENTS.md 存在） |
| 仓内单测（`cargo test`） | 新增/改动的单测（skill 资产与 frontmatter 契约、提示词锚点、门的解析与判定、session 接线）全在白名单文件内；渲染层落盘路径由既有 `src/config/render.rs:344` 用例覆盖（该文件不在白名单，本次不改） |

## 4 白名单

- `assets/skills/coding-discipline/SKILL.md`
- `src/config/skill_assets.rs`
- `src/config/manifest.rs`
- `src/change/coding_gate.rs`
- `src/change/session.rs`
- `src/change/mod.rs`
- `tests/e2e/fixtures/change_agents.json`
- `docs/plantree/plans/009-change-sop/decisions/README.md`

新增测试文件：无。本仓 Rust 单测内联在被改文件内（`#[cfg(test)]` 模块），本变更不需要新增测试文件，也不需要新增 e2e 脚本。

## 5 验收标准

- 给定 `ConfigManifest::builtin()` → `coding-agent`、`coding-review`、`change-coding-agent`、`change-coding-review` 四个 Agent 的 `skills` 都含 `coding-discipline`。
- 给定 `ConfigManifest::builtin()` → `plan-agent`、`plan-review`、`change-plan-agent`、`change-plan-review`、`test-agent`、`test-review`、`change-test-agent`、`change-test-review`、`doc-fix`、`doc-agent`、`doc-review` 这十一个 Agent 的 `skills` 都不含 `coding-discipline`（与 `src/config/manifest.rs:1026` 的 15 个 Agent 断言对账：4 正 + 11 负 = 15；`plan-review` 的 skills 是 `["plan-tree"]`，见 `src/config/manifest.rs:779`）。
- 给定 `builtin_skill_files("coding-discipline")` → 返回 `Some`，且文件清单含 `SKILL.md` 一项（该项正文就是落进 profile 的内容）。
- 给定 `builtin_skill_files("coding-discipline")` 的 `SKILL.md` 正文 → frontmatter 含 `name: coding-discipline` 与一行**非空** `description`（pi skill 加载器缺 `description` 即不加载该 skill，见 §2.3）。
- 给定 `builtin_skill_files("coding-discipline")` 的 `SKILL.md` 正文 → frontmatter **不含** `disable-model-invocation: true`（该字段为真时 pi 会把该 skill 排除出系统提示的技能清单，coding agent 不会知道它存在；取值决定见 §1.2）。
- 给定 `docs/plantree/plans/009-change-sop/decisions/README.md` → 含一条 D023 决策，登记「工程纪律 skill 供给 + change 路径的先证后报机械门 + 机械门只落 change 路径」三点。
- 给定 `coding-discipline` skill 正文 → 至少含 12 个纪律条目，每条都有「反例」与「证据形态」两节。
- 给定 `coding-discipline` skill 正文 → 同时出现这 12 个纪律关键词：行为断言、先证后报、减法优先、最小 diff、根因、边界收口、类型纪律、读者负担、上下文守护、不阻塞人类、可验证单元、幂等。
- 给定 `coding-discipline` skill 正文 → 同时出现 `cursor/plugins/pstack`、`MIT`、`Lauren Tan` 三个字符串（来源与许可登记）。
- 给定 `CODING_AGENT_PROMPT` → 同时含 `coding-discipline`、`行为断言`、`先证后报` 三个字符串。
- 给定 `CHANGE_CODING_AGENT_PROMPT` → 同时含 `coding-discipline`、`行为断言`、`先证后报` 三个字符串。
- 给定 `CODING_REVIEW_PROMPT` 与 `CHANGE_CODING_REVIEW_PROMPT` → 各自含 `先证后报`（审查面锚点）。
- 给定 `change-coding-agent` 的 `skills` 长度 → 等于 5（`src/config/manifest.rs:1093` 的断言同步为 5 后通过）。
- 给定黑板上不存在 `evidence/self-check.log` → `check_self_check` 返回 Quality 错误，错误文本含 `evidence/self-check.log`。
- 给定 `evidence/self-check.log` 只有 `command:` 而没有退出码 → `check_self_check` 返回 Quality 错误，错误文本同时含「退出码」与 `evidence/self-check.log`。
- 给定 `check_self_check` 返回的任一 Quality 错误 → 错误文本不含 `evidence/baseline-test.log`（新门文案必须参数化到自己的证据文件，不能沿用 `src/change/coding_gate.rs:66` 的基线文案）。
- 给定 `parse_baseline_log` 的函数签名 → 仍是单参 `fn(text: &str) -> Result<BaselineEvidence>`（未新增必填参数，第二调用方 `src/change/testing_gate.rs:27` 零改动）。
- 给定 `check_regression_claim` 传入缺退出码的日志文本 → 返回 Quality 错误且错误文本以「回归日志不可解析」开头（回归路径的文案行为不变）。
- 给定 `evidence/self-check.log` 的退出码是 1 → `check_self_check` 返回 Quality 错误，错误文本含 `exit_code=1`。
- 给定 `evidence/self-check.log` 含 `command: cargo test` 与 `exit_code: 0` → `check_self_check` 返回 Ok。
- 给定 change coding 轮：Agent 输出 `<stage-complete>` 但黑板无合规自证证据 → `run_coding` 返回 Quality 错误，且 coding review 轮次为 0（不进审查）。
- 给定 change coding 轮：Agent 输出 `<stage-complete>` 且黑板有合规自证证据 → `run_coding` 正常进入 coding review（轮次为 1）。
- 给定用 `cargo test --lib dump_change_agents_fixture -- --ignored` 重生成夹具 → `change_agents_fixture_matches_builtin` 通过（夹具与 builtin 逐字段一致）。

## 6 风险

- **R1 · 引擎审计对新增文件的误报（处理方式已落定）**：`src/change/plan_audit.rs:88` 的白名单存在性检查会把尚未创建的新文件报为「不存在」。本计划的唯一新文件 `assets/skills/coding-discipline/SKILL.md` 必然命中这一条——这是**待建文件误报**，不是边界问题：该检查与「新增文件豁免」的原意不一致（原意见 `src/change/plan_audit.rs:290` 的测试注释「按『新增文件』豁免」），coding 阶段把该文件建出来即消解。**处理方式已落定（选项 a：保留新增文件）**；硬约束是它作为 `src/config/skill_assets.rs` 里 `include_str!` 的编译期依赖，必须与该文件的注册改动同一次提交落地（否则 `cargo build`/`cargo test` 编不过）。
- **R2 · 真 Agent 合规风险（先证后报证据门）**：coding agent 可能不写 `evidence/self-check.log` → 门失败即 Quality 且不自动重启（与 `src/change/coding_gate.rs:75` 的 baseline 门同策略）。同款门在真链上已被验证可靠（`tests/e2e/test_26_change_e2e.py` 真链 31 PASS/0 FAIL 实证）。缓解：提示词（`src/config/manifest.rs:553`）与 kickoff（`src/change/session.rs:130`）双写格式要求，门失败信息直接给出文件路径与写法。
- **R3 · 运行时清单缓存**：`src/main.rs:150` 显示 `~/.devforge/manifest.json` 存在即用落盘清单。仓内改完 builtin 不会自动生效——必须重存清单（PUT `/api/v1/config/manifest`）并重渲染 profile，新 skill 才会进 profile。仓内无法验证这一段，属部署侧动作。
- **R4 · 文档与代码不一致（来源台账）**：`docs/plantree/plans/005-coding-stage/evidence/skills-sources.md:1` 记录的是「原样 vendor 四语言 skills」的台账，本变更引入的是蒸馏改写内容且不入该台账，来源登记只写在 skill 文件内 → 台账与实际入库不一致，差异已知并接受（该台账属 P005 收官证据，不在本次改动面内）。
- **R5 · 提示词与 profile 体积**：四个 Agent 的 AGENTS.md 各增一节，四个 profile 各多一个 skill 目录 → 上下文占用与安装体积上升；审查 Agent 多读一份 skill 会拖慢一轮审查（thinking medium）。
- **R6 · 与既有纪律的方向冲突**：`src/config/manifest.rs:99`（正文 `:99`–`:106`）的「plan 偏差协议」（存疑必须问用户）与 pstack「不阻塞人类」方向相反。本次不改偏差协议，纪律条目只限定「白名单内且可逆的小决策自决并留痕」，两者在提示词里并存，可能让模型在边界处犹豫。
- **R7 · 两条路径门禁强度不等**：机械门只加在 change 路径，MVP coding 剧本（`src/sop/coding.rs:113`）只有提示词纪律、没有门 → 同一套纪律在两条路径上的强制力不同。
- **R8 · 许可与归属**：pstack 为 MIT（Copyright 2026 Lauren Tan，上游 `cursor/plugins/pstack`，本地镜像是 `main` 分支 + 手工改写）。蒸馏改写保留归属声明；不逐字 vendor 原文，避免上游变动与许可头丢失。
- **R9 · 夹具漂移**：`tests/e2e/fixtures/change_agents.json` 与 builtin 逐字段比对（`src/config/manifest.rs:1162`），改提示词必须重生成夹具，否则仓内单测直接红。
- **R10 · 测试阶段与「不改产品代码」的张力**：本仓无 Rust 集成测试目录，验收标准以内联单测为主 → 测试阶段需在白名单内的 `.rs` 文件追加 `#[cfg(test)]` 用例，这与 change-test-agent 的「不改产品代码」口径有张力，须按「只追加 test 模块、不改实现」执行。
- **R11 · 兼容性**：无 schema / 表 / 协议 / 对外契约变更（`AgentManifest.skills` 仍是 `Vec<String>`，`src/config/manifest.rs:438`）；无数据迁移。黑板侧只新增一个证据文件。
- **R12 · 新门错误文案误指基线文件（处理方式已落定）**：`src/change/coding_gate.rs:66` 的「缺退出码」报错把 `evidence/baseline-test.log` 硬编码进文案。若新门直接复用 `parse_baseline_log`，报出来的会是「基线证据缺退出码」并指向基线文件——排查时会去找错文件。**处理方式已落定（选项 b）**：不改 `parse_baseline_log(text)` 的签名与文案（它有第二个调用方 `src/change/testing_gate.rs:27`，该文件不在本计划白名单内，改它会越界并被 `src/change/boundary.rs:102` 判成越界、`:115` 判 `OutOfBoundary` 回退 plan）；改为在本文件内另起**参数化私有解析**（文件名 + 文案作参数），`parse_baseline_log` 退化为薄封装、行为与文案完全不变，新门只调私有解析、错误文案指向 `evidence/self-check.log`。§5 已给四条字面量验收兜住这条。
- **R13 · 断点续跑时的旧证据**：change SOP 的断点续跑会从 `ChangePhase::Coding` 重新进入 coding 阶段（`src/change/orchestrator.rs:171`），而黑板只在 `src/change/prepare.rs:68` 建目录、**不清理既有文件** → 上一轮遗留的 `evidence/self-check.log` 可能让新门直接放行（拿旧证据当新证据）。引擎层不做旧证据清理（超出白名单，且黑板不清理是既有设计），缓解写在本计划内：kickoff（`src/change/session.rs:130`）要求本轮**覆盖写**该证据文件，审查指令（`src/change/session.rs:94`）要求把该证据与本轮 diff 对账；残留旧证据被误放行的兜底在 coding review 一侧。
- **R14 · 决策链与台账同步**：`src/config/manifest.rs:73`（并见 `:134`）写明提示词「勿手改——改决策先改 plan tree」；本次给 coding 侧新增 skill 供给与一道机械门（决策级），change 侧提示词挂在 P009 决策链上（`src/config/manifest.rs:552`、`:601`）。因此白名单**含** `docs/plantree/plans/009-change-sop/decisions/README.md`，由 coding 阶段登记 D023（台账现登记至 D022a）。MVP 侧（P005）本次只加提示词小节、不加门不改阶段，不构成新决策 → 005 台账不动；若将来也给 MVP 路径加机械门，必须先回 005 台账与 `src/sop/coding.rs` 的 API 决策。
- **R15 · pi skill 加载契约**：frontmatter 缺 `description` → pi 不加载该 skill（pi 包内 `dist/core/skills.js` 的加载器在 `hasDescription` 为假时直接返回空 skill）；`disable-model-invocation: true` → 该 skill 被排除出系统提示的技能清单（同一文件的清单过滤）。任一条踩中，纪律就「装了但 agent 看不见」——改了代码却零效果，且这类失败在单测里只看 frontmatter 字符串、不看真实加载。§5 已给三条字面量验收。
