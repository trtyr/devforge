# 变更计划 · change-20261022-f6-1-boot-start-10s-run-attempt-2-src-en

目标仓：`/home/trtyr/code-work/devforge`
基线：`a0be9477a9ece7b12b5ec345cdaab141b619eb2b`（`docs/plantree/changes/change-20261022-f6-1-boot-start-10s-run-attempt-2-src-en/baseline.txt`）
上一场：`change-20261022-1-2026-10-21-c3-textkit-run-test-agent-r`（已 Settled，HEAD=`a0be947`）
文档档位：none（计划阶段照常读已有文档；不跑文档流水线）
本场是**自举第六场·单靶**：只治「**引擎可观测性与死亡取证**」——① 原始错误全量可见、② 里程碑日志、③ 死亡取证（panic 墓碑 + run 心跳）；**附带**修掉 boot 期「基础设施故障盲重启」的**根因**（渲染闸门，用户裁决 A）。**不做** F10（守卫常量物化）、**不做** F8（plan 延迟优化）、**不做** F11（补边界引用卫生）。

---

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

三次实测事故都指向同一个病根：**引擎在自己内部发生了什么，事后查不出来**。本场交付三件互相咬合的东西：

1. **原始错误全量可见** —— 今天 `run_with_retry` 决定重启前，**原始错误全文**（含 source 链）**根本不落盘**。本靶让它**分级落盘**（用户裁决 Q7：基础/非质量类失败 → **ERROR**，质量类失败 → **WARN**，两者字段同构）。现在这条错误只活在函数内的 `Vec<String>` 里（`src/engine/retry.rs:81`），进程活着时看不到、死了更看不到 → 现场只剩一句「基础设施故障，自动重启 run（attempt=2）」。
2. **里程碑日志** —— 编排器每个阶段转换、每轮 fix 开始/结束、每次审查开庭（含轮次）、基线门/步提交对账结果，各落一条结构化 INFO（`component`/`event` 可过滤，`target` 由发出模块自动给出）。今天从 plan agent 交卷到重审开庭之间是 15 分钟的**空洞**（10-21 c5 场 21:20–21:35），引擎一条记录都不留。
3. **死亡取证** —— ① panic hook：panic 原文 + 位置 + 当时阶段写进 HOME 下固定路径的墓碑文件；② run 心跳：阶段转换与关键事件时更新心跳文件（阶段/步骤/时间戳），异常死亡后凭心跳即可回答「死在哪个阶段」；③ 两件事都必须**机械可验**（注入 panic → 墓碑存在且含 panic 原文；阶段推进 → 心跳内容随之变化）。

**外加一件**（需求 1 的后半句「根因若在计划期能从代码静态定位，直接修复并配单测」）：boot 期六场六中的基础设施重试，根因**已静态定位**（§1.2），因此本场**必须**含根因修复段（不是只做可见性）：开跑前若有渲染在途则等所需 profile 就绪（渲染闸门），渲染结束后门卫复查，仍缺 profile → **明确失败**（不再被当成基础设施故障盲重启 10s）。

**明确不做（边界）**：

- 不引入新外部依赖（`Cargo.toml` 不动；心跳/墓碑只用 `std` + 既有 `tracing`）；
- 遵守 `assets/skills/coding-discipline/SKILL.md`（本变更**遵守**它，不改它）；
- F10（白名单守卫常量物化）、F8（plan 延迟优化）、F11（补边界引用卫生）**不在本靶**（守卫常量的**例行刷新**照旧，§1.4 Q9）；
- 不改 run 级重试策略本身（4 次尝试、10s/30s/90s 退避，`src/engine/retry.rs:18`/`:22`）：本靶只让「为什么重试」看得见，并修掉「不该重试的那次重试」；
- 不改任何 SOP 的**业务判定**（审查三通道、质量失败不重启、越界门、验收对账口径一律不动）。

### 1.2 boot 六连的根因（静态定位，高置信 · 三条独立证据）

**证据 1 · 本场实测日志**（`~/.devforge/logs/devforge.log.jsonl`，UTC）：

- `05:22:22.385` devforge 启动；`05:22:22.593`–`05:22:22.620` 十二个 profile 渲染完成；`05:22:25.400` `change-plan-agent` 渲染完成（packages=1）；
- **`05:22:32.604` WARN「基础设施故障，自动重启 run（从断点继续）」attempt=2**；
- `05:22:32.610` INFO「change 门 0 完成」；`05:22:36.663`/`05:22:36.926` `change-coding-agent`/`change-test-agent` 渲染完成（packages=3）。

即：attempt=2 的 WARN 落在 **start+10.000s**（= 退避第一档 `10_000ms`，`src/engine/retry.rs:22`），而 attempt 2 只用 **5.6ms** 就跑完门 0 → **attempt 1 是在开跑瞬间（≈`05:22:22.605`）失败的，不是跑到一半失败**。

**证据 2 · profile 文件 mtime**（同一 workspace 的 profile 目录）：

- `change-plan-agent/AGENTS.md` 写在 `05:22:22.611`，`change-plan-agent/settings.json` 的**最后一次**写入在 `05:22:25.385`；`change-coding-agent/settings.json` 的最后一次写入在 `05:22:36.649`。
- 渲染器的写入次序是「建目录 + 写 `AGENTS.md`（`src/config/render.rs:50`）→ 写 `settings.json`（`src/config/render.rs:63`）→ 装包（`src/config/render.rs:205`，装包期间安装器还会**再回写** `settings.json`，`src/config/render.rs:192`）」→ **`05:22:22.605` 那一刻，`change-plan-agent` 的目录刚被建出来、`settings.json` 还没落盘**。

**证据 3 · 代码路径**：

- 竞态的另一端：`PUT /config/manifest` **立即返回 202**、渲染丢进后台线程（`src/server/handlers.rs:365`、`:383`、`:390`）→ `POST .../start` 完全不等渲染就开跑（`src/server/handlers.rs:202`、`:237`）；
- 门卫判据只有「`settings.json` 在不在」（`src/server/mod.rs:253`–`:256`）→ 半渲染的 profile 一律判「未渲染」；
- 门卫抛的是**裸** `DevforgeError::System`（`src/server/mod.rs:257`）→ `is_infra_failure` 只认裸 `System`（`src/engine/retry.rs:60`）→ 判成基础设施故障 → 退避 10s 后自动重启（`src/engine/retry.rs:98`）→ WARN 落盘（`src/server/mod.rs:1264`）。
- boot 路径上（`require_profile` → `prepare` → 门 0）**只有 `require_profile` 会抛裸 `System`**；其余是 `WithContext`/`Io`/`Quality`（`is_infra_failure` 都不认）→ 排除法也指向同一处。且门卫失败**不经** `map_intervention`（它的 `?` 在 watchdog/编排之前直接上抛，直达重试层 `src/server/mod.rs:1249`）。

**结论**：**boot 六连 = `start` 与异步渲染的竞态**（profile 半渲染 → 门卫误判「未渲染」→ 裸 `System` → 被当基础设施故障盲重启 10s）。六场六中，是因为每场都是「全新 HOME → PUT 清单（装包十几秒）→ 立刻 start」。

**残余不确定性（写进风险 R1）**：本靶的 ERROR 落盘上线后，若实测显示 boot 失败点**不是** profile 未就绪（例如是 git 基线或装包失败），则根因修复段按 R1 降级为「只做可见性 + 登记待查」，其余三件交付物不受影响。

### 1.3 代码事实（定位到行）

| # | 事实 | 位置 |
| --- | --- | --- |
| 1 | 原始错误只进函数内的 `Vec<String>`，**从不落盘**（`log.push(format!("第{attempt}次: {e}"))`） | `src/engine/retry.rs:81` |
| 2 | 错误全文只在**四次尝试全败**后才被拼进最终错误串（`共尝试 N 次；尝试记录：…`） | `src/engine/retry.rs:86`–`:95` |
| 3 | 「该不该重启」只认裸 `DevforgeError::System`（`WithContext`/`Io`/`Quality` 都不认） | `src/engine/retry.rs:60`–`:62` |
| 4 | 退避档位 10s/30s/90s、总尝试 4 次 | `src/engine/retry.rs:18`、`:22` |
| 5 | 唯一的 boot 期裸 `System` 门：`require_profile`（判据 = `settings.json` 存在）；change SOP 的门卫调用点在 `src/server/change_engine.rs:79`（其余五个 SOP 在 `src/server/mod.rs:268`–`:269`/`:326`–`:327`/`:400`–`:401`/`:809`–`:810`/`:929`–`:930`），**门卫失败不经 `map_intervention`**（`?` 直接上抛到重试层） | `src/server/mod.rs:253`–`:262` |
| 6 | boot 期重试 WARN（**不带错误原文**） | `src/server/mod.rs:1264`–`:1270` |
| 7 | run 级重试闭包（`attempt>1` 时落 attempt、记 WARN、发 `run.retry` 事件） | `src/server/mod.rs:1249`–`:1281` |
| 8 | `map_intervention`：**只作用于 SOP/编排执行期**的错误（`sop_result` 的 Err）：Quality → 明确 Failed（不重启）；其余原样上抛（`:1012`）→ 重试。**启动门卫的失败不经它** | `src/server/mod.rs:993`–`:1014` |
| 9 | `PUT /config/manifest` 立即 202，渲染在后台线程（完成信号只经 SSE `config.rendered`/`config.render_failed` 广播） | `src/server/handlers.rs:365`–`:402` |
| 10 | `POST .../start` 不等渲染，直接 spawn run 线程 | `src/server/handlers.rs:202`–`:246`、`:237` |
| 11 | 渲染器写 `settings.json` 的两个时点（早写 / 装包后回写），早写发生在装包**之前** | `src/config/render.rs:63`、`:192`、`:205` |
| 12 | 门卫判据只要求文件存在，**不**要求 npm 包装齐 | `src/server/mod.rs:255`–`:256` |
| 13 | change SOP 的六个 profile 清单（已存在，可直接被闸门引用） | `src/server/change_engine.rs:24`–`:33` |
| 14 | 审查三通道共享门（change 三角色 + MVP 三循环共用），已知轮次号 `base_round + opened`；**全仓 11 处调用点**：生产 4（`src/sop/planning.rs:114`、`src/sop/coding.rs:149`、`src/sop/testing.rs:189`、`src/change/session.rs:237`）+ 仓内单测 5（`src/sop/review_gate.rs:233`/`:250`/`:273`/`:297`/`:316`）+ 仓外验收文件 2（`tests/change_review_protocol/main.rs:1131`/`:1157`） | `src/sop/review_gate.rs:78`–`:146` |
| 15 | change 侧三角色审查的唯一入口（`review_once`，轮次恒传 0），它有 **3 个调用点** = 三角色各一 | `src/change/session.rs:232`–`:244`、`:237`、`:270`、`:361`、`:429` |
| 16 | doc 侧两处审查**不走**共享门（自己 `factory.open()`） | `src/sop/doc_from_zero.rs:317`、`src/sop/doc_fix.rs:92` |
| 17 | change 编排层的阶段写入点（**5 处** `state.phase = …`；`:219`/`:265` 是 `save(run.store, &state)?`，**不是**阶段写入） | `src/change/orchestrator.rs:160`、`:187`、`:257`、`:278`、`:286` |
| 18 | change 侧三处机械门的调用点（基线门 / 自证门 / 步提交对账） | `src/change/session.rs:338`、`:354`、`:357` |
| 19 | agent 轮次的唯一收口（`wait_settled*` 返回 = 交卷；`spec.role` 已知） | `src/engine/client.rs:186`、`:210`、`:43` |
| 20 | 日志面：JSONL 文件层 + stderr 人类层，级别由 `RUST_LOG` 决定（默认 info） | `src/logging.rs:33`–`:52` |
| 21 | HOME 根 = `HOME/.devforge`（日志目录 `HOME/.devforge/logs` 的同根） | `src/main.rs:143`–`:146` |
| 22 | 计划六块的机械门 + 白名单「文件必须存在」检查 + 验收标准「必须含 →」检查 | `src/change/plan_audit.rs:12`–`:20`、`:262`–`:267`、`:281`–`:288` |
| 23 | 计划里的 `path:line` 引用会被逐条验存在性 | `src/change/plan_audit.rs:253`–`:258` |
| 24 | 验收标准只认**列表行**（表格行不算）；白名单按反引号路径抽取 | `src/change/plan_audit.rs:365`–`:378`、`:336`–`:352` |
| 25 | 白名单守卫的三个常量副本（守卫 + 上一场验收文件 ×2），互相比对 | `tests/change_ask_ring.rs:31`、`:35`；`tests/change_failure_context/main.rs:32`、`:36`；`tests/change_review_protocol/main.rs:44`、`:46`、`:71` |
| 26 | 白名单守卫还断言「上一场遗留的两个文件不得进本变更 diff」 | `tests/change_ask_ring.rs:653`–`:661`（`src/change/coding_gate.rs`、`src/config/skill_assets.rs`） |
| 27 | 守卫用例当前带 `#[ignore]`（F10：两场之间恒红），由每场 step 1 摘除 | `tests/change_ask_ring.rs:615`–`:618` |
| 28 | 仓内**没有**任何 heartbeat / tombstone / panic hook 概念（无重复造轮子风险） | 全仓检索无命中 |

