# 变更计划 · change-20261022-c7-1-start-src-server-tests-rs-145-fixtu（第 2 次计划轮 · 修订版）

目标仓：`/home/trtyr/code-work/devforge`
基线：`a36f6b79dec2e681db8daed0da067f9abd47a9f5`（`docs/plantree/changes/change-20261022-c7-1-start-src-server-tests-rs-145-fixtu/baseline.txt`；10-22 操作员补录重锚：resume2 死后 scan 区间必须含全部本场步提交）
上一场：`change-20261022-1-2026-10-21-c3-textkit-run-test-agent-r`（已 Settled，HEAD=`a0be947`）
文档档位：none（计划阶段照常读已有文档；不跑文档流水线）
本场是**自举第六场·单靶 F6**：只治「**引擎可观测性与死亡取证**」——① 原始错误全量可见、② 里程碑日志、③ 死亡取证（panic 墓碑 + run 心跳）；并**附带**修掉 boot 期「基础设施故障盲重启」的**根因**（渲染闸门，用户裁决 A）。**不做** F10（守卫常量物化）、**不做** F8（plan 延迟优化）、**不做** F11（补边界引用卫生）。

> **本文件是第 2 次计划轮（attempt 2）的修订版。**
> 前 18 步已落地（`HEAD=458c969`），测试阶段以「**计划 §5 的数字与现状不一致**」Quality 回注计划轮
> （见 `evidence/regression.log` 结尾与状态机 `test_reroutes=1`）。本轮修订只做两件事：
> ① **文本订正**：把计划里滞后于现状的数字（白名单 24 / coding 步 23 / 基线 sha / 机械门 finding 数）
>    与已自相矛盾的口径（`src/change/coding_gate.rs` 到底在不在本变更）订正到与代码一致，并把
>    `path:line` 重新锚到当前 `HEAD=458c969`；
> ② **实现对齐（用户裁决，见 Q14）**：把上一轮编码审查留下的 4 条观察项按计划 §3.1 的字面口径修掉
>    （R-1 attempt 时序 / R-2 闸门放行条件 / R-3 `end_render` 兜底 / R-4 守卫常量注释），
>    **不登记为遗留**。
> 计划里的 `R-1`..`R-4` = 本轮修订项；`R1`..`R29` = 风险编号（§6），两套编号互不混用。

---

## 1 需求复述与歧义

### 1.1 需求复述（我自己的话）

三次实测事故都指向同一个病根：**引擎在自己内部发生了什么，事后查不出来**。本场交付三件互相咬合的东西，外加一件根因修复：

1. **原始错误全量可见** —— 改前 `run_with_retry` 决定重启前，**原始错误全文**根本不落盘：失败只进函数内的 `Vec<String>`（`src/engine/policy.rs:81` 的 `log.push`），进程活着时看不到、死了更看不到 → 现场只剩一句「基础设施故障，自动重启 run（从断点继续）」（`src/server/mod.rs:1408`）。本靶让它**分级落盘**（用户裁决：基础类失败 → **ERROR**，质量类失败 → **WARN**，字段同构），落盘点在**决定重试之前**。**已落地**（step 3）：分级落点在 `src/engine/policy.rs:88`（`log_attempt_failed`），终态条目在 `src/engine/policy.rs:155`。
2. **里程碑日志** —— 编排器每个阶段转换、每轮 fix 开始/结束、每次审查开庭（含轮次）、基线门/步提交对账结果，各落一条结构化 INFO（`component`/`event` 可过滤，`target` 由发出模块自动给出）。改前从 plan agent 交卷到重审开庭之间是 15 分钟的**空洞**（10-21 c5 场 21:20–21:35），引擎一条记录都不留。**已落地**（steps 6、7、13）：八个事件名内联写在调用点（`target` = 发出它的模块，`src/change/orchestrator.rs:30`、`src/engine/client.rs:276` 等）。
3. **死亡取证** —— ① panic hook：panic 原文 + 位置 + 当时阶段写进 HOME 下固定路径的墓碑文件；② run 心跳：阶段转换与关键事件时更新心跳文件（阶段/步骤/时间戳），异常死亡后凭心跳即可回答「死在哪个阶段」；③ 两件事都必须**机械可验**（注入 panic → 墓碑存在且含 panic 原文；阶段推进 → 心跳内容随之变化）。**已落地**（steps 2、4、5、8）：模块在 `src/observability/mod.rs:84`（run 上下文）、`src/observability/heartbeat.rs:36`（心跳覆盖写）、`src/observability/tombstone.rs:44`（hook 安装 + best-effort 墓碑）。
4. **根因修复**（用户裁决 A 渲染闸门）—— boot 期六场六中的基础设施重试，根因**已静态定位**（§1.2），因此本场**必须**含根因修复段（不是只做可见性）：开跑前若有渲染在途则等所需 profile 就绪（渲染闸门），渲染结束后门卫复查，仍缺 profile → **明确失败**（不再被当成基础设施故障盲重启 10s）。**已落地**（step 4）：闸门在 `src/server/mod.rs:1066`（`RenderGate`）/ `src/server/handlers.rs:240`（`start_task` 的等待点），门卫错误种类在 `src/server/mod.rs:269`（`require_profile`）。

**本轮（attempt 2）仍要改的**（用户裁决 Q14，四项，逐条对应 §3.1 的 R-1..R-4 与 §5 的 4 条新判据）：

- **R-1 · attempt 写入时机**：`set_attempt` 现在落在**失败分支内**（`src/engine/policy.rs:87`）→ 第 2 次及以后的尝试期间若失败被 `map_intervention` 收成质量失败，读到的 `attempt` 是**上一次**的值（陈旧 1）。计划 §3.1 的字面口径是「**每次尝试开始时**把 `attempt` 写进 run 上下文」→ 按字面前移。
- **R-2 · 闸门放行条件**：`RenderGate::wait_for_profiles` 的 `rx.changed()` 分支**直接 `return`**（`src/server/mod.rs:1124`）→ 并发两轮渲染时，任意一轮结束就会放行，与 §5「有渲染在途且所需 profile 未就绪 → 不得立即开跑」有缝隙 → 改为**回环重判**「无渲染在途 **或** 所需 profile 已就绪」。
- **R-3 · `end_render` 兜底**：渲染线程的 `emit` 在 `catch_unwind` **之外**（`src/server/handlers.rs:405`/`src/server/handlers.rs:408`/`src/server/handlers.rs:411`）→ 通知发出时抛异常则 `end_render()`（`src/server/handlers.rs:415`）永不执行 → `start` 永久等待 → 用 drop 守卫保证在途计数必归零。
- **R-4 · 守卫常量注释**：三份守卫常量的**注释**仍写旧值（`tests/change_ask_ring.rs:32` 写「基线 = `18ff755…`」、`tests/change_ask_ring.rs:38` 写「22 个」，而常量实际是 `a36f6b7…` 与 **23** 个）→ 注释与值对齐（纯注释，零行为）。

**明确不做（边界）**：

- 不引入新外部依赖（`Cargo.toml` 不动；心跳/墓碑只用 `std` + 既有 `tracing`）；
- 遵守 `assets/skills/coding-discipline/SKILL.md`（本变更**遵守**它，不改它）；
- F10（白名单守卫常量物化）、F8（plan 延迟优化）、F11（补边界引用卫生）**不在本靶**（守卫常量的**例行刷新**照旧，Q10）；
- 不改 run 级重试策略本身（4 次尝试、10s/30s/90s 退避，`src/engine/policy.rs:17`、`src/engine/policy.rs:112`）：本靶只让「为什么重试」看得见，并修掉「不该重试的那次重试」；
- 不改任何 SOP 的**业务判定**（审查三通道、质量失败不重启、越界门、验收对账口径一律不动）；
- **不重做已落地的 18 步**：本轮只做 R-1..R-4 四项（Q14 裁决），不扩大靶子。

### 1.2 boot 六连的根因（静态定位 · 高置信 · 三条独立证据）

**证据 1 · 本场实测日志（本机 `~/.devforge/logs/devforge.log.jsonl`，UTC；本场 change-id 自己的 boot）**：

- `06:29:26.687` devforge 启动；`06:29:26.688` API 就绪；
- `06:29:26.878`–`06:29:26.905` **十二个** profile 渲染完成（`packages=0`，无需装包）；
- `06:29:30.844` `change-plan-agent` 渲染完成（`packages=1` → 装包 ~4s）；
- **`06:29:36.893` WARN「基础设施故障，自动重启 run（从断点继续）」`attempt=2`**（`component=run`）；
- `06:29:36.898` INFO「change 门 0 完成」（`change_id=change-20261022-c7-1-start-src-server-tests-rs-145-fixtu`）→ **attempt 2 只用了 5.6ms 就跑完门 0**；
- `06:29:41.015` / `06:29:41.543` `change-test-agent` / `change-coding-agent` 才渲染完成（`packages=3`）——**在 attempt 1 失败之后**。

即：attempt=2 的 WARN 落在 **start+10.206s**（= 退避第一档 `10_000ms`，`src/engine/policy.rs:17`），而 attempt 2 只用 **5.6ms** 跑完门 0 → **attempt 1 是在开跑瞬间失败的，不是跑到一半失败**；且 WARN 之前**没有任何 ERROR 条目**（原始错误全文从未落盘——正是交付物 ① 要修的）。

**证据 2 · profile 文件 mtime（同一 workspace 的 profile 目录）**：渲染器的写入次序是「建目录 + 写 `AGENTS.md`（`src/config/render.rs:50`）→ 写 `settings.json`（`src/config/render.rs:63`）→ 装包（`src/config/render.rs:205`，装包期间安装器还会**再回写** `settings.json`，`src/config/render.rs:192`）」→ attempt 1 那一刻，`change-test-agent` / `change-coding-agent` 的 `settings.json` **还没落盘**（证据 1 里它们 06:29:41 才渲染完）。

**证据 3 · 代码路径**：

- 竞态的另一端：`PUT /config/manifest` **立即返回 202**、渲染丢进后台线程（`src/server/handlers.rs:371`、`src/server/handlers.rs:392`）→ `POST .../start` 改前完全不等渲染就开跑（`src/server/handlers.rs:243`）；
- 门卫判据只有「`settings.json` 在不在」（`src/server/mod.rs:271`）→ 半渲染的 profile 一律判「未渲染」；
- 门卫改前抛的是**裸** `DevforgeError::System` → `is_infra_failure` 只认裸 `System`（`src/engine/policy.rs:60`）→ 判成基础设施故障 → 退避 10s 后自动重启（`src/engine/policy.rs:112`）→ WARN 落盘（`src/server/mod.rs:1408`）；
- change SOP 的门卫调用点在 `src/server/change_engine.rs:78`（`for p in CHANGE_PROFILES { self.require_profile(p)?; }`），**先于**门 0（`src/server/change_engine.rs:110` 起的 `prepare`）→ 与证据 1「attempt 2 五毫秒跑完门 0」完全吻合；
- boot 路径上（`require_profile` → `prepare` → 门 0）**只有 `require_profile` 会抛裸 `System`**；其余是 `WithContext`/`Io`/`Quality`（`is_infra_failure` 都不认）→ 排除法也指向同一处。且门卫失败**不经** `map_intervention`（它的 `?` 在 watchdog/编排之前直接上抛，直达重试层 `src/server/mod.rs:1388`）。

**结论**：**boot 六连 = `start` 与异步渲染的竞态**（profile 半渲染 → 门卫误判「未渲染」→ 裸 `System` → 被当基础设施故障盲重启 10s）。六场六中，是因为每场都是「全新 HOME → PUT 清单（装包十几秒）→ 立刻 start」。

**现状（本轮修订后）**：闸门已落地（`src/server/handlers.rs:240` 等所需 profile 就绪或渲染结束才开跑），门卫错误种类已从 `System` 改成 `Config`（`src/server/mod.rs:269` 的 `require_profile`）→ `is_infra_failure` 不认它 → **不再盲重启**，改为第一次就明确失败。剩余缝隙只有 R-2（并发渲染下的放行条件）与 R-3（通知抛异常时的闸门释放），两者都在本轮修掉。

**残余不确定性（写进风险 R1）**：本靶的 ERROR 落盘上线后，若实测显示 boot 失败点**不是** profile 未就绪（例如是 git 基线或装包失败），则根因修复段按 R1 降级为「只做可见性 + 登记待查」，其余三件交付物不受影响。

### 1.3 代码事实（定位到行 · 当前 `HEAD=458c969`）

| # | 事实 | 位置 |
| --- | --- | --- |
| 1 | 原始错误只进函数内的 `Vec<String>`（`log.push(format!("第{attempt}次: {e}"))`），**从不落盘** | `src/engine/policy.rs:81` |
| 2 | 错误全文只在**四次尝试全败**后才被拼进最终错误串（`共尝试 N 次；尝试记录：…`） | `src/engine/policy.rs:92` |
| 3 | 「该不该重启」只认裸 `DevforgeError::System`（`WithContext`/`Io`/`Quality` 都不认） | `src/engine/policy.rs:60` |
| 4 | 退避档位 10s/30s/90s、总尝试 4 次（`default_backoff_ms` / `sleep`） | `src/engine/policy.rs:17`、`src/engine/policy.rs:112` |
| 5 | 分级落盘的**唯一实现**：非质量 → ERROR、质量 → WARN，字段同构（`component`/`event`/`attempt`/`error`/`error_chain`/`retryable`） | `src/engine/policy.rs:124` |
| 6 | 终态条目 `event="run.attempt_exhausted"`（含总尝试次数） | `src/engine/policy.rs:155` |
| 7 | **R-1 的落点**：`set_attempt(attempt)` 现在写在**失败分支内**（计划 §3.1 的字面口径是「每次尝试开始时」） | `src/engine/policy.rs:87` |
| 8 | 唯一的 boot 期裸 `System` 门：`require_profile`（判据 = `settings.json` 存在）；change SOP 的门卫调用点在 `src/server/change_engine.rs:78`，其余五个 SOP 在 `src/server/mod.rs:288`、`src/server/mod.rs:347`、`src/server/mod.rs:422`、`src/server/mod.rs:832`、`src/server/mod.rs:953`；**门卫失败不经 `map_intervention`** | `src/server/mod.rs:269` |
| 9 | 门卫错误文本（**一字不改**，含处置建议） | `src/server/mod.rs:277` |
| 10 | boot 期重试 WARN（**不带错误原文**） | `src/server/mod.rs:1408` |
| 11 | run 级重试闭包（`attempt>1` 时落 attempt、记 WARN、发 `run.retry` 事件） | `src/server/mod.rs:1388` |
| 12 | `map_intervention`：**只作用于 SOP/编排执行期**的错误；其**质量分支**（`src/server/mod.rs:1030`）把 `DevforgeError::Quality` 转成 `Ok(RunOutcome::Failed)`（`src/server/mod.rs:1044` 补了一条同形状的 WARN，既有那条在 `src/server/mod.rs:1032`）→ **生产链路上「SOP 执行期」的质量失败到不了 `run_with_retry`**；非质量类则原样上抛 → 重试。**启动门卫的失败不经它** | `src/server/mod.rs:1017` |
| 13 | `PUT /config/manifest` 立即 202，渲染在后台线程（完成信号只经 SSE `config.rendered`/`config.render_failed` 广播） | `src/server/handlers.rs:371`、`src/server/handlers.rs:392` |
| 14 | `POST .../start` 的闸门等待点（**awaitable**，不占 tokio worker） | `src/server/handlers.rs:240` |
| 15 | 渲染器写 `settings.json` 的两个时点（早写 / 装包后回写），早写发生在装包**之前** | `src/config/render.rs:63`、`src/config/render.rs:192`、`src/config/render.rs:205` |
| 16 | 门卫判据只要求文件存在，**不**要求 npm 包装齐 | `src/server/mod.rs:271` |
| 17 | change SOP 的六个 profile 清单（已存在，可直接被闸门引用） | `src/server/change_engine.rs:24` |
| 18 | 六个 SOP 的 profile 清单的**唯一事实源**（门卫与闸门读同一张表） | `src/server/mod.rs:253` |
| 19 | 审查三通道共享门（change 三角色 + MVP 三循环共用），实例数 `opened` 是**返回值**（调用前拿不到）；**全仓 11 处调用点**：生产 4（`src/sop/planning.rs:132`、`src/sop/coding.rs:184`、`src/sop/testing.rs:224`、`src/change/session.rs:281`）+ 仓内单测 5（`src/sop/review_gate.rs:233`、`src/sop/review_gate.rs:250`、`src/sop/review_gate.rs:273`、`src/sop/review_gate.rs:297`、`src/sop/review_gate.rs:316`）+ 仓外验收文件 2（`tests/change_review_protocol/main.rs:1134`、`tests/change_review_protocol/main.rs:1160`） | `src/sop/review_gate.rs:78`、`src/sop/review_gate.rs:130` |
| 20 | change 侧三角色审查的唯一入口（`review_once`，轮次恒传 0，调用点 `src/change/session.rs:283` 的 `review_with_gate(factory, 0, …)`），它有 **3 个调用点** = 三角色各一 | `src/change/session.rs:264`、`src/change/session.rs:316`、`src/change/session.rs:477`、`src/change/session.rs:548` |
| 21 | doc 侧两处审查**不走**共享门（自己 `factory.open()`） | `src/sop/doc/runner.rs:347`、`src/sop/doc/fix.rs:101` |
| 22 | change 编排层的阶段写入点（**5 处** `phase_change(&mut state, …)`；`src/change/orchestrator.rs:232` 是 `save(run.store, &state)?`，**不是**阶段写入） | `src/change/orchestrator.rs:204`、`src/change/orchestrator.rs:231`、`src/change/orchestrator.rs:317`、`src/change/orchestrator.rs:346`、`src/change/orchestrator.rs:361` |
| 23 | change 侧三处机械门的调用点（基线门 / 自证门 / 步提交对账） | `src/change/session.rs:389`、`src/change/session.rs:428`、`src/change/session.rs:474` |
| 24 | agent 轮次的唯一收口（`wait_settled*` 返回 = 交卷；`spec.role` 已知） | `src/engine/client.rs:158`、`src/engine/client.rs:186`、`src/engine/client.rs:212` |
| 25 | 「agent 轮次结束」里程碑的**唯一**落点（`event="agent.turn_end"`） | `src/engine/client.rs:276` |
| 26 | 日志面：JSONL 文件层 + stderr 人类层，级别由 `RUST_LOG` 决定（默认 info） | `src/logging.rs:33` |
| 27 | HOME 根 = `HOME/.devforge`（日志目录 `HOME/.devforge/logs` 的同根）；观测根同根，`DEVFORGE_HOME` 可覆盖 | `src/main.rs:143`、`src/main.rs:144`、`src/main.rs:149` |
| 28 | 计划六块的机械门 + 白名单「文件必须存在」检查 + 验收标准「必须含 →」检查 | `src/change/plan_audit.rs:13`、`src/change/plan_audit.rs:266`、`src/change/plan_audit.rs:281` |
| 29 | 计划里的 `path:line` 引用会被逐条验存在性 | `src/change/plan_audit.rs:256` |
| 30 | 验收标准只认**列表行**（表格行不算）；白名单按反引号路径抽取 | `src/change/plan_audit.rs:364`、`src/change/plan_audit.rs:336` |
| 31 | 白名单守卫的三个常量副本（守卫 + 上一场验收文件 ×2），互相比对 | `tests/change_ask_ring.rs:35`、`tests/change_ask_ring.rs:39`、`tests/change_failure_context/main.rs:36`、`tests/change_failure_context/main.rs:40`、`tests/change_review_protocol/main.rs:47`、`tests/change_review_protocol/main.rs:51`、`tests/change_review_protocol/main.rs:79` |
| 32 | 白名单守卫还断言「上一场遗留的两个文件不得进本变更 diff」——该豁免的前提已钉在「该文件在本场 `WHITELIST` 常量内」 | `tests/change_ask_ring.rs:663` |
| 33 | 守卫用例当前带 `#[ignore]`（F10：两场之间恒红），由每场 step 1 摘除 | `tests/change_ask_ring.rs:629` |
| 34 | 仓内**已有** heartbeat / tombstone / panic hook 概念（本场新增，勿重复造轮子）；`DevforgeError::Config` 已存在（用作「配置错误」种类） | `src/observability/heartbeat.rs:16`、`src/observability/tombstone.rs:44`、`src/error.rs:33` |
| 35 | **R-2 的落点**：闸门等待循环（`rx.changed()` 分支直接 `return` → 并发渲染下会提前放行） | `src/server/mod.rs:1106` |
| 36 | **R-3 的落点**：渲染线程的 `emit` 在 `catch_unwind` 之外、`end_render()` 在线程体末尾 | `src/server/handlers.rs:405`、`src/server/handlers.rs:415` |
| 37 | **R-4 的落点**：三份守卫常量的注释写旧值（`18ff755…` / 「22 个」）而常量已是 `a36f6b7…` / 23 个 | `tests/change_ask_ring.rs:32`、`tests/change_ask_ring.rs:38`、`tests/change_failure_context/main.rs:33`、`tests/change_failure_context/main.rs:39`、`tests/change_review_protocol/main.rs:44`、`tests/change_review_protocol/main.rs:1316` |

