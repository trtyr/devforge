# 文档审查报告 — unit-src-19（第 33 轮，复评）

- **单元**：`unit-src-19` → `src/change/session/session_tests/runtime.rs`（638 行，19 个 `#[test]`）
- **文档**：`docs/contexts/src/change/session/session_tests/runtime.rs.md`（92 行，frontmatter `type: module` / `unit: unit-src-19`）
- **输入**：上述文档 + `_meta/facts/unit-src-19.md` + `_meta/gaps.md` + `_meta/inventory.json` + plan tree（`docs/plantree/ideas/arc-r6-bulk-split-plan-20261024.md`）+ 引擎审计（覆盖 1/1、缺失 0、doc-lint 0 违规、Runbook 命令块）
- **DR-12**：不以源码推导行为；源码只做实体存在性与行号机械核对（`grep -n` / `sed` 定点），行为事实以 facts + 本轮从零实跑为准
- **裁决**：**PASS**。上轮 5 项（2 阻塞 + 3 非阻塞）全部闭环；19/19、34/34 实测与文档计数逐字一致；余 2 条非阻塞建议（§7）

## 1. 覆盖（采信引擎 1/1）

模块档位要求集合 1 条（module），文档 frontmatter `module_path` 自报家门即满足，缺失 0。另按 `inventory.json` 实况核对：本单元登记 18 条 behavior + 1 条 module；文档「用例与断言」表 19 行，18 条 behavior 全有落点，另含清单未登记的 `coding_runs_gates_and_returns_changed_files`（`:7`，`gaps.md:134` 已披露）——无悬空、无幽灵。

## 2. findings 处置（N/A）

工作区无 `testing/findings/`（`/home/trtyr/code-work/devforge` 下不存在 `testing/`），kickoff 未附本单元 doc-side findings → 待处置清单 `N/A`，不构成漏项。

## 3. 完整性 / N-A（通过）

- 模块三必填节齐备：职责（含「不负责」三项：plan 轮与提示词契约、夹具基座、三通道判定逻辑）、公开成员（`N/A` + 「无 `pub` 项」理由）、依赖（上游夹具 / 被测方法 / 断言工具 / 外部 crate）。
- 私有助手 4 条按「位置 + 能力」承接（`ScriptedResultFactory` / `new` / `ScriptedResultReview` / `ok`），符合内部符号档位。
- Runbook 七件套逐件有落点：环境（Rust 1.85+ / `git`）/ 依赖（`cargo fetch` + 离线）/ 配置项（`N/A` + 理由）/ 启停+健康检查（`N/A` + 理由）/ 种子数据（`fixture(true)` + `coding_ready` + 手写证据，并注明路由早退例外）/ 测试运行（命令 + 计数）/ 公开接口清单（`N/A` + 回指 `session.rs.md`）。无 `TODO`/`TBD`。

## 4. 真实性 / 置信度（通过）

- 全文 5 处 🟢，每处均带 `path:line`，无「标绿无落点」。
- 行号机械复核全中：`runtime.rs` `:3`/`:7`/`:44`/`:108`/`:139`/`:162`/`:187`/`:205`/`:230`/`:253`/`:283`/`:316`/`:339`/`:371`/`:377`/`:389`/`:413`/`:419`/`:436`/`:445`/`:469`/`:499`/`:521`/`:551`/`:575`/`:603`；`session.rs` `:49`/`:332`/`:375`/`:495`/`:520`-`:525`/`:565`；`observability/mod.rs` `:422`/`:464`/`:478`；`error.rs:74`；`review_gate.rs:16`/`:19`；`testing_gate.rs:18`；`session_tests/mod.rs:68`/`:90`。
- 语义口径复核（定点）：`assert_milestone_shape` 确为「level=INFO + component」（`:478`-`:487`）；路由早退确在证据校验之前（`session.rs:520`-`:525`）；对账项无 `path:line` 走 `Quality("…没有指到测试…")`（`session.rs:540`-`:546`）——与文档「对账项无 path:line 被拒」一致；四个 testing 类用例均手写回归日志 + 对账表，`:316` 路由用例两份都不写，与 §核心口径第 5 条一致。
- 与 plan tree 无冲突：ARC-R6「tests mod 内迁 `session_tests/`、纯移动」与文档首段口径一致。