### 1.4 歧义

- [必须裁决] **Q1 · boot 六连的根因修法** —— 根因已静态定位（§1.2），修法有多种，影响验收口径（“第一次尝试即通过”还是“明确失败不重启”）、影响 `start` 端点语义、影响白名单（是否要动 `src/server/handlers.rs`）。
  备选：A 渲染闸门（开跑前等所需 profile 就绪；渲染结束后门卫复查，仍缺 → 明确失败不重启）／B 门卫自等（run 线程内等 profile 就绪，start 语义不变）／C 只可见不修（错误降级 + ERROR 落盘 + 登记待查）／D 按需同步渲染（门卫就地渲染缺失的 profile）。
  用户裁决：A 渲染闸门
- [必须裁决] **Q2 · 里程碑日志与 run 心跳的覆盖面** —— 需求点名的四类里程碑（阶段转换 / fix 往返 / 审查开庭 / 门禁对账）里，「阶段转换」「fix 往返」「审查开庭」在 MVP 与 doc 侧各有实现，铺开与否直接决定白名单大小与回归面。
  备选：A 全 SOP 全覆盖（change + MVP 三循环 + doc 两处都出里程碑与心跳）／B change 细粒度 + 全 SOP 的 SOP 级心跳／C 只做 change SOP。
  用户裁决：全 SOP 全覆盖
- [自决] **Q3 · 心跳与墓碑的落点、粒度与形状** —— 落点：HOME 下 `.devforge/`（与日志目录同根，`src/main.rs:144`）：心跳 `HOME/.devforge/heartbeat/<runid>.json`（**每 run 一份、覆盖写**），墓碑 `HOME/.devforge/tombstones/<ts>-<runid>.txt`（**每次 panic 一份、逐个追加新文件**）。**root 可注入**（解析顺序）：`DEVFORGE_HOME` 环境变量 > 进程内显式注入（单测用公开 setter）> `HOME/.devforge`（生产默认）→ 仓内单测把 root 指向临时目录，**不落用户 HOME**（`src/server/tests/mod.rs:145` 的 fixture 与 `:668` 起的 `start_task_runs_engine_and_records` 会走真实 start 路径 → `src/server/handlers.rs:237` → `src/server/mod.rs:1249` → 置位 run 上下文 → 若不注入，用户的 `HOME/.devforge/heartbeat/` 会被单测写脏，R13）。理由：①需求原文已指定墓碑在 HOME 下固定路径；②心跳与墓碑同根 → 验尸时一处看全；③覆盖写符合纪律第 12 条（幂等收敛：状态文件要能被覆盖写，不是追加成第二份真相）；④墓碑逐个落文件 → 多次 panic 不互相覆盖；⑤root 可注入是**单测不污染用户 HOME**的唯一干净做法（与既有 `DEVFORGE_ADDR` / `DEVFORGE_DOC_LEVEL` / `DEVFORGE_RUN_IDLE_MS` 等 env 入口同族）。
- [自决] **Q4 · 里程碑的字段口径** —— 每条里程碑 = `component`（组件名）+ `event`（事件名，snake_case 点分）+ 事件特有字段（如 `from`/`to`/`role`/`round`/`result`/`count`）；`target` 由 `tracing` 自动给出 = 发出该宏的模块。**必须写在内联调用点**（不用包装函数）：包装函数会让所有里程碑的 `target` 变成同一个模块 → 「target 可过滤」这条需求直接作废（这是「一处实现」在本处不可行的硬理由）。事件名固定八个：`phase.change` / `fix.start` / `fix.end` / `review.open` / `gate.baseline` / `gate.self_check` / `gate.step_commits` / `agent.turn_end`。
- [自决] **Q5 · 「审查开庭」里程碑的落点与**轮次号**口径** —— 落在**8 个落点**（change 三角色经 `review_once` 的 3 个调用 `src/change/session.rs:270`/`:361`/`:429`；MVP planning/coding/testing 各 1，即 `src/sop/planning.rs:114`、`src/sop/coding.rs:149`、`src/sop/testing.rs:189`；doc-from-zero 与 doc-fix 各 1，即 `src/sop/doc_from_zero.rs:317`、`src/sop/doc_fix.rs:92`），字段含 `role`（角色名）与 `round`（**该角色在本次 run 内的审查序号**）。**轮次号必须是调用前可知的**：`review_with_gate` 的实例数（`opened`）是**返回值**（`src/sop/review_gate.rs:130`），调用前拿不到 → change 侧不能用它。口径表：

  | 角色 | `round` 来源（调用前可知） | 落点 |
  | --- | --- | --- |
  | change-plan-review | 首审 = 1；`fix_plan` 的修复轮 + 1；`amend_plan` 的 roundtrip + 1 | `src/change/session.rs:270` |
  | change-coding-review | 本阶段内 = 1 | `src/change/session.rs:361` |
  | change-test-review | 本阶段内 = 1 | `src/change/session.rs:429` |
  | plan-review（MVP） | `rounds + 1`（`src/sop/planning.rs:116` 的 `rounds`） | `src/sop/planning.rs:114` |
  | coding-review（MVP） | `review_rounds + 1` | `src/sop/coding.rs:149` |
  | test-review（MVP） | `report.review_rounds + 1` | `src/sop/testing.rs:189` |
  | doc-review（doc-from-zero） | 单元内审查计数（`src/sop/doc_from_zero.rs:316` 附近） | `src/sop/doc_from_zero.rs:317` |
  | doc-review（doc-fix） | `report.review_rounds + 1`（`src/sop/doc_fix.rs:91` 附近） | `src/sop/doc_fix.rs:92` |

  事件一律**在调用前**落（“开庭”就是发起那一刻）：change 侧的 `round` 是**角色内审查序号**（不是实例数）；违例重审/实例失败重试**开过几个实例**由 `review_gate` 自己的既有 WARN 如实记录（`src/sop/review_gate.rs:139`/`:163`），不塞进本事件。**不改 `review_with_gate` 的签名**（它全仓 11 处调用点：生产 4 + 仓内单测 5 + 仓外验收文件 2，见事实 #14；加参数会波及上一场的验收面）；理由：角色名只有调用点知道，轮次号调用点也知道（共享门里知道实例数、但那里不知道角色）。
- [自决] **Q6 · 「agent 轮次结束」里程碑的落点** —— 落在 `src/engine/client.rs:186`/`:210`（`wait_settled*` 返回处，**一处覆盖全部 SOP**），字段含 `role`（`spec.role`）、`turn_ms`（耗时）、`chars`（回复长度）。理由：证据 B 的 15 分钟空洞要能归因，必须有**两端**——「交卷」这一端若散落在 9 个调用点则必然漏；RPC 收口是唯一「所有 agent 轮次必经」的地方（`role` 在那里现成）。
- [必须裁决] **Q7 · 「原始错误全量可见」的**级别**口径** —— 这条**影响验收口径**（§5 第 1–3 条：质量失败到底落不落 ERROR），不由 agent 单方面定。三种口径：A 每次尝试失败都落 ERROR（含质量失败）；B 只在「即将触发重启」时落 ERROR；C 分级——质量类失败落 WARN、其余失败落 ERROR。需求原文写的是「run_with_retry **触发重试前**，必须以 ERROR 级落盘原始错误全文」，字面偏向 B；标题「全量可见」偏向 A；既有代码 `src/engine/retry.rs:80`–`:82` 对所有失败无条件记录。
  用户裁决：分级：基础 ERROR / 质量 WARN
  （口径展开：每次尝试失败落一条**分级**日志——质量类失败（`DevforgeError::Quality`）落 **WARN**，其余一切失败（`System` 及其他种类）落 **ERROR**；两者字段同构：`component="run-retry"`、`attempt`、`error`（顶层 Display 逐字）、`error_chain`（source 链逐层，`→` 连接）、`retryable`（本次是否将重启）。落盘点在**决定重试之前**（`src/engine/retry.rs:79`–`:83` 附近）；**终态失败**（4 次耗尽）再落一条同级的终态条目（含最终错误全文 + `attempts`）。形状：一条一个事件（source 链拼进一个字段），不拆成多条——免得同一错误被过滤成多行而看不出因果。§5 第 1–4 条按此口径写，二者不得再互斥。）
