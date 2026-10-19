# T019/T020 · 单元审 + 终审编排 证据

- 日期：2026-10-19
- 改动：`src/sop/doc_from_zero.rs`（`doc_final_instruction`）、`src/docgen/audit.rs`（`duplicate_mentions`）、`src/server/mod.rs`（终审 + 运行账单）

## 一、两个时机，同一个 profile

| 时机 | 范围 | 输入 |
|---|---|---|
| **单元审**（循环内每单元每次） | 只判本单元：该单元条目的覆盖 + 文档 lint | 引擎注入单元级覆盖报告 + lint |
| **终审**（循环结束后一次） | 全库：跨模块重复 / 汇编引用一致性 / 全量覆盖与 lint / 置信度抽样 | 引擎注入全量覆盖 + 全量 lint + 重复名单 + Runbook 命令块 |

两者都用 `doc-review` profile（`RpcReviewFactory::per_round_in`），每轮全新实例、用完即弃。

## 二、终审查的东西（单元审看不到的）

1. **跨模块重复**：`audit::duplicate_mentions` 列出"同一符号出现在 ≥2 个非 `_meta` 文档"的名单（元数据目录天然列全名，排除），交由终审判定该拆还是该引用
2. **汇编层引用**：总览/架构里的链接由 lint 的 link 检查兜底（死链 FAIL），上层断言可溯源由终审人工判
3. **全量 lint 与覆盖**：引擎审计直接给结论，指令要求"勿重算"
4. **置信度诚实性抽样**：抽 3 条 🟢 看能否给 `path:line`

## 三、终审失败即 run 失败

- `PASS` → `RunOutcome::Completed`
- `FAIL` → 返回 `System("终审未通过（route=…）：issues…")`，由既有 `map_intervention` 处理
- 协议违例 → 同样判基础设施错误（不静默通过）

## 四、运行账单（t12 前置）

终审时写 `docs/contexts/_meta/runs/<run-id>/report.md`：run id、完成单元、审查轮次、累计问题、终审结论、终审审计全文。

## 五、单元 FAIL→route→修→PASS 的完整回路

单测 `fail_message_contains_route_and_issues`：agent 第一轮发完成标记 → 审查 FAIL + `<route>phase1</route>` + issues → 编排层回注**含 route 与问题清单**的消息 → agent 第二轮修完再发标记 → 审查 PASS → 单元落盘、状态写入。断言了回注消息里确实带回 `<route>phase1</route>` 与具体问题。

## 六、单测与门禁

新增：`duplicate_mentions_flags_repeated_symbol`、`duplicate_mentions_ignores_meta_dir`。
`cargo test` **257 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。
