# 第三方审计核实 — code-review 线批次（2026-10-25 第六份）

> 外部第三方对全量审计流水线（`src/audit/**`）的 22 条发现（8 major + 14 minor），
> 本档为逐条源码核实记录。**判定：22/22 属实**。
> 本档只记录，修复待用户拍板。
> 核实方式：逐条对源码（file:line 实读）。

## 总评（第三方口径，核实一致）

code-review 线在**正常路径**可跑通，但达不到「自审自己」的可靠标准：
丢发现（M1）、整次审计因单条格式抖动报废（M2）、跑一次数小时且无内部超时（M3/M4）、
覆盖不可对账（M6）、报告层仍有幽灵与重复计数（M7/M8）。

## major（8 条，全部属实）

| # | 判定 | 源码证据（实读核实） |
|---|---|---|
| 1 去重键丢发现且可降级 severity | ✅ 属实 | report.rs:9-23 key = `(path, line, view)`；同键命中时只比 `evidence.len()` 取长的一条，**severity/claim 不参与**——同位置同视角不同主张直接合并，critical 可被 nit 顶掉。文件注释（:6-7）与函数名承诺「同位置不同主张 → 保留」，实现相反。测试同款盲区：report.rs:151 `same_position_different_claims_are_kept` 用的是 Correctness vs **Security**（不同 view）→ 删掉 claim 字段仍绿 |
| 2 Stage 0/2 无重试，单次抖动报废整次 | ✅ 属实 | Stage 1 有 2 次换实例（pipeline.rs:149 `for attempt in 0..2`）；Stage 0 的 `extract_json_block(...).map_err(Quality)`（pipeline.rs:104-107）与 Stage 2 同款（pipeline.rs:211-214）都是单发；runner.rs:59/158 也不重试。Stage 2 在全部批次跑完之后 → 一次 ```json 不合规即整次 Err |
| 3 Stage 1 串行，与设计不符 | ✅ 属实 | runner.rs:97 `for unit in &ordered { ... stage1_review_batch(...) }` 单线程顺序；pipeline.rs:5 注释「批次间无污染、可并行」。M3 实测 ~6 分钟/批 × 87 批 ≈ 9h |
| 4 审计内部无单批/总时长超时 | ✅ 属实 | builtin.rs:132 把 `self.idle_ms` 交给 watchdog，而 main.rs:185 默认 `None`（DEVFORGE_RUN_IDLE_MS 未设 = 永不收割）；run_audit 无单批超时、无总预算。M3 的 14h 挂死正是这条路径 |
| 5 空仓库/错 workspace → Completed + 空报告 | ✅ 属实 | mod.rs:45 `build_plan` 无 Module/Symbol 条目时 units 为空（:53-57 其余类型静默丢弃）；runner.rs:44 直接 build_plan 后无空校验，Stage 0 空规划 → 循环 0 次 → runner.rs:212 `Ok(Completed)`，报告 0 条 |
| 6 「全量」不可对账 | ✅ 属实 | runner.rs:81 `filter(|u| !review_plan.skip.contains(&u.id))`——skip 列表来自 LLM（pipeline.rs:99 instruction），无白名单校验、被 skip 的单元不进报告；mod.rs:53-57 非 Module/Symbol 条目静默丢；报告字段只有 units_total/batches_done（runner.rs:180-182） |
| 7 report.json 的 cross 未经核验 | ✅ 属实 | runner.rs:187 `"cross_analysis": cross`（原始集合），而 severity_summary（:188）与 MD（:199）用的是 `cross_kept`——机器读 JSON 会拿到刚被 verify 丢弃的幽灵，人读 MD 与机器读 JSON 结论不一致 |
| 8 严重度汇总与 MD 重复计数 | ✅ 属实 | runner.rs:264-283 `severity_summary(&deduped, &cross_kept)` 直接相加；report.rs:44 `local.iter().chain(cross.iter())` 同样直接拼——跨单元与本地的同一发现算两次、列两行 |

## minor（14 条，全部属实）

| # | 判定 | 源码证据 |
|---|---|---|
| 1 断点产物损坏硬失败 | ✅ 属实 | runner.rs:313-323 `read_stage0/read_stage2` 直接 `?`；而 `Progress::load` 口径是「损坏即重跑（WARN）」——两处不一致 |
| 2 已完成批次发现文件损坏 = 永久丢 | ✅ 属实 | runner.rs:288-311 `load_persisted_findings` 缺文件/坏 JSON 只 WARN 跳过，但 `done_batches` 仍含该批 → runner.rs:98 断点继续跳过，永不重审 |
| 3 Markdown 表格可被内容破坏 | ✅ 属实 | report.rs:95-102 直接把 claim/evidence 插进 `|` 表格（未转义 `|`/换行），view 用 `{:?}`（`Correctness` 而非 kebab-case，与 JSON 不一致） |
| 4 Stage 2 静默丢条目 | ✅ 属实 | pipeline.rs:218-221 单条反序列化失败 `continue`；:226-236 concern_verdicts 非二元组静默丢弃——无计数无 WARN |
| 5 预查数字会骗人 | ✅ 属实 | runner.rs:231-246 只跳过 `starts_with("//")` 的行；行尾注释、字符串字面量里的 `.unwrap()`、`#[cfg(test)]` 代码全计入，而提示词要求「预查是确定性事实，直接采信」 |
| 6 行号截断 | ✅ 属实 | finding.rs:73-76 `as_u64() ... as u32`，超大值静默回绕成合法小行号 |
| 7 提示词承诺未被解析器执行 | ✅ 属实 | code-review-agent.md 要求协议块「回复第一行」，finding.rs:54-60 用 `reply.find(...)` 任意位置都收，前置散文不影响 |
| 8 Stage 2 输入未去重 | ✅ 属实 | runner.rs:158 传 `&verified.kept`（核验后、去重前）——跨单元分析看到重复发现 |
| 9 审计无 phase 推进 | ✅ 属实 | server/mod.rs:468 `other => other` → code-review 的 phase 恒为 "code-review"；src/audit 内无任何 set_phase 调用，心跳/SSE 全程静态 |
| 10 报告缺溯源 | ✅ 属实 | runner.rs:177-189 报告字段无 workspace / commit / inventory 哈希 |
| 11 符号链接可越界 | ✅ 属实（低危） | verify.rs:23-25 `path_traversal` 只挡绝对路径与 `..`；仓库内 `link -> /etc` 仍会被读（只取行数，无内容外泄） |
| 12 代码卫生 | ✅ 属实 | runner.rs:108-109 同一句注释重复两遍；runner.rs:158 `factory.open()...?;        let out = ...` 两条语句挤一行 |
| 13 abandoned 不持久 | ✅ 属实 | pipeline.rs:242-246 `Progress` 只有 stage0_done/done_batches/stage2_done；abandoned 是 runner 局部变量，重跑会重试（可接受）但报告口径不完整 |
| 14 无发现数量上限 | ✅ 属实 | finding.rs:64 循环无上限；报告层也无 cap |

