# 文档审查报告 — unit-src-09（第 8 轮）

- 单元：`unit-src-09` → `src/change/mod.rs` + `src/change/orchestrator.rs`
- 文档：`docs/contexts/src/change/mod.rs.md`、`docs/contexts/src/change/orchestrator.rs.md`
- 轮次：round 8（该单元首轮人工审；此前 review-1..7 未覆盖本单元）
- 引擎审计（确定性采信，未重算）：要求 24 条 / 已覆盖 24 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）

## 0. 输入与边界

两篇单元文档 + `_meta/facts/unit-src-09.md` + plan tree 锚点（P009 T009 / T011b / T011c / T012 / T013，D025 / D026）+ 引擎审计。按 DR-12 不读源码；实体存在性以引擎 lint 与 facts 为准。跨文档一致性核对使用了同为文档侧产物的 `facts/unit-src-01/04/07/12.md`、`glossary.md` 与已审文档。

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 doc-fix findings；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与本单元无关。**无待处置清单 → N/A，不构成遗漏**。

## 2. plan tree 一致性

- T013（`roadmap.md:52-54`）：往返上限 3、第 4 次停下并报错、`history.md` 逐次落盘 → 文档 `MAX_ROUNDTRIPS`=3 / 第 4 次 Quality / `append_roundtrip` 一致。
- T011b / T011c + D025：断点落黑板 `change/state.json`；失败上下文 `round + issues`；覆盖 / 消费即清 / 终态清 / 超限不写不清；`run_coding(last_failure)` 送达 → 文档核心口径与 run 分支逐条吻合。
- T012：报告含 diff / 审查轮次 / 往返 / 用量 / 验收对账 → 一致。
- **无硬冲突**。附注：`topics/boundary-protocol.md:45` 写「停下问人」，本文档写 Quality 停止；T013 验收只要求「停下并报错」，不判冲突，建议汇编时统一措辞。

## 3. 内部自洽 / 真实性（FAIL 项）

- **FAIL-1（转出口径与事实矛盾）**：`mod.rs.md:13` 称「把各子模块的公开符号统一 `pub use` 转发导出」，`:18` 复述「转发导出每个子模块的公开符号」。事实并非「每个」：
  - `facts/unit-src-12.md:8`：`plan_audit` 只转发 `PlanAudit` / `audit as audit_plan` / `review_instruction` 三项，`plan_facts` / `audit_verdicts` / `audit_plan_file` / `path_refs` 走模块路径；
  - `facts/unit-src-04.md:8`：`coding_gate` 只转发 5 个公开符号（`parse_steps` / `changed_files` / `commits_since` / `step_numbers_in` / `impl_steps_path` 等未转发）；
  - `facts/unit-src-07.md:8`：`docs` 只转发 3 项，`inventory_path` 未转发（与 `docs.rs.md:31` 对得上）。
  影响：第三方会误以为 `crate::change::plan_facts` / `inventory_path` / `commits_since` 等在模块根可用，直接破坏「按文档写测试」。文档「转发导出别名」小节也只列了 6 条转发行，未给全量清单。建议改为「转发导出各子模块选定的公开符号」，并补全转发行或明确「其余经子模块路径访问」。
- **FAIL-2（同一行号被指为两个不同 import）**：`orchestrator.rs.md:43-45` 把依赖 import 标为 `boundary`(`:11`) / `report`(`:12`) / `state`(`:13`)；但 `boundary.rs.md:43` 与 `facts/unit-src-01.md:50` 明确 `src/change/orchestrator.rs:13` 引入的是 `boundary::{self, BoundaryVerdict, ImpactGap}`。`:13` 不可能同时是 `boundary` 与 `state` 的 import，三行中至少一处错误（`:11`/`:12` 也随之可疑）。需按源码核对后统一。
- 其余内部自洽通过：mod 的 13 个声明 ↔ 公开成员表 ↔ 子模块文档单元号逐项对应；orchestrator 9 个公开符号 ↔ 逐节详述一一对应；`ChangeSession` 默认方法、`run` 的三段分支与错误面、测试表 23 例与 facts 锚点均吻合。
- 轻度概括问题（非阻塞）：`orchestrator.rs.md:176`「任何阶段失败都……把阶段回推并保存」比同一文档的分支描述更宽——`OutOfBoundary`、plan 超限、验收缺项分支并未写「回推/保存」。建议限定为「涉及阶段迁移与失败上下文的分支才保存」。