### 1.4 歧义

- [必须裁决] **Q1 · boot 六连的根因修法** —— 根因已静态定位（§1.2），修法有多种，影响验收口径（「第一次尝试即通过」还是「明确失败不重启」）、影响 `start` 端点语义、影响白名单（是否要动 `src/server/handlers.rs`）。备选：A 渲染闸门（开跑前等所需 profile 就绪；渲染结束后门卫复查，仍缺 → 明确失败不重启）／B 门卫自等（run 线程内等 profile 就绪，start 语义不变）／C 只可见不修（错误降级 + ERROR 落盘 + 登记待查）／D 按需同步渲染（门卫就地渲染缺失的 profile）。
  用户裁决：A 渲染闸门
- [必须裁决] **Q2 · 「原始错误全量可见」的级别口径** —— 这条**影响验收口径**（质量失败到底落不落 ERROR），不由 agent 单方面定。三种口径：A 每次尝试失败都落 ERROR（含质量失败）／B 只在「即将触发重启」时落 ERROR／C 分级——质量类失败落 WARN、其余失败落 ERROR。需求原文写的是「run_with_retry **触发重试前**，必须以 ERROR 级落盘原始错误全文」，字面偏向 B；标题「全量可见」偏向 A；既有代码 `src/engine/policy.rs:81` 对所有失败无条件记录。
  用户裁决：分级：基础 ERROR / 质量 WARN
  （口径展开：每次尝试失败落一条**分级**日志——质量类失败（`DevforgeError::Quality`）落 **WARN**，其余一切失败（`System` 及其他种类）落 **ERROR**；两者字段同构：`component`、`event="run.attempt_failed"`、`attempt`、`error`（顶层 Display 逐字）、`error_chain`（source 链逐层，`→` 连接）、`retryable`（本次是否将重启）。落盘点在**决定重试之前**（`src/engine/policy.rs:79` 附近）；**终态失败**（4 次耗尽）再落一条同级的终态条目 `event="run.attempt_exhausted"`（含最终错误全文 + `attempts`）。形状：一条一个事件（source 链拼进一个字段），不拆成多条——免得同一错误被过滤成多行而看不出因果。**两个落点、字段集完全一致**：①重试层（`src/engine/policy.rs:79`，`component="run-retry"`）——覆盖「全部非质量失败」与「SOP 之前」的质量失败（`src/server/change_engine.rs:96`）；②`map_intervention` 的质量分支（`src/server/mod.rs:1030`，`component` 取该 SOP 的既有 component 名，如 `change-engine`）——覆盖生产链路上「SOP 执行期」的质量失败。**为什么必须有落点 ②**：质量失败在生产链路上被 `map_intervention` 转成 `Ok(RunOutcome::Failed)`（`src/server/mod.rs:1044`）→ **到不了重试层**（`src/engine/policy.rs:68`）；只改重试层的话，本裁决的「质量类落 WARN、字段同构、含 source 链每层」在生产**不可达**（唯一现成触发器是仓内 mock 引擎 `src/server/tests/mod.rs:35`）→ 落点 ② **只加日志、不改判定**（仍是 `Ok(Failed)`、仍不重启），既有那条 WARN（`src/server/mod.rs:1032`）保留不动。两处的 `attempt` 由**观测模块的 run 上下文**统一给出（重试层每次尝试时更新，`src/engine/policy.rs:87`；`map_intervention` 从上下文读）→ 字段集真正同构。`error_chain` 对 `Quality` 只有顶层一段（`DevforgeError::Quality` 无 `#[source]`，`src/error.rs:30`）——字段存在、值为顶层文本，不留空。落点 ② 的仓内覆盖点在 `src/server/mod.rs` 的 `#[cfg(test)]` 模块内（`map_intervention` 是私有方法，仓外验收文件 `tests/f6_observability/main.rs` 调不到）。**本轮修订**：R-1 把「重试层每次尝试时更新」前移到**尝试开始处**（`src/engine/policy.rs:79` 之前）→ 落点 ② 读到的 `attempt` 不再陈旧 1，见 R-1 与 §5 新增判据。）
- [必须裁决] **Q3 · 里程碑日志与 run 心跳的覆盖面** —— 需求点名的四类里程碑（阶段转换 / fix 往返 / 审查开庭 / 门禁对账）里，「阶段转换」「fix 往返」「审查开庭」在 MVP 与 doc 侧各有实现，铺开与否直接决定白名单大小与回归面。备选：A 全 SOP 全覆盖（change + MVP 三循环 + doc 两处都出里程碑与心跳）／B change 细粒度 + 全 SOP 的 SOP 级心跳／C 只做 change SOP。
  用户裁决：全 SOP 全覆盖
- [自决] **Q4 · 观测根的落点、解析顺序与仓内单测隔离路径** —— 落点：HOME 下 `.devforge/`（与日志目录同根，`src/main.rs:144`）：心跳 `root/heartbeat/<runid>.json`（**每 run 一份、覆盖写**），墓碑 `root/tombstones/<ts>-<runid>.txt`（**每次 panic 一份、逐个追加新文件**）。**root 可注入**（解析顺序）：**进程内显式注入**（公开 setter，单测用，`src/observability/mod.rs:251`）> `DEVFORGE_HOME` 环境变量（`src/observability/mod.rs:262`）> `HOME/.devforge`（生产默认）→ 仓内单测把 root 指向临时目录，**不落用户 HOME**。**仓内单测隔离路径取「① src/server/tests/mod.rs 进白名单并在夹具里注入临时 root」**（不取「② 观测模块给测试态默认临时目录」）：理由 ① 生产代码不引入「测试态」隐式分支（行为差异藏在一个 `cfg(test)` 分支里 = 事故现场会与生产不一致）；② 隔离是**显式可断言**的（夹具注入 → 断言落在该临时目录）；③ 已知坑清单已给出该路径的配套计数改写值。代价：`src/server/tests/mod.rs` 进 coding 步白名单（**23 个**，见 §4），并连带改写两处硬编码断言（Q10）。另注：进程级 root setter 被并行单测共享（谁最后设谁生效）→ 表现为落点漂移，**不影响正确性**（文件名含 run id，互不覆盖），登记为遗留（R12）。**已落地**：夹具注入在 `src/server/tests/mod.rs:111`（`obs_root`）与 `src/server/tests/mod.rs:144`（写进 `ServerState.obs_root`），服务端按 run 捕获落点在 `src/server/mod.rs:1360`。
- [自决] **Q5 · 「agent 轮次结束」里程碑的落点（二选一，不许两处都写）** —— 落在 `src/engine/client.rs:186` / `src/engine/client.rs:212`（`wait_settled*` 返回处，**一处覆盖全部 SOP**），字段含 `role`（`spec.role`，`src/engine/client.rs:158`）、`turn_ms`、`chars`；落点在 `src/engine/client.rs:276`。**change 侧不再另落交卷事件**：`src/change/session.rs:293`、`src/change/session.rs:382`、`src/change/session.rs:500` 三处**不写** `agent.turn_end`（理由：change 的三个干活 agent 也走 `RpcPlanSession::converse` → `src/engine/client.rs:212`，同一轮只会有一条；两处都写 = 同一轮两条交卷事件，反而看不清）。本场**只保留这一个交卷事件名**，不另立事件名。
- [自决] **Q6 · 「审查开庭」里程碑的落点与轮次号口径** —— 落在 **8 个落点**（change 三角色经 `review_once` 的 3 个调用 `src/change/session.rs:316` / `src/change/session.rs:477` / `src/change/session.rs:548`；MVP planning / coding / testing 各 1，即 `src/sop/planning.rs:132` / `src/sop/coding.rs:184` / `src/sop/testing.rs:224`；doc-from-zero 与 doc-fix 各 1，即 `src/sop/doc/runner.rs:338` / `src/sop/doc/fix.rs:93`），字段含 `role`（角色名）与 `round`（**该角色在本次 run 内的审查序号**）。**轮次号必须是调用前可知的**：`review_with_gate` 的实例数 `opened`（`src/sop/review_gate.rs:130`）是**返回值**，调用前拿不到。逐站点口径（**逐站点核对，不许统一 +1**）：

  | 角色 | `round` 来源（调用前可知） | 落点 |
  | --- | --- | --- |
  | change-plan-review | 该角色在本次 run 内的开庭序号（1-based，观测模块按角色计数，`src/observability/mod.rs:196`） | `src/change/session.rs:316` |
  | change-coding-review | 同上（角色内序号） | `src/change/session.rs:477` |
  | change-test-review | 同上（角色内序号） | `src/change/session.rs:548` |
  | plan-review（MVP） | `rounds + 1`（`rounds` 是**已开过的实例数**，在 `src/sop/planning.rs:146` 之后才自增） | `src/sop/planning.rs:132` |
  | coding-review（MVP） | `review_rounds + 1`（在 `src/sop/coding.rs:191` 之后才自增） | `src/sop/coding.rs:184` |
  | test-review（MVP） | `report.review_rounds + 1`（在 `src/sop/testing.rs:242` 之后才自增） | `src/sop/testing.rs:224` |
  | doc-review（doc-from-zero） | `bump_review` 的返回值（`src/sop/doc/runner.rs:87` 先自增，1-based） | `src/sop/doc/runner.rs:338` |
  | doc-review（doc-fix） | `report.review_rounds`（`src/sop/doc/fix.rs:91` **先自增再开实例**，1-based → **不带 +1**） | `src/sop/doc/fix.rs:93` |

  事件一律**在调用前**落（「开庭」= 发起那一刻）。**不改 `review_with_gate` 的签名**（全仓 11 处调用点，见事实 #19；加参数会波及上一场的验收面）。change 侧用「角色内序号」的理由：change 侧 `review_once` 恒传 `base_round=0`（`src/change/session.rs:283`），调用前拿不到实例数；且**不给 `ProdChangeSession` 加字段**（上一场加 `plan_review_instances` 时踩过「全仓字面量构造点必须同步补」的编译期耦合，见 R21）→ 角色内序号由观测模块按 `(run, role)` 计数提供。