## 测试质量（第三方口径，核实一致）

- **空洞/误导**：report.rs:151 用不同 view 绕开了 M1 的真实缺陷——是本轮最该先修的测试。
- **缺失回归**：空 plan（M5）、skip 覆盖（M6）、Stage 0/2 抖动（M2）、去重降级（M1）、
  跨层重复计数（M8）、损坏 findings 重跑（m2）、Markdown 转义（m3）全无测试。
- **有效的**：verify 的零证据/line 0/穿越、Stage 2 幽灵进 dropped、LIFO→FIFO、实例失败打断。

## 修复优先级（第三方建议，核实后认同）

| 级别 | 项 | 理由 |
|---|---|---|
| P0 | M1 去重键 + 测试重写 | 直接丢发现、可降级 critical，自审结论失真 |
| P0 | M2 Stage 0/2 重试 | 一次抖动报废数小时 |
| P0 | M5 空 plan 显式失败 | 假成功最难发现 |
| P1 | M7 cross 写核验后集合 | 机器报告含幽灵 |
| P1 | M8 跨层去重 | 数字虚高 |
| P1 | M6 覆盖对账 + skip 校验 | 「全量」可证 |
| P1 | M4 单批超时 | 防 M3 式挂死 |
| P2 | M3 批次并行 | 自审 9h → 可接受 |
| P2 | m1/m2 断点自愈 | 长跑可靠性 |
| P3 | m3-m14 | 体验与口径 |

建议落地顺序：M1+M2+M5（纯逻辑）→ M7/M8+m2（报告正确性）→ M6+M4（可观测与兜底）→ M3（并行）→ 再做一次真实自审。

## 与前五份的关系

- 连续第七份外部报告 **0 误报**（22/22 实锤），六批累计 84 条发现 84 条实锤。
- M1 与 P013 归档时我们自己写下的「同位置不同主张→保留」承诺正相反——**注释/函数名承诺与实现相反**这一类病在 change/doc 批次已各出过一次。
- M2 与 doc-from-zero 批次 #6（终审无协议重试）同族：**同一类韧性只落在部分阶段**。
- M4 与 engine 批次 #9（两条 kill 路径）/ reverify 的 budget-fuse 同族：**有兜底机制但默认不启用**。
- M6 与 change 批次 #3（changed_files 过滤清单）同族：**分母不透明 = 指标可造假**，这次是「审计自己审了哪些文件」不透明。
- M3 意味着 P013 的 M3 长跑（9h/87 批）在当前串行实现下不可重复执行——自审流程的天花板在吞吐，不在单条质量。

