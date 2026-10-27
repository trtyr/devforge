# 文档审查报告 — unit-src-09（第 14 轮）

- 单元：`unit-src-09` → `src/change/mod.rs` + `src/change/orchestrator.rs`
- 文档：`docs/contexts/src/change/mod.rs.md`（95 行）、`docs/contexts/src/change/orchestrator.rs.md`（259 行）
- 引擎审计（采信，未重算）：要求 24 条 / 已覆盖 24 条 / 缺失 0；doc-lint 通过（0 违规）
- 边界：按 DR-12 不读源码；事实以引擎审计为准，交叉核对只用文档侧产物（`_meta/facts/unit-src-09.md`、`glossary.md`、兄弟单元文档）、plan tree（P009 T013/T011b/T011c/T012、D025、`topics/boundary-protocol.md`）与实跑输出

## 1. findings 处置完整性

工作区无 `testing/findings/`（kickoff 未附本单元 findings）；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与本单元无关。**N/A，不构成遗漏**。

## 2. 覆盖对账（24/24 采信）

要求集合 = 2 个模块（`mod:src/change/mod.rs`、`mod:src/change/orchestrator.rs`）+ 22 个符号（`mod.rs:10`–`22` 的 13 个子模块 + orchestrator 的 9 个公开符号：`MAX_TEST_REROUTES:21` … `run:155`）。

- `mod.rs.md`：13 个子模块逐个进「公开成员」表并给出单元号；另有 23 行转发块逐行拆成的「转发导出清单」（13 行，行段 24–27/28/29/30/31/32/33–35/36/37/38–40/41/42/43–46 合计 23 行，与 `:24`-46 严丝合缝），无伪覆盖。
- `orchestrator.rs.md`：9 个符号各有独立小节（常量 2、枚举/结构/trait 6、`run` 1），标题锚点与「公开成员」表一一对应。
- 清单外的 23 条 behavior 由「测试覆盖」表承接，名称/行号与 facts 的 23 个锚点逐条相同。引擎 24/24 与文档面吻合。

## 3. 完整性 / N-A（通过）

- 模块×2：职责（含「不负责」）/ 公开成员 / 依赖 三件齐备。
- 公开类型六件（PlanTurn、CodingTurn、TestingTurn、ChangeSession、ChangeRun、ChangeOutcome）：职责、构造/初始化、公开成员、生命周期、典型用法齐全；`ChangeRun` 显式写「派生 trait：N/A（无 derive）」。
- 两常量：摘要/取值/行为语义/参数 N/A/返回 N/A/异常 N/A/示例 齐备。
- `run`：摘要、行为语义、参数、返回、异常、示例 六节齐备；`phase_change` / `fix_step` 属内部符号，按契约只需摘要+行为语义，实际还多给了参数/返回/异常。
- Runbook 七件套：环境要求（edition 2024 → Rust 1.85+、`RUSTUP_TOOLCHAIN`）、依赖安装（`cargo fetch` + `tempfile`）、测试运行方式（命令+**预期输出 `23 passed; 0 failed; 577 filtered out`**）、隔离方式（每例独立 tempdir、不碰真实 plans 根、无 git/端口/网络）由「测试运行与环境」承担；配置项/启停与健康检查/种子数据/公开接口清单四项均写 `N/A` **并给理由**，无「省略当不适用」。无 TODO/TBD、无日志粘贴。

## 4. 真实性 / 置信度诚实性

- 23 个测试行号、9 个符号行号、13 个 `pub mod`/转发行号与 facts、inventory、glossary 逐条一致；依赖 import 表（`:11` Path、`:13` boundary、`:14` report、`:15` state、`:16` error、`:17` StateStore、`:18` UserLink）与 facts 的 `:13` boundary 记录不再冲突（round 8 的 FAIL-2 已修）。
- 文档给出的可重放 grep（5 个别名调用点为 0）本轮复跑：`grep -rn "change::run_change\|…" src tests` → EXIT=1（无命中），与正文一致；「同名词不同实体」的 `change_engine.rs:66 run_change` / `server/mod.rs:240` 分支确有命中，提示正确。
- 🟢 全部 14+4 处均带 `CONFIRMED <path>:<line>`，无裸绿；唯一的 🟡（`:194` 未落盘分支的内存字段不回写）标注 INFERRED 并给出推理依据，属诚实披露。
- 一处**锚点笔误**（非阻塞，见建议 1）：`:247` 写 `edition 2024（Cargo.toml:5）`，实际 `Cargo.toml:4`（`:5` 为空行）；`Cargo.toml:27 = tempfile` 正确。

