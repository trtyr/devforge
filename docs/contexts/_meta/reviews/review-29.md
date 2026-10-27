<doc-verdict>PASS</doc-verdict>
<route>none</route>
<issues>无</issues>

# 文档审查报告 — unit-src-12（第 29 轮）

- 单元：`unit-src-12` → `src/change/plan_audit.rs`；文档 `docs/contexts/src/change/plan_audit.rs.md`（387 行，frontmatter `type: module` / `unit: unit-src-12`）
- 引擎审计快照（确定性，采信）：覆盖 要求 19 条 / 已覆盖 19 条 / 缺失 0；doc-lint **16 失败 / 0 提示 → 不通过**
- **快照→现树的漂移（逐条验收所得）**：16 条 `[entity] 幽灵路径` 在本轮审查期间（19:21:55 / 19:21:58，在途 doc-fix）已被改写为目录形态；现树 16 处全部指向真实存在的实体；用仓内门禁二进制复跑 `./target/debug/devforge doc-lint docs/contexts --repo .` → `doc-lint: 通过（0 违规）`，EXIT=0。故门禁状态按现树为**通过**，快照的 16 条不再成立（明细见 §4）。本单元文档在快照 16 条中 **0 命中**。
- 输入边界（DR-12）：未打开源码；事实以单元文档、`_meta/facts/unit-src-12.md`、`_meta/gaps.md`、`_meta/inventory.json`、plan tree、引擎审计与本轮实跑输出为准。
- 单元认定：`_meta/state.json` 的 `files` 仍写 `mod.rs` + `prepare.rs`（旧一版映射），与 `inventory.json` 的 `unit_id`（plan_audit 全部 19 条）及文档 frontmatter 不符；本轮按 inventory + frontmatter 认定 unit-src-12 = `plan_audit.rs`（facts 文件首段亦已注明该漂移）。

## 1. 覆盖（19/19 采信）

要求集合 = module ×1 + 公开符号 ×18。「公开成员」表 19 行逐项链到独立小节，锚点（`#plan_blocks`、`#三个问询环字面量`、`#verdictaudit-的-is_clean` 等）与标题 slug 一致，无悬空条目。inventory 本单元 38 条（+19 behavior）由「测试覆盖（19 例）」表逐行承接，`status` 仍全为 `uncovered`（无标记工具写入，非文档缺陷）。

## 2. findings 处置（N/A）

工作区无 `testing/findings/`（`/home/trtyr/code-work/devforge` 下不存在 `testing/`），kickoff 未附本单元 doc-side findings → 待处置清单 `N/A`，不构成漏项。

## 3. 完整性 / N-A（通过）

- 模块三件齐备：职责（含「不负责」四项：不判语义、不写/不改计划、不跑 git、不注入自身结果）、公开成员、依赖（消费方 8 行表 + 向下依赖 + 测试依赖）。
- 19 个条目按类型齐备：2 个 struct 走类模板 5 节（职责/构造/公开成员/生命周期/典型用法）；方法 6 节（参数/异常显式 `N/A`）；函数 6 节；3 个字面量条目给出名称/取值/类型/影响/示例，参数·返回·异常 `N/A` 带理由。
- 私有符号 11 个（`VAGUE_ACCEPTANCE` + 10 helper）以「摘要 + 行为语义」满足内部档位。
- Runbook 七件套逐件有落点：环境（edition 2024 ⇒ Rust 1.85+、无默认 toolchain 时显式指定）/ 依赖（`tempfile` dev-dep、隔离方式）/ 配置项（`N/A` + 理由）/ 启停+健康检查（`N/A` + 理由：库内纯函数）/ 种子数据（`N/A` + 理由：输入是计划文本）/ 测试运行（命令 + 预期计数 + 隔离）/ 公开接口清单（即「公开成员」表，`N/A`：无 HTTP/CLI 端点）。省略项均有 `N/A` 声明，无「省略当不适用」。

## 4. 真实性 / 置信度（通过）

- 本单元文档 20 处 🟢：19 处带 `path:line` 或 `:X` 至 `:Y` 跨度，1 处（§测试运行命令）以可重放命令 + 实跑计数作证，符合契约 §五-2；1 处 🟡（`PLAN_BLOCKS` 子串判据）标 `INFERRED` 并给依据，未把推断写成事实。
- gaps 本单元 6 条（行号范围不校验 / 扩展名白名单窄 / 六块子串判断 / `section_lines` 子串切块 / `PATH_MIGRATIONS` 手工维护 / `render_facts` 空分支不可达）与正文「不负责」「缺口」逐条对应，置信度一致。
- **16 条 lint 快照逐条验收**：四类旧平铺路径 = `tests/change_failure_context.rs` ×7（`change/mod.rs.md:45`、`orchestrator.rs.md:41`/`:258`、`session.rs.md:45`、`state.rs.md:64`、`testing_gate.rs.md:51`/`:194`）+ `src/server/tests.rs` ×4 + `tests/f6_observability.rs` ×4 + `tests/change_review_protocol.rs` ×1（`config/installer.rs.md:38`、`config/mod.rs.md:54`/`:55`/`:62`）。逐行核对现文：全部已写 `…/main.rs`、`src/server/tests/mod.rs`；对应实体（4 个目录 + `main.rs`/`mod.rs`）实存，4 个平铺 `.rs` 确认不存在；复跑门禁 0 违规。即：快照真，现树已修，失效原因是时间差而非误报。
- 本单元文档只在 `PATH_MIGRATIONS` 的 fenced 取值表里登记这四个旧路径，并明写「左列路径在现状树里故意不存在」——属迁移事实，不是幽灵引用。

