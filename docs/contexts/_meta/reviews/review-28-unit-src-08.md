# 文档审查报告 — unit-src-08（第 28 轮）

- 单元：`unit-src-08` → `src/change/gate0.rs` + `src/change/history.rs`；文档 `docs/contexts/src/change/gate0.rs.md`（233 行）、`docs/contexts/src/change/history.rs.md`（137 行），两者 frontmatter 均 `type: module` / `unit: unit-src-08`
- 引擎审计（确定性，采信不重算）：覆盖 要求 10 条 / 已覆盖 10 条 / 缺失 0 → 100%；doc-lint **16 失败 / 0 提示 → 不通过**
- 输入边界（DR-12）：未打开源码；事实以两篇单元文档、`_meta/facts/unit-src-08.md`、`_meta/gaps.md`、`_meta/inventory.json`、plan tree、引擎审计与本轮实跑输出为准
- 落盘说明：`review-28.md` 已被 `unit-src-32`（第 28 轮先到者）占用，按 `review-23-unit-src-24.md` 先例写 `review-28-unit-src-08.md`，不改动他轮报告
- 裁决 **FAIL**：本单元两篇文档五个审查面均过；阻断面是 round 级门禁（doc-lint 全量 corpus 16 条幽灵路径）

## 1. 覆盖（通过，10/10 采信）

`inventory.json` 本单元实际 19 条（gate0：module×1 + symbol×6 + behavior×6；history：module×1 + symbol×2 + behavior×3），`status` 仍全为 `uncovered`（无标记工具写入，非文档缺陷）。19 条在文档里都有实体落点，覆盖档位要求的 10 条自然在内：

- gate0：`Baseline`/`DocLevel`/`parse`/`as_str`/`needs_docs`/`ensure_baseline` 各有独立小节，「公开成员」表 6 行逐行指向条目；6 例测试由「测试覆盖」表逐行承接。
- history：`history_path`/`append_roundtrip` 各 6 节；3 例测试同表承接；模块自报家门 + 「不负责」四项界定边界。

## 2. findings 处置（N/A）

工作区无 `testing/findings/`（`/home/trtyr/code-work/devforge/testing` 与沙箱根均不存在），kickoff 未附本单元 doc-side findings → 待处置清单 `N/A`，不构成漏项。

## 3. 完整性 / N-A（通过）

- 模块三件（职责含「不负责」/ 公开成员 / 依赖）齐备；gate0 的「不负责」四项（不生成 change-id、不执行文档动作、不判越界、不写历史）与 history 的四项互补，无重叠真空。
- 条目节按类型齐备：`Baseline`/`DocLevel` 各 5 节（职责/构造/公开成员/生命周期/典型用法）；4 个函数各 6 节（摘要/行为语义/参数/返回/异常/示例），`as_str`、`needs_docs` 的参数与异常显式 `N/A`；5 个私有条目（`BASELINE_FILE` + 4 helper）以摘要 + 行为语义满足内部符号档位。
- 配置项 `DEVFORGE_DOC_LEVEL` 七件齐（名称/类型/默认值/是否必填/取值范围/影响/示例）；history 无配置项，在「不适用声明」内给了 N/A + 理由。
- Runbook 七件套逐件有落点：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN=stable`、git 2.53.0）/ 依赖（`tempfile` dev-dep + 自动获取 + git 在 PATH）/ 配置项（gate0 真实项，history N/A）/ 启停+健康检查（两篇均 N/A+理由：库内模块无进程面）/ 种子数据（N/A+理由：测试自建临时目录）/ 测试运行（完整命令 + 预期计数 + 单例命令）/ 公开接口清单（HTTP/CLI N/A，Rust 面即「公开成员」表）。

## 4. 真实性 / 置信度（本单元通过，round 级 FAIL）

- gate0 18 处 🟢、history 6 处 🟢：除两条标记在「实跑命令」上（可重放命令，符合契约 §五-2）外，其余全部带 `path:line` 或跨度；history 的 5 处 🟡 一律标 `INFERRED` 并给依据（调用顺序推得、仅文本拼接、无锁、空切片循环不执行），未把推断写成事实。
- 行号边界：gate0.rs 现 323 行 / history.rs 现 85 行，文档引用最高 `:322` / `:79`，未越界；两篇在本轮 16 条失败中 **0 命中**（引擎清单逐条比对），与 `facts/unit-src-08.md` 的「全库 16 条失败均不在本单元两篇文档」一致。
- **round 级红门禁**：16 条全是 `[entity] 幽灵路径`，按实体分四类——①`tests/change_failure_context.rs` 7 条（`change/mod.rs.md:45`、`orchestrator.rs.md:41`/`:258`、`session.rs.md:45`、`state.rs.md:64`、`testing_gate.rs.md:51`/`:194`）；②`src/server/tests.rs` 4 条（`config/installer.rs.md:38`、`config/mod.rs.md:54`/`:55`/`:62`）；③`tests/f6_observability.rs` 4 条（同上四处）；④`tests/change_review_protocol.rs` 1 条（`config/mod.rs.md:55`）。7+4+4+1=16，合计与引擎一致。
- 成因确定：\(ARC-R6 批 3（commit `a9bcd2c`）把四个单体测试文件目录化\)，现树只有 `tests/change_failure_context/{main,support,ctx…}.rs`、`tests/f6_observability/main.rs`、`tests/change_review_protocol/main.rs`、`src/server/tests/`（已核：四个目录存在、四个 `.rs` 平铺文件不存在）→ 文档平铺写法指向不存在的实体。
- 归属不构成豁免：门禁量的是整份 docs corpus，引擎已明示「不通过」；归属只影响 doc-fix 处置顺序。

## 5. 可跑（本单元 Runbook 从零实跑，全绿）

从零（空 target、离线）按文档命令原样执行，仅把 `$HOME/.rustup` 换成真实 rustup 根（见 §7-1）：

```
RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable CARGO_NET_OFFLINE=true \
CARGO_TARGET_DIR=<仓外空目录>/target cargo test --lib change::gate0
→ 6 passed; 0 failed; 0 ignored; 0 measured; 592 filtered out   EXIT=0（从零编译 33.7s）