- [自决] **Q7 · 里程碑的字段口径** —— 每条里程碑 = `component`（组件名）+ `event`（事件名，snake_case 点分）+ 事件特有字段（如 `from`/`to`/`role`/`round`/`result`/`count`）；`target` 由 `tracing` 自动给出 = 发出该宏的模块。**必须写在内联调用点**（不用包装函数）：包装函数会让所有里程碑的 `target` 变成同一个模块 → 「target 可过滤」这条需求直接作废。事件名固定八个：`phase.change` / `fix.start` / `fix.end` / `review.open` / `gate.baseline` / `gate.self_check` / `gate.step_commits` / `agent.turn_end`；重试侧另两个（不属于里程碑八件）：`run.attempt_failed` / `run.attempt_exhausted`。
- [自决] **Q8 · 心跳的写入点位与节奏** —— **事件驱动为主**（run 开始 / 阶段转换 / 关键步骤 / fix 起止 / 审查开庭 / 门禁结果 / 重试）+ **低频兜底**（watchdog 轮询线程按固定间隔刷新时间戳，证明进程还活着而不是卡住）。兜底来源 = **既有 watchdog 的轮询线程**（`src/server/mod.rs:1518`，线程体 `src/server/mod.rs:1530`，每 300ms 一跳 `src/server/mod.rs:1578`，**与 `idle_ms` 无关**——失活收割那段在 `if let Some(idle)` 里 `src/server/mod.rs:1539`）；**六个 SOP 全部经过它**——planning `src/server/mod.rs:318`、coding `src/server/mod.rs:389`、doc-from-zero `src/server/mod.rs:661`、testing `src/server/mod.rs:875`、doc-fix `src/server/mod.rs:991`、**change `src/server/change_engine.rs:203`** → change run（本靶主场景：boot 六连 + 15 分钟 agent 轮空洞）**同样有兜底**，长 agent 轮里心跳不会陈旧（「卡死 vs 已死」仍可分）。**不新增线程**（兜底刷新在 `src/server/mod.rs:1539` 调 `observability::touch_run`）。理由：事件驱动能回答「死在哪个阶段」（硬验收），低频兜底能区分「卡死」与「已死」——只有事件驱动时，一份 40 分钟前的心跳既可能是死了，也可能是活着在等模型，二者不可分。
- [自决] **Q9 · 墓碑写入的失败语义与 HOME 缺失降级** —— 墓碑写入是 **best-effort**：①hook 内**绝不** panic（写失败只记一行 stderr；hook 里二次 panic 会 abort，连 stderr 链回都丢 → R19）；②无论写成功与否，hook **都**链回上一个 hook（用 `std::panic::take_hook` 取回并链回，默认 stderr 输出不丢；安装接口要能取回上一个 hook 供单测还原，`src/observability/tombstone.rs:28`）；③HOME 未设 / root 不可写 / 磁盘满 → **降级**：不写墓碑、不 panic、照常链回。**库路径不经过 `src/main.rs:143` 的 HOME 检查**（验收测试直调 hook 安装接口）→ hook 自己必须能承受 HOME 缺失（root 解析失败时用占位 run id + 不落盘，`src/observability/tombstone.rs:19`）。
- [自决] **Q10 · 白名单守卫常量的例行刷新口径（F10 现状沿用）** —— 本场 step 1 刷三份常量副本（`tests/change_ask_ring.rs:35`、`tests/change_ask_ring.rs:39`、`tests/change_failure_context/main.rs:36`、`tests/change_failure_context/main.rs:40`、`tests/change_review_protocol/main.rs:47`、`tests/change_review_protocol/main.rs:51`、`tests/change_review_protocol/main.rs:79`）**并摘除** `tests/change_ask_ring.rs:629` 的 `#[ignore]`；本场收尾（提交完成后）**复贴**该 `#[ignore]`（与上一场收尾提交 `a0be947` 同款「两场之间 ignore 复贴」）。F10 的真修（常量 gate 时物化）**不在本靶**。**并且必须同步改写两处硬编码断言**（否则 step 1 之后测试面直接红）：①`tests/change_review_protocol/main.rs:1337`（a29）硬编码「守卫 WHITELIST 里**不得**含 `tests/change_review_protocol/main.rs`」→ 本场的 coding 步白名单**含**该文件（它要在 step 1 刷常量）→ 排除项改为**本场测试阶段文件** `tests/f6_observability/main.rs`（或泛化成「测试阶段文件不得进 coding 步白名单常量」）；②`tests/change_review_protocol/main.rs:1437`（a32 `a32_the_plan_whitelist_has_twenty_three_files`）硬编码 `PLAN_WHITELIST.len() == 20` / `CODING_WHITELIST.len() == 19`，并断言「§4 白名单含**唯一的**测试阶段文件 `tests/change_review_protocol/main.rs`」→ 改为 **24 / 23**（24 = 23 个 coding 步文件 + 1 个测试阶段文件；Q4 选了路径 ① → coding 步白名单含 `src/server/tests/mod.rs`），测试阶段文件断言改为 `tests/f6_observability/main.rs`。**口径选择**：保留「coding 步 23 个 + 测试阶段 1 个」的结构（**不**把既有验收文件的常量刷新推到测试阶段——那样 a29/a30 在 coding 步就会拿旧常量比对新守卫而变红），代价就是必须改写上述两处硬编码（列入 §3.1 与 §5）。**本轮修订**：这三处硬编码已在 step 15/16 落地（a32 断言现为 `tests/change_review_protocol/main.rs:1439` 的 24 / `tests/change_review_protocol/main.rs:1440` 的 23），本轮只剩 R-4（注释与值一致）。
- [自决] **Q11 · 「profile 未就绪」的最终错误种类** —— 从裸 `System` 改为 `DevforgeError::Config`（配置错误，`src/error.rs:33` 已存在）：`is_infra_failure` 不认它 → **不触发自动重启**（attempt 恒为 1）。**错误文本一字不改**（`src/server/mod.rs:277` 的原文已含「profile `X` 未渲染（缺 <路径>/settings.json）——先 PUT /api/v1/config/manifest 完成安装渲染，再启动 run」；追加同类句子会重复）→ 仓内 **5** 条既有断言（`src/server/mod.rs:1918`、`src/server/mod.rs:1932`、`src/server/mod.rs:1949`、`src/server/mod.rs:1968`、`src/server/mod.rs:1984`，用 `contains("未渲染")`；分属门卫文本用例 `src/server/mod.rs:1910` 与四个 run_* 用例 `src/server/mod.rs:1922`/`src/server/mod.rs:1940`/`src/server/mod.rs:1959`/`src/server/mod.rs:1976`）与两个 e2e 必然仍绿。理由：「未渲染」是**配置问题**不是基础设施故障（重启不会让它变好）。
- [自决] **Q12 · 文档登记落点** —— 追加 `## D006 ·` 到 `docs/plantree/plans/010-run-resume/decisions/README.md`（010 是 run 生命周期/韧性的归属计划，D004「基础设施故障自动重启」就在那里；D006 已在 `docs/plantree/plans/010-run-resume/decisions/README.md:53` 落地，**只追加、不动 D001–D005**）+ `docs/plantree/plans/010-run-resume/roadmap.md` 追加一条本场条目（现 75 行）。**不改** `docs/plantree/plans/009-change-sop/**`（本靶不改 change SOP 的业务语义）与 `docs/plantree/plans/007-testing-stage/**`（那里只是本靶的**引证来源**：装包分钟级实测在 `docs/plantree/plans/007-testing-stage/roadmap.md:33` 与 `docs/plantree/plans/007-testing-stage/evidence/e2e-t008-findings.md:51`，**不在** 002-config-management）。
- [自决] **Q13 · 「全 SOP 全覆盖」的具体边界** —— 里程碑覆盖：change 编排层（阶段转换 + fix 往返 + 审查开庭 + 门禁对账）、MVP planning/coding/testing 三循环（阶段推进 + fix 往返 + 审查开庭）、doc-from-zero（单元推进 + 审查开庭 + **单元审查 FAIL → 回注 agent 重写**，`src/sop/doc/runner.rs:427`）、doc-fix（审查轮 + 回注，`src/sop/doc/fix.rs:136`）。**不覆盖**：MVP 侧的「基线门/步提交对账」（那是 change 路径独有的机械门）、doc 侧的单元级 lint 细节。理由：Q3 裁决的是「全 SOP 全覆盖」，但「基线门/步提交对账」在 MVP/doc 侧**不存在对应物**（不是漏做，是无物可做）；「每轮 fix 开始/结束」在四个非 change 循环里都有对应物 → 一律覆盖。
- [必须裁决] **Q14 · 本轮（attempt 2）修订计划的范围：只订正文本，还是同时把上一轮编码审查的 4 条观察项按计划字面口径修掉** —— 测试阶段的回注只点了「计划 §5 的数字与现状不一致」（文本层）；上一轮编码审查另留了 4 条**实现与计划字面有出入**的观察项（attempt 写入时机、闸门放行条件、`end_render` 兜底、守卫常量注释）。两者的差别直接决定下一轮 coding 是否有真实改动、以及 §5 是否新增判据 → **影响验收口径**，不由 agent 单方面定。备选：A 文本订正 + 4 处实现对齐（本轮全修掉，不登记遗留）／B 只订正文本，4 处偏差登记为遗留（§6）／C 只做闸门相关的两处，其余登记为遗留。
  用户裁决：文本订正 + 4 处实现对齐（推荐）——用户拍板（2026-10-23）：§3.1 新增 R-1..R-4 四行、§5 新增 4 条对应判据，四条观察项（attempt 时序前移 / 闸门放行条件回环重判 / end_render drop 守卫 / 守卫常量注释同步）全部在本场修掉，不登记遗留。

---

## 2 影响面

> 说明：本节每条都带 `path:line`；**四个新增文件已存在**（`src/observability/mod.rs`、`src/observability/heartbeat.rs`、`src/observability/tombstone.rs`、`tests/f6_observability/main.rs`，steps 2/4 落地），机械门对白名单路径只核**存在性**（`src/change/plan_audit.rs:266`），本节按当前 `HEAD=458c969` 的行号给锚点。

### 2.1 直接命中

- `src/engine/policy.rs:68` —— `run_with_retry`：需求 1 的**主落点**。现状：每次尝试失败先落一条分级条目（`src/engine/policy.rs:88` → `src/engine/policy.rs:124`），再决定重试（`src/engine/policy.rs:83`），终态再落一条（`src/engine/policy.rs:155`）。**本轮 R-1**：把 `set_attempt(attempt)`（`src/engine/policy.rs:87`）从失败分支前移到**尝试开始处**（`src/engine/policy.rs:79` 之前）→ 第 2 次及以后的尝试期间若失败被 `map_intervention` 收成质量失败，落点 ② 读到的 `attempt` 不再是上一次的值。
- `src/engine/policy.rs:60` —— `is_infra_failure`：决定「这次失败会不会被自动重启」的总开关（只认裸 `System`）。本靶**不改它的判据**，但 Q11 已把「profile 未就绪」这一处错误的**种类**改成 `Config`（`src/server/mod.rs:269`）。
- `src/server/mod.rs:269` —— `require_profile`：boot 根因的落点。判据（`settings.json` 存在，`src/server/mod.rs:271`）与错误种类（已是 `Config`）都在这里；错误**文本**保持（Q11，`src/server/mod.rs:277`）。
- `src/server/mod.rs:1066` —— `RenderGate`：渲染闸门状态（在途计数 + `watch` 通知）。**本轮 R-2**：`wait_for_profiles`（`src/server/mod.rs:1106`）的 `rx.changed()` 分支改为**回环重判**放行条件（无渲染在途 **或** 所需 profile 全部就绪）→ 并发两轮渲染时不会因另一轮结束而提前放行。
- `src/server/mod.rs:1388` —— `run_in_background` 的重试闭包：boot 期 WARN 在 `src/server/mod.rs:1408`（**不带错误原文**）；run 级里程碑与心跳的落点（run 开始 `src/server/mod.rs:1360`、run 结束清位），并置位/清位 run 上下文。
- `src/server/mod.rs:1017` —— `map_intervention`：**只作用于 SOP/编排执行期**的错误（调用点 `src/server/mod.rs:339`、`src/server/mod.rs:414`、`src/server/mod.rs:806`、`src/server/mod.rs:945`、`src/server/mod.rs:1012` 与 `src/server/change_engine.rs:240`，全部挂在 `sop_result` 的 Err 上）：Quality → 明确 Failed（不重启，`src/server/mod.rs:1030`），其余原样上抛 → 重试。**启动门卫的失败不经它** → 本靶**不改判定**（Quality 仍转 `Ok(Failed)`、仍不重启），但**在质量分支按 Q2 的同形状补了一条分级 WARN**（`src/server/mod.rs:1044`），既有那条 WARN（`src/server/mod.rs:1032`）保留不动。**R-1 的读侧**：这条 WARN 的 `attempt` 取自观测模块的 run 上下文（`src/observability/mod.rs:182`）→ 写入时机前移后，本条读到的才是本次尝试号。
- `src/server/change_engine.rs:78` —— change SOP 的**启动门卫**（`for p in CHANGE_PROFILES { self.require_profile(p)?; }`）：boot 期失败从**这里**直接上抛（**不经** `map_intervention`）→ 直达 `run_with_retry`（`src/server/mod.rs:1388`）→ `is_infra_failure`（`src/engine/policy.rs:60`）→ 退避 10s → attempt=2（**这正是 boot 六连的触发点**，证据 1 的 06:29:36.893）。本靶**不改这个文件**（错误种类在 `src/server/mod.rs:269` 改；闸门所需清单在 `src/server/mod.rs:253` 的统一表里取）→ 但**渲染闸门必须覆盖这六个 profile**（清单事实源 `src/server/change_engine.rs:24`）。
- `src/server/handlers.rs:371` —— `put_manifest`：渲染闸门的**置位点**（`src/server/handlers.rs:390`）、后台渲染线程（`src/server/handlers.rs:392`）、渲染结束清位（`src/server/handlers.rs:415`）。**本轮 R-3**：`emit`（`src/server/handlers.rs:405`/`src/server/handlers.rs:408`/`src/server/handlers.rs:411`）在 `catch_unwind` 之外 → 通知发出时抛异常则 `end_render()` 永不执行 → `start` 永久等待 → 改用 drop 守卫保证在途计数必归零。
- `src/server/handlers.rs:202` —— `start_task`：渲染闸门的**等待点**（`src/server/handlers.rs:240`，awaitable，不占 tokio worker），随后才 spawn run 线程（`src/server/handlers.rs:243`）；串行槽快判在 `src/server/handlers.rs:224`，真正占槽的 `begin_run` 在 `src/server/mod.rs:1340`。
- `src/server/mod.rs:253` —— `profiles_for_sop`：**六个 SOP 所需 profile 的唯一事实源**（门卫与闸门读同一张表）：planning `src/server/mod.rs:288`、coding `src/server/mod.rs:347`、doc-from-zero `src/server/mod.rs:422`、testing `src/server/mod.rs:832`、doc-fix `src/server/mod.rs:953` 各自的 `require_profile` 调用都查这张表；change 一项**引用** `src/server/change_engine.rs:24` 的 `CHANGE_PROFILES`（那个文件本身不改，见 §3.2）。
- `src/main.rs:143` —— HOME 根（`src/main.rs:144`）与日志初始化（`src/main.rs:146`）、观测根解析（`src/main.rs:149`）：panic hook 的**安装点**（进程启动即装，覆盖整个进程生命周期）。
- `src/lib.rs:16` —— 模块清单：挂载新模块 `observability`。
- `src/engine/client.rs:186`、`src/engine/client.rs:212` —— `wait_settled` / `wait_settled_with_ui`：**agent 轮次结束**里程碑的**唯一**落点（`src/engine/client.rs:276`；`spec.role` 现成，`src/engine/client.rs:158`；Q5：change 侧不再另落）。
- `src/change/orchestrator.rs:30` —— `phase_change`（阶段转换的唯一入口，**5 处**调用：`src/change/orchestrator.rs:204`、`src/change/orchestrator.rs:231`、`src/change/orchestrator.rs:317`、`src/change/orchestrator.rs:346`、`src/change/orchestrator.rs:361`；`src/change/orchestrator.rs:232` 是 `save(...)`，**不是**阶段写入）；`src/change/orchestrator.rs:45` —— `fix_step`（fix 起止里程碑 + 心跳步骤字段的唯一入口）。
- `src/change/session.rs:264` —— `review_once`（change 三角色共用的审查入口）：「审查开庭」里程碑的落点（含 `role` 与 `round`；Q6）。
- `src/change/session.rs:292` —— `run_plan_with`：plan 段的阶段内动作（`src/change/session.rs:293` 是 plan agent 交卷 → **由 `src/engine/client.rs` 统一落交卷事件，本处不落**，Q5）。
- `src/change/session.rs:375` —— `run_coding`：基线轮交卷（`src/change/session.rs:382` → 同上，**不落**）、基线门（`src/change/session.rs:389`）、自证门（`src/change/session.rs:428`）、步提交对账（`src/change/session.rs:474`）、coding 轮审查的里程碑。
- `src/change/session.rs:495` —— `run_testing`：test agent 轮次（`src/change/session.rs:500` → 同上，**不落**）、test 审查（`src/change/session.rs:477`）的里程碑。
- `src/sop/planning.rs:108` —— MVP planning 循环：plan 阶段推进、审查开庭（`src/sop/planning.rs:132`，含轮次）、判负回注 fix 起止的里程碑。
- `src/sop/coding.rs:162` —— MVP coding 循环：stage 推进（`src/sop/coding.rs:194`/`src/sop/coding.rs:202`）、审查开庭（`src/sop/coding.rs:184`，含轮次）、打回 fix 起止的里程碑。
- `src/sop/testing.rs:202` —— MVP testing 循环：阶段推进（`src/sop/testing.rs:245`/`src/sop/testing.rs:249`）、审查开庭（`src/sop/testing.rs:224`，含轮次）、doc-fix 移交（`src/sop/testing.rs:256`）与回注（`src/sop/testing.rs:260`）的里程碑。
- `src/sop/doc/runner.rs:289` —— doc 单元循环：单元推进、审查开庭（`src/sop/doc/runner.rs:338`，含轮次）、**单元审查 FAIL → 回注 agent 重写**（`src/sop/doc/runner.rs:427`，fix 往返的落点）的里程碑。
- `src/sop/doc/fix.rs:93` —— doc-fix：审查开庭（含轮次 = `report.review_rounds`，**不带 +1**，因 `src/sop/doc/fix.rs:91` 已先自增）、回注 fix 起止（`src/sop/doc/fix.rs:136`/`src/sop/doc/fix.rs:143`）的里程碑。
- `src/sop/review_gate.rs:78` —— `review_with_gate`（全仓共用的审查门）：**不改**（三通道机制与上限常量不动），但它的**生产 4 处调用点**是「审查开庭」里程碑的落点（change 侧经 `review_once` 展开为 3 个落点，见 Q6）。
- `src/observability/mod.rs:84` —— `begin_run_in`：run 上下文置位 + **按 run 捕获观测根**（单测把落点钉在自己的临时目录 → 不落用户 HOME，R12）；`src/observability/mod.rs:133` / `src/observability/mod.rs:150` —— `set_phase` / `set_step`（阶段与步骤的唯一写入口，顺带刷心跳）；`src/observability/mod.rs:170` —— `set_attempt`（**R-1 的写侧**：重试层每次尝试时更新，`map_intervention` 从 `src/observability/mod.rs:182` 读同一个值）。
- `src/observability/heartbeat.rs:36` —— 心跳**覆盖写**（幂等收敛）；路径 `root/heartbeat/<runid>.json`（`src/observability/heartbeat.rs:31`）；读回 `src/observability/heartbeat.rs:55`。
- `src/observability/tombstone.rs:44` —— panic hook 安装（`std::panic::take_hook` + 链回，还原接口 `src/observability/tombstone.rs:28`）；墓碑写入 `src/observability/tombstone.rs:72`（**best-effort**，Q9）；无活动 run 的占位 run id `src/observability/tombstone.rs:19`。
- `src/server/tests/mod.rs:100` —— `fixture_full`（仓内单测的夹具，`src/server/tests/mod.rs:107` 建临时目录、观测根注入在 `src/server/tests/mod.rs:111`、写进 `ServerState` 在 `src/server/tests/mod.rs:144`、`Fixture` 构造于 `src/server/tests/mod.rs:153`）：**Q4 路径 ① 的注入点**——使走真实 start 路径的单测（`src/server/tests/mod.rs:958` 的 `start_task_runs_engine_and_records` → `src/server/handlers.rs:243` → `src/server/mod.rs:1360` 置位 run 上下文 → 心跳真写 root）**不落用户 HOME**（R12）。
- `tests/change_ask_ring.rs:35` —— 白名单守卫常量（`tests/change_ask_ring.rs:39`）与 `#[ignore]`（`tests/change_ask_ring.rs:629`）、「上一场遗留文件不得进本变更」断言（`tests/change_ask_ring.rs:663`）：F10 例行刷新的落点（Q10）+ **R-4 的注释订正落点**（`tests/change_ask_ring.rs:32`/`tests/change_ask_ring.rs:38`）。
- `tests/change_failure_context/main.rs:36` —— 守卫常量的孪生副本（`tests/change_failure_context/main.rs:40`）与比对用例（a16，`tests/change_failure_context/main.rs:959`）：必须与守卫逐字一致；**R-4** 的注释订正落点（`tests/change_failure_context/main.rs:33`/`tests/change_failure_context/main.rs:39`）。
- `tests/change_review_protocol/main.rs:47` —— 上一场验收文件的第三份常量副本（`tests/change_review_protocol/main.rs:51`、`tests/change_review_protocol/main.rs:79`）与三条对账用例（a29 `tests/change_review_protocol/main.rs:1319`、a30 `tests/change_review_protocol/main.rs:1378`、a31 `tests/change_review_protocol/main.rs:1398`）；**a32 的计数断言**在 `tests/change_review_protocol/main.rs:1439`/`tests/change_review_protocol/main.rs:1440`；**R-4** 的注释订正落点（`tests/change_review_protocol/main.rs:44`/`tests/change_review_protocol/main.rs:1316`）。
- `src/change/coding_gate.rs:266` —— `check_step_commits`（步提交对账门）：本变更**已合法改它**（step 15 的 F15/F16 解析容差），它的**判定语义不动**；`src/change/coding_gate.rs:92`（基线门）、`src/change/coding_gate.rs:118`（自证门）、`src/change/coding_gate.rs:193`（提交集合扫描）同理。
- `docs/plantree/plans/010-run-resume/decisions/README.md:53` —— 决策台账 D006（三件交付物 + 闸门口径 + 分级口径 + 分类修正 + 遗留清单；**只追加、不动 D001–D005**）。
- `docs/plantree/plans/010-run-resume/roadmap.md` —— roadmap 末尾（现 75 行）：本场条目（Phase 4 / T010 / M5）。
- `tests/f6_observability/main.rs` —— 本靶的**验收对账测试**（黑盒口径，仿 `tests/change_review_protocol/main.rs` 的风格：只经公开 API 观察，每条用例首行写「错实现形态」）：逐条对账 §5；含「注入 panic → 墓碑存在且含 panic 原文」与「阶段推进 → 心跳文件内容随之变化」两条硬验收。