## 修复状态表（2026-10-25，22/22 全部落地）

> 终验 828 绿 0 failed（lib 718 + integration 19/16/31/43 + doctest 1，TEST_EXIT=0），
> clippy lib 11 条（比修复前 13 条**降 2**，无新增；CLIPPY_EXIT=0）。单个代码 commit `6134b71`。
> 新增回归 12 条。

| # | 落地内容 |
|---|---|
| M1 | 去重键 = `(path, line, view, 归一化 claim)`（小写+压缩空白）；真重复仍合并但 **severity 取更重**、证据取更长。测试重写为同视角不同主张（旧测试用不同 view 绕开缺陷）+ 新增「critical 不被 nit 顶掉」断言 |
| M2 | `stage0_plan`/`stage2_cross` 拆出纯解析函数 `parse_stage0`/`parse_stage2`；runner 统一走 `with_stage_retry`（2 次换实例，只重试 Quality，System/Agent 直接上抛）。回归：两阶段各一条格式抖动用例 |
| M3 | `AuditSessionFactory` 增 `fork()`（默认 None = 测试替身串行）；生产桥用共享 spec 生成器的工厂副本；Stage 1 固定并发（`DEVFORGE_AUDIT_CONCURRENCY`，默认 4，1..=8）工作窃取派发，结果按下标排序落盘；`RpcReviewFactory` 的 spec 生成器与序号计数器下沉 `Arc<Mutex>` 保证 session 目录唯一。回归：并行路径两批全完成 |
| M4 | code-review 默认失活阈值 `DEFAULT_AUDIT_IDLE_MS = 15min`（agent 流式输出持续刷新 liveness，静默 15 分钟只可能是挂死）；其余 SOP 仍默认不收割。注：病态挂死现在会被 watchdog 收割成 Failed 并由断点续跑接手，**未做单批级 abandon**（需可中断的会话隔离，另立） |
| M5 | `plan.units.is_empty()` → `Err(System("无可审源码…workspace=…"))`；报告新增 `files_total`/`workspace`/`workspace_sha`。回归：空仓失败 |
| M6 | 报告新增 `coverage {source_files, sliced_files, excluded_entries, units_total, skipped_units, batches_done, batches_abandoned}`；`AuditPlan` 携带 source_files/excluded_entries；skip 做白名单校验（不存在单元号 WARN 忽略）+ **全量 skip 直接 Quality 拒收**。回归：skip 全量拒收 |
| M7 | JSON `cross_analysis = {findings: 核验后, dropped_ghosts, concern_verdicts, parse_errors}`——不再写原始集合。回归：JSON 无幽灵 |
| M8 | 跨层去重一次（deduped + cross_kept）→ 汇总与 MD 都用合并集；`severity_summary_of` 改单集合入参。回归：跨层重复只算一次（major=1）、幽灵不进汇总 |
| m1 | `read_stage0/read_stage2` 改返回 `Option`（WARN），损坏时清标记重算。回归：损坏 stage0 重算 |
| m2 | `load_persisted_findings` 返回 `(findings, bad_ids)`，坏批次从 `done_batches` 摘除并重审。回归：损坏发现文件被重审覆盖 |
| m3 | MD 表格单元转义（`|`→`\|`，换行→空格）、view 用 kebab-case。回归：转义与结构列数 |
| m4 | `CrossAnalysis.parse_errors` 计数（单条反序列化失败 / 非二元组 verdict）+ WARN + 入报告 |
| m5 | 预查剔除行尾注释；口径降级为「参考计数」（含字符串与 `#[cfg(test)]` 命中），**提示词与 batch_instruction 同步改为非裁决依据** |
| m6 | `u32::try_from(line)`，越界即协议违例。回归：4294967296 被拒 |
| m7 | 提示词改为「协议块必须在正文之前（允许一句总述），取第一个块」——与解析器行为一致 |
| m8 | Stage 2 输入改 `deduped`（去重后） |
| m9 | 五段 `set_phase`：audit-slice / audit-plan / audit-batch / audit-cross / audit-report |
| m10 | 报告带 `workspace` + `workspace_sha`（git HEAD，非 git 仓 = null） |
| m11 | 核验读取改为 canonicalize 后前缀校验（仓内符号链接越界 → 拒读 + WARN） |
| m12 | 重复注释与两条语句挤行已清（随 rewrite） |
| m13 | `Progress.abandoned`（serde default）入断点，报告取历史 + 本次 |
| m14 | `MAX_FINDINGS_PER_BATCH = 100` 截断 + WARN。回归：超限截断 |