- [自决] **Q8 · 「profile 未就绪」的最终错误种类** —— 从裸 `System` 改为 `DevforgeError::Config`（配置错误）：`is_infra_failure` 不认它 → **不触发自动重启**（attempt 恒为 1）。**错误文本一字不改**（`src/server/mod.rs:257`–`:261` 的原文已含「profile `X` 未渲染（缺 <路径>/settings.json）——先 PUT /api/v1/config/manifest 完成安装渲染，再启动 run」；追加同类句子会重复）→ 仓内 4 条既有断言（`src/server/mod.rs:1745` 起的 profile 缺失用例，用 `contains("未渲染")`）与两个 e2e（`tests/e2e/test_12_sop_override.py`、`tests/e2e/test_18_sop_registry.py`）必然仍绿。理由：「未渲染」是**配置问题**不是基础设施故障（重启不会让它变好，010 D004 的口径）。
- [自决] **Q9 · 白名单守卫常量的例行刷新口径（F10 现状沿用）** —— 本场 step 1 刷三份常量副本（`tests/change_ask_ring.rs:31`/`:35`、`tests/change_failure_context/main.rs:32`/`:36`、`tests/change_review_protocol/main.rs:44`/`:46`/`:71`）**并摘除** `tests/change_ask_ring.rs:618` 的 `#[ignore]`；本场收尾（提交完成后）**复贴**该 `#[ignore]`（与上一场收尾提交 `a0be947` 同款「两场之间 ignore 复贴」）。F10 的真修（常量 gate 时物化）**不在本靶**。口径理由：守卫常量是「每场一刷」的现物，本靶沿用，不顺手改它的机制。**并且必须同步改写两处硬编码断言**（否则 step 1 之后测试面直接红）：①`tests/change_review_protocol/main.rs:1333`–`:1336`（a29）硬编码「守卫 WHITELIST 里**不得**含 `tests/change_review_protocol/main.rs`」→ 本场的 coding 步白名单**含**该文件（它要在 step 1 刷常量，否则 a29/a30 拿旧常量比对新守卫而变红）→ 排除项改为**本场测试阶段文件** `tests/f6_observability/main.rs`（或泛化成「测试阶段文件不得进 coding 步白名单常量」）；②`tests/change_review_protocol/main.rs:1431`–`:1443`（a32 `a32_the_plan_whitelist_has_twenty_files`）硬编码 `PLAN_WHITELIST.len() == 20` / `CODING_WHITELIST.len() == 19`，并断言「§4 白名单含**唯一的**测试阶段文件 `tests/change_review_protocol/main.rs`」→ 改为 **22 / 21**，测试阶段文件断言改为 `tests/f6_observability/main.rs`。**口径选择**：保留「coding 步 21 个 + 测试阶段 1 个」的结构（**不**把既有验收文件的常量刷新推到测试阶段——那样 a29/a30 在 coding 步就会拿旧常量比对新守卫而变红），代价就是必须改写上述两处硬编码（列入 §3.1 与 §5）。
- [自决] **Q10 · 文档登记落点** —— 追加 `## D006 ·` 到 `docs/plantree/plans/010-run-resume/decisions/README.md`（010 是 run 生命周期/韧性的归属计划，D004「基础设施故障自动重启」就在那里）+ `docs/plantree/plans/010-run-resume/roadmap.md` 追加一条本场条目。**不改** `docs/plantree/plans/009-change-sop/**`（本靶不改 change SOP 的业务语义，其 topic 文档与代码无冲突）与 `docs/plantree/ideas/selfhost-findings-20261021.md`（该文件按其自述「由操作员维护」→ F6 的状态更新由操作员做）。
- [自决] **Q11 · 心跳的写入节奏** —— **事件驱动为主**（run 开始 / 阶段转换 / fix 起止 / 审查开庭 / 门禁结果 / 重试）+ **低频兜底**（watchdog 线程按固定间隔刷新时间戳，证明进程还活着而不是卡住）。兜底来源 = **既有 watchdog 的轮询线程**（`src/server/mod.rs:1361`–`:1420`）：该线程每 300ms 一跳，**与 `idle_ms` 无关**（失活收割那段在 `if let Some(idle)` 里，`src/server/mod.rs:1393`，所以即使不开启收割，刷新照样跑）；**六个 SOP 全部经过它**——planning `src/server/mod.rs:298`、coding `:368`、doc-from-zero `:639`、testing `:852`、doc-fix `:967`、**change `src/server/change_engine.rs:203`** → change run（本靶主场景：boot 六连 + 15 分钟 agent 轮空洞）**同样有兜底**，长 agent 轮里心跳不会陈旧（「卡死 vs 已死」仍可分）。**不新增线程**。理由：事件驱动能回答「死在哪个阶段」（硬验收），低频兜底能区分「卡死」与「已死」——只有事件驱动时，一份 40 分钟前的心跳既可能是死了，也可能是活着在等模型，二者不可分。
- [自决] **Q12 · 「全 SOP 全覆盖」的具体边界** —— 里程碑覆盖：change 编排层（阶段转换 + fix 往返 + 审查开庭 + 门禁对账）、MVP planning/coding/testing 三循环（阶段推进 + fix 往返 + 审查开庭）、doc-from-zero（单元推进 + 审查开庭 + **单元审查 FAIL → 回注 agent 重写**，`src/sop/doc_from_zero.rs:397`–`:407`）、doc-fix（审查轮 + 回注）。**不覆盖**：MVP 侧的「基线门/步提交对账」（那是 change 路径独有的机械门）、doc 侧的单元级 lint 细节。理由：Q2 裁决的是「全 SOP 全覆盖」，但「基线门/步提交对账」在 MVP/doc 侧**不存在对应物**（不是漏做，是无物可做）；「每轮 fix 开始/结束」在四个非 change 循环里都有对应物（planning 的回注重查、coding/testing 的打回重做、doc-from-zero 的单元重写、doc-fix 的 findings 回注）→ 一律覆盖。
- [自决] **Q13 · 墓碑写入的失败语义与 HOME 缺失降级** —— 墓碑写入是 **best-effort**：①hook 内**绝不** panic（写失败只记一行 stderr；hook 里二次 panic 会 abort，连 stderr 链回都丢 → R20）；②无论写成功与否，hook **都**链回上一个 hook（默认 stderr 输出不丢；安装接口要能取回上一个 hook 供单测还原）；③HOME 未设 / root 不可写 / 磁盘满 → **降级**：不写墓碑、不 panic、照常链回。**库路径不经过 `src/main.rs:143` 的 HOME 检查**（`tests/f6_observability/main.rs` 直调 hook 安装接口）→ hook 自己必须能承受 HOME 缺失（root 解析失败时用占位 run id + 不落盘）。

---

## 2 影响面

> 说明：本节每条都带 `path:line`；**三个新增文件尚不存在**（`src/observability/mod.rs`、`src/observability/heartbeat.rs`、`src/observability/tombstone.rs`），没有既有行号可指，按「文件名（**新增** · 全文）」记（机械门会核对引用存在性，`src/change/plan_audit.rs:253`）。

### 2.1 直接命中

- `src/engine/retry.rs:68`–`:106` —— `run_with_retry`：需求 1 的**唯一落点**。现状：失败只进内存 `log`（`:81`），决定重启前什么都不落盘（`:83`–`:97`），退避在 `:98`–`:103`。改后：每次尝试失败先落一条 ERROR（含 source 链），再决定重试。
- `src/engine/retry.rs:60`–`:62` —— `is_infra_failure`：决定「这次失败会不会被自动重启」的总开关（只认裸 `System`）。本靶**不改它的判据**，但 Q8 要改「profile 未就绪」这一处错误的**种类**（`System` → `Config`）。
- `src/server/mod.rs:253`–`:262` —— `require_profile`：boot 根因的落点。判据（`settings.json` 存在，`:255`–`:256`）与错误种类（裸 `System`，`:257`）都要动；错误**文本**保持（Q8）。
- `src/server/mod.rs:1249`–`:1281` —— `run_in_background` 的重试闭包：boot 期 WARN 在 `:1264`–`:1270`（**不带错误原文**）；run 级里程碑与心跳的落点（run 开始 / 重试 / 结束）。
- `src/server/mod.rs:993`–`:1014` —— `map_intervention`：**只作用于 SOP/编排执行期**的错误（调用点 `src/server/mod.rs:319`/`:393`/`:784`/`:922`/`:988` 与 `src/server/change_engine.rs:240`，全部挂在 `sop_result` 的 Err 上）：Quality → 明确 Failed（不重启），其余原样上抛（`:1012` 的 `Err(e)`）→ 重试。**启动门卫的失败不经它**（见下条）→ 本靶**不改判定**，但 Q8 改了「profile 未就绪」的错误种类后，它只影响编排期错误；boot 期失败直接走 `run_with_retry`（`:1249`）。
- `src/server/change_engine.rs:79` —— change SOP 的**启动门卫**（`for p in CHANGE_PROFILES { self.require_profile(p)?; }`）：boot 期失败从**这里**直接上抛（**不经** `map_intervention`——它在 watchdog/编排之前 `?` 出去）→ 直达 `run_with_retry`（`src/server/mod.rs:1249`）→ `is_infra_failure`（`src/engine/retry.rs:60`）认裸 `System` → 退避 10s → attempt=2（**这正是 boot 六连的触发点**）。本靶**不改这个文件**（错误种类在 `src/server/mod.rs:253` 改；闸门所需清单在 `src/server/mod.rs` 的统一表里取）→ 但**渲染闸门必须覆盖这六个 profile**（清单事实源 `src/server/change_engine.rs:24`）。
- `src/server/mod.rs:1021`–`:1044` —— `ServerState`：渲染闸门状态（新字段）的落点。
- `src/server/handlers.rs:365`–`:402` —— `put_manifest`：渲染闸门的**置位点**（当前 `:383` 起后台线程、`:390` 渲染、`:392`/`:395` 发 SSE）。
- `src/server/handlers.rs:202`–`:246` —— `start_task`：渲染闸门的**等待点**（当前 `:237` 直接 spawn run 线程；闸门等待窗口与串行槽快判的关系见 R19）。
- `src/server/mod.rs:268`–`:269`、`:326`–`:327`、`:400`–`:401`、`:809`–`:810`、`:929`–`:930` —— 其余五个 SOP 的既有门卫清单（planning 两角色、coding 两角色、doc-from-zero 两角色、testing 两角色、doc-fix 两角色）；`src/server/change_engine.rs:24`–`:33` —— `CHANGE_PROFILES`（change SOP 的六 profile 清单）。**闸门所需清单的统一入口定在 `src/server/mod.rs`**（一张 per-SOP 的 profile 表，如 `profiles_for_sop(sop)`）：六个 SOP 各自的 `require_profile` 调用改为查这张表 → 门卫与闸门读**同一张表**（不在别处再抄一份），change 一项**引用** `CHANGE_PROFILES`（那个文件本身不改，见 §3.2）。
- `src/main.rs:143`–`:146` —— HOME 根与日志初始化：panic hook 的**安装点**（进程启动即装，覆盖整个进程生命周期）。
- `src/lib.rs:8`–`:17` —— 模块清单：挂载新模块 `observability`。
- `src/engine/client.rs:186`、`:210` —— `wait_settled` / `wait_settled_with_ui`：**agent 轮次结束**里程碑的唯一落点（`spec.role` 现成，`:43`）。
- `src/change/orchestrator.rs:141`–`:176` —— plan 段与 fix 循环：阶段转换（`:160`，Plan→Coding）、fix 开始/结束（plan 修复轮 `:144`–`:175`）的里程碑与心跳落点。
- `src/change/orchestrator.rs:179`–`:236` —— coding ⇄ plan 往返段：阶段转换（`:187`，Coding→Testing）、补边界往返的 fix 起止（`:200`–`:230`）的里程碑（`:219` 是 `save(...)`，**不是**阶段写入）。
- `src/change/orchestrator.rs:238`–`:296` —— testing 段与回注：阶段转换（`:257` Testing→Done、`:278` Testing→Coding、`:286` Testing→Plan）、fix 起止（testing→coding 回注 `:263`–`:280`）的里程碑（`:265` 是超限分支的 `save(...)`，该分支**阶段未变** → **不**落 `phase.change`，只落 fix 事件）。
- `src/change/session.rs:232`–`:244` —— `review_once`（change 三角色共用的审查入口）：「审查开庭」里程碑的落点（含 `role` 与 `round`）。
- `src/change/session.rs:248`–`:283` —— `run_plan_with`：plan agent 交卷（`:249`）与 plan 审查（`:250`–`:283`）的里程碑。
- `src/change/session.rs:328`–`:376` —— `run_coding`：基线轮交卷（`:335`）、基线门（`:338`）、自证门（`:354`）、步提交对账（`:357`）、coding 轮交卷与审查的里程碑。
- `src/change/session.rs:378`–`:470` —— `run_testing`：test agent 轮次（`:383`）、test 审查的里程碑。
- `src/sop/planning.rs:95`–`:150` —— MVP planning 循环：plan 阶段推进（`:99`–`:104`）、审查开庭（`:114`）、判负回注（`:133`–`:145`）的里程碑。
- `src/sop/coding.rs:114`–`:190` —— MVP coding 循环：stage 推进（`:130`–`:167`）、审查开庭（`:149`）、打回（`:159`–`:170`）的里程碑。
- `src/sop/testing.rs:133`–`:235` —— MVP testing 循环：阶段推进（`:170`、`:210`/`:214`）、审查开庭（`:189`）、doc-fix 移交（`:221`）的里程碑。
- `src/sop/doc_from_zero.rs:269`–`:407` —— doc 单元循环：单元推进（`:269`–`:293`）、审查开庭（`:317`）、结论（`:360`）、**单元审查 FAIL → 回注 agent 重写**（`:397`–`:407`，fix 往返的落点）的里程碑。
- `src/sop/doc_fix.rs:75`–`:130` —— doc-fix：agent 轮（`:86`）、审查开庭（`:92`）、回注（`:123`）的里程碑。
- `src/sop/review_gate.rs:78`–`:146` —— `review_with_gate`（全仓共用的审查门）：**不改**（三通道机制与上限常量不动），但它的**生产 4 处调用点**是「审查开庭」里程碑的落点（change 侧经 `review_once` 展开为 3 个落点，见 Q5）。
- `tests/change_ask_ring.rs:31`、`:35`、`:615`–`:618`、`:653`–`:661` —— 白名单守卫常量与 `#[ignore]`：F10 例行刷新的落点（Q9）。
- `tests/change_failure_context/main.rs:32`、`:36`、`:950`（a16） —— 守卫常量的孪生副本与比对用例（a16）：必须与守卫逐字一致。
- `tests/change_review_protocol/main.rs:44`、`:46`、`:71`、`:1315`、`:1374`、`:1393` —— 上一场验收文件的第三份常量副本与三条对账用例（a29/a30/a31）：必须与守卫逐字一致。
- `docs/plantree/plans/010-run-resume/decisions/README.md:43`、`:51` —— 决策台账末尾（D005 之后）：追加 `## D006 ·`。
- `docs/plantree/plans/010-run-resume/roadmap.md:58` —— roadmap 末尾：追加本场条目。
- `src/observability/mod.rs`（**新增** · 全文） —— 可观测性模块出口：里程碑/心跳/墓碑的公共面 + 进程级「当前 run 上下文」（run id / 阶段 / 步骤 / attempt / root）+ **root 解析**（Q3：`DEVFORGE_HOME` 环境变量 > 进程内显式注入（单测用 setter）> `HOME/.devforge` 生产默认）→ 单测注入临时目录，**不落用户 HOME**（R13）。
- `src/observability/heartbeat.rs`（**新增** · 全文） —— 心跳文件读写（阶段 / 步骤 / 时间戳 / attempt / run id；落点 = root 下 `heartbeat/<runid>.json`，root 由 `src/observability/mod.rs` 解析，Q3）；**覆盖写**（幂等收敛）；无活动 run 时**不写**（run 上下文未置位即不写）——**但仓内单测会置位**（`src/server/tests/mod.rs:145` 的 fixture 与 `:668` 起的 start 用例走真实 start 路径 → `src/server/handlers.rs:237` → `src/server/mod.rs:1249`）→ **root 注入**才是「不落用户 HOME」的关键（R13）。
- `src/observability/tombstone.rs`（**新增** · 全文） —— panic hook 安装与墓碑写入（panic 原文 / 位置 / 当时阶段 / 时间戳 / run id；落点 = root 下 `tombstones/<ts>-<runid>.txt`）；**写入 best-effort**（Q13：写失败只记一行 stderr，**绝不**在 hook 里 panic；HOME 缺失 / root 不可写 → 降级不写、不 panic）；**链回上一个 hook**（默认 stderr 输出不丢）；安装接口要能取回上一个 hook 供单测还原。

