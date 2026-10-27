# 文档审查报告 — unit-src-22（第 18 轮）

- 单元：`unit-src-22` → `src/change/testing_gate.rs`（文档 `docs/contexts/src/change/testing_gate.rs.md`，178 行）
- 引擎审计（采信，不重算）：要求 11 条 / 已覆盖 11 条 / 缺失 0；doc-lint 通过（0 违规）
- 输入边界（DR-12）：只读单元文档、`_meta/facts/unit-src-22.md`、`_meta/glossary.md`、兄弟单元文档、plan tree、审查历史；不打开源码，实体存在性以 doc-lint 为准

## 1. findings 处置完整性

工作区无 `testing/findings/` 目录；`_meta/gaps.md` 仅登记 unit-src-01 与 unit-src-24，无本单元 doc-side finding。**N/A，不构成遗漏**。

## 2. 覆盖验收（11/11 采信）

inventory 本单元 23 条 = 1 module + 10 symbol + 12 behavior；档位要求集合（module + 10 符号 = 11）与引擎审计 11/11 吻合。「公开成员」表 10 行，每行指向独立小节，`#regression_log` / `#failure_block_title` 等锚点可解析；12 条 behavior 由「测试覆盖」表逐例承接，行号 155/164/171/177/184/194/204/211/217/240/250/260 与 facts 逐条相同，无悬空条目。

## 3. 完整性 / 必填节 / N-A

- 模块三件齐备：职责（含「不负责」三条边界）/ 公开成员 / 依赖（上游消费方 + 向下依赖 + 外部 crate）。
- 7 个函数六件齐备：摘要、行为语义、参数、返回、异常、示例；`parse_acceptance_table` 等纯函数的「异常：N/A」均给了理由。
- `AcceptanceMatch` 五件（职责/构造/公开成员/生命周期/典型用法）+ 派生 trait。
- 两个常量给出名称/类型/取值/影响/示例；运行时可配项已在 Runbook 段声明 N/A（编译期字面量），N-A 规则成立。
- Runbook 七件套：环境（edition 2024 `Cargo.toml:4`、Rust 1.85+、无默认 toolchain 须显式指定）、依赖获取（`cargo fetch`，`tempfile`=`Cargo.toml:27`）、测试运行（命令 + 预期输出 + 12 例）、隔离方式（临时目录、无 git/端口/服务/网络）在册；配置项 / 启停与健康检查 / 种子数据 / 公开接口清单四项 N/A 且给理由。
- 无 TODO/TBD、无整段日志粘贴、无死链。

## 4. 真实性 / 置信度诚实性

- 10 处 🟢 全部带 `src/change/testing_gate.rs:<line>`，行号与 glossary:160-169 逐条相同（18/22/28/45/57/77/118/128/137/142）；运行命令一条 🟢 本轮实跑复核。
- 非 🟢 交叉引用抽查均对得上：`mod.rs:22`/`:43`-46 ↔ `mod.rs.md:66`；`session.rs:20` ↔ `session.rs.md:47`；`orchestrator.rs:333`/`:360` ↔ `orchestrator.rs.md:54`；`:334`/`:345`/`:355`-357 ↔ `orchestrator.rs.md:175`/`:191`；`session.rs:1047` ↔ `session.rs.md:147`/`:200`；`report.rs:125` + `orchestrator:309`/`:372` ↔ `report.rs.md:37`；`Cargo.toml:4`/`:27` 本轮实测确认。
- doc-lint 0 违规 ⇒ 无幽灵实体（含 `tests/change_failure_context.rs` 引用）。

## 5. plan tree 一致性

P009 T011（roadmap:67，回归优先 + 验收对账）、T011c（roadmap:73，标题行 + 明细逐条原文）、D017（引擎不跑回归、只核证据）、D018 旁注 + D025（「不做定位」口径被 D025 改写、三出口同源、不剥 `path:line`、零信息话术）、D019（testing 不给 codegraph）——文档首页与「核心口径」逐条对应，无硬冲突。唯一精度小瑕：首页只写 P009 T011，未点名承载回注块的 T011c（非冲突）。

## 6. 内部自洽

- 「不负责」边界（不决定何时回注 / 不跑测试 / 不解析通用证据格式）与各函数语义一致，未越界。
- `parse_acceptance_table`「按最后一个 → 切、空引用丢弃」与 `reconcile_acceptance`「空 `test_ref` 仅手构造表触发」两条口径互补、无矛盾。
- `reconcile_acceptance` 无生产调用点的事实，与运行时两步对账（`session` parse + `path_refs`、`report::summarize_acceptance`）并存且已显式披露，不算冲突。
- 返回 `Ok(criteria.len())` 与「全对 → `Ok(1)`」示例一致；三出口同源（WARN/Quality/送达块）落点前后一致。

## 7. 可跑（照 Runbook 从零 boot）

- 环境：`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`（本机无默认 toolchain，文档已要求显式指定）、`TMPDIR=/var/tmp/dfz-r18/tmp`（`/tmp` 99% 满）、`CARGO_TARGET_DIR=/var/tmp/dfz-r18/target`（全新空目录）。
- 依赖安装（文档原文 `cargo fetch`）：0.37s、**EXIT=0**。
- 首次从零全量编译 37.09s；`cargo test --lib change::testing_gate` → `running 12 tests`、`test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 586 filtered out`、**EXIT=0**；12 个用例名与测试表逐名一致。
- 与文档预期输出逐字吻合（`586 filtered out` 一致；耗时 43.71s→37.09s 属机器抖动，文档已注明可变量）。
- 配置 / 启停与健康检查 / 种子数据：库内模块，N/A 理由成立。

## 8. 第三方可测标准

字段语义、三态判定（绿 / 红如实 / 红称绿）、宽松包含匹配、空引用、示例断言与 12 例测试表，足以在不看源码的前提下写出等价测试；「不负责」边界避免把编排行为误写进本单元用例。

## 结论

覆盖 11/11、lint 0 违规采信；findings N/A；plan tree 无硬冲突；必填节与 N-A 齐备；10 处 🟢 全部可回指且行号与 glossary/facts 一致；Runbook 从零实跑（fetch EXIT=0、12/12 EXIT=0）且预期输出逐字吻合。**PASS**。

## 非阻塞建议（随修）

1. `:171` 代码块仍是占位符（`<仓外独立空目录>`）且未含 `RUSTUP_TOOLCHAIN`/`TMPDIR`，建议给一条可直接复制执行的完整命令（环境前提在 `:167`，不阻断）。
2. 首页 `:10` 建议补 `T011c`，与回注送达块的承载任务精确对齐。
3. `:104` 的 contains 匹配是 `reconcile_acceptance` 自用语义，而 `:111` 说明生产对账走 `summarize_acceptance` 的精确相等；建议加一句「生产判定以精确相等为准，contains 仅本函数自用」，避免第三方误判。
4. `tests/change_failure_context.rs:278`（`:135` 引用）未见于 facts（facts 仅记 24/298），建议 facts 留痕，便于后续轮次核对行号。
5. `:97` 的 `tests/test_b.py:7` 与 `:120` 的「接口返回 500」属示例字面值，建议在 facts 补记一行，减少下轮信息缺口。
6. `:135`「`无上下文时不得出现`的反向断言」措辞拗口，建议改为「无上下文时开工语不得出现该标题（反向断言）」。
