# 文档审查报告 — unit-src-15（第 15 轮）

- 单元：`unit-src-15` → `src/change/prepare.rs`（251 行）
- 文档：`docs/contexts/src/change/prepare.rs.md`（122 行）
- 引擎审计（采信，未重算）：要求 3 条 / 已覆盖 3 条 / 缺失 0；doc-lint 通过（0 违规）
- 边界：按 DR-12 不读源码；交叉核对只用文档侧产物（`_meta/facts/unit-src-15.md`、`change_id.rs.md`、`gate0.rs.md`、`docs.rs.md`、`session.rs.md`、`report.rs.md`、`coding_gate.rs.md`、`glossary.md`）、plan tree（P009 T015、D001-D003、README 落盘/运行契约）与实跑输出。

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 findings；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP。**N/A，不构成遗漏**。

## 2. 覆盖对账（3/3 采信）

要求集合 = 模块 1（`mod:src/change/prepare.rs`）+ 公开符号 2（`ChangePrep:15`、`prepare:30`）。文档「公开成员」表逐项列名并链到独立小节；清单外 7 条 behavior（测试 `:96/:152/:169/:187/:204/:220/:236`）由「测试覆盖」表逐例承接，无伪覆盖。与引擎 3/3 一致。

## 3. plan tree 一致性（无硬冲突）

- P009 T015（`roadmap.md:94`）：change-id/黑板 → doc_level（none/essential 落地、complete 拒跑指向 doc-from-zero）→ 基线（无 `.git` 则 init+全量提交）→ baseline.txt 与黑板骨架。文档 §职责/§行为语义/§示例逐条对应。
- 验收「裸仓有且仅一条 baseline 提交、baseline.txt==sha；重复启动复用 id 且不重建仓库」↔ 文档 `created_repo=true` → 第二次 `false`、sha 不变（`:204`）。
- D001/D002/D003：基线、档位、计划落目标仓（`decisions/README.md:6/13/20`）与文档一致；P009 README 落盘表「计划在目标仓、黑板放 impl-steps/evidence」与文档 §核心口径第 2 条一致（review-12 的 FAIL-A/B 已修）。

## 4. 完整性 / 必填节 / N-A（通过）

- 模块三件：职责（含不负责）/ 公开成员 / 依赖 齐备。
- `ChangePrep`（公开类）：职责、构造/初始化、6 个公开成员、生命周期、典型用法、派生 trait 齐备。
- `prepare`（公开函数）：摘要、行为语义（10 步）、7 参数、返回、三类异常含不回滚边界、示例齐备。
- Runbook 七件套：环境（edition 2024、Rust 1.85+、`RUSTUP_TOOLCHAIN`、`git`）、依赖获取（`cargo fetch`、`tempfile`=`Cargo.toml:27`）、测试运行（命令+预期输出+7 例）、隔离（每例独立 tempdir、无网络/端口/服务）齐备；配置项/启停与健康检查/种子数据/公开接口清单均写 **N/A 并给理由**，无「省略当不适用」。无 TODO/TBD、无日志粘贴。

## 5. 真实性 / 置信度诚实性（基本通过）

- 6 处 🟢（含 `:112` 运行命令）均同行给出 `CONFIRMED`：
  - `impl_steps_path` 无调用点（重放 `grep -rn impl_steps_path src/` → 仅定义 `coding_gate.rs:323`，与正文一致）；
  - 不回滚顺序 `prepare.rs:53/58/65`；
  - `ChangePrep:15`、`prepare:30`、示例 `:96`；
  - 测试命令与结果 `:112`。
- 无裸绿；`🟡` 已按 review-12 建议升级（本轮实跑）。
- 行号抽查：`mod.rs:18/37`、`change_engine.rs:10/101/102/103/246/279/317`、`change_id.rs:15/20/62-73/76-78`、`gate0.rs:34-43/66-111/171`、`report.rs:139`、`coding_gate.rs:20/322-325` 与 facts/兄弟文档一致。
- 一处非阻塞笔误：`:110` 写 `Cargo.toml:5`，实际 edition 在 `Cargo.toml:4`（`:5` 为空行）；facts 同错，建议一并改。

## 6. 可跑（从零 boot，通过）

按文档「测试运行与环境」原文执行，未做替代：

- 环境：`TMPDIR=/home/trtyr/code-work/tmp-r15`（`/tmp` 已 99% 满）；`RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`；`rustc 1.99.0` / `cargo 1.99.0`；`git 2.53.0`。
- 依赖：`CARGO_NET_OFFLINE=true cargo fetch` → EXIT=0（无输出）。
- 启动/健康检查（库内模块以单测代探活）：`CARGO_TARGET_DIR=/home/trtyr/code-work/dfz-r15-target`（全新空目录，从零编译 34.92s）`cargo test --lib change::prepare` → **7 passed; 0 failed; 0 ignored; 593 filtered out; EXIT=0**；7 个用例名与文档测试表逐名一致。
- 预期输出与文档 `7 passed; 0 failed; 0 ignored; 593 filtered out` 吻合（引擎输出多 `0 measured` 与耗时，不影响）。
- 配置/种子/进程级启停：本单元确为库内模块，N/A 理由成立。

## 7. 第三方可测标准（通过）

`ChangePrep` 字段语义、`prepare` 七参数/返回/三类异常/不回滚/幂等口径、7 例测试描述与示例，足以让第三方在不看源码的前提下写等价测试；「不负责」边界也避免了把 session/编排行为误写进本单元用例。

## 结论

引擎覆盖 3/3、lint 0 违规采信；findings N/A、plan tree 无硬冲突、必填节/N-A 合规、🟢 全带证据；Runbook 从零实跑 **7/7 通过、EXIT=0** 且预期输出吻合。**PASS**。下列为非阻塞建议，不阻断第三方测试与复跑。

## 非阻塞建议

1. `:42`/`:74` 的 `change/<change-id>` 若与同文「黑板 = `plans/<project>/<task>/change`」字面拼接，会读成 `.../change/change/<id>`；facts 是 `change_root/<id>`。建议改为 `change_root/<change-id>`（或 `<change-id>/` 子目录），消除歧义。
2. `:110` `Cargo.toml:5` → `Cargo.toml:4`；`facts/unit-src-15.md:54` 同错，建议一并改。
3. 跨文档：`session.rs.md:66` 把 `ChangeCtx.blackboard` 写成 `plans/<p>/<t>`，本篇与 P009 README:78 均为 `.../change`；建议在汇编/该单元轮统一。
4. 跨文档：`docs.rs.md:58` 称 `prepare` 按 `doc_action` 触发 `FullChainRequired` 完整链分派，与 `prepare.rs.md:62`（complete 在 `:58` 直接报错、`FullChainRequired` 不出现在返回值）相反；建议改 docs.rs 侧措辞。
5. 配置 N/A 的理由只举了 `DEVFORGE_DOC_LEVEL`；P009 README:88 的运行契约还有 `DEVFORGE_TODAY`，建议在 N/A 理由或依赖行补一句交叉引用。