### 2.2 调用链

- **boot 六连全链（根因链，逐跳带位置）**：`POST /start`（`src/server/handlers.rs:202`）→ `run_in_background`（`src/server/mod.rs:1207`）→ `RunEngine::execute`（`src/server/mod.rs:233`）→ `run_change`（`src/server/change_engine.rs:66`）→ **change 启动门卫**（`src/server/change_engine.rs:79` → `require_profile` `src/server/mod.rs:253`）→ 裸 `System`（`src/server/mod.rs:257`）→ **不经 `map_intervention`**（门卫的 `?` 在 watchdog/编排之前直接上抛；`map_intervention` 只管 `sop_result` 的 Err，调用点 `src/server/change_engine.rs:240` 与 `src/server/mod.rs:319`/`:393`/`:784`/`:922`/`:988`）→ 直达 `run_with_retry`（`src/server/mod.rs:1249`）→ `is_infra_failure`（`src/engine/retry.rs:60`）→ 退避 10s（`src/engine/retry.rs:22`、`:98`）→ attempt=2 的 WARN（`src/server/mod.rs:1264`）。**其余五个 SOP 同构**（门卫的 `?` 也在 watchdog 之前，如 planning `src/server/mod.rs:268`–`:269`）→ 同样直达重试层。**竞态的另一端**：`put_manifest`（`src/server/handlers.rs:365`）→ 后台渲染线程（`:383`）→ `render_all`（`src/config/render.rs:222`）→ `render_agent`（`:38`）：先建目录+`AGENTS.md`（`:50`），后写 `settings.json`（`:63`）→ 装包（`:205`）。**Q8 之后**：门卫失败是 `Config` → `is_infra_failure` 不认 → **不重启** → 由 `src/server/mod.rs:1293` 以错误原文记 Failed。
- **渲染闸门的新链（用户裁决 A）**：`start_task`（`src/server/handlers.rs:202`）→ 查本 run 所需 profile（清单的唯一事实源：`src/server/change_engine.rs:24` 的 `CHANGE_PROFILES` 与各 SOP 的既有门卫调用点）→ 若有渲染在途 → 等「所需 profile 的 `settings.json` 落盘」或「渲染结束（成功/失败）」→ 再 spawn run（`:237`）→ `require_profile` 复查（`src/server/mod.rs:253`）→ 仍缺 → `Config` 明确失败（`:257`，不再重启）。
- **原始错误可见的新链**：run 失败 → `run_with_retry`（`src/engine/retry.rs:79`）→ 每次尝试失败落一条**分级**条目（质量类失败 WARN、其余 ERROR，见 Q7 的用户裁决；`src/engine/retry.rs:81` 附近）→ `tracing` → `src/logging.rs:33` 的 JSONL 层 → `HOME/.devforge/logs/devforge.log.jsonl`。
- **里程碑链**：各 SOP 循环 / 编排层 → 内联 `tracing::info!`（调用点即 target）→ `src/logging.rs:33`–`:52` → `HOME/.devforge/logs/devforge.log.jsonl`。
- **审查链（8 个「审查开庭」落点）**：`src/change/session.rs:270`/`:361`/`:429`（`review_once` → change 三角色）、`src/sop/planning.rs:114`、`src/sop/coding.rs:149`、`src/sop/testing.rs:189` → 全部经 `review_with_gate`（`src/sop/review_gate.rs:78`，**生产 4 处调用点**）；`src/sop/doc_from_zero.rs:317`、`src/sop/doc_fix.rs:92` → 直连 `factory.open()`（**不走**共享门）。**轮次号口径**：一律取「该角色在本次 run 内的审查序号」（调用前可知，Q5 的表）——共享门返回的实例数 `opened`（`src/sop/review_gate.rs:130`）是**返回值**，调用前拿不到，故不进本事件。
- **agent 轮次链**：`src/sop/rpc_bridge.rs:24`（`RpcPlanSession::converse`）/`:76`（`RpcReviewSession::review`）→ `src/engine/client.rs:186`/`:210`（`wait_settled*`）→ 里程碑落点（角色 = `src/engine/client.rs:43` 的 `spec.role`）。**一处覆盖全部 SOP**（change 六角色 / MVP 六角色 / doc 三角色都走这条链）。
- **心跳链**：编排层与引擎 → `observability` 的 run 上下文（阶段/步骤）→ `src/observability/heartbeat.rs` → root 下 `heartbeat/<runid>.json`（root 解析顺序见 Q3：env > 注入 > `HOME/.devforge`）；低频兜底刷新挂在**既有 watchdog 的轮询线程**上（`src/server/mod.rs:1361`–`:1420`；**六个 SOP 全覆盖**：planning `src/server/mod.rs:298`、coding `:368`、doc-from-zero `:639`、testing `:852`、doc-fix `:967`、**change `src/server/change_engine.rs:203`**；该线程每 300ms 一跳，与 `idle_ms` 无关）→ **不新增线程**。
- **墓碑链**：`std::panic` → `src/observability/tombstone.rs` 的 hook（进程启动时装在 `src/main.rs:143` 之后）→ 读 run 上下文取「当时阶段/步骤」→ 写 root 下 `tombstones/<ts>-<runid>.txt` → hook 内再链回上一个 hook（默认 stderr 输出不丢）。**写入 best-effort**（Q13）：写失败 / 无 HOME → 只记一行 stderr，不 panic，照常链回。

### 2.3 隐式耦合

- **错误种类 = 「会不会被自动重启」的总开关**：`src/engine/retry.rs:60` 只认裸 `System`；`src/server/mod.rs:993` 把 Quality 变成明确的 Failed（不重启），其余原样上抛 → 重启（`src/server/mod.rs:1249`）。**同一句失败文案，错误种类选错 = 一次整 run 重启**（本靶两处都动这个分岔：Q8 把「profile 未就绪」挪出 System；需求 1 只加可见性、**不改**分类）。
- **渲染的「完成」不是同步事件**：`put_manifest` 返回 202 时渲染还在跑（`src/server/handlers.rs:383`），完成信号只经 SSE 广播（`src/server/handlers.rs:392`/`:395`，给前端看的）→ **闸门必须自己持有一份「在途」状态**，不能靠 SSE 事件流。
- **门卫判据是「文件存在」而不是「profile 可用」**：`settings.json` 在每个 profile 渲染的**早期**就落盘（`src/config/render.rs:63`，装包之前），装包期间还可能被安装器回写（`:192`）→ 闸门等「所需 profile 的 `settings.json` 落盘」是**秒级**；等「整个渲染任务结束」是**分钟级**（装包实测见 `docs/plantree/plans/002-config-management` 的 CAL-19-RENDER 记录）→ 闸门等前者，否则 `start` 端点会变成一个分钟级阻塞点（e2e 的 start 请求默认超时只有 10s，`tests/e2e/common.py:132`）。
- **`tracing` 的 `target` = 发出宏的模块**（不是包装函数的模块）→ 里程碑必须内联写（Q4）。这同时意味着「一处实现」在里程碑上不可行，只能在**事件名**上统一口径。
- **既有日志的文案不能与新里程碑混淆**：既有 WARN 都带 `component`（`src/server/mod.rs:1264`、`src/sop/review_gate.rs:139`、`:163`、`src/server/mod.rs:1400`、`:1408`）→ 新里程碑一律用**新 event 名**（Q4 的八个），且事件名不得复用既有文案。
- **`review_with_gate` 全仓 11 处调用点**（生产 4：planning/coding/testing/change-session；仓内单测 5：仓内共享门自己的用例；仓外验收文件 2：`tests/change_review_protocol/main.rs` 的直调）→ **不改它的签名**（Q5）；角色由调用点自己写。
- **不给 `ProdChangeSession` 加字段**：上一场加 `plan_review_instances` 时踩过「全仓字面量构造点必须同步补」的编译期耦合（生产装配 `src/server/change_engine.rs:180`、仓内单测、两份仓外夹具）→ 本靶的轮次信息走**已有的** `roundtrip` 参数（`src/change/session.rs:290`、`:328`），不改结构体。
- **闸门的等待发生在 axum 的 async handler 里**：`start_task`（`src/server/handlers.rs:202`）是 `async fn`（在 tokio worker 上执行）→ 闸门若在 handler 里**阻塞**等待渲染，会占住 tokio worker 线程（并发请求被拖住；worker 数有限 → 拖满即全站假死）。等待必须用**可 await 的原语**（如 `tokio::sync::Notify` / `watch`），或整体挪进 `spawn_blocking`；**不得**用 `std::thread::sleep` 轮询。（R18）
- **`review_with_gate` 的调用点计数**：生产 4 处 + 仓内单测 5 处 + 仓外验收文件 2 处 = 全仓 11 处（事实 #14）→ 改它的签名会波及上一场的验收面（本靶不改它，见 Q5）。
- **`src/change/coding_gate.rs` 不得进本变更**：白名单守卫断言「上一场遗留的两个文件不得进本变更 diff」（`tests/change_ask_ring.rs:653`–`:661`）→ 基线门/自证门/步提交对账的里程碑落在**调用点**（`src/change/session.rs:338`/`:354`/`:357`），门本身不动。
- **心跳是覆盖写**（纪律第 12 条：状态文件要能被覆盖写，不能追加成第二份真相）→ 心跳只留「最后一次状态」；「本 run 发生过几次阶段转换」由里程碑日志回答，**不**靠心跳。
- **run 上下文是进程级单例**：MVP 是串行执行（`src/server/handlers.rs:220`–`:226` 的 `has_running_run` 守卫）→ 同一时刻只有一个 run；将来并发化时心跳/墓碑会互相覆盖（登记为遗留，R8）。
- **panic hook 是进程级全局**（`std::panic::set_hook` 只能有一个）→ 仓内单测若装 hook 会影响同进程其他测试 → 验收测试放**独立测试二进制**（`tests/f6_observability/main.rs`），且安装接口要能取回上一个 hook 以便还原。
- **tracing 的落盘不是零丢失**：`src/logging.rs:38`–`:44` 的文件层是 `Mutex<File>` 同步写，但进程猝死（SIGKILL）时缓冲区里的最后几条可能丢 → **里程碑日志不保证零丢失**（它是「过程记录」），墓碑与心跳才是「死亡现场」的硬证据。
- **计划文本的机械门**：六块名固定（`src/change/plan_audit.rs:12`–`:20`）；白名单里的路径按**反引号**抽取（`:336`–`:352`）→ 新文件必须用反引号写进白名单（否则不算白名单 → coding 步提交它 = 越界）；`path:line` 引用逐条验存在性（`:253`）→ 新文件**不能**带 `:行号`；验收标准只认**列表行**（`:365`–`:378`）→ §5 禁用表格。
- **三份守卫常量副本互相比对**（事实 #25）→ 本场刷新必须**三份一起刷**，漏一份即红（a29/a30/a16）。
- **两处硬编码断言会因本场的白名单口径而变红**（Q9）：`tests/change_review_protocol/main.rs:1333`–`:1336`（a29）硬编码「守卫 WHITELIST 里**不得**含 `tests/change_review_protocol/main.rs`」、`:1431`–`:1443`（a32 `a32_the_plan_whitelist_has_twenty_files`）硬编码 `PLAN_WHITELIST.len() == 20` / `CODING_WHITELIST.len() == 19` 且断言「§4 白名单含**唯一的**测试阶段文件 `tests/change_review_protocol/main.rs`」→ 本场口径是「coding 步 21 个（含 `tests/change_review_protocol/main.rs`）+ 测试阶段 1 个（`tests/f6_observability/main.rs`）」→ **这两处必须改写**（排除项换成 `tests/f6_observability/main.rs`、计数改 22/21、测试阶段文件断言换人），否则 step 1 之后测试面直接红。
- **仓内单测会真的落盘**：单测里 `src/server/tests/mod.rs:145` 的 fixture 构造 router、`:668` 起的 `start_task_runs_engine_and_records` 走**真实 start 路径** → `src/server/handlers.rs:237` → `src/server/mod.rs:1249` → 置位 run 上下文 → 心跳**真写**进 root（run id 唯一、run 结束按 Q3 保留）→ 若 root 默认 `HOME/.devforge`，用户的 HOME 会被单测写脏 → **root 必须可注入**（Q3 的 `DEVFORGE_HOME` / 显式 setter），单测指向临时目录（R13）。
- **panic hook 里二次 panic = abort**：写墓碑失败若在 hook 里 panic，会在 panic 中 panic → 进程 abort，**连 stderr 链回都丢**（最后一条遗言也没了）→ 写入必须 best-effort（Q13）、hook 必须无条件链回上一个 hook。**库路径不经过 `src/main.rs:143` 的 HOME 检查**（`tests/f6_observability/main.rs` 直调安装接口）→ hook 自己要能承受 HOME 缺失。
- **`docs/plantree/ideas/selfhost-findings-20261021.md` 由操作员维护**（文件头自述）→ 本靶不写（F6 状态更新由操作员做）。

