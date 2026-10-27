# 文档审查报告 — unit-src-22（第 20 轮）

- 单元：`unit-src-22` → `src/change/testing_gate.rs`；文档 `docs/contexts/src/change/testing_gate.rs.md`（250 行）
- 引擎审计（确定性，采信不重算）：要求 11 条 / 已覆盖 11 条 / 缺失 0；doc-lint 通过（0 违规）
- 边界（DR-12）：不打开源码；事实以 `_meta/facts/unit-src-22.md`、`_meta/gaps.md`、plan tree、引擎审计与实跑输出为准。

## 1. findings 处置

工作区无 `testing/findings/`；kickoff 未附本单元 doc-side finding。`gaps.md` 登记本单元 3 条：`reconcile_acceptance` 无生产调用点、空 `test_ref` 分支不可达且无测试、回归门只管单向。文档在 `:62`（反向不判）、`:68`（宽松匹配 + 仅 re-export）、`:107`-`:114` 与「未覆盖」段（空引用分支）逐条披露并指回 gaps，**无遗漏**。

## 2. 覆盖验收（11/11 采信）

11 = module ×1 + 公开符号 ×10（`REGRESSION_LOG`、`AcceptanceMatch`、7 个函数、`FAILURE_BLOCK_TITLE`）。「公开成员」表 10 行逐行链到独立小节，锚点 `#regression_log` / `#acceptancematch` / … 可解析；12 条 behavior 由「测试覆盖」表逐例承接，行号 155/164/171/177/184/194/204/211/217/240/250/260 与 facts 逐条一致。无悬空条目。

## 3. 完整性 / N-A

- 模块三件（职责含「负责/不负责」、公开成员、依赖）齐备。
- 7 个公开函数六件齐备（摘要/行为语义/参数/返回/异常/示例）；`parse_acceptance_table` / `route_back_message` / `failure_details` / `coding_failure_block` 的异常 N/A 均给理由。
- `AcceptanceMatch` 五件（职责/构造/公开成员/生命周期/典型用法）齐备；两个常量按配置项七字段写；`reconcile_acceptance` 的空引用分支与无生产调用点显式披露。
- Runbook 七件套：环境（edition 2024 ⇒ Rust 1.85+）、依赖（`cargo fetch` + `tempfile`）、测试运行（命令 + 预期输出 + 12 例）、隔离（tempdir、不联网/不 spawn）在册；配置项 / 启停与健康检查 / 种子数据 / 公开接口清单四项 N/A 且逐项给理由（库内模块）。
- 无 TODO/TBD、无整段日志粘贴、无死链。

## 4. 真实性 / 置信度

- 17 处 🟢 均带 `src/change/testing_gate.rs:<line>` 或 `src/change/session.rs` / `orchestrator.rs` 的 path:line；3 处 🟡（最后箭头切分、`reconcile_acceptance` 未被运行时使用等）标 INFERRED/依据，诚实。
- 与 facts 逐项核对：符号行 18/22/28/45/57/77/118/128/137/142；消费点 `mod.rs:22/43-46`、`session.rs:20-22/136/165/194/530/536/538-545/1006/1033/1060`、`orchestrator.rs:333/334/345/356/360/364`、`coding_gate.rs:104/557`、`tests/change_failure_context.rs:24/278/298/477/491`；测试行号与 facts 全同。
- 两处证据括注需校正（非阻断，见 notes）：`:65` 的零信息话术断言在 `:260` 而非 `:250`；`:44` 的签名钉扎用例起于 `:501`，`:502` 是其注释。

## 5. plan tree 一致性

P009 T011（回归优先 + 验收对账，`roadmap.md:67`）、T011c（失败明细原文送达，`:73`）、D017（引擎不跑回归）、D018/D025（回注带 `path:line`、三出口同源、不剥不改写）、D019（testing 不给 codegraph）与文档逐条对应，无硬冲突。

## 6. 内部自洽

职责边界（不决定何时回注/不跑测试/不解析通用证据格式）与各函数语义一致；`parse_acceptance_table` 丢弃空引用行 ↔ `reconcile_acceptance` 空引用分支仅手构造可达，两条口径互补；`reconcile_acceptance` 无生产调用点与运行时 `summarize_acceptance` 精确相等对账已显式并列；返回 `Ok(criteria.len())` 与示例一致。未发现行为语义矛盾。

## 7. 可跑（照 Runbook 从零 boot）

- 环境：`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`、`TMPDIR=/var/tmp/r20t/tmp`（`/tmp` 已 100%）、全新 `CARGO_TARGET_DIR=/var/tmp/r20t/target`。
- `cargo fetch`：EXIT=0。
- `cargo test --lib change::testing_gate`：从零编译 38.21s，`running 12 tests`，`test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 586 filtered out; finished in 0.00s`，EXIT=0；12 个用例名与文档测试表逐名一致，预期输出吻合（编译耗时 38.78s→38.21s 属机器抖动）。
- 配置 / 启停 / 健康检查 / 种子数据：库内模块，无独立进程，N/A 理由成立。

## 8. 第三方可测标准

字段/常量语义、三态回归判定（绿 / 红如实 / 红称绿）、验收宽严两套判据、空引用分支、回注消息与 coding 送达块的逐字断言、12 例测试表与示例，足以让第三方在不看源码的情况下写出等价测试；「不负责」边界避免把编排层行为误写进本单元用例。

## 结论

覆盖 11/11、doc-lint 0 采信；findings 无遗漏；plan tree 无冲突；必填节/N-A 齐备；🟢 可回指；Runbook 从零实跑 `cargo fetch` EXIT=0、12/12 EXIT=0 且输出吻合。**PASS**。两处 test 行号括注建议顺手修正，不阻断。

## 非阻塞建议

1. `:65` 的「用例 `:217` / `:250` 反向钉住…三种零信息话术」：`:250` 是 `failure_details_render...`（只断言逐字渲染），三种话术的反向断言在 `:260` `coding_failure_block...`；建议改为 `:217` / `:260`（测试覆盖表已按 :260 写）。
2. `:44` 的「`src/change/coding_gate.rs:502` 的签名钉扎用例」：用例起于 `:501`，`:502` 是其注释（facts 与 `coding_gate.rs.md` 均记 :501）；建议写成「`:501` 用例（`:502` 注释）」。
3. `:64` 的 `:79`-84 只覆盖「计划无验收标准」分支；「缺任一条」在 `:86`-99，建议补锚或拆句。
4. `:234` 的命令块用 `/var/tmp/dfz-src22b/{tmp,target}` 绝对路径，建议给 `mktemp -d` 或先 `mkdir -p`，便于第三方复制（本机已有该目录，本轮换 `/var/tmp/r20t` 验证）。