### 2.2 调用链

- **boot 六连全链（根因链，逐跳带位置）**：`POST /start`（`src/server/handlers.rs:202`）→ **闸门等待**（`src/server/handlers.rs:240`）→ `run_in_background`（`src/server/mod.rs:1332`）→ `RunEngine::execute`（`src/server/mod.rs:233`）→ `run_change`（`src/server/change_engine.rs:66`）→ **change 启动门卫**（`src/server/change_engine.rs:78` → `require_profile` `src/server/mod.rs:269`）→ `Config` 错误（`src/server/mod.rs:277`）→ **不经 `map_intervention`**（门卫的 `?` 在 watchdog/编排之前直接上抛；`map_intervention` 只管 `sop_result` 的 Err，调用点 `src/server/change_engine.rs:240` 与 `src/server/mod.rs:339` 等）→ 直达 `run_with_retry`（`src/server/mod.rs:1388`）→ `is_infra_failure`（`src/engine/policy.rs:60`，**不认 `Config`**）→ **不重启**，由 `src/server/mod.rs:1429` 以错误原文记 Failed。**其余五个 SOP 同构**（门卫的 `?` 也在 watchdog 之前，如 planning `src/server/mod.rs:288`）。**竞态的另一端**：`put_manifest`（`src/server/handlers.rs:371`）→ 后台渲染线程（`src/server/handlers.rs:392`）→ `render_all`（`src/config/render.rs:222`）→ `render_agent`（`src/config/render.rs:38`）：先建目录+`AGENTS.md`（`src/config/render.rs:50`），后写 `settings.json`（`src/config/render.rs:63`）→ 装包（`src/config/render.rs:205`）。
- **渲染闸门的新链（用户裁决 A）**：`start_task`（`src/server/handlers.rs:202`）→ 查本 run 所需 profile（清单的唯一事实源：`src/server/mod.rs:253` 的 per-SOP 表，change 项引用 `src/server/change_engine.rs:24`）→ 若有渲染在途 → 等「所需 profile 的 `settings.json` 落盘」**或**「**无渲染在途**」（**R-2 改的就是这条放行条件**：`src/server/mod.rs:1106`）→ 再 spawn run（`src/server/handlers.rs:243`）→ `require_profile` 复查（`src/server/mod.rs:269`）→ 仍缺 → `Config` 明确失败（不再重启）。
- **原始错误可见的新链**：run 失败 → `run_with_retry`（`src/engine/policy.rs:68`）→ 每次尝试失败落一条**分级**条目（`src/engine/policy.rs:88` → `src/engine/policy.rs:124`）→ `tracing` → `src/logging.rs:33` 的 JSONL 层 → `HOME/.devforge/logs/devforge.log.jsonl`。
- **里程碑链**：各 SOP 循环 / 编排层 → 内联 `tracing::info!`（调用点即 target）→ `src/logging.rs:33` → `HOME/.devforge/logs/devforge.log.jsonl`。
- **审查链（8 个「审查开庭」落点）**：`src/change/session.rs:316`/`src/change/session.rs:477`/`src/change/session.rs:548`（`review_once` → change 三角色，里程碑本体在 `src/change/session.rs:273`）、`src/sop/planning.rs:132`、`src/sop/coding.rs:184`、`src/sop/testing.rs:224` → 全部经 `review_with_gate`（`src/sop/review_gate.rs:78`，**生产 4 处调用点**）；`src/sop/doc/runner.rs:347`、`src/sop/doc/fix.rs:101` → 直连 `factory.open()`（**不走**共享门）。**轮次号口径**：逐站点表见 Q6（**doc-fix 不带 +1**）。
- **agent 轮次链**：`src/sop/rpc_bridge.rs:24`（`RpcPlanSession::converse`）/ `src/sop/rpc_bridge.rs:76`（`RpcReviewSession::review`）→ `src/engine/client.rs:186`/`src/engine/client.rs:212`（`wait_settled*`）→ 里程碑落点 `src/engine/client.rs:276`（角色 = `src/engine/client.rs:158` 的 `spec.role`）。**一处覆盖全部 SOP**（change 六角色 / MVP 六角色 / doc 三角色都走这条链；change 的三个干活 agent 走 `RpcPlanSession` → `src/engine/client.rs:212`）。
- **心跳链**：编排层与引擎 → 观测模块的 run 上下文（阶段/步骤）→ `src/observability/heartbeat.rs:36` → root 下 `heartbeat/<runid>.json`（root 解析顺序见 Q4；按 run 捕获在 `src/server/mod.rs:1360`）；低频兜底刷新挂在**既有 watchdog 的轮询线程**上（`src/server/mod.rs:1518`、线程体 `src/server/mod.rs:1530`、300ms 一跳 `src/server/mod.rs:1578`；**六个 SOP 全覆盖**：planning `src/server/mod.rs:318`、coding `src/server/mod.rs:389`、doc-from-zero `src/server/mod.rs:661`、testing `src/server/mod.rs:875`、doc-fix `src/server/mod.rs:991`、**change `src/server/change_engine.rs:203`**）→ **不新增线程**。
- **墓碑链**：`std::panic` → `src/observability/tombstone.rs:44` 的 hook（进程启动时装在 `src/main.rs:146` 之后）→ 读 run 上下文取「当时阶段/步骤」→ 写 root 下 `tombstones/<ts>-<runid>.txt`（`src/observability/tombstone.rs:72`）→ hook 内再链回上一个 hook。**写入 best-effort**（Q9）：写失败 / 无 HOME → 只记一行 stderr，不 panic，照常链回。

### 2.3 隐式耦合

- **错误种类 = 「会不会被自动重启」的总开关**：`src/engine/policy.rs:60` 只认裸 `System`；`src/server/mod.rs:1017` 把 Quality 变成明确的 Failed（不重启），其余原样上抛 → 重启（`src/server/mod.rs:1388`）。**同一句失败文案，错误种类选错 = 一次整 run 重启**（本靶两处都动这个分岔：Q11 把「profile 未就绪」挪出 System；需求 1 只加可见性、**不改**分类）。
- **渲染的「完成」不是同步事件**：`put_manifest` 返回 202 时渲染还在跑（`src/server/handlers.rs:392`），完成信号只经 SSE 广播（`src/server/handlers.rs:405`）→ **闸门必须自己持有一份「在途」状态**（`src/server/mod.rs:1066`），不能靠 SSE 事件流。
- **在途计数与放行是两件事（R-2 的根因）**：`RenderGate` 的在途计数是**计数器**（`src/server/mod.rs:1083`/`src/server/mod.rs:1089`），而放行通知是**广播**（`watch`）→ 「任意一次渲染结束」与「渲染全部结束」在并发场景下不是一回事 → 放行条件必须回到计数器上重判，不能由通知直接决定。
- **闸门释放必须有异常兜底（R-3 的根因）**：在途计数是**手工**递减的（`src/server/handlers.rs:415`）→ 线程体里任何一次提前 panic/提前返回都会让计数**永久大于 0** → 之后的 `start` 永远在等 → 必须用 drop 守卫让「计数归零」成为**不可绕过的**事实。
- **门卫判据是「文件存在」而不是「profile 可用」**：`settings.json` 在每个 profile 渲染的**早期**就落盘（`src/config/render.rs:63`，装包之前），装包期间还可能被安装器回写（`src/config/render.rs:192`）→ 闸门等「所需 profile 的 `settings.json` 落盘」是**秒级**；等「整个渲染任务结束」是**分钟级**（装包实测：首次全量八 profile 65.4s、旧串行基线 >20min，见 `docs/plantree/plans/007-testing-stage/roadmap.md:33`；八 profile 全供给渲染 >20 分钟的现场记录见 `docs/plantree/plans/007-testing-stage/evidence/e2e-t008-findings.md:51`；代码侧同口径注释在 `src/config/render.rs:219`）→ 闸门等前者，否则 `start` 端点会变成一个分钟级阻塞点（e2e 的 start 请求默认超时只有 10s，`tests/e2e/common.py:132`）。
- **`tracing` 的 `target` = 发出宏的模块**（不是包装函数的模块）→ 里程碑必须内联写（Q7）。这同时意味着「一处实现」在里程碑上不可行，只能在**事件名**上统一口径。
- **既有日志的文案不能与新里程碑混淆**：既有 WARN 都带 `component`（`src/server/mod.rs:1408`、`src/sop/review_gate.rs:139`、`src/sop/review_gate.rs:163`、`src/server/mod.rs:1401`）→ 新里程碑一律用**新 event 名**（Q7 的八个 + 重试侧两个），事件名不得复用既有文案。
- **`review_with_gate` 全仓 11 处调用点**（生产 4：planning/coding/testing/change-session；仓内单测 5：仓内共享门自己的用例；仓外验收文件 2：`tests/change_review_protocol/main.rs` 的直调）→ **不改它的签名**（Q6）；角色与轮次由调用点自己写。
- **不给 `ProdChangeSession` 加字段**：上一场加 `plan_review_instances` 时踩过「全仓字面量构造点必须同步补」的编译期耦合（生产装配 `src/server/change_engine.rs:180`、仓内单测、两份仓外夹具）→ 本靶的 change 侧轮次信息走**观测模块的角色计数**（Q6），不改结构体。
- **闸门的等待发生在 axum 的 async handler 里**：`start_task`（`src/server/handlers.rs:202`）是 `async fn`（在 tokio worker 上执行）→ 闸门若在 handler 里**阻塞**等待渲染，会占住 tokio worker 线程（并发请求被拖住；worker 数有限 → 拖满即全站假死）。等待必须用**可 await 的原语**（`tokio::sync::watch`，`src/server/mod.rs:1070`），或整体挪进 `spawn_blocking`；**不得**用 `std::thread::sleep` 轮询（R17）。
- **`src/change/coding_gate.rs` 已在本变更内**（操作员补录扩容，step 15）：它的**判定语义**（基线门/自证门/步提交对账的通过条件）一字未改，本变更只改了**它自己的解析器容差**（F15 步号只认紧跟标记的数字、F16 证据日志认 markdown 列表项）与守卫的豁免前提（step 17）→ 里程碑仍落在**调用点**（`src/change/session.rs:389`/`src/change/session.rs:428`/`src/change/session.rs:474`），门本身不加日志。
- **心跳是覆盖写**（纪律第 12 条：状态文件要能被覆盖写，不能追加成第二份真相）→ 心跳只留「最后一次状态」；「本 run 发生过几次阶段转换」由里程碑日志回答，**不**靠心跳。
- **run 上下文是线程级归属 + 进程级快照表**（`src/observability/mod.rs:59`/`src/observability/mod.rs:43`）：MVP 是串行执行（`src/server/handlers.rs:224` 的 `has_running_run` 守卫）→ 同一时刻只有一个 run；将来并发化时心跳/墓碑会互相覆盖（登记为遗留，R3）。
- **panic hook 是进程级全局**（`std::panic::set_hook` 只能有一个）→ 仓内单测若装 hook 会影响同进程其他测试 → 验收测试放**独立测试二进制**（`tests/f6_observability/main.rs`），且安装接口要能取回上一个 hook 以便还原（`src/observability/tombstone.rs:28`）。
- **闸门给「先 PUT 清单再 start」的 e2e 脚本新增了一次等待（新失败面）** —— 闸门等的是「本 run 所需 profile 的 `settings.json` 落盘」，而 e2e 的 start 请求默认超时只有 10s（`tests/e2e/common.py:132`）。已核事实：**8 个脚本是「先 PUT 清单再 start」**——`tests/e2e/test_07_rerun_serial.py:46`→`:107`、`tests/e2e/test_08_e2e_planning.py:86`→`:128`、`tests/e2e/test_09_run_control.py:34`→`:47`、`tests/e2e/test_13_coding_preflight.py:33`→`:46`、`tests/e2e/test_14_coding_full_chain.py:59`→`:94`、`tests/e2e/test_15_coding_run_control.py:32`→`:47`、`tests/e2e/test_16_run_liveness.py:71`→`:83`、`tests/e2e/test_19_testing_full_chain.py:67`→`:101`；其中 **5 个在 start 前已等齐自己 SOP 所需的全部 profile**（`tests/e2e/test_07_rerun_serial.py:49`–`:59` 等两个 profile 的安装产物、`tests/e2e/test_08_e2e_planning.py:88`–`:98`、`tests/e2e/test_09_run_control.py:38`、`tests/e2e/test_14_coding_full_chain.py:64`、`tests/e2e/test_19_testing_full_chain.py:72` 等八个 profile 的 `settings.json`）→ 闸门**不新增等待**；**3 个不等或只等一个 profile**：`tests/e2e/test_13_coding_preflight.py:33`（完全不等渲染）、`tests/e2e/test_15_coding_run_control.py:35`–`:39`（只等 `coding-agent`）、`tests/e2e/test_16_run_liveness.py:73`–`:77`（只等 `plan-agent`）→ 闸门会**为同一 SOP 的第二个 profile 追加一次等待**（`coding-review` / `plan-review`）→ 若该 profile 的 `settings.json` 尚未落盘（它落在自己那个渲染 worker 的早写点 `src/config/render.rs:63`，但要等 worker 轮到它），start 请求会**超过 10s 预算**（`tests/e2e/common.py:132`）→ 这三个脚本是闸门引入的新失败面（缓解与遗留见 R2，回归项见 §3.3）。**R-2 收紧放行条件后，这个面只会更大不会更小**（并发渲染时多等一轮，见 R26）。
- **tracing 的落盘不是零丢失**：`src/logging.rs:36`–`:40` 的 JSON 层用 `Mutex<File>` 同步写（writer 在 `src/logging.rs:40`），但进程猝死（SIGKILL）时缓冲区里的最后几条可能丢 → **里程碑日志不保证零丢失**（它是「过程记录」），墓碑与心跳才是「死亡现场」的硬证据。
- **质量失败在生产链路上到不了重试层（Q2 的可达性陷阱）** —— `map_intervention` 的质量分支（`src/server/mod.rs:1030`）把 `DevforgeError::Quality` 转成 `Ok(RunOutcome::Failed)`（`src/server/mod.rs:1044`）→ 该错误**不会**回到 `run_with_retry`（`src/engine/policy.rs:68`）→ 只改重试层的话，Q2 的「质量类落 WARN、字段同构（含 source 链每层）」在生产**不可达**（唯一现成触发器是仓内 mock 引擎 `src/server/tests/mod.rs:35`，它绕过 `map_intervention` 直接返回 `Err(Quality)`）→ 生产链路上那条同形状的 WARN 落在 `src/server/mod.rs:1044`（既有那条 `src/server/mod.rs:1032` 保留不动）。两个落点的 `attempt` 由观测模块的 run 上下文统一给出 → 字段集真正同构；**R-1 修的正是这个「同构」的取值**（写入时机前移 → 落点 ② 读到的不是上一次的值）。
- **计划文本的机械门**：六块名固定（`src/change/plan_audit.rs:13`）；白名单里的路径按**反引号**抽取（`src/change/plan_audit.rs:336`）→ 新文件必须用反引号写进白名单（否则不算白名单 → coding 步提交它 = 越界）；`path:line` 引用逐条验存在性（`src/change/plan_audit.rs:256`）→ 新文件**不能**带 `:行号`；验收标准只认**列表行**（`src/change/plan_audit.rs:364`）→ §5 禁用表格。
- **三份守卫常量副本互相比对**（事实 #31）→ 本场刷新必须**三份一起刷**，漏一份即红（a29/a30/a16）。
- **两处硬编码断言已随本场白名单口径改写**（Q10）：`tests/change_review_protocol/main.rs:1337`（a29：守卫 WHITELIST 里**不得**含 `tests/f6_observability/main.rs`）与 `tests/change_review_protocol/main.rs:1437`（a32：计数）→ 现值见 `tests/change_review_protocol/main.rs:1439`/`tests/change_review_protocol/main.rs:1440`（24 / 23）；排除项已换成本场测试阶段文件 `tests/f6_observability/main.rs`。
- **仓内单测会真的落盘**：单测里 `src/server/tests/mod.rs:153` 的夹具构造 router、`src/server/tests/mod.rs:958` 起的 `start_task_runs_engine_and_records` 走**真实 start 路径** → `src/server/handlers.rs:243` → `src/server/mod.rs:1360` → 置位 run 上下文 → 心跳**真写**进 root（run id 唯一、run 结束保留）→ 若 root 默认 `HOME/.devforge`，用户的 HOME 会被单测写脏 → **root 必须可注入**（Q4），本场取路径 ①：夹具注入临时 root（`src/server/tests/mod.rs:111` 落点）。
- **panic hook 里二次 panic = abort**：写墓碑失败若在 hook 里 panic，会在 panic 中 panic → 进程 abort，**连 stderr 链回都丢**（最后一条遗言也没了）→ 写入必须 best-effort（Q9）、hook 必须无条件链回上一个 hook。**库路径不经过 `src/main.rs:143` 的 HOME 检查**（`tests/f6_observability/main.rs` 直调安装接口）→ hook 自己要能承受 HOME 缺失。
- **本机日志文件不在仓内**：`~/.devforge/logs/devforge.log.jsonl`（证据 1 的来源）→ 不可作为仓内测试的断言依据，只作根因定位的旁证。