---

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `src/observability/mod.rs`（**新增**） | 可观测性模块出口：①**进程级 run 上下文**（run id / 阶段 / 步骤 / attempt / root 路径）的读写接口（run 开始时置、结束时清；阶段与步骤由编排层更新）——心跳与墓碑都从这里取「当时是谁、在哪个阶段」；②**root 解析**（Q3：`DEVFORGE_HOME` 环境变量 > 进程内显式注入（单测 setter）> `HOME/.devforge` 生产默认）→ 单测注入临时目录，**不落用户 HOME**（R13）；③里程碑与心跳的**公共面**（不封装 `tracing::info!` 的包装函数，见 Q4，但公共的字段词汇与「上下文更新 → 顺手刷心跳」的动作放这里，避免每个调用点各写一遍心跳） | 需求 2、3 |
| `src/observability/heartbeat.rs`（**新增**） | 心跳文件读写：落点为 root 下 `heartbeat/` 目录里的**单文件**（文件名 = run id，内容为结构化 JSON；root 由 `src/observability/mod.rs` 解析，Q3）；内容含 **run id / 阶段 / 步骤 / 时间戳 / attempt**（Q3）；**覆盖写**（幂等收敛）；无活动 run 时**不写**（run 上下文未置位即不写）——**但仓内单测会置位**（`src/server/tests/mod.rs:145` 的 fixture、`:668` 起的 start 用例走真实 start 路径）→ **root 注入**才是「不落用户 HOME」的关键（R13） | 需求 3b、3c |
| `src/observability/tombstone.rs`（**新增**） | panic hook：安装（进程启动时，`src/main.rs`）→ panic 时写 root 下 `tombstones/<ts>-<runid>.txt`，内容含 **panic 原文 + panic 位置（file:line）+ 当时阶段/步骤 + 时间戳 + run id**（无活动 run 时 run id 用占位值，不丢证据）；**写入 best-effort**（Q13：写失败只记一行 stderr，**绝不**在 hook 里 panic——二次 panic 会 abort；HOME 缺失 / root 不可写 → 降级不写、不 panic）；**链回上一个 hook**（默认 stderr 输出不丢）；安装接口要能取回上一个 hook 供单测还原 | 需求 3a、3c |
| `src/lib.rs` | 挂载新模块 `pub mod observability;`（模块清单，`:8`–`:17`） | 需求 2、3 |
| `src/main.rs` | 进程启动时安装 panic hook，并**设定 root**（`HOME/.devforge`，与日志目录同根，`:143`–`:146`；`DEVFORGE_HOME` 可覆盖，Q3） | 需求 3a |
| `src/engine/retry.rs` | ①每次尝试失败 → 落一条**分级**日志（Q7）：质量类失败（`Quality`）落 **WARN**、其余失败落 **ERROR**，内容均为原始错误全文 + source 链（在决定重试**之前**，改 `:79`–`:83` 附近）；②终态失败（4 次耗尽）→ 再落一条同级的终态条目（含最终错误全文 + attempts）。**不改**重试策略（`:18`/`:22`）、**不改** `is_infra_failure` 的判据（`:60`） | 需求 1；Q7 |
| `src/engine/client.rs` | agent 轮次结束里程碑（`wait_settled` `:186` / `wait_settled_with_ui` `:210` 返回处）：字段 `component="engine"`、`event="agent.turn_end"`、`role`（`spec.role`）、`turn_ms`、`chars`。**一处覆盖全部 SOP**（Q6） | 需求 2 |
| `src/server/mod.rs` | ①`require_profile`（`:253`–`:262`）：错误种类 `System` → `Config`（Q8），**错误文本一字不改**（原文已含处置建议）；②**per-SOP 的 profile 统一入口**（新增一张表，如 `profiles_for_sop(sop)`）：六个 SOP 现有的 `require_profile` 调用（`:268`–`:269`/`:326`–`:327`/`:400`–`:401`/`:809`–`:810`/`:929`–`:930`）改为查这张表 → 门卫与闸门读**同一张表**（change 一项引用 `src/server/change_engine.rs:24` 的 `CHANGE_PROFILES`）；③`ServerState`（`:1021`–`:1044`）：加**渲染闸门状态**（在途计数 + 可 await 的完成通知）；④`run_in_background`（`:1207`–`:1281`）：run 级里程碑与心跳（run 开始 / 每次重试 / run 结束），并把**当前 run 上下文**置位/清位 | 需求 1、2、3b；裁决 A |
| `src/server/handlers.rs` | ①`put_manifest`（`:365`–`:402`）：后台渲染线程**登记渲染在途**（闸门置位），渲染结束（成功/失败）清位并通知；②`start_task`（`:202`–`:246`）：**开跑前等闸门**——等「本 run 所需 profile 的设置文件落盘」或「渲染结束」，再 spawn run（`:237`）；等待必须用**可 await 的原语**（不得在 tokio worker 上阻塞，见 §2.3 / R18）。所需 profile 清单读 `src/server/mod.rs` 的**统一入口表**（不在本文件另抄一份） | 需求 1（根因修复）；裁决 A |
| `src/change/orchestrator.rs` | 阶段转换与 fix 往返的里程碑 + 心跳：**5 处**阶段写入（`:160`/`:187`/`:257`/`:278`/`:286`）各落一条 `event="phase.change"`（含 `from`/`to`；`:219`/`:265` 是 `save(...)` 不是阶段写入，`:265` 所在超限分支阶段未变 → **不**落 phase.change）；plan 修复轮（`:144`–`:175`）、补边界往返（`:200`–`:230`）、testing→coding 回注（`:263`–`:280`）各落 `event="fix.start"` / `event="fix.end"`（含轮次） | 需求 2、3b |
| `src/change/session.rs` | ①「审查开庭」里程碑（`review_once` `:232`–`:244`，三角色共用 → 角色名经调用点传入；`round` = **该角色在本次 run 内的审查序号，调用前可知**——共享门返回的实例数 `opened`（`src/sop/review_gate.rs:130`）是返回值，调用前拿不到，故不进本事件；口径表见 Q5）；②agent 交卷里程碑（`:249`/`:335`/`:383` 等，change 侧的阶段内动作）；③三道机械门的**结果**里程碑（基线门 `:338`、自证门 `:354`、步提交对账 `:357` —— **门本身不改**，见 §2.3：白名单守卫钉住那三个文件不得进本变更） | 需求 2、3b |
| `src/sop/planning.rs` | MVP planning 循环的里程碑：plan 阶段推进（`:99`–`:104`）、审查开庭（`:114`，含轮次）、判负回注 fix 起止（`:133`–`:145`） | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/coding.rs` | MVP coding 循环的里程碑：stage 推进（`:130`–`:167`）、审查开庭（`:149`，含轮次）、打回 fix 起止（`:159`–`:170`） | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/testing.rs` | MVP testing 循环的里程碑：阶段推进（`:170`、`:210`/`:214`）、审查开庭（`:189`，含轮次）、doc-fix 移交（`:221`）与回注 fix 起止（`:216`–`:230`） | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/doc_from_zero.rs` | doc 单元循环的里程碑：单元推进（`:269`–`:293`）、审查开庭（`:317`，含轮次）、**单元审查 FAIL → 回注 agent 重写**（`:397`–`:407`）的 fix 起止 | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/doc_fix.rs` | doc-fix 的里程碑：审查开庭（`:92`，含轮次）、回注 fix 起止（`:123`） | 需求 2；裁决「全 SOP 全覆盖」 |
| `tests/f6_observability/main.rs`（**新增**） | 本靶的**验收对账测试**（黑盒口径，仿 `tests/change_review_protocol/main.rs` 的风格：只经公开 API 观察，每条用例首行写「错实现形态」）：逐条对账 §5；含「注入 panic → 墓碑存在且含 panic 原文」与「阶段推进 → 心跳内容变化」两条硬验收 | 需求 3c；需求 1、2 |
| `tests/change_ask_ring.rs` | F10 例行刷新（Q9）：`BASELINE`（`:31`）刷成本场基线、`WHITELIST`（`:35`）刷成本场 **coding 步白名单**；step 1 摘除 `#[ignore]`（`:618`）、本场收尾复贴；`tests/change_ask_ring.rs:653`–`:661` 的「上一场遗留文件不得进本变更」断言**保持有效**（本靶不改上一场遗留的那两个文件，见 §2.3） | 需求 4（边界口径） |
| `tests/change_failure_context/main.rs` | 守卫常量的孪生副本：同步刷 `BASELINE`（`:32`）/ `WHITELIST`（`:36`）→ a16（`:950`）的比对才绿 | 需求 4（边界口径） |
| `tests/change_review_protocol/main.rs` | ①第三份常量副本：同步刷 `BASELINE`（`:44`）/ `PLAN_WHITELIST`（`:46`）/ `CODING_WHITELIST`（`:71`）→ a29/a30/a31（`:1315`/`:1374`/`:1393`）才绿；②**改写两处硬编码断言**（Q9，否则 step 1 之后必红）：a29 的排除项（`:1333`–`:1336`）从 `tests/change_review_protocol/main.rs` 换成**本场测试阶段文件** `tests/f6_observability/main.rs`（或泛化）；a32 `a32_the_plan_whitelist_has_twenty_files`（`:1431`–`:1443`）的计数 `20`/`19` 改成 **`22`/`21`**，且「§4 白名单含唯一的测试阶段文件」的断言换人 | 需求 4（边界口径）；Q9 |
| `docs/plantree/plans/010-run-resume/decisions/README.md` | 追加 `## D006 ·`（Q10）：登记本靶三件交付物（原始错误可见 / 里程碑 / 死亡取证）+ 渲染闸门口径（用户裁决：A 渲染闸门）+ 「原始错误落盘分级：基础 ERROR / 质量 WARN」（用户裁决：Q7）+ 「profile 未就绪 = 配置错误不是基础设施故障」的分类修正 + 遗留清单。**只追加、不动 D001–D005** | 需求 1、2、3 |
| `docs/plantree/plans/010-run-resume/roadmap.md` | 追加本场条目（三件交付物 + 根因修复 + 验收证据位置） | 需求 1、2、3 |

### 3.2 受影响不改

