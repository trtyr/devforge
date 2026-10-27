# 文档审查报告 — unit-src-30（第 27 轮）

- 单元：`unit-src-30` → `src/docgen/audit.rs`；文档 `docs/contexts/src/docgen/audit.rs.md`（255 行，frontmatter `type: module` / `unit: unit-src-30`）
- 引擎审计（确定性，采信不重算）：覆盖 要求 13 条 / 已覆盖 13 条 / 缺失 0 → 100%；doc-lint **75 个失败 / 2 个提示 → 不通过**
- 输入边界（DR-12）：不打开源码；事实以单元文档、`_meta/facts/unit-src-30.md`、`_meta/gaps.md`、`_meta/state.json`、`inventory.json`、plan tree、引擎审计与本轮实跑输出为准
- 裁决 **FAIL**：本单元文档四个审查面均过，唯一阻断面是 round 级门禁——doc-lint 全量 corpus 不通过（引用不存在的实体）

## 1. 覆盖与 findings 处置（通过）

- 覆盖 13/13 采信。`inventory.json` 本单元 25 条（module×1 + symbol×12 + behavior×12）均有落点：模块自报家门 + 「公开成员」12 行表逐行指向条目 + 「内部条目」表 + 12 例测试表；`status` 仍为 `uncovered` 属无标记工具写入（`state.json` 已注明）。
- 工作区无 `testing/findings/`，kickoff 未附本单元 doc-side findings → doc-side 待处置清单 `N/A`。
- `gaps.md` 本单元 5 条（双 `DocLevel` 语义分叉 / 非模块条目子串判定过宽 / Runbook 只认文件名 / 重复正文按物理行 / 三个 `Result` 签名无 `Err` 路径）在「核心口径」与「证据与缺口」逐条承接，置信度与 gaps 一致（🟡），无漏项。

## 2. 完整性 / N-A（通过）

- 模块三件齐备：职责（含「不负责」五项：不生成清单、不做 lint、不写盘、不做影响面、不跑 Runbook 命令）、公开成员、依赖（上游 CLI/wiring、下游 inventory、零外部依赖）。
- 12 个公开条目按契约节齐备：类条目 5 节（职责/构造/成员/生命周期/用法）×4，函数条目 6 节（摘要/语义/参数/返回/异常/示例）×8；私有条目以汇总表满足「摘要 + 语义」。
- Runbook 七件套逐件有落点：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN=stable`）、依赖（`cargo fetch`、`tempfile`、离线开关）、配置项（N/A+理由）、启停+健康检查（N/A+理由，库内纯函数）、种子数据（N/A+理由，输入是 inventory 与 md）、测试运行（完整命令 + 预期 12/0）、公开接口清单（CLI + Rust 面 12 名）。所有 N/A 均带理由，无「省略即不适用」。

## 3. 真实性 / 置信度（本单元通过，round 级 FAIL）

- 引擎 75 条失败逐条验收：主体是 `[entity] 行号超出文件范围`，全部落在 `change/*` 文档（按引擎清单点数：`session.rs.md` 40、`orchestrator.rs.md` 31、`testing_gate.rs.md` 2、`history.rs.md` 1、`prepare.rs.md` 1），另有 `config/render.rs.md:190`、`:195` 两条 `[banned] 未完成标记`。**本单元文档 0 命中**。
- 成因：`src/change/session.rs` 已缩到 589 行，`orchestrator.rs` 也在 ARC-R6 拆分在途（工作区 `M` + 未跟踪目录），文档行号仍指 1700+/1100+ 行 → 指向不存在的实体（契约 §五-1，门禁判 FAIL）。
- 「与本单元无关」不构成豁免：门禁衡量的是整份 docs corpus，引擎已明示「不通过」；审查不得以归属为由把红门禁改判为绿。归属只影响 doc-fix 的处置顺序，不影响裁决。
- 置信度诚实性：12 条断言全部 🟢 + `src/docgen/audit.rs:<line>` 或可重放命令；2 处 🟡（双 `DocLevel`、子串判定）标 INFERRED 并给依据；未把推断写成事实。

## 4. 可跑（本单元 Runbook 从零可跑；全局门禁红）

- 装依赖 + 测试（按 Runbook 命令原样，隔离环境 `TMPDIR=/var/tmp/r27boot/tmp`、`RUSTUP_HOME`、`RUSTUP_TOOLCHAIN=stable`、`CARGO_NET_OFFLINE=true`、仓外空 `CARGO_TARGET_DIR=/var/tmp/r27boot/target`）：`cargo test --lib docgen::audit` → `12 passed; 0 failed; 0 ignored; 586 filtered out`，EXIT=0，与文档预期逐字吻合；12 个用例名与测试表一一对应。从零编译 45.5s。
- 宿主面：`cargo build --bin devforge` EXIT=0（10.7s）。
- 文档列出的「可重放命令」`devforge doc-audit docs/contexts --repo .`：EXIT=1——覆盖 983 条 / 已覆盖 959 / 缺 24（`domain/*`、`main.rs`/`observability`/`incremental.rs` 配置键、`tests/e2e` 等尚未落文档的单元，属流程未走到），lint **78 失败**（比引擎多：`session.rs.md` 2 处新漂移 + `docgen/impact.rs.md:62`/`:64` 两条 🟢 无 path:line、`:66` 幽灵路径 `src/30`）。
- 配置 / 启停 / 健康检查 / 种子数据：文档标 N/A 并给理由，本单元无进程面，验收通过；因此本轮「boot 不起来」不是阻断项，阻断项是 §3 的门禁。

## 5. 内部自洽 / plan tree

- 文档与 P008 D013 / T015/T016 口径一致（「缺一条即零覆盖」「单元级 + 全局两口径」），与 `_meta/facts` 无实质冲突，无 plan 冲突。
- 唯一漂移：`facts/unit-src-30.md` 把 `render` 写成「三行头 + 每条 miss 一行 + 结论一行」，文档写「1 行头 + 每条 miss 1 行 + 1 行结论」；引擎审计文本实样为 1 行头 → 文档正确，facts 措辞不准（非阻塞）。
- 文档内部无互斥断言：档位表 ↔ `parse` 语义、数据模型 `required ≥ covered` ↔ 遍历口径、`render` 结论文案 ↔ 引擎输出，均对得上。

## 6. 第三方可测

12 个符号的签名/返回/异常/示例、`CoverageReport` 字段语义、`render` 三种行形状、12 例测试的断言面、测试命令与隔离方式齐备，第三方可据此写出等价测试，无需追问实现。

## 7. 非阻塞建议

1. `:211`-229 与 `:350`-352 等区间写法建议统一为 `` `:211`-`:229` `` 或 `:211`–`:229`，避免区间后半段被读成裸行号。
2. 「核心口径」首条把「缺一条即零覆盖」同时归到 `:70` 与 `:95`，建议明确「判据在 `:70`，文案在 `:95`」。
3. 可在「证据与缺口」补一行「本单元文档在 27 轮 lint 中 0 违规」，便于下一轮定位红门禁的归属。

## 结论

覆盖 13/13、必填节/N-A 合规、置信度诚实、本单元 Runbook 从零实跑 12/12（EXIT=0）、无 plan 冲突——本单元文档四面均过。但本轮引擎门禁为 **doc-lint 75 失败 / 2 提示 → 不通过**，失败项是「引用不存在的实体」与「未完成标记」，属契约 §五/§六的硬 FAIL 面，且门禁口径是整份 docs corpus。据此判 **FAIL**，issue 见协议块（回注 doc-fix）。