## 5. 可跑（从零实跑）

```
① 文档工具链自检原样执行：
   RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable cargo --version
   → cargo 1.99.1 (5f94df478 2026-08-27)   EXIT=0

② 文档代码块原样执行（不带环境前缀）：
   CARGO_TARGET_DIR=$(mktemp -d)/target cargo test --lib change::plan_audit
   → error: rustup could not choose a version of cargo to run …   EXIT=1
     （本机无默认 toolchain；文档上一段已要求"无默认值时显式指定"）

③ 加上文档声明的 toolchain 前提，TMPDIR 用默认（mktemp 落 /tmp）：
   → rustc-LLVM ERROR: IO failure on output stream: No space left on device   EXIT=101
     （/tmp 为 3.9G tmpfs，已用 88%）

④ 同上 + TMPDIR 指向大磁盘（仓外、空目录）：
   → test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 579 filtered out
     EXIT=0，从零编译 34.8s
```

- 19 个用例名与「测试覆盖」表逐行一一对应；文档写的「19 passed、0 failed、退出码 0」与 ④ 逐字吻合，两个文档守卫用例（`:735` 决策台账、`:759` agents-change 口径）实跑通过。
- 依赖面：`CARGO_NET_OFFLINE=true` 下从零编译成功（本地 registry 已含 `tempfile` 等）。文档「不需要 … 网络」按「**运行期**不需要网络」成立；首次构建的获取步骤未写（建议见 §7）。
- 配置 / 启停 / 健康检查 / 种子数据：文档 `N/A` + 理由，核验一致——本模块是纯函数库，无进程面，可跑的等价面就是测试运行。
- 结论：按文档声明的工具链前提 + 合理 `TMPDIR`，从零一次跑绿；②③ 的红是**命令块加固**问题（与上下文声明不一致、未提示避开 /tmp），非内容错误，记 §7。

## 6. 内部自洽 / plan tree（通过）

- 与 P009 一致：D012（引擎算确定性、审查官只判语义）、D024 四层（供给 / 结构契约 / 硬门 / 审查审点）、`[必须裁决]`/`[自决]`/`用户裁决：` 三字面量、缺非空留痕 → `PlanTurn::Fail` 且不进审查轮；`PLAN_BLOCKS` 六值与 README 六块口径相容（需求复述 / 影响面 / 要改的 / 白名单 / 验收标准 / 风险）。plan tree 中 D024 只登记一次、D009 带 D024 旁注——由库内两个守卫用例实跑钉住。
- 文档内部无互斥断言：公开成员 19 行 ↔ 19 节；「`VAGUE_ACCEPTANCE` 与 10 个 helper」↔ 内部表 11 行；「19 例里 11 例错实现形态守卫（`:609` 起）」↔ `:608` 起 11 例；四项检查在「职责」「核心口径」「audit 条目」三处口径相同。
- 一处口径偏差（文档已披露，非冲突，不作 FAIL）：plan tree `topics/agents-change.md:71` 要求「证据真实 = `path:line` 存在、**行号在范围内**，引擎先扫」，本模块只判路径存在；文档在「不负责」与 gaps 首条明写，未冒充合规。建议加一行回指（§7-5）。

## 7. 非阻塞建议

1. 命令块补前缀 `RUSTUP_HOME=… RUSTUP_TOOLCHAIN=stable`（与本节「无默认值须显式指定」自洽，消除 ② 的 EXIT=1）。
2. 命令块把 target/TMPDIR 指到大磁盘（同 corpus `prepare.rs.md` 已有「别用 /tmp」告警，本文缺；消除 ③ 的 ENOSPC）。
3. 依赖安装补一句：首次 `cargo fetch` 需网络，离线仅在本地 registry 已缓存时可行。
4. 「异常：无 IO 失败面（IO 只在 `audit_plan_file` 里）」与「只读仓库（存在性探测）」措辞可精确为「无 `Result`/无 panic；仅 metadata 探测，失败即视作不存在」。
5. 「不负责：不校验行号范围」处加回指 `agents-change.md:71`，让读者一眼看出这是 plan tree 已知缺口而非文档漏写。
6. `_meta/state.json` 的 `files` 映射应与 `inventory.json` 同步（现仍指向 mod.rs+prepare.rs），避免后续轮次认错单元。
7. 后续改这几篇时保持「旧平铺路径只出现在 fenced 迁移表/历史语境，正文一律目录形态」——现树已达此状态，勿回退。

## 结论

覆盖 19/19；findings `N/A`；必填节与 `N/A` 规则合规；20 处 🟢 均有 `path:line` 或可重放命令，🟡 标注诚实；Runbook 按文档声明的 toolchain 前提从零实跑 19 passed / 0 failed / EXIT=0，计数与文档吻合；与 plan tree 无冲突（唯一偏差已披露）；快照的 16 条幽灵路径已在现树修净并复跑门禁 0 违规。判 **PASS**，附 7 条非阻塞建议回注 doc-fix。
