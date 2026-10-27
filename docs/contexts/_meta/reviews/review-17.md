# 文档审查报告 — unit-src-21（第 17 轮）

- 单元：`unit-src-21` → `src/change/state.rs`（文档 `docs/contexts/src/change/state.rs.md`，273 行）
- 引擎审计（采信，未重算）：要求 18 条 / 已覆盖 18 条 / 缺失 0；doc-lint 通过（0 违规）
- 边界（DR-12）：不读源码；交叉核对只用文档侧产物（`_meta/facts/unit-src-21.md`、`_meta/glossary.md`、`mod.rs.md`、`orchestrator.rs.md`、`session.rs.md`、`testing_gate.rs.md`、`boundary.rs.md`）、plan tree 与实跑/编译探针输出

## 1. findings 处置完整性

工作区（`/home/trtyr/code-work/devforge`、`/home/trtyr/code-work`）无 `testing/findings/`，本轮未附本单元 doc-side finding；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP。**N/A，不构成遗漏**。

## 2. 覆盖对账（18/18 采信）

要求集合 = `mod:src/change/state.rs` + 17 个符号（`CHANGE_SOP:15` … `save:168`）。文档「公开成员」表 17 行，每行链到独立小节（17 个 `###` 标题），锚点全部可解析；清单外 10 条 behavior 由「测试覆盖」表逐例承接，行号与 inventory（191/209/224/234/255/266/296/328/339/347）逐条相同。与引擎 18/18 吻合。

## 3. plan tree 一致性（无硬冲突）

- P009 T011b（`roadmap.md:70`）：checkpoint 住 `plans/<p>/<t>/change/state.json`、存阶段/基线/往返/已完成步骤与验收/审查轮次 ↔ 文档 §核心口径断点位置与字段表。
- 010 D001（按 SOP 分文件）/D002（start 默认续跑）/D005（引擎管位置与原子写，SOP 管扩展段）↔ 文档首页分工表与 §续跑语义，口径一致；009 D022（`:148`）「引擎写、agent 不碰」与 D022a（`:154`）落点改住黑板均在文档有对应表述。
- 唯一小瑕（非阻塞）：首页「plan 010 D001/D022a」把 D022a 挂在 010 名下，实际在 009（plan tree `roadmap.md:70` 也用同一简写），易读但不构成冲突。

## 4. 内部自洽（通过，上轮 FAIL 全部消除）

第 13 轮四处硬伤逐条复核：

- FAIL-A（load 频次）：现文「入口 load 一次、之后仅在迁移/计数分支 save、没有每次进阶段前重新 load」前后一致，且与 `orchestrator.rs.md` 保存矩阵吻合。
- FAIL-B（写者归属）：改为「引擎管落盘（路径/身份/schema/原子写）、本模块管 `data["change"]` 结构」的分层表，并明确「是分层，不是同一件事的两个写者」。
- FAIL-C：`save` 节改为「只覆盖 `data` 的 change 键、其它键保留；其它 SOP 的段不出现是同目录隔离的目录级事实」，不再与每 SOP 独立文件冲突。
- FAIL-D：`baseline_sha` 改为「基本不变（源码注释口径）+ 载入为空时编排层回填一次」，与 `orchestrator.rs.md:172` 对齐。
- `as_str` 已成独立小节，六件（摘要/行为/参数/返回/异常/示例）+ 落盘一致性证据（`:304`↔`:320`）齐备。

其余一致性抽查：save 七个调用点 `:205/:232/:279/:320/:325/:347/:362` 与 orchestrator 保存矩阵逐行对得上（含「超限分支先存后返错」）；区间上限归 boundary、回注上限归 orchestrator 的归属划分无重叠；`roundtrips` 跨会话保留口径全文统一。

## 5. 完整性 / 必填节 / N-A（通过）

- 模块三件（职责含不负责 / 公开成员 / 依赖）齐备；`ChangePhase`、`TestFailureContext`、`ChangeState` 三类条目均有职责、构造、公开成员、生命周期、典型用法、派生 trait；13 个函数均有摘要/行为/参数/返回/异常/示例。
- `CHANGE_SOP` 明确写「常量，无必填/默认值语义」，属显式 N-A 而非省略。
- Runbook 七件套：环境（edition 2024、Rust 1.85+、`RUSTUP_TOOLCHAIN`）、依赖获取（`cargo fetch`、`tempfile`=`Cargo.toml:27`）、测试运行（命令+预期输出+10 例）、测试依赖与隔离（独立 tempdir、无 git/端口/服务）齐备；配置项/启停与健康检查/种子数据/公开接口清单均写 N/A 并给理由。
- 无 TODO/TBD、无日志粘贴、无死链。