## 4. 覆盖率与条目对账

- 24 条要求 = mod 模块 1 + 13 个子模块符号 + orchestrator 模块 1 + 9 个公开符号；两篇文档均有落点（子模块表 / 逐节小节），与引擎 24/24 一致。
- 模块级「公开成员」三件套（职责 / 公开成员 / 依赖）齐备；`orchestrator` 的 6 个类/接口条目均含职责 / 构造 / 公开成员 / 生命周期 / 典型用法；`run` 六节齐备；常量与内部符号的 N/A 均带理由（「常量」「无 derive」等）。
- 测试覆盖表 23 例与 facts 行号逐一吻合；`#[cfg(test)]`、`Scripted`、`NoUser`、`tempfile` 与隔离描述一致。

## 5. 置信度诚实性

全部 🟢 均带 `CONFIRMED path:line`（orchestrator 13 处、mod 3 处），无裸绿。唯一 🟡（`:223`「未实跑复现」）本轮已实跑通过，可升级为 🟢（建议，非阻塞）。FAIL-2 的三行 import 未标 🟢，但仍是事实性引用，需修正。

## 6. 完整性（必填节 / N-A）

- 模块：职责（含不负责）/ 公开成员 / 依赖齐备。
- 公开函数 `run`：摘要 / 行为语义 / 参数 / 返回 / 异常 / 示例齐备；6 个公开接口条目按 §2.4 齐备；常量按 §2.3 变体齐备；内部 `phase_change` / `fix_step` 摘要 + 行为语义齐备（超出内部要求）。
- N/A：常量参数/返回/异常、`ChangeRun` 无 derive、`ChangeSession` 不可实例化均附理由；未发现 TODO/TBD/日志粘贴/大段代码复述。
- Runbook 七件套：本单元为库内模块（非独立服务），启停/配置/种子数据/公开端点 N/A 成立；「测试运行与环境」承担依赖（`tempfile`）、命令与隔离。项目级 `10-runbook.md` 属汇编阶段。

## 7. 可跑（从零 boot 实跑）

- 环境：审计沙箱 `HOME=/var/tmp/devforge-accept` 使 rustup 默认 home 为空；改用已装 toolchain：`RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`。
- 装依赖：`cargo fetch` → **EXIT=0**（下载完成）。
- 隔离：`CARGO_TARGET_DIR=/tmp/dfz-r8-target`（空目录，未复用仓库 `target/`）。
- 启动/健康检查（库内模块以单测代替）：`cargo test --lib change::orchestrator` → **23 passed; 0 failed; 577 filtered out; EXIT=0**；23 个测试名与文档测试表逐条一致。
- 结论：文档命令从零可跑；建议补「edition 2024 ⇒ Rust 1.85+，需已装 toolchain」环境前提，并把 `:223` 的 🟡 升级为 🟢。

## 结论

覆盖 24/24、lint 0 违规（确定性采信），findings N/A、plan 无硬冲突、必填节/N-A 完整、测试命令实跑 23/23 通过。**FAIL** 源于两处事实性/一致性缺陷：mod 文档把「选定的转发子集」写成「每个公开符号」，以及 orchestrator 依赖 import 的 `:13` 与 boundary 文档/facts 冲突。修正后可复审。

<notes>
- `facts/unit-src-09.md:55` 把 `orchestrator` 的 `pub use` 记作 `mod.rs:37`，与 `mod.rs.md:49` 的 `:34`、`facts/unit-src-12.md` 的 `plan_audit:36` 及声明顺序不一致；疑为 facts 笔误，请以源码核对后同步修正（不影响本单元文档裁决）。
- `orchestrator.rs.md:176` 的「任何阶段失败都保存」建议按实际分支收窄。
- `boundary-protocol.md:45`「停下问人」与 Quality 停止的措辞建议在汇编层统一。
- 测试运行段可补工具链版本与安装前提；`:223` 的 🟡 已由本轮实跑证实，可升级 🟢。
</notes>