| 文件 / 位置 | 为什么可以不改 |
| --- | --- |
| `src/change/coding_gate.rs` | 三道机械门（基线门 `:87`、自证门 `:113`、步提交对账 `:256`）的**判定**不变；里程碑落在调用点（`src/change/session.rs:338`/`:354`/`:357`）。且白名单守卫断言本变更不得动它（`tests/change_ask_ring.rs:653`–`:661`） |
| `src/sop/review_gate.rs:78`–`:146` | 审查三通道机制与两个上限常量不动（Q5：改签名会波及全仓 11 处调用点与上一场验收测试）；「审查开庭」里程碑落在 8 个调用点 |
| `src/engine/retry.rs:18`、`:22`、`:60` | 重试次数、退避档位、`is_infra_failure` 的判据全不动（本靶只加可见性 + 修掉「profile 未就绪」这一处错误种类） |
| `src/config/render.rs` | 渲染器本身不动（写 `settings.json` 的次序不动）；闸门等的是「文件已落盘」这个既有事实，不改渲染语义 |
| `src/server/change_engine.rs:24`–`:33`、`:79` | `CHANGE_PROFILES`（change SOP 的六 profile 清单）是闸门所需清单的**事实源**；`:79` 是 change 的**启动门卫**（`for p in CHANGE_PROFILES { self.require_profile(p)?; }`）——boot 六连的触发点。两者都**只被引用/被观察**（错误种类在 `src/server/mod.rs:253` 改，闸门清单在 `src/server/mod.rs` 的统一表里取）→ 这个文件**不改**、**不进白名单** |
| `src/config/manifest.rs` | 角色提示词不动（本靶不碰任何 agent 契约）→ e2e 夹具 `tests/e2e/fixtures/change_agents.json` **不用重生成** |
| `src/change/state.rs` | 断点结构不动（不给 `ProdChangeSession` 加字段 → 不触发全仓字面量构造点的编译期耦合，见 §2.3） |
| `src/change/prepare.rs`、`src/change/gate0.rs` | 门 0（change-id / 文档档位 / 基线）的语义不动 |
| `src/sop/doc_protocol.rs`、`src/sop/testing_protocol.rs` | doc 侧协议解析不动（本靶只在 doc 循环加里程碑） |
| `src/sop/coding_protocol.rs`、`src/sop/output.rs` | 协议解析不动 |
| `src/server/tests/mod.rs:524`–`:540` | 既有源码守卫（协议违例上限 → Quality、实例失败上限 → System）不动：本靶不动那三处文案，守卫仍绿 |
| `src/server/mod.rs:1745`–`:1800`（profile 缺失的 4 条既有单测） | 错误**文本**不变（Q8 只改种类）→ 这 4 条断言继续绿 |
| `docs/plantree/plans/009-change-sop/**` | 本靶不改 change SOP 的业务语义 → 009 的 topic 文档与代码无冲突，既有守卫（`src/change/plan_audit.rs:711`–`:757`、`src/change/coding_gate.rs:490`–`:502`）继续绿 |
| `docs/plantree/ideas/selfhost-findings-20261021.md` | 该文件按其自述「由操作员（江晚棠会话）维护」→ F6 的状态更新由操作员做（Q10） |
| `tests/e2e/**` | 不新增/不改 e2e：真链是否复现 boot 竞态取决于渲染时序，断言押在时序上 = 假红。既有 e2e 的行为不回归即可（`tests/e2e/test_10_timeout_async_render.py` 已等 `config.rendered` 再 start；`tests/e2e/test_12_sop_override.py`、`tests/e2e/test_18_sop_registry.py` 不 PUT 清单 → 闸门不触发） |
| `assets/skills/coding-discipline/SKILL.md` | 本变更**遵守**它，不改它 |
| `Cargo.toml` / `Cargo.lock` | 不引入新外部依赖（心跳/墓碑只用 `std` + 既有 `tracing`） |

### 3.3 必须回归验证

| 模块 | 为什么风险在这里 |
| --- | --- |
| `src/engine/retry.rs` 的既有单测（`#[cfg(test)]` 在 `:108`，`mod tests {` 起于 `:109`） | 六条用例钉着既有口径：`two_infra_failures_then_success_settles_on_third_attempt`（尝试数与调用数）、`persistent_infra_failure_stops_at_four_attempts`（错误串含每次尝试末段 + 「共尝试 4 次」）、`quality_failure_is_not_retried`、`cancelled_run_is_not_retried`、`backoff_schedule_matches_decision`、`non_system_errors_are_not_infra`。加 ERROR 落盘**不得**改这些语义（尤其「错误串带每次尝试末段」这条） |
| `src/server/mod.rs:1361`–`:1420`（watchdog）、六个调用点（planning `:298`、coding `:368`、doc-from-zero `:639`、testing `:852`、doc-fix `:967`、**change `src/server/change_engine.rs:203`**）与 `:1745`–`:1800`（profile 缺失四条） | watchdog 是本靶「心跳低频兜底刷新」的挂载点（**六个 SOP 全覆盖，change 也不例外**；不得改其取消/超时/失活收割语义，也不得依赖 `idle_ms` 开启）；profile 缺失四条断言错误文本（Q8 只改种类 → 文本不变） |
| `src/server/tests/mod.rs` 全量（该文件共 **1087 行**——这是**行数**，不是用例数；用例逐条看 `:668` 起的 `start_task_runs_engine_and_records` 等） | 端到端跑真 run 路径的用例（`quality_failure_path_is_not_retried_end_to_end` 等）依赖「Quality 不重启 / System 才重启」的分岔 —— 本靶改了「profile 未就绪」的种类，必须确认没有别的用例依赖它被重启。**另**：这些用例会走真实 start 路径 → 置位 run 上下文 → 若 root 未注入，心跳会落进用户 HOME（R13）→ 单测必须把 root 指向临时目录 |
| `src/change/orchestrator.rs` 的既有单测（`#[cfg(test)]` 在 `:319`，`mod tests {` 起于 `:320`） | 编排层的行为（阶段推进、回注、往返计数、验收对账）不得因加里程碑而变；既有断言（如 `happy_path_plan_coding_testing_report` 的 `review_rounds == 2`）是回归闸门 |
| `src/change/session.rs` 的既有就地单测（`#[cfg(test)]` 在 `:468`，`mod tests {` 起于 `:469`） | 计划/编码/测试三条链的既有期望值（判负通道不变、门禁拦截点不变、审查轮次计数）不得回归 |
| `src/sop/planning.rs`、`src/sop/coding.rs`、`src/sop/testing.rs` 的既有单测 | 三个 MVP 循环的轮次/问题数/阶段完成的既有语义不得回归（本靶只在循环里加日志与心跳） |
| `src/sop/doc_from_zero.rs`、`src/sop/doc_fix.rs` 的既有单测 | doc 侧的单元推进与审查轮语义不得回归 |
| `src/sop/review_gate.rs` 的既有单测（`#[cfg(test)]` 在 `:172`，`mod tests {` 起于 `:173`） | 三通道机制与两个上限常量不动（Q5）——这五条是「共享门没被动过」的闸门 |
| `src/change/plan_audit.rs` 的既有守卫（`:711`–`:757`） | 读 009 的台账与 topic 文档；本靶不改那两份 → 必须仍绿 |
| `tests/change_ask_ring.rs:528`–`:546`（夹具漂移）、`:556`–`:575`（台账）、`:578`–`:597`（文档锚点）、`:619`–`:661`（白名单守卫） | 四条都是**读本靶要改的文件**的守卫：白名单常量必须刷新、上一场遗留文件不得进本变更、夹具/台账/文档锚点一字不动 |
| `tests/change_failure_context/main.rs`（a1–a17，尤其 a16（`:950`）/ a17（`:1022`）） | 本靶**刷新**它的 `BASELINE`/`WHITELIST`；a16 比对的就是守卫常量、a17 扫的是本场提交内容 → 这两条是本靶最容易漏的红 |
| `tests/change_review_protocol/main.rs`（a29/a30/a31 + **a32** + 上一场的 28 条协议用例） | 本靶**刷新**它的三份常量；a29/a30/a31 逐条对账本场；**a32（`:1431`–`:1443`）与 a29 的排除项（`:1333`–`:1336`）必须按 Q9 改写**（计数 20/19 → 22/21、排除项换成本场测试阶段文件），否则 step 1 之后必红；上一场的协议用例（审查三通道、尾部协议位）不得回归 |
| `tests/f6_observability/main.rs`（本靶新增） | 逐条对账 §5；每条用例首行写「错实现形态」 |
| 仓内单测（`cargo test`） | 本靶的新增/改写单测全在白名单文件内（新增的 `tests/f6_observability/main.rs` + 各文件既有 `#[cfg(test)]` 模块） |
| `tests/e2e/test_10_timeout_async_render.py`、`test_12_sop_override.py`、`test_18_sop_registry.py`、`test_25_fault_retry.py` | ①test_10 验「PUT 立即 202 + 后台渲染 + SSE 通知」——闸门**不得**把 PUT 变成阻塞；②test_12/test_18 验「profile 未渲染时 run 秒级 Failed、串行槽即时释放」——Q8 的错误种类改动不得让它变慢或变语义；③test_25 验「真实故障 → run 级自动重启 attempt=2」——重试策略没动，它必须仍绿 |
| `tests/e2e/test_26_change_e2e.py`、`test_27_change_ask_ring.py` | 真链：改了引擎（闸门 + 里程碑 + 心跳）后 change run 仍要 Settled（本靶不新增 e2e 断言，但要绿） |
| 引擎越界门（`src/change/boundary.rs:133` + `src/change/orchestrator.rs:183`） | §4 白名单必须覆盖**整个提交集合**（含 3 个新增文件 + 1 个新增测试文件 + 2 份文档），少写一个 = 差集非空 + 无 `<impact-gap>` 申报 = 「越界改动未申报（范围膨胀）」直接 Quality |
| 部署侧（不在仓内） | `~/.devforge/manifest.json` 缓存与 profile 重渲染（`src/main.rs:146`–`:152`）→ 仓内改完不等于用户态生效；心跳/墓碑的落点依赖 `HOME`（`src/main.rs:143`） |

---

## 4 白名单

- `src/observability/mod.rs`（**新增**）
- `src/observability/heartbeat.rs`（**新增**）
- `src/observability/tombstone.rs`（**新增**）
- `src/lib.rs`
- `src/main.rs`
- `src/engine/retry.rs`
- `src/engine/client.rs`
- `src/server/mod.rs`
- `src/server/handlers.rs`
- `src/change/orchestrator.rs`
- `src/change/session.rs`
- `src/sop/planning.rs`
- `src/sop/coding.rs`
- `src/sop/testing.rs`
- `src/sop/doc_from_zero.rs`
- `src/sop/doc_fix.rs`
- `tests/f6_observability/main.rs`（**新增**）
- `tests/change_ask_ring.rs`
- `tests/change_failure_context/main.rs`
- `tests/change_review_protocol/main.rs`
- `docs/plantree/plans/010-run-resume/decisions/README.md`
- `docs/plantree/plans/010-run-resume/roadmap.md`

白名单共 **22 个文件** = **coding 步的 21 个** + **测试阶段的 1 个**（`tests/f6_observability/main.rs`）。三层「白名单」口径不同、**不可互相替代**：