---

## 3 三张表

### 3.1 要改的

| 文件 | 为什么改 | 对应需求点 |
| --- | --- | --- |
| `src/observability/mod.rs` | 可观测性模块出口：①**run 上下文**（run id / 阶段 / 步骤 / attempt）的读写接口（`begin_run_in` `src/observability/mod.rs:84`、`set_phase` `src/observability/mod.rs:133`、`set_step` `src/observability/mod.rs:150`、`set_attempt` `src/observability/mod.rs:170`）；②**root 解析**（Q4：进程内注入 > `DEVFORGE_HOME` > `HOME/.devforge`，`src/observability/mod.rs:262`）；③里程碑与心跳的**公共面**（字段词汇、角色计数 `src/observability/mod.rs:196`、「上下文更新 → 顺手刷心跳」）。**已落地**（steps 2/5/8），本轮**不改**（R-1 只改**调用时机**，不改这里） | 需求 2、3 |
| `src/observability/heartbeat.rs` | 心跳文件读写：落点为 root 下 `heartbeat/` 目录内、**以 run id 命名**的单个 JSON 文件（`src/observability/heartbeat.rs:31`）；内容含 **run id / 阶段 / 步骤 / 时间戳 / attempt / pid**；**覆盖写**（`src/observability/heartbeat.rs:36`）；无活动 run 时**不写**；run 正常结束**保留**最后一次状态（不删）。**已落地**（step 2），本轮**不改** | 需求 3b、3c |
| `src/observability/tombstone.rs` | panic hook：安装（`src/observability/tombstone.rs:44`）→ panic 时写 root 下 `tombstones/<ts>-<runid>.txt`（`src/observability/tombstone.rs:72`），内容含 **panic 原文 + panic 位置（file:line）+ 当时阶段/步骤 + 时间戳 + run id**（无活动 run 时用占位值 `src/observability/tombstone.rs:19`）；**写入 best-effort**（Q9）；**链回上一个 hook**（还原接口 `src/observability/tombstone.rs:28`）。**已落地**（steps 2/5），本轮**不改** | 需求 3a、3c |
| `src/lib.rs` | 挂载新模块 `pub mod observability;`（`src/lib.rs:16`）。**已落地**（step 2），本轮**不改** | 需求 2、3 |
| `src/main.rs` | 进程启动时安装 panic hook，并**设定 root**（`HOME/.devforge`，与日志目录同根，`src/main.rs:143`/`src/main.rs:144`；`DEVFORGE_HOME` 可覆盖，Q4，`src/main.rs:149`）。**已落地**（step 2），本轮**不改** | 需求 3a |
| `src/engine/policy.rs` | ①每次尝试失败 → 落一条**分级**日志（Q2 落点 ①，`src/engine/policy.rs:88` → `src/engine/policy.rs:124`）：质量类失败落 **WARN**、其余落 **ERROR**，内容均为原始错误全文 + source 链（在决定重试**之前**）；②终态失败（4 次耗尽）→ 再落一条同级的终态条目（`src/engine/policy.rs:155`）；③每次尝试开始时把 `attempt` 写进**观测模块的 run 上下文**（`src/engine/policy.rs:79` 之前调 `src/observability/mod.rs:170`），供 Q2 落点 ② 的 `map_intervention` 取同一字段 → 两处字段集真正同构。**不改**重试策略（`src/engine/policy.rs:17`/`src/engine/policy.rs:112`）、**不改** `is_infra_failure` 的判据（`src/engine/policy.rs:60`）。**本轮 R-1**：把 `set_attempt(attempt)`（现落在失败分支内 `src/engine/policy.rs:87`）**前移到尝试开始处** | 需求 1；Q2；**R-1** |
| `src/engine/client.rs` | agent 轮次结束里程碑（`wait_settled` `src/engine/client.rs:186` / `wait_settled_with_ui` `src/engine/client.rs:212` 返回处，落点 `src/engine/client.rs:276`）：字段 `component="engine"`、`event="agent.turn_end"`、`role`（`src/engine/client.rs:158`）、`turn_ms`、`chars`。**一处覆盖全部 SOP**（Q5：change 侧 `src/change/session.rs:293`/`:382`/`:500` **不**再落交卷事件）。**已落地**（step 7），本轮**不改** | 需求 2；Q5 |
| `src/server/mod.rs` | ①`require_profile`（`src/server/mod.rs:269`）：错误种类 `System` → `Config`（Q11），**错误文本一字不改**（`src/server/mod.rs:277`）；②**per-SOP 的 profile 统一入口**（`profiles_for_sop` `src/server/mod.rs:253`）：六个 SOP 的 `require_profile` 调用（`src/server/mod.rs:288`/`:347`/`:422`/`:832`/`:953`）改为查这张表 → 门卫与闸门读**同一张表**（change 一项引用 `src/server/change_engine.rs:24`）；③`ServerState`（`src/server/mod.rs:1139`）：渲染闸门状态（`src/server/mod.rs:1164`）+ 观测根注入位（`src/server/mod.rs:1167`）；④`run_in_background`（`src/server/mod.rs:1332`）：run 级里程碑与心跳（run 开始 `src/server/mod.rs:1360` / 每次重试 / run 结束清位）；⑤watchdog 轮询线程（`src/server/mod.rs:1530`）：挂心跳低频兜底刷新（`src/server/mod.rs:1539`，不改其取消/超时/失活收割语义，不依赖 `idle_ms` 开启）；⑥`map_intervention`（`src/server/mod.rs:1017`）的**质量分支**（`src/server/mod.rs:1030`）：按 Q2 落点 ② 补一条**同形状**的分级 WARN（`src/server/mod.rs:1044`），既有那条（`src/server/mod.rs:1032`）保留不动、**判定语义一字不改**（仍 `Ok(Failed)`、仍不重启）。**本轮 R-2**：`RenderGate::wait_for_profiles`（`src/server/mod.rs:1106`）的放行条件收紧为「**无渲染在途** 或 **所需 profile 全部就绪**」——`rx.changed()` 分支（`src/server/mod.rs:1124`）**回环重判**而不是直接 `return` | 需求 1、2、3b；裁决 A；Q2；**R-2** |
| `src/server/handlers.rs` | ①`put_manifest`（`src/server/handlers.rs:371`）：后台渲染线程**登记渲染在途**（`src/server/handlers.rs:390`），渲染结束（成功/失败）清位并通知（`src/server/handlers.rs:415`）；②`start_task`（`src/server/handlers.rs:202`）：**开跑前等闸门**（`src/server/handlers.rs:240`），再 spawn run（`src/server/handlers.rs:243`）；等待必须用**可 await 的原语**（不得在 tokio worker 上阻塞，见 §2.3 / R17）。所需 profile 清单读 `src/server/mod.rs:253` 的**统一入口表**（不在本文件另抄一份）。**本轮 R-3**：渲染线程的「在途计数归零」改用 **drop 守卫**保证必达——现状是手工递减（`src/server/handlers.rs:415`），而 `emit`（`src/server/handlers.rs:405`/`:408`/`:411`）在 `catch_unwind` 之外，通知发出时抛异常就会让计数**永久大于 0** → 之后的 `start` 永远在等 | 需求 1（根因修复）；裁决 A；**R-3** |
| `src/server/tests/mod.rs` | **Q4 路径 ①**：在仓内单测的夹具里把观测 root 注入到该夹具的临时目录（`src/server/tests/mod.rs:100` 的 `fixture_full`；临时目录建于 `src/server/tests/mod.rs:107`，`obs_root` 在 `src/server/tests/mod.rs:111`，写进 `ServerState` 在 `src/server/tests/mod.rs:144`，`Fixture` 构造于 `src/server/tests/mod.rs:153`）→ 走真实 start 路径的单测（`src/server/tests/mod.rs:958`）心跳落在临时目录，**用户 HOME 的 `.devforge/heartbeat/` 不被写脏**（R12）。**已落地**（step 4）；另含**测试基础设施修复**（step 8/15）：`wait_settled` 辅助（`src/server/tests/mod.rs:160`）由「取第一个 run」改为「取最后一个 run」——修「同一任务起第二次 run 时误判已终态」的竞态（**不是**放宽断言，无既有断言被削弱） | 需求 3c；Q4 |
| `src/change/orchestrator.rs` | 阶段转换与 fix 往返的里程碑 + 心跳：**5 处**阶段写入（`src/change/orchestrator.rs:204`/`:231`/`:317`/`:346`/`:361`）各落一条 `event="phase.change"`（含 `from`/`to`；`src/change/orchestrator.rs:232` 是 `save(...)` 不是阶段写入，超限分支阶段未变 → **不**落 phase.change）；plan 修复轮（`src/change/orchestrator.rs:177`/`:185`）、补边界往返（`src/change/orchestrator.rs:254`/`:262`）、testing→coding 回注（`src/change/orchestrator.rs:338`/`:348`）各落 `event="fix.start"` / `event="fix.end"`（含轮次）。**已落地**（step 6/8），本轮**不改** | 需求 2、3b |
| `src/change/session.rs` | ①「审查开庭」里程碑（`review_once` `src/change/session.rs:264`，三角色共用 → 角色名经调用点传入；`round` 口径见 Q6 逐站点表，change 三角色用角色内序号）；②三道机械门的**结果**里程碑（基线门 `src/change/session.rs:389`、自证门 `src/change/session.rs:428`、步提交对账 `src/change/session.rs:474`）。**不**在此落 agent 交卷事件（Q5）。**已落地**（step 6/8/14/18），本轮**不改** | 需求 2、3b；Q6 |
| `src/sop/planning.rs` | MVP planning 循环的里程碑：plan 阶段推进（`src/sop/planning.rs:108`）、审查开庭（`src/sop/planning.rs:132`，轮次 = `rounds + 1`）、判负回注 fix 起止。**已落地**（step 7），本轮**不改** | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/coding.rs` | MVP coding 循环的里程碑：stage 推进（`src/sop/coding.rs:162`）、审查开庭（`src/sop/coding.rs:184`，轮次 = `review_rounds + 1`）、打回 fix 起止。**已落地**（step 7），本轮**不改** | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/testing.rs` | MVP testing 循环的里程碑：阶段推进（`src/sop/testing.rs:202`）、审查开庭（`src/sop/testing.rs:224`，轮次 = `report.review_rounds + 1`）、doc-fix 移交（`src/sop/testing.rs:256`）与回注（`src/sop/testing.rs:260`）的里程碑。**已落地**（step 7），本轮**不改** | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/doc/runner.rs` | doc 单元循环的里程碑：单元推进（`src/sop/doc/runner.rs:289`）、审查开庭（`src/sop/doc/runner.rs:338`，轮次 = `bump_review` 返回值，1-based）、**单元审查 FAIL → 回注 agent 重写**（`src/sop/doc/runner.rs:427`）的 fix 起止。**已落地**（step 7），本轮**不改** | 需求 2；裁决「全 SOP 全覆盖」 |
| `src/sop/doc/fix.rs` | doc-fix 的里程碑：审查开庭（`src/sop/doc/fix.rs:93`，轮次 = `report.review_rounds`，**不带 +1**，因 `src/sop/doc/fix.rs:91` 已先自增）、回注 fix 起止（`src/sop/doc/fix.rs:136`/`:143`）。**已落地**（step 7），本轮**不改** | 需求 2；裁决「全 SOP 全覆盖」；Q6 |
| `src/change/coding_gate.rs` | **操作员补录扩容**（原计划未涉及，step 15 落地）：resume5 死因 F16 的解析器修复落点——证据日志认 markdown 列表项（F16）、步号只认紧跟标记的数字（F15）；**判定语义不动**（基线门 `src/change/coding_gate.rs:92`、自证门 `src/change/coding_gate.rs:118`、步提交对账 `src/change/coding_gate.rs:266` 的通过条件一字未改），里程碑仍落在调用点 | 需求 4（边界口径） |
| `tests/f6_observability/main.rs` | 本靶的**验收对账测试**（黑盒口径，仿 `tests/change_review_protocol/main.rs` 的风格：只经公开 API 观察，每条用例首行写「错实现形态」）：逐条对账 §5；含「注入 panic → 墓碑存在且含 panic 原文」与「阶段推进 → 心跳文件内容随之变化」两条硬验收。**已落地**（测试阶段），本轮**不改**（测试阶段文件） | 需求 3c；需求 1、2 |
| `tests/change_ask_ring.rs` | F10 例行刷新（Q10）：`BASELINE`（`tests/change_ask_ring.rs:35`）刷成本场基线、`WHITELIST`（`tests/change_ask_ring.rs:39`）刷成本场 **coding 步白名单 23 个**；step 1 摘除 `#[ignore]`（`tests/change_ask_ring.rs:629`）、本场收尾复贴；`tests/change_ask_ring.rs:663` 的「上一场遗留文件不得进本变更」断言**保持有效**（其豁免前提钉在「该文件在本场 `WHITELIST` 常量内」，`src/change/coding_gate.rs` 因而在白名单内、本变更合法改它，见 §3.1 的 `src/change/coding_gate.rs` 行与 §2.3）。**本轮 R-4**：常量**注释**（`tests/change_ask_ring.rs:32`/`tests/change_ask_ring.rs:38`）与常量**值**（`a36f6b7…` / 23 个）一致 | 需求 4（边界口径）；**R-4** |
| `tests/change_failure_context/main.rs` | 守卫常量的孪生副本：同步刷 `BASELINE`（`tests/change_failure_context/main.rs:36`）/ `WHITELIST`（`tests/change_failure_context/main.rs:40`）→ a16（`tests/change_failure_context/main.rs:959`）的比对才绿。**本轮 R-4**：注释（`tests/change_failure_context/main.rs:33`/`tests/change_failure_context/main.rs:39`）与值一致 | 需求 4（边界口径）；**R-4** |
| `tests/change_review_protocol/main.rs` | ①第三份常量副本：同步刷 `BASELINE`（`tests/change_review_protocol/main.rs:47`）/ `PLAN_WHITELIST`（`tests/change_review_protocol/main.rs:51`）/ `CODING_WHITELIST`（`tests/change_review_protocol/main.rs:79`）→ a29/a30/a31 才绿；②**改写两处硬编码断言**（Q10，否则 step 1 之后必红）：a29 的排除项（`tests/change_review_protocol/main.rs:1337`）从 `tests/change_review_protocol/main.rs` 换成**本场测试阶段文件** `tests/f6_observability/main.rs`；a32（`tests/change_review_protocol/main.rs:1437`）的计数 `20`/`19` 改成 **`24`/`23`**（现值见 `tests/change_review_protocol/main.rs:1439`/`:1440`），且「§4 白名单含唯一的测试阶段文件」的断言换人。**本轮 R-4**：注释（`tests/change_review_protocol/main.rs:44`/`tests/change_review_protocol/main.rs:1316`）与值一致 | 需求 4（边界口径）；Q10；**R-4** |
| `docs/plantree/plans/010-run-resume/decisions/README.md` | 追加 `## D006 ·`（Q12，已落地于 `docs/plantree/plans/010-run-resume/decisions/README.md:53`）：登记本靶三件交付物（原始错误可见 / 里程碑 / 死亡取证）+ 渲染闸门口径（用户裁决：A 渲染闸门）+ 「原始错误落盘分级：基础 ERROR / 质量 WARN」（用户裁决：Q2）+ 「profile 未就绪 = 配置错误不是基础设施故障」的分类修正 + 遗留清单。**只追加、不动 D001–D005** | 需求 1、2、3 |
| `docs/plantree/plans/010-run-resume/roadmap.md` | 追加本场条目（三件交付物 + 根因修复 + 验收证据位置）。**已落地**（step 9），本轮**不改** | 需求 1、2、3 |

### 3.2 受影响不改