## 5. 从零 boot 实测（照 Runbook）

| 步骤（按文档） | 实测 |
|---|---|
| 环境：`edition = "2024"` ⇒ 1.85+ | `rustc 1.99.0` / `cargo 1.99.1` ✅ |
| 依赖：`cargo fetch`（离线可加 `CARGO_NET_OFFLINE=true`） | 成功，EXIT=0 ✅ |
| `cargo test --lib change::session::session_tests::runtime` | `19 passed; 0 failed; 0 ignored; 594 filtered out`，EXIT=0 ✅（与文档逐字一致） |
| `cargo test --lib change::session` | `34 passed; 0 failed; 0 ignored; 579 filtered out`，EXIT=0 ✅（与文档逐字一致） |
| 种子/隔离：`TMPDIR` 必须已存在 | `TMPDIR=/var/tmp/no-such-u19` 负例：`0 passed; 19 failed`，报 `Custom { kind: NotFound, error: PathError { path: "/var/tmp/no-such-u19/.tmpij3Cnh", … } }`，与文档 `PathError { kind: NotFound }` 一致 ✅ |

唯一摩擦：本次 shell 的 `$HOME=/var/tmp/devforge-accept`，该 HOME 下 `.rustup` 是**不完整快照**（`components` 只有 cargo/clippy/rust-docs/rust-std/rustfmt，**缺 rustc**，且无 `settings.toml`）。因此：
- 不带 `RUSTUP_TOOLCHAIN` 时确如文档所述报「rustup could not choose a version of cargo to run」（逐字命中）；
- 但文档建议的 `RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable` 在此环境展开到该残缺工具链，`cargo fetch` 即报 `missing manifest in toolchain 'stable-x86_64-unknown-linux-gnu'`；
- 改用 `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`（与 `docs/contexts/src/config/mod.rs.md:120-122` 的既有口径、`_meta/state.json` 的取证口径一致）后，上表全通。

结论：**命令、计数、种子与隔离说明均可重放**；仅环境前缀的 `$HOME` 展开在本沙箱失配 → 列为非阻塞建议（§7-1），不判 FAIL。

## 6. 上轮 5 项闭环

| 上轮问题 | 现状 |
|---|---|
| 🟢 标题把「子串断言」写成「按字段断言」 | 已改为「按字段字面形态做子串匹配，不解析 JSON」，并保留 `contains("\"passed\":true")` 等落点 ✅ |
| `testing_*` 写证据覆盖过头 | 已限定四个具名用例，并显式写明 `testing_route_coding_returns_failures`（`:316`）例外及原因 ✅ |
| `capture_jsonl`/`assert_milestone_shape`/`test_support` 行号漂移 | 已按现树改 `:464`/`:478`/`:422`，与 facts 一致 ✅ |
| 「唯一测试侧依赖 tempfile」 | 已改为「本文件经夹具用到 `tempfile`」并点名 `tower`/`http-body-util` 供服务侧 ✅ |
| 环境要求过粗 | 已补 `edition 2024` ⇒ 1.85+、`RUSTUP_TOOLCHAIN`、`cargo fetch` 网络前提 ✅ |

## 7. 非阻塞建议

1. **环境前缀的 `$HOME` 展开**（`runtime.rs.md:77`、`:82`）：`RUSTUP_HOME=$HOME/.rustup` 只在 `$HOME` 恰好是 rustup 根时成立。本机服务账号 `HOME=/var/tmp/devforge-accept` 下的 stable 快照缺 rustc 组件，照抄会在 `cargo fetch` 报 `missing manifest in toolchain`。建议与 `config/mod.rs.md:120-122` 对齐，写成「`RUSTUP_HOME=<rustup 根>`（本机 `/home/trtyr/.rustup`；该工具链须含 `rustc` 组件，`rustup component list --installed` 可查）」，或至少补一句「`stable` 工具链必须完整安装」。
2. **「字段顺序改也会变红」过强**（`:37`）：用例全部是 `contains` 子串断言，JSON 字段重排不影响判定；只有字段改名、字段值形态变化、事件挪到别的模块（`target` 变）、级别/`component` 缺失才会红。建议改为「字段改名 / 值形态变化 / 事件挪到别的 target 才会红」，与同句「不解析 JSON」保持一致。
