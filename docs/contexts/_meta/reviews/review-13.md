# 文档审查报告 — unit-src-21（第 13 轮）

- 单元：`unit-src-21` → `src/change/state.rs`（文档 `docs/contexts/src/change/state.rs.md`，239 行）
- 引擎审计（采信，未重算）：要求 18 条（模块 1 + 公开符号 17）/ 已覆盖 18 条 / 缺失 0；doc-lint 通过（0 违规）
- 边界：按 DR-12 不读源码；交叉核对只用文档侧产物（`facts/unit-src-21.md`、`mod.rs.md`、`orchestrator.rs.md`、`session.rs.md`、`testing_gate.rs.md`、`glossary.md`）、plan tree（P009 T011b/D022/D022a/D025、010 D001-D005）与实跑输出

## 1. findings 处置完整性

工作区（`/home/trtyr/code-work/devforge`、`/home/trtyr/code-work`）无 `testing/findings/`，kickoff 未附本单元 findings；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与本单元无关。**N/A，不构成遗漏**。

## 2. 覆盖对账（18/18 采信）

要求集合 = `mod:src/change/state.rs` + 17 个符号（`CHANGE_SOP:15` … `save:168`，含 `as_str:28`）。文档「公开成员」表 16 行、`as_str` 折在 `ChangePhase` 行内，与 inventory 行号逐条可对上；清单外 10 条 behavior 由「测试覆盖」表逐例承接（`:191`–`:347`，与 facts 一致），无伪覆盖。引擎 18/18 与文档面吻合。

## 3. 内部自洽（FAIL）

**FAIL-A（load 频次前后矛盾）**：`:48` 写 `run` 「每次进阶段前 `load`、变更后 `save`」；`:107` 写「每次 `run` 开始时 `load`、阶段/计数变化后 `save`」。同一文档对同一行为的调用频次给了两种口径。`orchestrator.rs.md:165/167` 支持「run 开始载入一次、阶段变迁时保存」，故 `:48` 的「每次进阶段前」是错的那一侧。第三方若按 `:48` 设计断言（每阶段重读断点）会写错测试。

**FAIL-B（写者归属自相矛盾）**：`:10` 同句既说「**引擎写**、住黑板」，又说扩展段「**SOP 自己读写**」。plan tree 的口径是分工而非并用：P009 D022「引擎写、agent 不碰」、010 D005「引擎管文件位置与原子写，SOP 管扩展段结构」。读者无法判定扩展段的写入者是引擎还是 SOP。

**FAIL-C（「不动其它 SOP 的段」不成立）**：`:213` 称 `save`「只覆盖 `"change"` 扩展段，不动其它 SOP 的段」。但按 010 D001 与同文 `:59`（跨 SOP 隔离、每 SOP 一个 `StateKey` 目录），change 的 `state.json` 里根本不存在其它 SOP 的段；该句会让人误以为断点是「一份文件装多 SOP」，与 `:59` 及 `:15` 的断点路径冲突。应改为「不动 `data` 里 `"change"` 以外的键」。

**FAIL-D（基线可变性跨文档冲突）**：`:99` 称 `baseline_sha`「全程不动」，`orchestrator.rs.md:165` 则写「基线为空则回填」。两处对同一字段是否可被改写给出相反语义，需补「为空时由编排层回填，此后不再变」。

## 4. 必填节 / N-A 合规（FAIL）

`as_str` 是独立要求条目（`sym:28`），但文档只在 `:77` 用一句「变体名对应的字符串；无参数、无失败面」带过：无独立小节、无 `path:line` 证据、无示例（`:79` 的「观测层用 `as_str()`」是用法不是示例），也未写 `N/A` 说明。按契约 §2.3/§三（公开函数缺必填节 = FAIL）计缺节。其余：模块三件（职责含不负责 / 公开成员 / 依赖）、`ChangeState`/`TestFailureContext`（职责/构造/公开成员/生命周期/典型用法）、13 个函数（摘要/行为/参数/返回/异常/示例）齐备；`CHANGE_SOP` 缺「是否必填/默认值」但属常量、影响可忽略（非阻塞）。无 TODO/TBD、无日志粘贴、无死链（页内锚点全部可解析）。

## 5. 置信度诚实性（基本合规，1 处待升级）

16 处 🟢 全部同行给出 `CONFIRMED <path>:<line>`，无裸绿。`:237` 的运行命令标 🟡 INFERRED 并写明「rustup 无默认 toolchain、未实跑」，属诚实披露；本轮已实跑通过（见 §6），可升级。

## 6. 可跑（从零实跑，通过）

- 环境：`/tmp` tmpfs 98% 满 → `TMPDIR=/home/trtyr/code-work/tmp-r13`；`RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`；全新 `CARGO_TARGET_DIR=/home/trtyr/code-work/dfz-r13-target`（从零编译）；`CARGO_NET_OFFLINE=true`（离线依赖齐备）。
- 命令即文档 `:237` 原文：`cargo test --lib change::state` → 编译 32.17s 成功（13 warning，全在其它模块）。
- 健康检查（库内模块以单测代探活）：**10 passed; 0 failed; 0 ignored; 590 filtered out; EXIT=0**，10 个用例名与文档测试表逐名一致（roundtrip/checkpoint_path/cold_start/resume_roundtrips/idempotent/failure_persist/old_checkpoint/overwrite/clear/foreign_state）。
- 依赖获取（`tempfile` dev-dependency）与隔离（每例独立 tempdir、无 git/端口/网络）在文档中已交代；种子数据/公开接口对本单元 N/A。运行命令可用，**可跑面不构成 FAIL**；缺「预期输出」一行（本轮实测 `10 passed / 0 failed`）建议补上。

## 7. 第三方可测标准

`ChangeState` 字段语义、幂等/覆盖/清除/冷启动口径、10 例测试与行号足以支撑等价测试编写；但 §3 的四处矛盾（load 频次、写者归属、其它 SOP 段、基线回填）与 `as_str` 无证据会误导第三方，修订后方达「仅凭文档写测试」。

## 结论

覆盖 18/18、lint 0 违规采信通过；findings N/A、🟢 均有 path:line、文档命令从零实跑 10/10 EXIT=0。但内部自洽与必填节各命中硬伤：`:48` 与 `:107` 对 load 频次相反；`:10` 同时把扩展段写入者判给引擎与 SOP；`:213`「不动其它 SOP 的段」与每 SOP 独立文件（010 D001、`:59`）冲突；`:99`「全程不动」与 `orchestrator.rs.md:165`「基线为空则回填」冲突；`as_str` 缺独立小节与 `path:line`/示例。故本轮 **FAIL**。

**非阻塞建议（随修）**：
1. `:237` 🟡 升级 🟢，并在「测试运行与环境」补预期输出 `10 passed / 0 failed`（EXIT=0）。
2. `:12` 把 `roundtrips` 跨会话保留的出处由 `010 D003` 改为 `P009 T011b / 010 D001`（D003 讲 done 证据复核）。
3. `:213` 的参数名 `state: &mut RunState` 易与 `ChangeState` 混淆，建议按实际形参（`run`）书写。
4. `:58` 的「过期（`Unusable`）」缺依据，建议改为「版本/身份不符」或给出 `runstate` 侧证据。