| 文件 / 位置 | 为什么可以不改 |
| --- | --- |
| `src/sop/review_gate.rs:78` | 审查三通道机制与两个上限常量不动（Q6：改签名会波及全仓 11 处调用点与上一场验收测试）；「审查开庭」里程碑落在 8 个调用点 |
| `src/engine/policy.rs:17`、`src/engine/policy.rs:112`、`src/engine/policy.rs:60` | 重试次数、退避档位、`is_infra_failure` 的判据全不动（本靶只加可见性 + 修掉「profile 未就绪」这一处错误种类） |
| `src/config/render.rs` | 渲染器本身不动（写 `settings.json` 的次序不动，`src/config/render.rs:63`/`:192`）；闸门等的是「文件已落盘」这个既有事实，不改渲染语义 |
| `src/server/change_engine.rs:24`、`src/server/change_engine.rs:78` | `CHANGE_PROFILES`（change SOP 的六 profile 清单）是闸门所需清单的**事实源**（被 `src/server/mod.rs:253` 的统一表引用）；`src/server/change_engine.rs:78` 是 change 的**启动门卫**——boot 六连的触发点。两者都**只被引用/被观察**（错误种类在 `src/server/mod.rs:269` 改，闸门清单在 `src/server/mod.rs:253` 的表里取）→ 这个文件**不改**、**不进白名单** |
| `src/config/manifest.rs` | 角色提示词不动（本靶不碰任何 agent 契约）→ e2e 夹具 `tests/e2e/fixtures/change_agents.json` **不用重生成** |
| `src/change/state.rs` | 断点结构不动（不给 `ProdChangeSession` 加字段 → 不触发全仓字面量构造点的编译期耦合，见 §2.3）；`ChangePhase` 的四个变体（`src/change/state.rs:20`）是阶段词汇的唯一事实源（R24） |
| `src/change/prepare.rs`、`src/change/gate0.rs` | 门 0（change-id / 文档档位 / 基线）的语义不动（`src/change/gate0.rs:66`） |
| `src/change/docs.rs` | 档位动作不动（`none` → `Skipped`，`src/change/docs.rs:33`）——本场文档档位 none，不跑文档流水线 |
| `src/sop/doc_protocol.rs`、`src/sop/testing_protocol.rs` | doc 侧协议解析不动（本靶只在 doc 循环加里程碑） |
| `src/sop/coding_protocol.rs`、`src/sop/output.rs` | 协议解析不动 |
| `src/server/tests/mod.rs:23`（MockEngine：直接返回 `Err(Quality)`，绕过 `map_intervention`） | 既有源码守卫（协议违例上限 → Quality、实例失败上限 → System）不动：本靶不动那三处文案，守卫仍绿；该 mock 是「质量失败直达重试层」这条路径的唯一现成触发器 → Q2 落点 ① 在仓内可判（落点 ② 的覆盖点另在 `src/server/mod.rs` 的 `#[cfg(test)]` 模块内） |
| `src/server/mod.rs:1899`（门卫放行）、`src/server/mod.rs:1910`（门卫报错文本）、`src/server/mod.rs:1922`/`src/server/mod.rs:1940`/`src/server/mod.rs:1959`/`src/server/mod.rs:1976`（四个 `run_*` 用例） | profile 缺失的既有单测：Q11 只改错误**种类**，错误**文本**不变 → **5** 条 `contains("未渲染")` 断言（`src/server/mod.rs:1918`/`src/server/mod.rs:1932`/`src/server/mod.rs:1949`/`src/server/mod.rs:1968`/`src/server/mod.rs:1984`）继续绿 |
| `docs/plantree/plans/009-change-sop/**` | 本靶不改 change SOP 的业务语义 → 009 的 topic 文档与代码无冲突，既有守卫（`src/change/plan_audit.rs:712`、`src/change/plan_audit.rs:736`）继续绿 |
| `docs/plantree/plans/007-testing-stage/**` | 本靶不改测试阶段文档；那里只是**引证来源**（装包分钟级实测：`docs/plantree/plans/007-testing-stage/roadmap.md:33` 与 `docs/plantree/plans/007-testing-stage/evidence/e2e-t008-findings.md:51`） |
| `docs/plantree/plans/002-config-management/**` | 上一场计划把 CAL-19-RENDER 的实测引证错记在这里；本场纠正到 007 的两处（`docs/plantree/plans/007-testing-stage/roadmap.md:33`、`docs/plantree/plans/007-testing-stage/evidence/e2e-t008-findings.md:51`）→ 002 不动 |
| `docs/plantree/ideas/selfhost-findings-20261021.md` | 该文件按其自述「由操作员维护」→ F6 的状态更新由操作员做（Q12） |
| `tests/e2e/**` | 不新增/不改 e2e：真链是否复现 boot 竞态取决于渲染时序，断言押在时序上 = 假红。既有 e2e 的行为不回归即可（`tests/e2e/test_10_timeout_async_render.py` 已等 `config.rendered` 再 start；`tests/e2e/test_12_sop_override.py`、`tests/e2e/test_18_sop_registry.py` 不 PUT 清单 → 闸门不触发）。**「先 PUT 清单再 start」的 8 个脚本是闸门的新等待面**（清单见 §2.3 与 §3.3）：其中 `tests/e2e/test_13_coding_preflight.py`、`tests/e2e/test_15_coding_run_control.py`、`tests/e2e/test_16_run_liveness.py` 三个不等/只等一个 profile → 闸门会为同一 SOP 的第二个 profile 追加等待，可能撞上 start 的 10s 默认超时（`tests/e2e/common.py:132`）→ 登记为 R2 的遗留（python 脚本不在本靶白名单，本靶**不改**它们）。**R-2 收紧放行条件后这三个脚本的等待面只会更大**（见 R26） |
| `assets/skills/coding-discipline/SKILL.md` | 本变更**遵守**它，不改它 |
| `Cargo.toml` / `Cargo.lock` | 不引入新外部依赖（心跳/墓碑只用 `std` + 既有 `tracing`） |

### 3.3 必须回归验证

| 模块 | 为什么风险在这里 |
| --- | --- |
| `src/engine/policy.rs` 的既有单测（`#[cfg(test)]` 在 `src/engine/policy.rs:174`） | 六条用例钉着既有口径：`two_infra_failures_then_success_settles_on_third_attempt`（尝试数与调用数）、`persistent_infra_failure_stops_at_four_attempts`（错误串含每次尝试末段 + 「共尝试 4 次」）、`quality_failure_is_not_retried`、`cancelled_run_is_not_retried`、`backoff_schedule_matches_decision`、`non_system_errors_are_not_infra`。加 ERROR 落盘与 **R-1 的写入时机前移**都**不得**改这些语义（尤其「错误串带每次尝试末段」这条） |
| `src/server/mod.rs:1518`（watchdog）、六个调用点（planning `src/server/mod.rs:318`、coding `src/server/mod.rs:389`、doc-from-zero `src/server/mod.rs:661`、testing `src/server/mod.rs:875`、doc-fix `src/server/mod.rs:991`、**change `src/server/change_engine.rs:203`**）与 `src/server/mod.rs:1899` 起的 profile 缺失用例（门卫放行 `src/server/mod.rs:1899`、门卫文本 `src/server/mod.rs:1910`、四个 `run_*` 用例 `src/server/mod.rs:1922`/`src/server/mod.rs:1940`/`src/server/mod.rs:1959`/`src/server/mod.rs:1976`） | watchdog 是「心跳低频兜底刷新」的挂载点（**六个 SOP 全覆盖，change 也不例外**；不得改其取消/超时/失活收割语义，也不得依赖 `idle_ms` 开启）；profile 缺失的 **5** 条 `contains("未渲染")` 断言（`src/server/mod.rs:1918`/`src/server/mod.rs:1932`/`src/server/mod.rs:1949`/`src/server/mod.rs:1968`/`src/server/mod.rs:1984`）依赖错误文本（Q11 只改种类 → 文本不变）。**R-2 改的 `wait_for_profiles`（`src/server/mod.rs:1106`，放行分支在 `src/server/mod.rs:1124`）也在这一片**：闸门的既有单测（`src/server/mod.rs:2071` 起的闸门三例）必须仍绿 |
| `src/server/tests/mod.rs` 全量 | 端到端跑真 run 路径的用例（`quality_failure_path_is_not_retried_end_to_end` `src/server/tests/mod.rs:729`、`infra_failures_are_retried_up_to_three_times` `src/server/tests/mod.rs:850`、`persistent_infra_failures_fail_after_four_attempts` `src/server/tests/mod.rs:876`）依赖「Quality 不重启 / System 才重启」的分岔 —— 本靶改了「profile 未就绪」的种类，必须确认没有别的用例依赖它被重启。**另**：这些用例会走真实 start 路径 → 置位 run 上下文 → 若 root 未注入，心跳会落进用户 HOME（R12）→ 单测必须把 root 指向临时目录（Q4 路径 ① 落点在 `src/server/tests/mod.rs:111`） |
| `src/change/orchestrator.rs` 的既有单测（`#[cfg(test)]` 在 `src/change/orchestrator.rs:395`） | 编排层的行为（阶段推进、回注、往返计数、验收对账）不得因加里程碑而变；既有断言（如 `happy_path_plan_coding_testing_report` 的 `review_rounds == 2`）是回归闸门；里程碑形状用例在 `src/change/orchestrator.rs:562` |
| `src/change/session.rs` 的既有就地单测（`#[cfg(test)]` 在 `src/change/session.rs:589`） | 计划/编码/测试三条链的既有期望值（判负通道不变、门禁拦截点不变、审查轮次计数）不得回归；门禁里程碑用例在 `src/change/session.rs:1138` |
| `src/sop/planning.rs`、`src/sop/coding.rs`、`src/sop/testing.rs` 的既有单测 | 三个 MVP 循环的轮次/问题数/阶段完成的既有语义不得回归（本靶只在循环里加日志与心跳）；里程碑形状用例分别在 `src/sop/planning.rs:364`、`src/sop/coding.rs:401`、`src/sop/testing.rs:448` |
| `src/sop/doc/runner.rs`、`src/sop/doc/fix.rs` 的既有单测 | doc 侧的单元推进与审查轮语义不得回归（尤其 doc-fix 的 `report.review_rounds` 计数口径，Q6）；里程碑形状用例分别在 `src/sop/doc/runner.rs:652`、`src/sop/doc/fix.rs:285` |
| `src/sop/review_gate.rs` 的既有单测（`#[cfg(test)]` 在 `src/sop/review_gate.rs:172`） | 三通道机制与两个上限常量不动（Q6）——这五条是「共享门没被动过」的闸门 |
| `src/change/plan_audit.rs` 的既有守卫（`src/change/plan_audit.rs:712`） | 读 009 的台账与 topic 文档；本靶不改那两份 → 必须仍绿 |
| `tests/change_ask_ring.rs:599` 起的夹具/台账/文档锚点守卫、`tests/change_ask_ring.rs:630` 起的白名单守卫 | 都是**读本靶要改的文件**的守卫：白名单常量必须刷新、上一场遗留文件不得进本变更（豁免前提 = 在本场 `WHITELIST` 常量内）、夹具/台账/文档锚点一字不动；**R-4 改的注释**不得与断言里的常量值打架 |
| `tests/change_failure_context/main.rs`（a1–a17，尤其 a16 `tests/change_failure_context/main.rs:959` / a17 `tests/change_failure_context/main.rs:1034`） | 本靶**刷新**它的 `BASELINE`/`WHITELIST`；a16 比对的就是守卫常量、a17 扫的是本场提交内容 → 这两条是本靶最容易漏的红 |
| `tests/change_review_protocol/main.rs`（a29 `tests/change_review_protocol/main.rs:1319` / a30 `tests/change_review_protocol/main.rs:1378` / a31 `tests/change_review_protocol/main.rs:1398` + **a32** `tests/change_review_protocol/main.rs:1437` + 上一场的协议用例） | 本靶**刷新**它的三份常量；a29/a30/a31 逐条对账本场；**a32（`tests/change_review_protocol/main.rs:1439`/`:1440`）与 a29 的排除项（`tests/change_review_protocol/main.rs:1337`）已按 Q10 改写**（24/23、排除项换成本场测试阶段文件），不得回退；上一场的协议用例（审查三通道、尾部协议位）不得回归 |
| `tests/f6_observability/main.rs`（本靶新增） | 逐条对账 §5；每条用例首行写「错实现形态」 |
| 仓内单测（`cargo test`） | 本靶的新增/改写单测全在白名单文件内（新增的 `tests/f6_observability/main.rs` + 各文件既有 `#[cfg(test)]` 模块） |
| `tests/e2e/test_10_timeout_async_render.py`、`tests/e2e/test_12_sop_override.py`、`tests/e2e/test_18_sop_registry.py`、`tests/e2e/test_25_fault_retry.py` | ①test_10 验「PUT 立即 202 + 后台渲染 + SSE 通知」——闸门**不得**把 PUT 变成阻塞；②test_12/test_18 验「profile 未渲染时 run 秒级 Failed、串行槽即时释放」——Q11 的错误种类改动不得让它变慢或变语义；③test_25 验「真实故障 → run 级自动重启 attempt=2」——重试策略没动，它必须仍绿 |
| `tests/e2e/test_26_change_e2e.py`、`tests/e2e/test_27_change_ask_ring.py` | 真链：改了引擎（闸门 + 里程碑 + 心跳）后 change run 仍要 Settled（本靶不新增 e2e 断言，但要绿） |
| 「先 PUT 清单再 start」的 8 个 e2e 脚本：`tests/e2e/test_07_rerun_serial.py:46`→`:107`、`tests/e2e/test_08_e2e_planning.py:86`→`:128`、`tests/e2e/test_09_run_control.py:34`→`:47`、`tests/e2e/test_13_coding_preflight.py:33`→`:46`、`tests/e2e/test_14_coding_full_chain.py:59`→`:94`、`tests/e2e/test_15_coding_run_control.py:32`→`:47`、`tests/e2e/test_16_run_liveness.py:71`→`:83`、`tests/e2e/test_19_testing_full_chain.py:67`→`:101` | 闸门是本靶**唯一**改动 start 时序的东西（原来 start 立即 spawn，现在可能先等所需 profile 就绪）→ 这 8 个脚本的 start 必须**仍然 202 且不超时**。其中 5 个在 start 前已等齐自己 SOP 所需 profile（`tests/e2e/test_07_rerun_serial.py:49`、`tests/e2e/test_08_e2e_planning.py:88`、`tests/e2e/test_09_run_control.py:38`、`tests/e2e/test_14_coding_full_chain.py:64`、`tests/e2e/test_19_testing_full_chain.py:72`）→ 闸门不新增等待；**3 个是新增失败面**：`tests/e2e/test_13_coding_preflight.py:33`（完全不等）、`tests/e2e/test_15_coding_run_control.py:35`（只等 `coding-agent`）、`tests/e2e/test_16_run_liveness.py:73`（只等 `plan-agent`）→ 闸门为同 SOP 的第二个 profile 追加等待，可能撞上 start 的 10s 预算（`tests/e2e/common.py:132`）→ 这三个脚本**必须仍绿**；若真撞超时，按 R2 的处置（先查闸门是不是多等了「所需 profile 之外的东西」，再谈脚本改动——脚本改动不在本靶白名单）。**R-2 收紧后这一面更大**（见 R26） |
| 引擎越界门（`src/change/boundary.rs:133` + `src/change/orchestrator.rs:227`） | §4 白名单必须覆盖**整个提交集合**（含 4 个新增文件 + 2 份文档），少写一个 = 差集非空 + 无 `<impact-gap>` 申报 = 「越界改动未申报（范围膨胀）」直接 Quality |
| 部署侧（不在仓内） | **仓内改完不等于用户态生效**：profile 渲染的**唯一触发点**是 `PUT /api/v1/config/manifest`（`src/server/handlers.rs:392`）；`src/main.rs` 全程**不触发渲染**（只读/写清单 `src/main.rs:156`、装配安装器 `src/main.rs:201`）→ 用户态要生效须再 PUT 一次清单（且 `~/.devforge/manifest.json` 缓存会被沿用）；心跳/墓碑的落点依赖 `HOME`（`src/main.rs:143`） |

---

## 4 白名单

- `src/observability/mod.rs`
- `src/observability/heartbeat.rs`
- `src/observability/tombstone.rs`
- `src/lib.rs`
- `src/main.rs`
- `src/engine/policy.rs`
- `src/engine/client.rs`
- `src/server/mod.rs`
- `src/server/handlers.rs`
- `src/server/tests/mod.rs`
- `src/change/orchestrator.rs`
- `src/change/session.rs`
- `src/sop/planning.rs`
- `src/sop/coding.rs`
- `src/sop/testing.rs`
- `src/sop/doc/runner.rs`
- `src/sop/doc/fix.rs`
- `tests/f6_observability/main.rs`
- `tests/change_ask_ring.rs`
- `tests/change_failure_context/main.rs`
- `tests/change_review_protocol/main.rs`
- `src/change/coding_gate.rs`（**操作员补录扩容**：resume5 死因 F16 的解析器修复落点，原计划未涉及；step 15 已合法改它，守卫的豁免前提钉在「该文件在本场 `WHITELIST` 常量内」）
- `docs/plantree/plans/010-run-resume/decisions/README.md`
- `docs/plantree/plans/010-run-resume/roadmap.md`

白名单共 **24 个文件** = **coding 步的 23 个** + **测试阶段的 1 个**（`tests/f6_observability/main.rs`）。三层「白名单」口径不同、**不可互相替代**：

- **引擎的越界门**（`src/change/orchestrator.rs:227` → `src/change/boundary.rs:133` `classify`）拿 `git diff --name-only <baseline>`（`src/change/coding_gate.rs:193` = 本变更**全部**提交文件，含测试阶段）比**本节清单** → 本节必须覆盖**整个提交集合**（24 个），少写一个 = 差集非空 + 无 `<impact-gap>` 申报 = 直接 Quality。
- **提交内容守卫的 `WHITELIST` 常量**（`tests/change_ask_ring.rs:39`，孪生 `tests/change_failure_context/main.rs:40`、上一场文件 `tests/change_review_protocol/main.rs:79`）守的是**coding 步提交**：提交信息含 ` step ` 的提交里每个文件都必须在常量内；**不含** ` step ` 的提交（测试阶段自己的提交）只许是 `tests/` 前缀的文件（`tests/change_ask_ring.rs:644`）→ 本场常量列 **23 个 coding 步文件**（= 本节 24 个去掉 `tests/f6_observability/main.rs`），**不加**测试阶段文件（加了就把「白名单里的每个文件都要被本场 coding 步提交改到」打红）。三个新增源文件（`src/observability/` 三件套）**属 coding 步文件，要**在常量里。
- **上一场验收测试文件的常量也在这条链上**：`tests/change_failure_context/main.rs:36`/`tests/change_failure_context/main.rs:40` 与 `tests/change_review_protocol/main.rs:47`/`tests/change_review_protocol/main.rs:51`/`tests/change_review_protocol/main.rs:79` 的常量与守卫文件的常量互相比对（a16/a29/a30）→ 本场刷守卫常量时**必须**同步刷这两处（否则比对失败、必红）。

### 4.1 三层口径的差别（提交边界 / 守卫常量 / 测试阶段）

- **白名单（24）** = 提交边界（越界门比它）。
- **coding 步白名单（23）** = 守卫常量（比提交内容）。
- **测试阶段白名单（1）** = 测试阶段自己的提交只能是 `tests/` 前缀文件。