## 5. plan tree 一致性（无硬冲突）

- T013「往返上限 3 / 第 4 次停下并报错」↔ 文档 `MAX_ROUNDTRIPS=3` + 「第 4 次往返拒」用例（`:712`）一致。
- T011b 断点落点 `plans/<p>/<t>/change/state.json`、跨会话保计数 ↔ 「核心口径·断点」「保存与回推矩阵」一致。
- T011c/D025 失败上下文：写入、下一轮送达、覆盖/消费即清/终态清、**超限不写不清** ↔ `run` 三处迁移与 `MAX_TEST_REROUTES` 节一致。
- T012 报告（diff/审查轮/往返/用量/验收对账）↔「报告」段落与 `ChangeRun` 字段说明一致。
- 披露型差异（不判冲突）：`boundary-protocol.md:45` 写「停下问人」，文档明示编排层实际以 `Quality` 结束、`run` 不调用 `ask`；T013 验收口径是「停下并报错」，文档侧属如实描述并已加 ⚠️ 提示（与 round 8 结论一致）。

## 6. 可跑（从零 boot，通过）

按文档「测试运行与环境」原文执行，未做任何替代：

- 环境：`/tmp` 已 99% 满 → `TMPDIR=/home/trtyr/code-work/tmp-r14`；`RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`；`rustc 1.99.0` / `cargo 1.99.0`（满足 edition 2024 的 1.85+）。
- 装依赖：`CARGO_NET_OFFLINE=true cargo fetch` → EXIT=0。
- 启动/健康检查（库内模块以单测代探活）：`CARGO_TARGET_DIR=/home/trtyr/code-work/dfz-r14-target`（全新空目录，从零编译 40.3s）`cargo test --lib change::orchestrator` → **23 passed; 0 failed; 0 ignored; 577 filtered out; EXIT=0**，与文档写的预期输出逐字一致。
- 用例名核对：实测 23 个 `test change::orchestrator::tests::*` 与文档测试表 23 行 diff 为空（无缺、无多）。
- 编译告警 13 条全部落在 `session.rs` / `server/mod.rs` / `sop/doc_from_zero.rs`，与本文档两文件无关，不影响结果。
- 种子数据/配置项/进程级启停：本单元确为库内模块，`N/A` 理由成立，不构成缺件。

## 7. 第三方可测标准

`run` 的十二行「保存与回推矩阵」+ 三出口错误语义 + 失败上下文三规则 + 23 例测试口径，足以让第三方在不看源码的前提下写出等价测试（阶段/计数/断点落盘/超限/续跑均有可断言面）。两篇文档的「不负责」边界也避免了把 session/gate 行为误写进本单元用例。

## 结论

覆盖 24/24、lint 0 违规采信；findings N/A；必填节与 N/A 规则齐备；plan tree 无硬冲突；Runbook 从零实跑 **23/23 通过、EXIT=0** 且预期输出逐字吻合。仅余一处 `Cargo.toml` 行号笔误与两处非阻塞的穷举度/跨文档问题，不阻断第三方测试与复跑。**PASS**。

**非阻塞建议（随汇编修）**

1. `orchestrator.rs.md:247`：`Cargo.toml:5` → `Cargo.toml:4`（`:5` 是空行）；`facts/unit-src-09.md` 与 `prepare.rs.md:110` 同错，建议一并改。
2. `mod.rs.md:70`-80「未转发…典型公开符号」建议补一行 `testing_gate`：`REGRESSION_LOG`、`parse_acceptance_table`（`testing_gate.rs.md:39` 明示走模块路径），或注明「非穷举」。
3. 跨文档：`state.rs.md:99`「`baseline_sha` 全程不动」与本篇「基线为空则回填」仍相反（review-13 FAIL-D 的另一侧未修，属 unit-src-21）。
4. `mod.rs.md` 无独立「测试运行与环境」节（其余 10 篇模块文档均有），可加一句 N/A 并指向 `orchestrator.rs.md` 的运行命令。
5. `boundary-protocol.md:45`「停下问人」与 Quality 停止的措辞，建议在汇编层统一。
