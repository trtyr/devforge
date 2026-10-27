# 文档审查报告 — unit-src-16（第 11 轮）

- 单元：`unit-src-16` → `src/change/report.rs` + `src/change/session.rs`
- 文档：`docs/contexts/src/change/report.rs.md`（132 行）、`docs/contexts/src/change/session.rs.md`（228 行）
- 引擎审计（确定性采信，未重算）：要求 24 条 / 已覆盖 24 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）
- 边界：按 DR-12 不读源码；交叉核对只用文档侧产物（`facts/unit-src-16.md`、`facts/unit-src-04.md`、`mod.rs.md`、`orchestrator.rs.md`、`coding_gate.rs.md`、`boundary.rs.md`、`gate0.rs.md`、`plan_audit.rs.md`、`state.rs.md`）、plan tree 与实跑输出。

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 doc-fix findings；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与本单元无关。**N/A，不构成遗漏**。

## 2. 覆盖对账（24/24 采信）

要求集合 = 模块 2（`report.rs` / `session.rs`）+ 8 个 report 公开符号 + 14 个 session 公开符号 = 24。两文档的「公开成员」表逐项列名并链到独立小节，方法/关联函数嵌在结构体小节内（`AcceptanceSummary.passed`、`ChangeReport.render`、`ChangeCtx.plan_path` / `acceptance_path`），均有行为、参数、返回与异常口径，不是「只在 Runbook 提一句」的伪覆盖。清单外 6 + 34 条 behavior/spec 另由「测试覆盖」表逐例承接，行号与 inventory/facts 逐条一致。与引擎 24/24 吻合。

## 3. plan tree 一致性

- report：P009 T012（引擎写 `changes/<id>/report.md`：墙钟、diff、审查轮次、往返、用量、验收对账；漏一条验收标准要能看出来）→ 文档 §职责/§ChangeReport/§write_report 与 `failed_reconciliation_is_visible_in_report` 逐条对应。
- session：P009 T016（六 profile 装配、plan 审查注入机械审计、coding 先验基线绿/步骤对账/取 diff、testing 先看 route 再验回归与对账表）及审计修正 `65db146` / `b79e785` / `d52331f`（基线轮先于改码轮、`change_dir` 在目标仓、plan 指令带全文）→ 文档 §核心口径「门序不可乱」「路由优先于标记」「审查三通道」与 `ChangeCtx.change_dir` 描述一致。
- T011d（三通道、空箱拒收、尾部协议位）与 D008（越界只发路由 + `<impact-gap>`）均能在文档中找到同口径落点。**无硬冲突**。

## 4. 完整性 / 必填节 / N-A

- 模块三件（职责含「不负责」/ 公开成员 / 依赖）齐备；`ChangeCtx` / `ProdChangeSession` 的职责、构造、公开成员、生命周期、典型用法、派生 trait 齐备；函数条目摘要 / 行为语义 / 参数 / 返回 / 异常 / 示例齐备；两个常量的名称类型、取值、影响、示例齐备。
- N/A 均带理由（纯计算 / 纯组装 / 只读判断 / 纯拼接）。无 TODO/TBD、无日志粘贴、无大段复述代码。
- Runbook 七件套：本单元为库内模块，无独立服务/端口/配置项/种子数据/HTTP 端点，相应件 N/A 成立；「测试运行与环境」承担依赖（`tempfile`、`git`）、运行命令与隔离（独立 `tempdir`、脚本化假会话）。项目级 Runbook 属汇编阶段。

## 5. 真实性 / 内部自洽

抽查全部成立：`mod.rs:19/20/38-41`、`change_engine.rs:11/54/55/56/246/252`、`orchestrator.rs:14/151/309/371-373`、`report.rs:8/13/21/35/95/125/139`、`session.rs:16/19/30/55/69/89/103/125/146/172/175/179/579/584`；6 + 34 个测试行号与 facts/inventory 逐条相同；跨文档（`gate0.rs.md:39`、`plan_audit.rs.md:44`、`boundary.rs.md:44`、`coding_gate.rs.md:75/86/177/186`）无相互矛盾的硬引用。`🟢` 标记全部同行带 `CONFIRMED path:line`，无裸绿。

发现的只是可澄清点（不影响裁决，见 notes）：`session.rs.md:47` 把 coding_gate 的整组 import 记为 `:12`，而 `facts/unit-src-04.md:57` 记符号在 `:13-14`（多行 `use` 的起始行 vs 符号行，口径不同）；同表未列 `SELF_CHECK_LOG`（其消费点在 `:120`/`:160`）。`report.rs.md:125` 的「二进制跳过」与 `:93`「二进制 `-` 记 0 且 files+1」措辞不统一。

## 6. 置信度诚实性

report 6 处、session 13 处 `🟢 CONFIRMED` 均给得出 `path:line`；两处 `🟡 INFERRED` 明确写了推断依据与「未实跑复现」的原因，属诚实披露。本轮已实跑通过，可升级为 🟢（非阻塞）。

## 7. 可跑（从零 boot）

- 环境：沙箱未配默认 toolchain → `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`；`/tmp` 仅余 131M，改用独立目标目录。
- 装依赖 + 从零编译：`CARGO_TARGET_DIR=/home/trtyr/code-work/dfz-r11-target CARGO_NET_OFFLINE=true cargo test --lib change::` 在全新 target 下编译成功；`cargo test --lib change::` → **171 passed / 0 failed / EXIT=0**。
- 健康检查（库内模块以单测代替）：`cargo test --lib change::report` → **6 passed / 0 failed / 594 filtered out / EXIT=0**，测试名与文档表逐名一致；`cargo test --lib change::session` → **34 passed / 0 failed / 566 filtered out / EXIT=0**，34 个用例逐名吻合。
- 依赖/种子：`tempfile`（dev-dep）+ 本机 `git` 即可；无外部服务、端口、网络、种子数据。文档命令从零可跑，**不构成 FAIL**。

## 8. 第三方可测标准

公开面（字段语义、参数、返回、异常触发条件、幂等/无副作用、提示词锚点）与 6 + 34 例测试口径写全，命令可直接执行；第三方可复写等价测试并覆盖「漏一条验收标准 → 报告可见」「空箱路由拒收」「违例/实例失败三通道」等不变量。达到合格线。

## 结论

引擎覆盖 24/24、lint 0 违规采信通过；findings N/A、plan tree 无冲突、必填节/N-A 合规、置信度诚实、双模块单测从零实跑 6/6 与 34/34 全绿。未发现阻断性矛盾或幽灵引用，**PASS**。

**非阻塞建议（修订时顺带）**：
1. `report.rs.md:131`、`session.rs.md:226` 的 🟡 升级为 🟢（本轮已实跑 6/6、34/34）。
2. 给 `passed`（`report.rs:28`）、`render`（`:49`）、`plan_path`（`session.rs:45`）、`acceptance_path`（`:49`）补各自的 `path:line` 回指，当前只在结构体小节内描述。
3. `session.rs.md:47` 明确 coding_gate import 的多行跨度（起始行 vs 符号行），并补 `SELF_CHECK_LOG`；与 `facts/unit-src-04.md:57` 统一。
4. 「测试运行与环境」补预期输出（`6 passed` / `34 passed`）与「各例独立 tempdir、可并行」。
5. `report.rs.md:125` 的「二进制跳过」改为「二进制计数记 0、不跳过文件」，与 `:93` 一致。