**机械门 finding 登记项**：本计划在**计划期**跑机械门审计（`src/change/plan_audit.rs:238` `audit`）的 finding 集合**恰为 4 条**「白名单里的文件不存在」——`src/observability/mod.rs`、`src/observability/heartbeat.rs`、`src/observability/tombstone.rs`、`tests/f6_observability/main.rs`（新增文件在计划期必然不存在，预期内）；**这 4 条之外 finding 数必须为 0**（六块齐全、`path:line` 全部存在、「要改的」⊆「白名单」、验收标准可观测）。**当前（实现期）**：四个新增文件均已落盘 → 现状审计 finding 为 **0** 条（§5 已把该判据同步成 0；计划期快照仍留在本节备查）。

白名单之外不得有任何改动。本计划文件本身（计划阶段的交付物，位于 `docs/plantree/changes/change-20261022-c7-1-start-src-server-tests-rs-145-fixtu/`）与工作区里的未跟踪产物（归档目标目录、计划目录、e2e 结果文件）都不属 coding 阶段的改动面，一律不许 `git add`。

---

## 5 验收标准

- 给定一次 run 的尝试因**非质量类**失败（基础设施故障、存储/协议类等）失败 → 日志面留下**一条 ERROR** 条目，内容含**原始错误全文**（顶层错误逐字）与 **source 链每一层**的文本（`WithContext` 的上下文 + 内层错误逐层可见），字段含 `component="run-retry"`、`event="run.attempt_failed"` 与 `attempt`。
- 给定质量失败（`DevforgeError::Quality`）在**SOP 执行期**产生（审查打回 / 协议违例上限 / 越界申报 / 验收对账缺项）→ 日志面出现一条 **WARN**（**不是** ERROR）条目，字段与上一条**同构**（`event="run.attempt_failed"`、`attempt`、`error` 全文、`error_chain` 每层、`retryable=false`），落点在 `map_intervention` 的质量分支（`src/server/mod.rs:1030`）；且该 run **不**重启（`Ok(RunOutcome::Failed)` 语义不变）。
- 给定质量失败（`DevforgeError::Quality`）在 **SOP 之前**产生（change 的「缺少需求文件」，`src/server/change_engine.rs:96`）或由仓内 mock 引擎注入（`src/server/tests/mod.rs:35`）→ 日志面出现一条 **WARN**（**不是** ERROR）条目，字段与上一条**同构**（同一组字段名），落点在重试层的分级分支（`src/engine/policy.rs:88`）；该 run **不**重启。
- 给定基础设施故障发生在第 1 次尝试 → 该 ERROR 条目出现在「基础设施故障，自动重启 run」WARN **之前**（先记错误、再决定重启），且条目带 `attempt=1`。
- 给定 run 连续 4 次尝试都因基础设施故障失败 → 日志面出现 **4 条**尝试级 ERROR 条目（每次尝试一条）+ **1 条** `event="run.attempt_exhausted"` 终态条目（含总尝试次数 4）。
- 给定第 2 次及以后的尝试期间发生质量失败（该失败被 `map_intervention` 收成 `Ok(RunOutcome::Failed)`）→ 该条 WARN 的 `attempt` 字段等于**本次**尝试的序号（**不是**上一次的值）——R-1：attempt 必须在**每次尝试开始时**写入 run 上下文，而不是在失败分支里写。
- 给定 `POST .../start` 到达时**有渲染在途**且本 run 所需 profile 的 `settings.json` 尚未落盘 → 该 start 请求**不**立即开跑（等所需 profile 就绪或渲染结束后再开跑）。
- 给定本 run 所需 profile **已就绪**（`settings.json` 已落盘）但同一 manifest 的**其他** profile 仍在渲染 → `POST .../start` **立即返回 202**，**不等**渲染结束（闸门只等「所需 profile 的就绪」，不等整轮渲染）。
- 给定开跑时所需 profile 全部就绪 → 该 run 的**第一次尝试**即通过（日志面**不**出现 `attempt=2` 的自动重启 WARN）。
- 给定渲染**已结束**且目标 profile 仍缺 `settings.json` → run 以**明确错误**失败，错误文本点名缺失的 profile 名与 `settings.json` 路径，且**不**发生自动重启（尝试次数恒为 1）。
- 给定「profile 未渲染」这类失败 → 错误种类为配置类（`DevforgeError::Config`），不被判为基础设施故障（不进入自动重启分支）。
- 给定闸门正在等待渲染（渲染在途）→ 同一服务器对**其他**端点的请求仍能被及时处理（等待不得阻塞 tokio worker）。
- 给定两个并发 `POST .../start` 且闸门正在等待渲染 → **至多一个** run 真正开跑，另一个以 `run.error` 事件明确失败（**不产生双跑**；串行槽语义不变）。
- 给定**两轮**渲染在途、其中一轮结束 → `POST .../start` **不**放行（仍有渲染在途），直到「无渲染在途」**或**「本 run 所需 profile 全部就绪」二者之一成立——R-2：闸门的放行条件由「任意一次渲染结束」改为「回环重判」这两个条件。
- 给定渲染线程体在**发出完成通知时**抛异常 → 闸门的在途计数**仍然归零**（后续 `POST .../start` 不再无限等待）——R-3：在途计数的归零由 drop 守卫保证，不依赖线程体末尾的显式调用。
- 给定 change run 的阶段从 plan 推进到 coding → 日志面出现一条 `event=phase.change` 的 INFO，含 `from=plan` 与 `to=coding`。
- 给定 change run 的 testing 阶段回注 coding → 日志面出现一条 `event=fix.start` 与一条 `event=fix.end` 的 INFO，含回注轮次。
- 给定 change run 的 plan 审查打回后重写重审 → 日志面出现一对 `event=fix.start` / `event=fix.end` 的 INFO，含修复轮次。
- 给定 MVP planning / MVP coding / MVP testing / doc-from-zero / doc-fix 各自的 fix 往返（打回或回注 → 重写 → 重交）→ 日志面**各**出现一对 `event=fix.start` 与 `event=fix.end` 的 INFO，含轮次。
- 给定 MVP planning 循环的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**（首审轮次 = 1）。
- 给定 MVP coding 循环的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**（首审轮次 = 1）。
- 给定 MVP testing 循环的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**（首审轮次 = 1）。
- 给定 doc-from-zero 的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**（轮次 = `bump_review` 的 1-based 计数）。
- 给定 doc-fix 的一轮审查开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**，且该轮次**等于** `report.review_rounds`（`src/sop/doc/fix.rs:91` 已先自增 → **不加 1**：首审 = 1，第二轮 = 2）。
- 给定 change 侧任一审查角色（plan / coding / test）开庭 → 日志面出现一条 `event=review.open` 的 INFO，含**角色名**与**轮次**（该角色在本次 run 内的开庭序号，1-based）。
- 给定基线门判定（绿或红）→ 日志面出现一条 `event=gate.baseline` 的 INFO，含判定结论（红时含退出码）。
- 给定自证门判定（绿或红）→ 日志面出现一条 `event=gate.self_check` 的 INFO，含判定结论。
- 给定步提交对账判定（过或不过）→ 日志面出现一条 `event=gate.step_commits` 的 INFO，含步骤数与判定结论。
- 给定一次 agent 轮次结束（**任意** SOP，含 change 侧三个干活 agent）→ 日志面出现**恰好一条** `event=agent.turn_end` 的 INFO，含**角色名**与该轮耗时；同一轮**不出现第二条**交卷事件（落点唯一：change 侧 `src/change/session.rs:293`/`src/change/session.rs:382`/`src/change/session.rs:500` 不另落）。
- 给定全部里程碑条目 → 级别为 **INFO**、字段含 `component` 与 `event`，且 `target` 为**发出它的模块**（可按 target 过滤，不同模块的里程碑不共用同一个 target）。
- 给定注入一次 panic（`catch_unwind` 内）→ `<root>/tombstones/<ts>-<runid>.txt` 存在，内容含 **panic 原文**、**panic 位置**（`file:line`）与当时的**阶段/步骤**。
- 给定没有活动 run 时发生 panic → 墓碑文件**仍然存在**（run id 用占位值），证据不丢。
- 给定 panic hook 被安装后触发一次 panic → 墓碑文件存在，**且**上一个 hook 仍被调用（其副作用仍发生——链回语义）。
- 给定 root 指向不可写目录（或 HOME 未设）后触发一次 panic → 进程**不 abort**（panic 仍被正常捕获/传播）、**上一个 hook 仍被调用**，且不产生墓碑文件、hook 内不发生二次 panic。
- 给定 run 的阶段推进（plan → coding）→ `<root>/heartbeat/<runid>.json` 的内容**随之变化**（阶段字段改变），且文件内含**阶段 / 步骤 / 时间戳**三个要素。
- 给定 run 停留在同一阶段、一段时间内无阶段转换也无关键事件（典型：change run 里的长 agent 轮）→ 心跳文件的**时间戳**仍随之刷新（低频兜底刷新生效；六个 SOP 全覆盖，change 也不例外；能区分「卡死」与「已死」）。
- 给定进程在两个阶段之间异常死亡 → 最后写入的心跳文件仍能回答「死在哪个阶段」（阶段字段 = 死亡前最后到达的阶段）。
- 给定 run 正常结束 → 心跳文件**保留**最后一次状态（不被删除）。
- 给定心跳与墓碑的落点 → 均在 root 下的固定子目录（`heartbeat/` 与 `tombstones/`），文件名含 run id。
- 给定仓内走真实 start 路径的单测（`src/server/tests/mod.rs` 的夹具）运行 → 心跳文件落在**该单测的临时目录**里，用户 HOME 的 `.devforge/heartbeat/` **不新增**文件。
- 给定 `tests/change_ask_ring.rs` 的两个常量 → `BASELINE` 等于本场基线 sha `a36f6b79dec2e681db8daed0da067f9abd47a9f5`、`WHITELIST` 逐个列出本场 **coding 步白名单的 23 个文件**（含三个 `src/observability/` 文件、`src/server/tests/mod.rs`，及操作员补录扩容的 `src/change/coding_gate.rs`；**不含** `tests/f6_observability/main.rs`）；给定 `tests/change_failure_context/main.rs` 与 `tests/change_review_protocol/main.rs` 的同名常量 → 与守卫文件**逐字一致**。
- 给定三份守卫常量的**注释** → 注释里写的基线 sha 与白名单个数与常量的**实际值**一致（不残留 `18ff755…`、「22 个」这类陈旧数字）——R-4。
- 给定 a29 的排除项断言 → 断言的是「测试阶段文件 `tests/f6_observability/main.rs` 不得进 coding 步白名单常量」。
- 给定 a32 的计数断言 → `PLAN_WHITELIST` 为 **24** 个文件、`CODING_WHITELIST` 为 **23** 个文件，且计划白名单含唯一的测试阶段文件 `tests/f6_observability/main.rs`（10-22 操作员补录扩容 +1 后同步）。
- 给定白名单守卫的 `#[ignore]` → 本场 step 1 摘除（守卫在本场内生效）、本场收尾复贴（两场之间保持 ignore，F10 现状沿用）。
- 给定 `docs/plantree/plans/010-run-resume/decisions/README.md` → 含 `## D006 ·` 一节，写明三件交付物（原始错误可见 / 里程碑 / 死亡取证）、渲染闸门口径、「原始错误落盘分级：基础 ERROR / 质量 WARN」与「profile 未就绪 = 配置错误」的分类修正；既有 D001–D005 条目**一字不动**。
- 给定本计划文件跑机械门审计 → finding 为 **0 条**（计划期预期的 4 条「白名单里的文件不存在」已随新增文件落盘消解：`src/observability/mod.rs`、`src/observability/heartbeat.rs`、`src/observability/tombstone.rs`、`tests/f6_observability/main.rs` 均已存在；六块齐全、`path:line` 全部存在、「要改的」⊆「白名单」、验收标准可观测）。
- 给定本变更的提交内容 → §4 白名单（**24 个文件**，含操作员补录扩容的 `src/change/coding_gate.rs`）之外无任何改动；未跟踪产物（含本计划文件与 `docs/plantree/changes/`）一律不 `git add`。

## 6 风险