- **引擎的越界门**（`src/change/orchestrator.rs:183` → `src/change/boundary.rs:133` `classify`）拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:188` = 本变更**全部**提交文件，含测试阶段）比**本节清单** → 本节必须覆盖**整个提交集合**（22 个），少写一个 = 差集非空 + 无 `<impact-gap>` 申报 = 直接 Quality。
- **提交内容守卫的 `WHITELIST` 常量**（`tests/change_ask_ring.rs:35`，孪生 `tests/change_failure_context/main.rs:36`、上一场文件 `tests/change_review_protocol/main.rs:71`）守的是**coding 步提交**：提交信息含 ` step ` 的提交里每个文件都必须在常量内；**不含** ` step ` 的提交（测试阶段自己的提交）只许是 `tests/` 前缀的文件（`tests/change_ask_ring.rs:626`–`:632`）→ 本场常量列 **21 个 coding 步文件**（= 本节 22 个去掉 `tests/f6_observability/main.rs`），**不加**测试阶段文件（加了就把「白名单里的每个文件都要被本场 coding 步提交改到」打红）。三个新增文件（`src/observability/` 三件套）**属 coding 步文件，要**在常量里。
- **上一场验收测试文件的常量也在这条链上**：`tests/change_failure_context/main.rs:32`/`:36` 与 `tests/change_review_protocol/main.rs:44`/`:46`/`:71` 的常量与守卫文件的常量互相比对（a16/a29/a30）→ 本场刷守卫常量时**必须**同步刷这两处（否则比对失败、必红）。

**新增文件在计划期不存在**（`src/observability/` 三件套 + `tests/f6_observability/main.rs`）→ 机械门 `src/change/plan_audit.rs:262`–`:267` 会报「白名单里的文件不存在」；这是**预期内**的（c6 场同款：新增文件在白名单里必然报这一条），**但**必须用反引号写（否则 `section_paths` 抽不到 → coding 步提交它会被判越界）。

**机械门 finding 登记项**：本计划在计划期跑机械门审计（`src/change/plan_audit.rs:238` `audit`）的 finding 集合**恰为 4 条**「白名单里的文件不存在」——`src/observability/mod.rs`、`src/observability/heartbeat.rs`、`src/observability/tombstone.rs`、`tests/f6_observability/main.rs`（新增文件在计划期必然不存在，预期内，同 c6 场）；**这 4 条之外 finding 数必须为 0**（六块齐全、`path:line` 全部存在、「要改的」⊆「白名单」、验收标准可观测）。

白名单之外不得有任何改动。本计划文件本身（计划阶段的交付物）与工作区里的未跟踪产物（归档目标目录、计划目录、e2e 结果文件）都不属 coding 阶段的改动面，一律不许 `git add`。

## 5 验收标准

- 给定一次 run 的尝试因**非质量类**失败（基础设施故障、存储/协议类等）失败 → 该次尝试在日志面留下**一条 ERROR** 条目，内容含**原始错误全文**（顶层错误逐字）与 **source 链每一层**的文本（`WithContext` 的上下文 + 内层错误逐层可见）。
- 给定基础设施故障发生在第 1 次尝试 → 该 ERROR 条目出现在**重试 WARN 之前**（先记错误、再决定重启），且条目带 `attempt` 字段。
- 给定质量失败（`Quality`）→ 日志面出现一条 **WARN**（**不是** ERROR）条目，内容与 ERROR 条目同构（原始错误全文 + source 链每层），且 run **不**重启（既有口径不变）。
- 给定 run 连续 4 次尝试都因基础设施故障失败 → 日志面出现 **4 条** ERROR 原始错误条目（每次尝试一条）+ **1 条**同级终态条目（含总尝试次数）。
- 给定 `POST .../start` 到达时**有渲染在途**且本 run 所需 profile 的 `settings.json` 尚未落盘 → 该 start 请求**不**立即开跑（等所需 profile 就绪或渲染结束后再开跑），且 run 的**第一次尝试**不因「profile 未就绪」失败（日志面**不**出现 boot 期 attempt=2 的自动重启）。
- 给定渲染**已结束**且目标 profile 仍缺 `settings.json` → run 以**明确错误**失败，错误文本点名缺失的 profile 名与 `settings.json` 路径，且**不**发生自动重启（尝试次数恒为 1）。
- 给定闸门正在等待渲染（渲染在途）→ 同一服务器对**其他**端点的请求仍能被及时处理（等待不得阻塞 tokio worker，§2.3 / R18）。
- 给定两个并发 `POST .../start` 且闸门正在等待渲染 → **至多一个** run 真正开跑，另一个以 `run.error` 事件明确失败（**不产生双跑**；串行槽语义不变，见 R19）。
- 给定 change run 的阶段从 plan 推进到 coding → 日志面出现一条 `event=phase.change` 的 INFO，含 `from`/`to` 阶段名。
- 给定 change run 的 testing 阶段回注 coding → 日志面出现一条 `event=fix.start` 与一条 `event=fix.end` 的 INFO，含回注轮次。
- 给定 MVP planning / MVP coding / MVP testing / doc-from-zero / doc-fix 各自的 fix 往返（打回或回注 → 重写 → 重交）→ 日志面**各**出现一对 `event=fix.start` 与 `event=fix.end` 的 INFO，含轮次。
- 给定 MVP planning 循环的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**。
- 给定 MVP coding 循环的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**。
- 给定 MVP testing 循环的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**。
- 给定 doc-from-zero 的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**。
- 给定 doc-fix 的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**。
- 给定 change 侧任一审查角色（plan / coding / test）开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**（轮次 = 该角色在本次 run 内的审查序号，Q5 口径表）。
- 给定 `event=review.open` 里程碑的 `round` 字段 → 等于 Q5 口径表里该角色的**调用前可知**序号（change 侧 = 角色内序号；MVP/doc 侧 = 调用点自有计数），**不是**共享门返回的实例数（`opened` 是 `src/sop/review_gate.rs:130` 的返回值，调用前不可知 → 不进本事件）。
- 给定基线门判定（绿或红）→ 日志面出现一条 `event=gate.baseline` 的 INFO，含判定结论（红时含退出码）。
- 给定步提交对账判定（过或不过）→ 日志面出现一条 `event=gate.step_commits` 的 INFO，含步骤数与判定结论。
- 给定自证门判定（绿或红）→ 日志面出现一条 `event=gate.self_check` 的 INFO，含判定结论。
- 给定一次 agent 轮次结束 → 日志面出现一条 `event=agent.turn_end` 的 INFO，含**角色名**与该轮耗时。
- 给定全部里程碑条目 → 级别为 **INFO**、字段含 `component` 与 `event`，且 `target` 为**发出它的模块**（可按 target 过滤，不同模块的里程碑不共用同一个 target）。
- 给定注入一次 panic（`catch_unwind` 内）→ `HOME/.devforge/tombstones/<ts>-<runid>.txt` 存在，内容含 **panic 原文**、**panic 位置**（`file:line`）与当时的**阶段/步骤**。
- 给定没有活动 run 时发生 panic → 墓碑文件**仍然存在**（run id 用占位值），证据不丢。
- 给定 panic hook 被安装后触发一次 panic → 墓碑文件存在，**且**上一个 hook 仍被调用（其副作用仍发生——链回语义，R6）。
- 给定 HOME 指向不可写目录（或 HOME 未设）后触发一次 panic → 进程**不 abort**（panic 仍被正常捕获/传播）、**上一个 hook 仍被调用**，且不产生墓碑文件、hook 内不发生二次 panic（best-effort 语义，Q13）。
- 给定 change run 的阶段推进（plan → coding）→ `HOME/.devforge/heartbeat/<runid>.json` 的内容**随之变化**（阶段字段改变），且文件内含**阶段 / 步骤 / 时间戳**三个要素。
- 给定 run 停留在同一阶段、一段时间内无阶段转换也无关键事件（典型：change run 里的长 agent 轮）→ 心跳文件的**时间戳**仍随之刷新（低频兜底刷新生效，Q11：六个 SOP 全覆盖，change 也不例外；能区分「卡死」与「已死」）。
- 给定进程在两个阶段之间异常死亡 → 最后写入的心跳文件仍能回答「死在哪个阶段」（阶段字段 = 死亡前最后到达的阶段）。
- 给定 run 正常结束 → 心跳文件**保留**最后一次状态（不被删除）。
- 给定心跳与墓碑的落点 → 均在 `HOME/.devforge/` 下的固定子目录（`heartbeat/` 与 `tombstones/`），文件名含 run id。
- 给定 `tests/change_ask_ring.rs` 的两个常量 → `BASELINE` 等于本场基线 sha `a0be9477a9ece7b12b5ec345cdaab141b619eb2b`、`WHITELIST` 逐个列出本场 **coding 步白名单的 21 个文件**（含三个新增的 `src/observability/` 文件，**不含** `tests/f6_observability/main.rs`）；给定 `tests/change_failure_context/main.rs` 与 `tests/change_review_protocol/main.rs` 的同名常量 → 与守卫文件**逐字一致**。
- 给定白名单守卫的 `#[ignore]` → 本场 step 1 摘除（守卫在本场内生效）、本场收尾复贴（两场之间保持 ignore，F10 现状沿用）。
- 给定 `docs/plantree/plans/010-run-resume/decisions/README.md` → 含 `## D006 ·` 一节，写明三件交付物（原始错误可见 / 里程碑 / 死亡取证）、渲染闸门口径、「原始错误落盘分级：基础 ERROR / 质量 WARN」与「profile 未就绪 = 配置错误」的分类修正；既有 D001–D005 条目**一字不动**。
- 给定 root 被注入到临时目录后跑一次真实 start 路径（仓内单测）→ 心跳文件落在**该临时目录**里，用户 HOME 的 `.devforge/heartbeat/` **不新增**文件（root 注入语义，Q3/R13）。
- 给定本计划文件跑机械门审计 → finding **恰为 4 条**「白名单里的文件不存在」（`src/observability/mod.rs`、`src/observability/heartbeat.rs`、`src/observability/tombstone.rs`、`tests/f6_observability/main.rs`），**其余 finding 数为 0**（六块齐全、`path:line` 全部存在、「要改的」⊆「白名单」、验收标准可观测）。
- 给定本变更的提交内容 → §4 白名单（**22 个文件**）之外无任何改动；未跟踪产物一律不 `git add`。

## 6 风险

