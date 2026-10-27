# 文档审查报告 — unit-src-15（第 12 轮）

- 单元：`unit-src-15` → `src/change/prepare.rs`（文档 107 行）
- 文档：`docs/contexts/src/change/prepare.rs.md`
- 引擎审计（采信，未重算）：要求 3 条 / 已覆盖 3 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）
- 边界：按 DR-12 不读源码；交叉核对仅用文档侧产物（`facts/unit-src-15.md`、`gate0.rs.md`、`docs.rs.md`、`change_id.rs.md`、`mod.rs.md`、`session.rs.md`、`report.rs.md`、`coding_gate.rs.md`）、plan tree 与实跑输出

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 findings；`_meta/gaps.md` 只登记 unit-src-01 的 GAP。**N/A，不构成遗漏**。

## 2. 覆盖对账（3/3 采信）

要求集合 = 模块 1（`mod:src/change/prepare.rs`）+ 公开符号 2（`ChangePrep:15`、`prepare:30`）。「公开成员」表逐项列名并链到独立小节；清单外 7 条 behavior 由「测试覆盖」表逐例承接，行号与 inventory/facts 一致，无伪覆盖。与引擎 3/3 吻合。

## 3. plan tree 一致性（FAIL）

**FAIL-A（实施步骤落点与 plan tree 相反）**：`:41` 写计划目录（目标仓 `docs/plantree/changes/<id>`）「存 change-plan.md、**impl-steps.md**、baseline.txt 等可提交物」。但 P009 `README.md:90` 明确「计划 `change-plan.md` / 往返 `history.md` / 报告 `report.md` 在目标仓；**黑板上放 `impl-steps.md` 与 `evidence/`**」，`roadmap.md:58`（T010b）与 `decisions/README.md:105` 同口径；兄弟文档 `session.rs.md:66` 把 `impl-steps.md` 归 SOP 黑板，`coding_gate.rs.md:220` 的 `impl_steps_path` 也是 `blackboard.join(IMPL_STEPS)`。同文 `:54` 又说黑板「状态/证据/**实施步骤**/报告都在此」——`:41` 与 `:54` 直接自相矛盾。实盘 8 个 change 目录也只有 change-plan/baseline/report/history，无 impl-steps，印证 plan tree。第三方照 `:41` 写测试会把 impl-steps 定位到错误目录，直接测错不变量。

**FAIL-B（终报告落点与 report 模块冲突）**：`:54` 的黑板括注「…/报告都在此」，与 `report.rs.md:18/111`（`write_report` 写 `<repo>/docs/plantree/changes/<change-id>/report.md`）及 P009 `README.md:90`（报告 report.md 在目标仓）冲突。黑板上确有 `testing/reports`、`coding/reports` 骨架，但「报告」未加限定，读者会误判终报告位置。应改为「黑板放阶段报告（`testing/reports`、`coding/reports`）与证据；终报告 `report.md` 在目标仓计划目录」。

除此之外：T015（change-id/黑板 → doc_level 三档 → 基线 → baseline.txt 与骨架，验收「裸仓一条 baseline 提交、baseline.txt==sha；重复启动复用 id」）、D001/D002/D003 在文档 §职责/§行为语义/§示例 均有对应且无其它冲突（D001 的 `docs/plantree/<change-id>/` 属简写，与 README 的 `changes/<id>/` 不构成新矛盾）。

## 4. 完整性 / 必填节 / N-A

模块三件（职责含「不负责」/公开成员/依赖）齐备；`ChangePrep`（职责、构造、6 个公开字段、生命周期、典型用法、派生 trait）与 `prepare`（摘要、8 步行为语义、7 参数、返回、三类异常含不回滚边界、示例）齐备。无 TODO/TBD、无日志粘贴。Runbook 七件套对本单元为库内模块：无服务/端口/配置项/种子数据/HTTP 端点，相应件 N/A 成立；依赖获取（`tempfile`=`Cargo.toml:27` 已核对）、运行命令、隔离方式由「测试运行与环境」承担。缺一项非阻塞：未给预期输出（实跑为 `7 passed`）。

## 5. 置信度诚实性

4 处 🟢 均同行给出 `CONFIRMED <path>:<line>`（`:42`=`prepare.rs:53/58/65`、`:61`=`prepare.rs:15`、`:66`=`prepare.rs:30`、`:82`=`prepare.rs:96`），无裸绿。1 处 🟡（`:104` 运行命令）写明推断依据与「勘察环境未实跑」原因，诚实；本轮已实跑通过，可升级（非阻塞）。`doc_action` 字段（`:58`）「complete 在此层不会出现」仍混用 DocLevel/DocAction 命名，建议改为「`FullChainRequired` 不会出现在返回值中（complete 档直接报错）」。

## 6. 可跑（从零实跑）

- 环境：沙箱 `/tmp` tmpfs 已满（1.4M 可用）→ `TMPDIR=/home/trtyr/code-work/tmp-r12`；无 rustup 默认 toolchain → `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`；隔离 target `CARGO_TARGET_DIR=/home/trtyr/code-work/dfz-r12-target`；离线依赖可用。
- 装依赖 + 编译：`cargo test --lib change::prepare`（`CARGO_NET_OFFLINE=true`）→ 从零编译 34.0s 成功（13 warning，与本单元无关）。
- 健康检查（库内模块以单测代服务探活）：**7 passed; 0 failed; 0 ignored; 593 filtered out; EXIT=0**，7 个用例名与文档测试表逐名一致（bare/none/essential/complete/repeated/missing_repo/bad_doc_level）；文档「4 例起 `git` 子进程」「6 例固定日期 + repeated 跨日期」与 facts 相符。
- 种子数据/公开接口：N/A（库内模块）。命令按文档可直接执行，**可跑面不构成 FAIL**。

## 7. 第三方可测标准

`ChangePrep` 字段语义、`prepare` 七参数/返回/三类异常/不回滚/幂等口径与 7 例测试描述足以支撑等价测试编写；但 `:41` 的 impl-steps 落点与 `:54` 的「报告」归属会把第三方引向错误路径，修订后才能完全达到「仅凭文档写测试」。

## 结论

引擎覆盖 3/3、lint 0 违规采信通过；findings N/A、必填节/N-A 合规、置信度诚实、文档命令从零实跑 7/7 通过。但存在两处硬伤：`:41` 把 `impl-steps.md` 放进目标仓计划目录，与 plan tree（P009 README/ T010b / D00x）及 `session.rs.md:66`、`coding_gate.rs.md:220` 相反，且与同文 `:54` 自相矛盾；`:54` 的「报告都在此」与 `report.rs.md:18` 的终报告落点冲突。按 plan 一致性与内部自洽口径 **FAIL**。

**非阻塞建议（随修）**：
1. `:104` 的 🟡 升级 🟢，并在「测试运行与环境」补预期输出 `7 passed / 0 failed`。
2. `:58` 的 `doc_action` 说明改为按 `DocAction` 变体表述（`FullChainRequired` 不出现）。
3. `:105` 可补「无网络/无端口」与「每例独立 tempdir 可并行」已具备，建议同时提示 tmpfs/磁盘余量（本轮实证 `/tmp` 满会导致 target/临时目录写失败）。