- **R1 · 根因判断若错，本靶的修复段要降级** —— 根因是**高置信静态定位**（三条独立证据：日志时间线、profile 文件 mtime、代码路径排除法，§1.2），但仍是**推断**：ERROR 落盘上线后，若实测显示 boot 失败点不是「profile 未就绪」（例如是 git 基线失败或装包失败），则**根因修复段**（渲染闸门 + 门卫复查）不解决真问题。缓解：闸门与门卫复查是**通用**机制（任何「渲染在途时开跑」都被挡住），且 ERROR 落盘会把真因直接顶到脸上；真因不同则按「只做可见性 + 登记待查」处理，**其余三件交付物不受影响**。残留：真因未定位前，boot 期仍可能出现一次「明确失败」（不再盲重启）。
- **R2 · 渲染闸门把 `start` 端点变成阻塞点** —— 闸门要在 start 返回 202 之前等「所需 profile 就绪」。若闸门等的是**整个渲染任务结束**（而不是所需 profile 的 `settings.json` 落盘），装包实测可达分钟级（首次全量八 profile 65.4s、旧串行基线 >20min，见 `docs/plantree/plans/007-testing-stage/roadmap.md:33`；八 profile 全供给渲染 >20 分钟见 `docs/plantree/plans/007-testing-stage/evidence/e2e-t008-findings.md:51`）→ start 变成分钟级阻塞，而 e2e 的 start 请求默认超时只有 10s（`tests/e2e/common.py:132`）。缓解：闸门等的是**门卫的判据**（`settings.json` 落盘，早于装包，秒级），且渲染完成/失败都会结束等待（无任意超时）；闸门不设新超时 → 没有「等不到就硬跑」的分支。残留：渲染线程本身挂死时 start 会一直等（登记为遗留：闸门需要一个可配的兜底上界，本靶不做；**R-3 只保证「异常退出时计数归零」，不等于「给等待加超时」**）。**闸门引入的新失败面（e2e）**：「先 PUT 清单再 start」的 8 个脚本里，`tests/e2e/test_13_coding_preflight.py:33`（完全不等渲染）、`tests/e2e/test_15_coding_run_control.py:35`–`:39`（只等 `coding-agent`）、`tests/e2e/test_16_run_liveness.py:73`–`:77`（只等 `plan-agent`）三个，闸门会**为同一 SOP 的第二个 profile 追加一次等待**（`coding-review` / `plan-review`），而 start 的默认超时只有 10s（`tests/e2e/common.py:132`）→ 可能新增超时失败。缓解：①闸门等的是**门卫的判据**（`settings.json` 落盘，早于装包，`src/config/render.rs:63`），不是整轮渲染；②§5 有一条判据把这条语义钉死（「所需 profile 已就绪但仍有渲染在途 → start 立即 202，不等渲染结束」）；③这 8 个脚本列入 §3.3 回归项。残留：若 e2e 真撞上超时（渲染并发序导致第二个 profile 迟迟不落盘），修脚本（先等齐本 SOP 的两个 profile）**不在本靶白名单** → 登记为遗留，交由操作员/下一场处理；本靶**不**为闸门加超时兜底（那会改「不等渲染结束」以外的行为，需重新裁决）。
- **R3 · 全局 run 上下文是线程级归属 + 进程级快照表** —— 心跳与墓碑的「当时阶段/步骤」来自观测模块的上下文（`src/observability/mod.rs:84`）。MVP 是串行执行（`src/server/handlers.rs:224` 的 `has_running_run` 守卫）→ 同一时刻只有一个 run，暂不冲突。将来并发化时，两个 run 的上下文会互相覆盖 → 墓碑里的阶段可能张冠李戴。缓解：快照表以 run id 为键、run 结束即清（`src/observability/mod.rs:118`）；**登记为遗留**（并发化不在本靶）。
- **R4 · 心跳是覆盖写 → 「本 run 发生过几次阶段转换」查不到** —— 纪律第 12 条要求状态文件能被覆盖写（不能追加成第二份真相），所以心跳只留最后一次状态。缓解：过程历史由里程碑日志承担（`phase.change` / `fix.start` / `fix.end` / `review.open` 逐条落盘）。残留：只看心跳文件无法还原完整时间线（要看日志 + 心跳两份）。
- **R5 · panic hook 是进程级全局** —— `std::panic::set_hook` 只能有一个；仓内单测若装 hook，会影响同进程的其他测试（尤其会吞掉它们的默认 stderr 行为）。缓解：验收测试放**独立测试二进制**（`tests/f6_observability/main.rs`），且安装接口返回上一个 hook 供还原（`src/observability/tombstone.rs:28`）；hook 内部**链回**上一个 hook（默认输出不丢）。残留：若仓内将来有别的测试也要装 hook，需要同样的隔离约定。
- **R6 · 里程碑日志不保证零丢失** —— tracing 的文件层是同步写但进程猝死（SIGKILL）时缓冲区里的最后几条可能丢（`src/logging.rs:36`–`:40`，writer 在 `src/logging.rs:40`）。这正是「墓碑 + 心跳」存在的原因：**日志是过程记录，墓碑/心跳才是死亡现场的硬证据**。本靶不追求日志零丢失（那要换同步 fsync 通道，超出单靶范围），登记为遗留。
- **R7 · 全 SOP 覆盖 = 白名单 24 个文件、回归面大** —— MVP 三循环 + doc 两处 + change 编排层 + 引擎层都要动，改动点分散。缓解：①每个改动点只加「一条日志 + 一次心跳更新」，不改判定；②既有单测是回归闸门（§3.3 逐条列出）；③`review_with_gate` 与三个协议解析器**不动**。残留：若审查认为改动面过大，退路是**只保留 change 侧 + 引擎侧**（把 MVP/doc 的里程碑撤掉，Q3 的裁决降级为「change 细粒度 + 全 SOP 的 SOP 级心跳」）——但那需要重新裁决覆盖面。
- **R8 · 白名单守卫的三个副本必须同步刷新** —— `tests/change_ask_ring.rs`、`tests/change_failure_context/main.rs`、`tests/change_review_protocol/main.rs` 各存一份 `BASELINE`/`WHITELIST` 并互相比对（a16/a29/a30）。漏刷任一份 → 比对失败 → **必红**。**更隐蔽的一条**：上一场的验收文件里还有**两处硬编码断言**绑死了上一场的白名单形状——`tests/change_review_protocol/main.rs:1337`（a29：守卫 WHITELIST 里不得含 `tests/f6_observability/main.rs`）与 `tests/change_review_protocol/main.rs:1437`（a32：计数）→ 本场口径（coding 步 23 个含该文件与 `src/server/tests/mod.rs` + 测试阶段 1 个 `tests/f6_observability/main.rs`）**必然让这两条变红**，只刷常量不改断言 = step 1 之后测试面直接红。缓解：①Q10 已把两处改写写进口径（排除项换成本场测试阶段文件、计数改 24/23）；②§3.1/§3.3/§5 三处点名；③step 1 一次性刷完三份副本**并**改写这两处断言。**本轮 R-4** 再加一条：**注释**与常量值不一致（`tests/change_ask_ring.rs:32`/`tests/change_ask_ring.rs:38`、`tests/change_failure_context/main.rs:33`/`tests/change_failure_context/main.rs:39`、`tests/change_review_protocol/main.rs:44`/`tests/change_review_protocol/main.rs:1316`）→ 读者按注释会数出 22 个文件、按旧基线 sha 复核 → 与断言冲突时以常量为真，但**必须**订正。
- **R9 · 白名单守卫断言「上一场遗留的两个文件不得进本变更」** —— `tests/change_ask_ring.rs:663` 钉住 `src/change/coding_gate.rs` 与 `src/config/skill_assets.rs` 不得出现在本变更的 diff 里。**本变更已把 `src/change/coding_gate.rs` 收进 §4**（操作员补录，step 15）→ 该守卫的豁免前提被钉在「该文件在本场 `WHITELIST` 常量内」（step 17）→ 前提成立即豁免成立；`src/config/skill_assets.rs` **仍禁**。缓解：白名单与常量三处同步（§4 / `tests/change_ask_ring.rs:39` / `tests/change_failure_context/main.rs:40` / `tests/change_review_protocol/main.rs:79`），任何一处漏改 → 守卫立刻变红（这正是它的设计意图）。
- **R10 · 新增文件在计划期不存在，机械门会报 finding** —— `src/change/plan_audit.rs:266` 对白名单里的每个路径查存在性 → 3 个新增源文件 + 1 个新增测试文件在**计划期**必然报「白名单里的文件不存在」。这是**预期内**的，但**必须**用反引号写进白名单（否则 `section_paths` 抽不到 → coding 步提交它们会被判「越界改动未申报」→ Quality）。现状：四个文件均已落盘 → finding 已消解为 0（§5 已同步）。
- **R11 · `path:line` 引用被逐条验存在性** —— `src/change/plan_audit.rs:256` 会把计划里每个 `path:line` token 拿去 `repo.join(path).exists()` 验 → 引用了不存在的路径 = 一条确定性 finding。本计划的新增文件**一律不带 `:行号`**（写全文件名），其余引用都指向真实位置。风险：coding agent 改代码后行号漂移 → 计划里的行号会过期（计划是审查期的快照，审查时以代码为准，登记在 D006 的遗留清单里）。**本轮已把全部行号重锚到 `HEAD=458c969`**（上一版计划是按实现前的基线写的，行号已整体漂移）。
- **R12 · 心跳与墓碑依赖 root（生产默认 `HOME/.devforge`）** —— 服务以别的用户/容器跑、或 `HOME` 未设时，墓碑与心跳会落到意料之外的路径（或无处可落）。缓解：`HOME` 未设时 main 已在 `src/main.rs:143` 失败（既有行为）；容器场景登记为遗留（部署形态未定）。**另一条诚实缺口：仓内单测会真的落盘** —— `src/server/tests/mod.rs:153` 的夹具与 `src/server/tests/mod.rs:958` 起的 `start_task_runs_engine_and_records` 走**真实 start 路径**（`src/server/handlers.rs:243` → `src/server/mod.rs:1360`）→ run 上下文被置位 → 心跳**真写**进 root（run id 唯一、run 结束保留）→ root 若默认 `HOME/.devforge`，**用户的 HOME 会被单测写脏**。缓解：**root 可注入**（Q4：进程内注入 > `DEVFORGE_HOME` > `HOME/.devforge`），本场取路径 ①：夹具（`src/server/tests/mod.rs:111`）把 root 指向临时目录；验收标准单列一条「用户 HOME 不新增心跳文件」。残留：进程级 root setter 被并行单测共享（谁最后设谁生效）→ 表现为落点漂移、不影响正确性（每个 run 的文件名带 run id，互不覆盖），登记为遗留。
- **R13 · 门卫复查的「明确失败」文案必须与现状一致** —— Q11 **不改错误文本**：`src/server/mod.rs:277` 的原文（`profile X 未渲染（缺 …）——先 PUT /api/v1/config/manifest 完成安装渲染，再启动 run`）已含处置建议，追加会重复、改写会让仓内 **5** 条单测断言（用 `contains("未渲染")`：`src/server/mod.rs:1918`、`src/server/mod.rs:1932`、`src/server/mod.rs:1949`、`src/server/mod.rs:1968`、`src/server/mod.rs:1984`）与两个 e2e 变红 → 本靶只改错误**种类**（`System` → `Config`）→ 文本一字不动，那 5 条断言与 2 个 e2e 必然仍绿。缓解：验收标准把「错误文本点名缺失的 profile 与路径」单列一条。
- **R14 · 闸门所需 profile 清单的统一入口（落点已定）** —— 清单现在分散在五个 `run_*` 方法的 `require_profile` 调用（`src/server/mod.rs:288`/`src/server/mod.rs:347`/`src/server/mod.rs:422`/`src/server/mod.rs:832`/`src/server/mod.rs:953`）+ change 侧既有常量（`src/server/change_engine.rs:24`）→ 闸门若另抄一份 = 两处清单漂移（改了 SOP 的 profile 需求，闸门还在等旧清单）。**统一入口定在 `src/server/mod.rs`**（`profiles_for_sop`，`src/server/mod.rs:253`）：六个 SOP 的门卫调用改为查表，change 一项**引用** `CHANGE_PROFILES`（那个文件不改、不进白名单）→ 清单只有一份、闸门与门卫读同一张表。残留：表的键（按 sop 名）与返回形状由 coding 阶段定，但**落点与事实源已定**（都在白名单内的 `src/server/mod.rs`）→ 不会白生出「闸门另抄一份清单」的漂移，也不需要新的白名单条目。
- **R15 · 文档与代码的既有不一致（本靶之外，不修）** —— `docs/plantree/plans/009-change-sop/topics/vs-mvp.md` 与 `docs/plantree/plans/009-change-sop/topics/agents-change.md` 的旧口径是上一场已登记的遗留（`src/change/plan_audit.rs:712` 起的守卫只钉问询环锚点）；本靶不改 009 的文档（Q12）→ 遗留继续挂着。**另一条已纠正的引证错误**：上一场计划把「装包分钟级」的实测引证写到 `docs/plantree/plans/002-config-management`，实际证据在 `docs/plantree/plans/007-testing-stage/roadmap.md:33` 与 `docs/plantree/plans/007-testing-stage/evidence/e2e-t008-findings.md:51`（本场已按正确位置引用；002 不改）。
- **R16 · 审计纪律的「先证后报」对本靶的额外要求** —— 本靶的核心交付物全是「可观测性」，最容易被「加了几行日志就算做完」蒙混：每条验收标准都要有**可机械复核的证据**（日志条目原文 / 墓碑文件 / 心跳文件内容），不能只交「代码里有 `tracing::info!`」。缓解：§5 的每条标准都写成「给定 → 期望输出/状态变化」，测试阶段逐条对账。
- **R17 · 闸门在 axum async handler 内等待 → 拖住 tokio worker** —— `start_task`（`src/server/handlers.rs:202`）是 `async fn`（在 tokio worker 上执行），闸门若在这里**阻塞**等待渲染，会占住 worker 线程（并发请求被拖住；worker 数有限 → 拖满即全站假死，而 e2e 的 start 请求默认超时只有 10s，`tests/e2e/common.py:132`）。缓解：等待用**可 await 的原语**（`tokio::sync::watch`，`src/server/mod.rs:1070`）或整体挪进 `spawn_blocking`，**不得**用 `std::thread::sleep` 轮询；验收标准单列一条「等待期间其他请求仍被及时处理」。残留：渲染线程自身挂死且不发完成通知时 start 会一直等（闸门**没有兜底上界**——本靶刻意不引入新超时，登记为遗留；R-3 只解决「异常退出后计数不归零」，不解决「不退出」）。
- **R18 · 闸门等待窗口落在「串行槽快判」与「真正占槽」之间** —— `has_running_run()` 的快判在 `src/server/handlers.rs:224`（**闸门之前**），真正占槽的 `begin_run` 在 `src/server/mod.rs:1340`（**spawn 之后的线程里**）→ 两个并发 start 可能**都**过快判、都过闸门、然后各自 spawn；第二个线程的 `begin_run` 失败 → 发 `run.error` 事件（`src/server/mod.rs:1349`）并返回。**与既有口径的关系**：串行槽语义**没变**（同一时刻仍只有一个 run 能跑），只是「第二个请求被拒」的**时点**从「handler 早拒（409）」挪到「线程里晚拒（`run.error`）」；§3.3 里两个 e2e 断言的是「start 仍受理 == 202」与「槽即时释放」，**不受影响**。缓解：①行为退化可接受（并发 start 本就该只有一个成功）；②更严的做法是「闸门等待期间就持有串行槽」（把快判挪到闸门之后）——**本靶不做**（会改 `start` 的既有 409 语义，登记为遗留）。验收标准单列一条「并发 start 至多一个 run 开跑」。
- **R19 · panic hook 里二次 panic = abort（遗言全丢）** —— 写墓碑失败若在 hook 里 panic，就是「panic 中 panic」→ 进程 abort，**连 stderr 链回都丢**（最后一条遗言也没了，等于把「死亡取证」变成「死亡无证」）；且**库路径不经过 `src/main.rs:143` 的 HOME 检查**（`tests/f6_observability/main.rs` 直调安装接口）→ HOME 未设时 hook 自己也可能炸。缓解：写入 **best-effort**（Q9）——写失败只记一行 stderr，**绝不**在 hook 里 panic；无论成败都**链回上一个 hook**；HOME 缺失 / root 不可写 / 磁盘满 → 降级（不写墓碑、不 panic、照常链回）。验收标准单列一条「写不进去时不 abort、hook 仍被链回」。
- **R20 · change 侧「审查轮次」只能是角色内序号（不是实例数）** —— 共享门返回的实例数 `opened`（`src/sop/review_gate.rs:130`）是**返回值**，调用前拿不到；change 侧 `review_once` 恒传 `base_round=0`（`src/change/session.rs:283`）→ `event=review.open` 只能记「该角色在本次 run 内的开庭序号」。缓解：①该序号由观测模块按 `(run, role)` 计数提供（`src/observability/mod.rs:196`），**不**给 `ProdChangeSession` 加字段（上一场加字段踩过「全仓字面量构造点必须同步补」的编译期耦合，生产装配在 `src/server/change_engine.rs:180`，会逼出额外白名单条目）；②违例重审/实例失败重试的实例数由 `review_gate` 自己的既有 WARN 如实记录（`src/sop/review_gate.rs:139`/`src/sop/review_gate.rs:163`），两处信息互补。残留：只查 `review.open` 看不出「这一轮审了几个实例」→ 需并看 `review-gate` 的 WARN（登记为遗留，不在本靶补字段）。
- **R21 · 各站点的轮次公式不同，实现时最容易「统一 +1」而写错** —— doc-fix 的 `report.review_rounds` 在 `src/sop/doc/fix.rs:91` **先自增再开实例**（1-based）→ 落点轮次 = `report.review_rounds`（**不带 +1**）；而 MVP 三处的计数在**调用之后**才自增（`src/sop/planning.rs:146`、`src/sop/coding.rs:191`、`src/sop/testing.rs:242`）→ 落点轮次 = 计数 **+1**；doc-from-zero 的 `bump_review`（`src/sop/doc/runner.rs:87`）返回值已是 1-based → 不加减。缓解：Q6 的逐站点口径表 + 验收标准里 doc-fix 的「首审 = 1、第二轮 = 2」这条专门判据。残留：若实现时把 doc-fix 也写成 +1 → 首审记成 2，验收即红。
- **R22 · 「agent 轮次结束」落点必须唯一（不许两处都写）** —— 若实现时在 `src/engine/client.rs:186`/`src/engine/client.rs:212` 落交卷事件的同时，又在 change 侧 `src/change/session.rs:293`/`src/change/session.rs:382`/`src/change/session.rs:500` 落一条 → 同一轮出现**两条** `agent.turn_end`（角色与耗时还不完全对齐）→ 「交卷到重审开庭之间有多久」这个问题反而算不清。缓解：Q5 已定「只留 `src/engine/client.rs` 一处」，验收标准单列一条「恰好一条 + 同一轮不出现第二条」。
- **R23 · 心跳的阶段字段与里程碑的阶段名必须同一套词汇** —— 心跳回答「死在哪个阶段」，里程碑回答「阶段怎么变的」；两处若各写各的（`Plan` / `plan` / `planning` 混用），事后对不上号。缓解：阶段名取编排层既有的 `ChangePhase` 变体名（`src/change/state.rs:20`）与各 SOP 的阶段标识，Q7 的字段词汇统一由观测模块定义；验收标准里 `phase.change` 的 `from`/`to` 与心跳的阶段字段用同一套值。
- **R24 · 质量失败的分级落点分两处，字段同构靠「共享 attempt」保证** —— Q2 落点 ①（重试层 `src/engine/policy.rs:88`）与落点 ②（`map_intervention` 的质量分支 `src/server/mod.rs:1030`）必须给出**同一组字段名**，否则「原始错误全量可见」在生产上仍是碎的（两处各写各的 → 过滤时对不上）。`attempt` 只有重试层知道 → 由**观测模块的 run 上下文**传递（`src/observability/mod.rs:170` 写、`src/observability/mod.rs:182` 读）→ 两处字段集真正同构。缓解：①上下文里取不到 `attempt`（run 刚开跑就 SOP 失败）时落 `attempt=0` 而**不是缺字段**；②`component` 允许不同（落点 ① 用 `run-retry`，落点 ② 用该 SOP 的既有 component 名）——**字段名集合**同构、**取值**可不同。残留：`attempt` 取自进程级上下文（并发 run 会互相覆盖，R3）→ MVP 串行（`src/server/handlers.rs:224`）下暂不冲突。**本轮 R-1 把这条的取值也修了**：写入时机前移后，落点 ② 读到的不再是上一次尝试的号。
- **R25 · 证据粒度：每步证据落独立文件** —— 上一轮的 `evidence/self-check.log` 是**覆盖写**的，只覆盖最后一步，却承担了全变更的举证责任（编码审查观察项 8）。缓解：本轮起每步证据落独立文件（`evidence/step-N-*.log`），不自相覆盖；本靶**不改**代码，只改记账口径（登记为流程约定）。
- **R26 · R-2 收紧放行条件 = 并发渲染时 start 多等一轮（e2e 10s 预算）** —— 现状是「任意一次渲染结束就放行」，R-2 改成「无渲染在途 **或** 所需 profile 已就绪」→ 并发两轮渲染时，先结束的那一轮**不再**放行 start，要等第二轮结束或所需 profile 就绪 → 等待时间变长，可能撞上 e2e 的 10s 默认超时（`tests/e2e/common.py:132`）。缓解：①单次 PUT 清单的 8 个 e2e 脚本里**不存在并发渲染**（每个脚本只 PUT 一次）→ 该面的行为**不变**；②所需 profile 仍是最短路径（`settings.json` 早写，`src/config/render.rs:63`）；③放行条件的两个分支都有明确的终止条件，不会无限等。残留：若将来有脚本并发 PUT 两次清单，start 会多等一轮（登记为遗留）。
- **R27 · R-3 的 drop 守卫会改变「谁负责递减计数」的归属** —— 现状是渲染线程体末尾手工调 `end_render()`（`src/server/handlers.rs:415`）→ 改成 drop 守卫后，**递减必须由守卫做**、显式调用必须去掉（否则计数被减两次 → `saturating_sub` 会把 0 变 0，但 `notify` 会被多发一次 → 放行条件被提前满足一次）。缓解：守卫持有闸门引用并在 `Drop` 里做一次「递减 + 广播」；实现后必须用「渲染成功后 `is_in_flight()` 为假」的单测钉住（§5 有对应判据）。
- **R28 · R-1 前移 attempt 写入 = 成功路径也会写上下文** —— 把 `set_attempt` 前移到**每次尝试开始**（`src/engine/policy.rs:79` 之前）后，**成功**的尝试也会更新 run 上下文（原先只有失败才写）→ 心跳里的 `attempt` 字段在成功路径上也会随尝试次数变化。缓解：这是**期望语义**（心跳的 attempt 字段本就该表示「当前是第几次尝试」）；§5 的既有判据只钉「字段存在且同构」，不钉成功路径的取值 → 不与既有判据冲突。
- **R29 · R-4 是纯注释改动，仍要过白名单守卫** —— 三份守卫文件的**注释**改动落在白名单内（`tests/change_ask_ring.rs`、`tests/change_failure_context/main.rs`、`tests/change_review_protocol/main.rs` 均在 §4），但注释改动**不产生行为差异** → 若只看 diff 可能被误判为「无依据改动」。缓解：§3.1 的三行都写明「注释与常量值一致」的理由，§5 有对应判据（注释里不残留陈旧数字），审查可逐条对账。