- **R1 · 根因判断若错，本靶的修复段要降级** —— 根因是**高置信静态定位**（三条独立证据：日志时间线、profile 文件 mtime、代码路径排除法，§1.2），但仍是**推断**：ERROR 落盘上线后，若实测显示 boot 失败点不是「profile 未就绪」（例如是 git 基线失败或装包失败），则**根因修复段**（渲染闸门 + 门卫复查）不解决真问题。缓解：闸门与门卫复查是**通用**机制（任何「渲染在途时开跑」都被挡住），且 ERROR 落盘会把真因直接顶到脸上；真因不同则按「只做可见性 + 登记待查」处理，**其余三件交付物不受影响**。残留：真因未定位前，boot 期仍可能出现一次「明确失败」（不再盲重启）。
- **R2 · 渲染闸门把 `start` 端点变成阻塞点** —— 闸门要在 start 返回 202 之前等「所需 profile 就绪」。若闸门等的是**整个渲染任务结束**（而不是所需 profile 的 `settings.json` 落盘），装包实测可达分钟级（CAL-19-RENDER 记录）→ start 变成分钟级阻塞，而 e2e 的 start 请求默认超时只有 10s（`tests/e2e/common.py:132`）。缓解：闸门等的是**门卫的判据**（`settings.json` 落盘，早于装包，秒级），且渲染完成/失败都会结束等待（无任意超时）；闸门不设新超时 → 没有「等不到就硬跑」的分支。残留：渲染线程本身挂死时 start 会一直等（登记为遗留：闸门需要一个可配的兜底上界，本靶不做）。
- **R3 · 「profile 未就绪」不再自动重启 = 运维口径变了** —— 改前：这类失败会盲重启 3 次（每次 10s/30s/90s 退避），最后失败；改后：**第一次就明确失败**（错误点名缺失的 profile 与路径），等人处理。受益：不再浪费 2 分钟做注定失败的重启，且错误可归因；代价：以前「重试几次就撞上了」的运气没了（若渲染确实还在途，闸门已经先等过 → 真缺的场景本就该等人）。仓内 4 条单测与 2 个 e2e 依赖「秒级 Failed」，**不依赖**「重启过」→ 应仍绿（回归项）。
- **R4 · 全局 run 上下文是进程级单例** —— 心跳与墓碑的「当时阶段/步骤」来自进程级上下文（`src/observability/mod.rs`）。MVP 是串行执行（`src/server/handlers.rs:220`–`:226` 的 `has_running_run` 守卫）→ 同一时刻只有一个 run，暂不冲突。将来并发化时，两个 run 的上下文会互相覆盖 → 墓碑里的阶段可能张冠李戴。缓解：上下文以 run id 为键、run 结束即清；**登记为遗留**（并发化不在本靶，也不在 010 的计划里）。
- **R5 · 心跳是覆盖写 → 「本 run 发生过几次阶段转换」查不到** —— 纪律第 12 条要求状态文件能被覆盖写（不能追加成第二份真相），所以心跳只留最后一次状态。缓解：过程历史由里程碑日志承担（`phase.change` / `fix.start` / `fix.end` / `review.open` 逐条落盘）。残留：只看心跳文件无法还原完整时间线（要看日志 + 心跳两份）。
- **R6 · panic hook 是进程级全局** —— `std::panic::set_hook` 只能有一个；仓内单测若装 hook，会影响同进程的其他测试（尤其会吞掉它们的默认 stderr 行为）。缓解：验收测试放**独立测试二进制**（`tests/f6_observability/main.rs`），且安装接口返回上一个 hook 供还原；hook 内部**链回**上一个 hook（默认输出不丢）。残留：若仓内将来有别的测试也要装 hook，需要同样的隔离约定。
- **R7 · 里程碑日志不保证零丢失** —— tracing 的文件层是同步写但进程猝死（SIGKILL）时缓冲区里的最后几条可能丢（`src/logging.rs:38`–`:44`）。这正是「墓碑 + 心跳」存在的原因：**日志是过程记录，墓碑/心跳才是死亡现场的硬证据**。本靶不追求日志零丢失（那要换同步 fsync 通道，超出单靶范围），登记为遗留。
- **R8 · 全 SOP 覆盖 = 白名单 22 个文件、回归面大** —— MVP 三循环 + doc 两处 + change 编排层 + 引擎层都要动，改动点分散。缓解：①每个改动点只加「一条日志 + 一次心跳更新」，不改判定；②既有单测是回归闸门（§3.3 逐条列出）；③`review_with_gate` 与三个协议解析器**不动**。残留：若审查认为改动面过大，退路是**只保留 change 侧 + 引擎侧**（把 MVP/doc 的里程碑撤掉，Q2 的裁决降级为「change 细粒度 + 全 SOP 的 SOP 级心跳」）——但那需要重新裁决覆盖面。
- **R9 · 白名单守卫的三个副本必须同步刷新** —— `tests/change_ask_ring.rs`、`tests/change_failure_context/main.rs`、`tests/change_review_protocol/main.rs` 各存一份 `BASELINE`/`WHITELIST` 并互相比对（a16/a29/a30）。漏刷任一份 → 比对失败 → **必红**（上一场 c6 在这里栽过一次：a16/a17 是「本场最容易漏的红」）。**更隐蔽的一条**：上一场的验收文件里还有**两处硬编码断言**绑死了上一场的白名单形状——`tests/change_review_protocol/main.rs:1333`–`:1336`（a29：守卫 WHITELIST 里不得含 `tests/change_review_protocol/main.rs`）与 `:1431`–`:1443`（a32：计数 20/19 + 「§4 白名单含唯一的测试阶段文件 `tests/change_review_protocol/main.rs`」）→ 本场口径（coding 步 21 个含该文件 + 测试阶段 1 个 `tests/f6_observability/main.rs`）**必然让这两条变红**，只刷常量不改断言 = step 1 之后测试面直接红。缓解：①Q9 已把两处改写写进口径（排除项换成本场测试阶段文件、计数改 22/21）；②§3.1/§3.3/§5 三处点名，验收标准单列「a29/a32 改写后绿」；③step 1 一次性刷完三份副本**并**改写这两处断言。
- **R10 · 白名单守卫断言「上一场遗留的两个文件不得进本变更」** —— `tests/change_ask_ring.rs:653`–`:661` 钉住 `src/change/coding_gate.rs` 与 `src/config/skill_assets.rs` 不得出现在本变更的 diff 里 → 本靶**不得**动 `src/change/coding_gate.rs`（基线门/自证门/步提交对账的里程碑因此落在调用点 `src/change/session.rs:338`/`:354`/`:357`，门本身不动）。若 coding agent 想「顺手在门里加日志」→ 直接触发这条守卫变红。
- **R11 · 新增文件在计划期不存在，机械门会报一条 finding** —— `src/change/plan_audit.rs:262`–`:267` 对白名单里的每个路径查存在性 → 3 个新增源文件 + 1 个新增测试文件必然报「白名单里的文件不存在」。这是**预期内**的（c6 场同款），但**必须**用反引号写进白名单（否则 `section_paths` 抽不到 → coding 步提交它们会被判「越界改动未申报」→ Quality）。缓解：验收标准把「提交内容 ⊆ §4 白名单」单列一条。
- **R12 · `path:line` 引用被逐条验存在性** —— `src/change/plan_audit.rs:253`–`:258` 会把计划里每个 `path:line` token 拿去 `repo.join(path).exists()` 验 → 引用了不存在的路径 = 一条确定性 finding。本计划的新增文件**一律不带 `:行号`**（写「（**新增** · 全文）」），其余引用都指向真实位置。风险：coding agent 改代码后行号漂移 → 计划里的行号会过期（计划是审查期的快照，审查时以代码为准，登记在 D006 的遗留清单里）。
- **R13 · 心跳与墓碑依赖 `HOME`** —— 落点取 `HOME/.devforge`（`src/main.rs:143`–`:144`）→ 服务以别的用户/容器跑、或 `HOME` 未设时，墓碑与心跳会落到意料之外的路径（或无处可落）。缓解：`HOME` 未设时 main 已在 `src/main.rs:143` 失败（既有行为）；容器场景登记为遗留（部署形态未定）。**另一条诚实缺口：仓内单测会真的落盘** —— `src/server/tests/mod.rs:145` 的 fixture 与 `:668` 起的 `start_task_runs_engine_and_records` 走**真实 start 路径**（`src/server/handlers.rs:237` → `src/server/mod.rs:1249`）→ run 上下文被置位 → 心跳**真写**进 root（run id 唯一、run 结束按 Q3 保留）→ root 若默认 `HOME/.devforge`，**用户的 HOME 会被单测写脏**（本靶之前的「无活动 run 即不写」并不覆盖这条路径——单测里 run 是活动的）。缓解：**root 可注入**（Q3：`DEVFORGE_HOME` env > 进程内显式 setter > `HOME/.devforge`），仓内走 start 路径的单测把 root 指向临时目录；验收标准单列一条「用户 HOME 不新增心跳文件」。残留：并行单测共享进程级 root setter（谁最后设谁生效）→ 表现为落点漂移、不影响正确性（每个 run 的文件名带 run id，互不覆盖），登记为遗留。
- **R14 · 门卫复查的「明确失败」文案必须与现状一致** —— Q8 **不改错误文本**：`src/server/mod.rs:257`–`:261` 的原文（`profile X 未渲染（缺 …）——先 PUT /api/v1/config/manifest 完成安装渲染，再启动 run`）已含处置建议，追加会重复、改写会让仓内 4 条单测（用 `contains("未渲染")`）与两个 e2e 变红 → 本靶只改错误**种类**（`System` → `Config`）→ 文本一字不动，那 4 条单测与 2 个 e2e 必然仍绿。缓解：验收标准把「错误文本点名缺失的 profile 与路径」单列一条。
- **R15 · 闸门所需 profile 清单的统一入口（落点已定）** —— 清单现在分散在五个 `run_*` 方法的 `require_profile` 调用（`src/server/mod.rs:268`–`:269`/`:326`–`:327`/`:400`–`:401`/`:809`–`:810`/`:929`–`:930`）+ change 侧既有常量（`src/server/change_engine.rs:24`–`:33`）→ 闸门若另抄一份 = 两处清单漂移（改了 SOP 的 profile 需求，闸门还在等旧清单）。**统一入口定在 `src/server/mod.rs`**（一张 per-SOP 的 profile 表，如 `profiles_for_sop(sop)`）：六个 SOP 的门卫调用改为查表，change 一项**引用** `CHANGE_PROFILES`（那个文件不改、不进白名单）→ 清单只有一份、闸门与门卫读同一张表。残留：表的键（按 sop 名）与返回形状由 coding 阶段定，但**落点与事实源已定**（都在白名单内的 `src/server/mod.rs`）→ 不会白生出「闸门另抄一份清单」的漂移，也不需要新的白名单条目。
- **R16 · 文档与代码的既有不一致（本靶之外，不修）** —— `docs/plantree/plans/009-change-sop/topics/vs-mvp.md:34`–`:35` 与 `docs/plantree/plans/009-change-sop/topics/agents-change.md:210` 的旧口径是 c6 场已登记的遗留（`src/change/plan_audit.rs:735`–`:757` 的守卫只钉问询环锚点）；本靶不改 009 的文档（Q10）→ 遗留继续挂着。
- **R17 · 审计纪律的「先证后报」对本靶的额外要求** —— 本靶的核心交付物全是「可观测性」，最容易被「加了几行日志就算做完」蒙混：每条验收标准都要有**可机械复核的证据**（日志条目原文 / 墓碑文件 / 心跳文件内容），不能只交「代码里有 `tracing::info!`」。缓解：§5 的每条标准都写成「给定 → 期望输出/状态变化」，测试阶段逐条对账。
- **R18 · 闸门在 axum async handler 内等待 → 拖住 tokio worker** —— `start_task`（`src/server/handlers.rs:202`）是 `async fn`（在 tokio worker 上执行），闸门若在这里**阻塞**等待渲染，会占住 worker 线程（并发请求被拖住；worker 数有限 → 拖满即全站假死，而 e2e 的 start 请求默认超时只有 10s，`tests/e2e/common.py:132`）。缓解：等待用**可 await 的原语**（`tokio::sync::Notify` / `watch`）或整体挪进 `spawn_blocking`，**不得**用 `std::thread::sleep` 轮询；验收标准单列一条「等待期间其他请求仍被及时处理」。残留：渲染线程自身挂死且不发完成通知时 start 会一直等（闸门**没有兜底上界**——本靶刻意不引入新超时，登记为遗留）。
- **R19 · 闸门等待窗口落在「串行槽快判」与「真正占槽」之间** —— `has_running_run()` 的快判在 `src/server/handlers.rs:224`（**闸门之前**），真正占槽的 `begin_run` 在 `src/server/mod.rs:1215`（**spawn 之后的线程里**）→ 两个并发 start 可能**都**过快判、都过闸门、然后各自 spawn；第二个线程的 `begin_run` 失败 → 发 `run.error` 事件（`src/server/mod.rs:1224`）并返回。**与既有口径的关系**：串行槽语义**没变**（同一时刻仍只有一个 run 能跑），只是「第二个请求被拒」的**时点**从「handler 早拒（409）」挪到「线程里晚拒（`run.error`）」；§3.3 里两个 e2e 断言的是「start 仍受理 == 202」与「槽即时释放」，**不受影响**。缓解：①行为退化可接受（并发 start 本就该只有一个成功）；②更严的做法是「闸门等待期间就持有串行槽」（把快判挪到闸门之后）——**本靶不做**（会改 `start` 的既有 409 语义，登记为遗留）。验收标准单列一条「并发 start 至多一个 run 开跑」。
- **R20 · panic hook 里二次 panic = abort（遗言全丢）** —— 写墓碑失败若在 hook 里 panic，就是「panic 中 panic」→ 进程 abort，**连 stderr 链回都丢**（最后一条遗言也没了，等于把「死亡取证」变成「死亡无证」）；且**库路径不经过 `src/main.rs:143` 的 HOME 检查**（`tests/f6_observability/main.rs` 直调安装接口）→ HOME 未设时 hook 自己也可能炸。缓解：写入 **best-effort**（Q13）——写失败只记一行 stderr，**绝不**在 hook 里 panic；无论成败都**链回上一个 hook**；HOME 缺失 / root 不可写 / 磁盘满 → 降级（不写墓碑、不 panic、照常链回）。验收标准单列一条「写不进去时不 abort、hook 仍被链回」。
- **R21 · change 侧「审查轮次」只能是角色内序号（不是实例数）** —— 共享门返回的实例数 `opened`（`src/sop/review_gate.rs:130`）是**返回值**，调用前拿不到 → change 侧的 `event=review.open` 只能记「该角色在本次 run 内的审查序号」（Q5 口径表：plan 首审=1 / fix 后重审=修复轮+1 / amend 后重审=roundtrip+1；coding、test 各为 1）→ 事件本身**不**携带「本次开过几个实例」。缓解：违例重审/实例失败重试的实例数由 `review_gate` 自己的既有 WARN 如实记录（`src/sop/review_gate.rs:139`/`:163`），两处信息互补；验收标准按 Q5 口径表逐个核对。残留：只查 `review.open` 看不出「这一轮审了几次」→ 需并看 `review-gate` 的 WARN（登记为遗留，不在本靶补字段）。