… cargo test --lib change::history
→ 3 passed; 0 failed; 0 ignored; 0 measured; 595 filtered out   EXIT=0

… cargo test --lib change::gate0::tests::missing_repo_is_config_error -- --exact
→ 1 passed; 0 failed; 597 filtered out   EXIT=0
```

- 三个计数（592 / 595 / 597 filtered）与两篇文档写的预期逐字吻合；9 个用例名与两张「测试覆盖」表一一对应。
- 依赖面：离线即通过（`tempfile` 已在本地 registry），无需网络；4 例确实起 git 子进程（git 2.53.0 在 PATH），与文档「6 例中 4 例需 git」一致；构建产物落仓外，`git status` 未见 target 污染。
- 配置 / 启停 / 健康检查 / 种子数据：两篇均 N/A + 理由，本单元无进程面，故「boot 不起来」不是阻断项；阻断项是 §4 的门禁。

## 6. 内部自洽 / plan tree（通过）

- 与 plan tree 无冲突：默认档 `none`、三档取值、`complete` 拒跑并指向 doc-from-zero（P009 decisions:15、roadmap T002/T015）与 gate0 文档一致；往返上限 3、第 4 次停下问人、`history.md` 落 `docs/plantree/changes/<id>/history.md`（P009 T013、README:43、boundary-protocol §四）与 history 文档一致。
- 文档内无互斥断言：四态表 ↔ `ensure_baseline` 五步描述 ↔ 「先查记录文件、后看 git」动因；`created_repo` 仅第四条分支为 true ↔ 空仓分支 `:99`；「1 个私有常量 + 4 个私有 helper」↔「内部符号」5 条；「无去重」↔「测试覆盖」3 例口径。
- 与 `gaps.md` 本单元 3 条（竖线/换行不转义、读失败静默降级为空串、重复调用重复落行）逐条承接，置信度一致（🟡），无漏项；gate0 引用的同名 `DocLevel` 差异指向 `gaps.md:63`，实为该行（unit-src-30 段），指针有效、仅跨单元引用。

## 7. 非阻塞建议

1. 工具链前提写的是 `RUSTUP_HOME=$HOME/.rustup`，本沙箱 `HOME=/var/tmp/devforge-accept` 下有半装 toolchain（`missing manifest`），命令原样跑会红；建议补一句 `rustup toolchain list` 自检或给 `RUSTUP_HOME=/home/<user>/.rustup` 兜底示例。
2. 两处 `🟢 CONFIRMED，本单元实跑` 只带命令不带 `path:line`，合规但建议附「命令 + 计数 + EXIT=0」三元组，便于复跑比对。
3. `history.rs.md` 无独立「配置项」小节（N/A 写在「不适用声明」内），建议保留并前置一行小标题，减少查找成本。
4. `gate0.rs.md` 写 `src/docgen/audit.rs:14`（枚举声明）而 `gaps.md:63` 写 `:24`-30`（`parse` 区间），两处不矛盾但易被读成冲突，建议各自标注所指对象。
5. doc-fix 清理 §4 时建议按 `plan_audit.rs.md:137`-140 的旧→新映射表全量扫（全 corpus 共 31 行仍含四个平铺旧路径，其中只有带 `:line` 的 16 处被门禁点名）。

## 结论

覆盖 10/10、必填节与 N-A 合规、置信度诚实（🟢 均有 `path:line` 或可重放命令）、Runbook 从零实跑 6/6 + 3/3 + 单例 1/1（全 EXIT=0，计数与文档逐字吻合）、无 plan 冲突——本单元两篇文档五个审查面均过。但本轮引擎门禁 **doc-lint 16 失败 / 0 提示 → 不通过**，失败项全部是 `change/*.md` 与 `config/*.md` 里 ARC-R6 批 3 目录化前的平铺测试路径（引用不存在的实体，契约 §五-1/§六硬 FAIL 面），口径为整份 corpus，故判 **FAIL**，issue 回注 doc-fix。