## 6. 真实性 / 置信度诚实性（通过）

- 20 处 🟢 全部带 `CONFIRMED <path>:<line>` 或可重放命令（运行命令一条）。
- 行号交叉核对：state 侧 17 个符号行号与 `_meta/glossary.md:140`-156 逐条相同；orchestrator `:15/:160/:161-163/:242-244/:21/:324-328` 与 `orchestrator.rs.md`、facts 一致；`Cargo.toml:4`（edition）/`:27`（tempfile）本轮实测确认；`SCHEMA_VERSION`=`runstate/mod.rs:26` 由 glossary:543 佐证。
- 编译探针（不读源码）：仓外临时 crate 引用 `devforge::change::state`，`TestFailureContext` 的 Clone/Debug/PartialEq/Eq 与 `ChangePhase` 的 Copy 全部编译通过并运行 → `ok`，EXIT=0，文档派生 trait 清单属实。

## 7. 可跑（照 Runbook 从零，通过）

- 环境：`TMPDIR=/var/tmp/dfz-r17/tmp`（`/tmp` 99% 满，按文档告警改道）；`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`（本机默认 rustup home 的 stable 缺 manifest，属环境特例；文档已要求显式指定 toolchain）；`rustc/cargo 1.99.0`。
- 装依赖（文档原文 `cargo fetch`，首次联网）：EXIT=0（rsproxy 镜像，中途一次 spurious network retry）。离线态因 `wasm-bindgen` 未缓存而失败，与本单元文档的「首次需网络」口径一致，不构成文档缺陷。
- 启动/健康检查（库内模块以单测代探活）：全新空 `CARGO_TARGET_DIR` 从零编译 31.24s，`cargo test --lib change::state` → **10 passed; 0 failed; 0 ignored; 588 filtered out; EXIT=0**，10 个用例名与文档测试表逐名一致。
- 预期输出与文档 `test result: ok. 10 passed; 0 failed; 0 ignored; 588 filtered out` 逐字吻合（文档已声明 `filtered out` 随仓内测试总数变化）；编译耗时 39.00s→31.24s 属机器抖动。
- 配置/种子/进程级启停：本单元确为库内模块，N/A 理由成立。

## 8. 第三方可测标准（通过）

字段语义（phase/baseline/counters/change_id/last_test_failure）、幂等/覆盖/清除/冷启动语义、示例断言与 10 例测试表，足以在不看源码的前提下编写等价测试；「不负责」边界与 N/A 理由避免了把 section/编排行为误写进本单元用例。

## 结论

覆盖 18/18、lint 0 违规采信；findings N/A；plan tree 无硬冲突；上轮四处矛盾与 as_str 缺节全部修复；20 处 🟢 均有落点且行号经 glossary/facts 交叉核对；Runbook 从零实跑（fetch EXIT=0、测试 10/10 EXIT=0）且预期输出逐字吻合。**PASS**。

## 非阻塞建议（随修）

1. `:71` 「判定见 `src/runstate/mod.rs:89`-99」中 `:99` 实为 `Loaded` 枚举起行（glossary:541），`matches` 收在 `:94`；建议收窄为 `:89`-94，需要时另引 `:99`。
2. `:96` 引 `check_roundtrip` 位置 `boundary.rs:160`，而 `boundary.rs.md:157` 与 glossary:8 记 `:159`；跨文档行号漂移，建议统一（不影响本单元结论）。
3. 首页「plan 010 D001/D022a」建议写成「009 D022a + 010 D001」，与 D022a 实际归属一致（plan tree roadmap:70 的简写可保留）。
4. facts 的 `TestFailureContext` 只记 Serialize/Deserialize，建议补「Debug/Clone/PartialEq/Eq」（本轮编译探针已验证），减少后续轮次的信息缺口。
5. `:264` 命令块建议给完整可复制形态（含 `RUSTUP_TOOLCHAIN=stable`、`TMPDIR` 提示），与环境前提段呼应，第三方可直接粘贴。
