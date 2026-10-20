# 路线图（`010-run-resume`）

> 每项完成时把证据写在该项下方（命令 + 退出码 + 结论），并勾选。

## Phase 0 · 抽象与状态搬家

- [x] **(T001) 公共状态壳 + 黑板落点（按 SOP 隔离）** — 定义 `RunState`（公共壳：`schema_version` / `sop` / `project` / `task` / `updated_at` / `attempt` / SOP 扩展段 `data`），落点 `plans/<project>/<task>/<sop>/state.json`；原子写（临时文件 + rename）、损坏容错（解析失败按空状态 + 记 WARN，不炸 run）；加载时校验四元身份，不一致拒绝续跑、按冷启动处理。
  - 验收：单测覆盖「不存在 / 合法 JSON / 半截 JSON / 只读目录 / 身份不符」五种输入；反向——同一 task 下两份不同 SOP 的 state 互不覆盖；写一半中断后重启不得读到半个 JSON。
  - **证据**：commit `8ccc246`（`src/runstate/mod.rs`，9 单测）。
- [x] **(T002) SOP 接入 trait** — `Resumable`：`state_path` / `verify_done(&state) -> Vec<RedoUnit>` / `resume_brief(&state, redo)`；引擎在 start 与重启路径统一调用。
  - 验收：用一个假 SOP 证明引擎不关心单元语义（trait 里不出现任何 doc/coding 字样）。
  - **证据**：commit `272dfe9`（`src/runstate/resume.rs`，含 `trait_surface_stays_agnostic` 抽象守卫）。

## Phase 1 · 续跑语义

- [x] **(T003) `start` 默认续跑 + `fresh`** — 按请求里的 `sop` 定位该 SOP 自己的 state：有且未完成 → `resumed: true` 并从 done 之后推；`{"fresh": true}` → 清该 SOP 的检查点、`resumed: false`、全量重做；无 state → 冷启动。**不影响其他 SOP 的状态文件**。
  - 验收：单测三路输入；反向——fresh 之后 `done` 必须为空（不得残留旧单元）。
  - **证据**：commit `f0dcef2`（`start_gate` + 接口级单测 `start_reports_resumed_and_fresh_clears_only_that_sop`）。
- [x] **(T004) 续跑前复核 done** — 引擎调 `verify_done`，不合格单元从 `done` 摘掉、并入本次重做集合，写进 kickoff 之前的恢复语。
  - 验收：反向验证——手工删一篇已 done 单元的正文，续跑必须重做该单元（断言其 kickoff 出现）；证据齐备时跳过。
  - **证据**：commit `45be31c`（`DocResume` + `broken_evidence_unit_is_reincluded_and_rewritten` / `intact_evidence_is_not_redispatched`）。

## Phase 2 · 韧性

- [x] **(T005) 基础设施故障自动重启** — `DevforgeError::System` → 同 run 重启，最多 3 次，退避 10s/30s/90s，`attempt` 递增，从断点继续；3 次仍败 → `Failed`（错误串带每次尝试的末段）。
  - 验收：mock 引擎「前两次抛 System，第三次成功」→ run 终态 Settled 且 attempt=3；「连续四次 System」→ Failed 且 attempt=4（不无限）。
  - **证据**：commit `ea02a57`（`src/engine/retry.rs` + 接口级单测 `infra_failures_are_retried_up_to_three_times` / `persistent_infra_failures_fail_after_four_attempts`）。
- [x] **(T006) 质量失败不重启** — 明确代码路径：`Verdict::Fail` / 协议违例上限 / 越界申报 → 直接 Failed，不进重启分支。
  - 验收：反向验证——审查连续 FAIL 的 run，attempt 恒为 1。
  - **证据**：commit `814b308`（新增 `DevforgeError::Quality`，coding/testing/doc 三处协议违例上限与无标记卡死改判质量失败；单测 `quality_error_becomes_failed_and_is_not_retried`）。

## Phase 3 · 接入与实证

- [x] **(T007) `doc-from-zero` 接入** — 状态从 `docs/contexts/_meta/` 搬到黑板 `plans/<p>/<t>/doc-from-zero/state.json`（D001）；done 证据 = 产出文件清单 + `doc-lint` 退出码（D003）；恢复语按 D005 由 SOP 提供。
  - 验收：清空 `docs/contexts/_meta/` 后第二次 start 仍能续；仅留旧位置 state 时判冷启动。
  - **证据**：commit `45be31c` + `1dc1521`（`checkpoint_lives_on_blackboard_not_in_output_dir` / `legacy_output_dir_state_does_not_participate_in_resume`）。
## 验收阶段（不在本轮代码阶段）

> 本轮（代码阶段）只写单测，不跑端到端（用户 2026-10-20 口径）。以下两项待验收时再做，**不算本轮任务**。

- [ ] **(T008) 续跑实证（e2e）** — `test_22` 增一条：第一个单元完成后杀掉 run，第二次 start 不得重写第一单元、只推后续。
  - 验收：两轮产物 diff 为空（第一单元部分），会话里无该单元 kickoff。
- [ ] **(T009) 真实故障实证** — 用可控方式制造一次上游故障（指向坏 baseUrl 的 profile），确认自动重启真实发生并最终 Settled。
  - 验收：日志里出现两次重启 WARN 与一次成功；`_meta/runs/<id>/report.md` 记录尝试次数。

## Milestones

- **M0**：抽象 + 状态搬黑板（T001/T002）——引擎不再依赖任何 SOP 的落点。
- **M1**：续跑可用（T003/T004）——掉线重发同一条 start 即继续。
- **M2**：韧性可用（T005/T006）——外部故障自愈，质量失败照旧判死。
- **M3**：doc SOP 接入（T007）。
- **M4**（验收阶段）：e2e 续跑实证 + 真实故障实证（T008/T009）。

## Deferred

- 增量重生成（受影响单元才重做）——归 008 T023–T025。
- 跨机器共享断点（黑板在单机上，远端调度不在此计划）。
- run 级 UI 的「重试可视化」——只保证 API 字段可断言。
