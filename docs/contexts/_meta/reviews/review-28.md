# 文档审查报告 — unit-src-32（第 28 轮）

- 单元：`unit-src-32` → `src/docgen/impact.rs`；文档 `docs/contexts/src/docgen/impact.rs.md`（227 行，frontmatter `type: module` / `unit: unit-src-32`）
- 引擎审计（确定性，采信不重算）：覆盖 要求 12 / 已覆盖 12 / 缺失 0 → 100%；doc-lint **76 失败 / 2 提示 → 不通过**
- 输入边界（DR-12）：未打开源码；事实以单元文档、`_meta/facts/unit-src-32.md`、`_meta/gaps.md`、`_meta/state.json`、`inventory.json`、引擎审计与本轮实跑输出为准
- 裁决 **FAIL**：本单元文档四个审查面均过；阻断面是 round 级门禁（doc-lint 全量 corpus 不通过，主因「行号越界 = 引用不存在的实体」）

## 1. 覆盖（通过）

12/12 采信。`state.json` 记 18 条落点（module×1 + symbol×11 + behavior×6），正文对应关系可直接点验：模块自报家门 →「公开成员」11 行表 → 11 个小节（3 个类型/常量条目 + 8 个函数条目）→「内部条目」7 行表 → 6 例测试表。无「有清单无落点」的条目。

## 2. findings 处置（N/A）

工作区无 `testing/findings/`，kickoff 未附本单元 doc-side findings → 本轮待处置清单为 `N/A`，不构成漏项。

## 3. 完整性 / N-A（通过）

- 模块三件齐备：职责（含「不负责」五项：不建清单、不判内容、不改增量状态/不派发、不做 git 写操作）+ 公开成员 + 依赖（上游 wiring/incremental、下游 inventory、外部仅 `git`）。
- 条目节齐备：`ChangedFile` / `Impact` 各 5 节（职责/构造/公开成员/生命周期/典型用法）；`ASSEMBLY_PAGES` 6 节（名称类型/默认值/必填语义/取值范围/影响/示例）；8 个函数各 6 节（摘要/语义/参数/返回/异常/示例）。私有 7 项以汇总表满足摘要+语义。
- Runbook 七件套：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN=stable`、运行期需 `git` 在 PATH 且 6 例不依赖 git）、依赖（`cargo fetch` + `tempfile` + 离线开关）、配置项（N/A+理由：常量决定扫描面）、启停+健康检查（N/A+理由：库内模块无进程面）、种子数据（N/A+理由：输入=变更集+旧清单+现仓扫描）、测试运行（完整命令 + 预期 6/0）、公开接口（HTTP/CLI N/A，Rust 面 11 名）。N/A 均带理由。

## 4. 真实性 / 置信度（本单元通过，round 级 FAIL）

- **本单元文档 0 条命中** 76 条失败（引擎清单逐条比对，`src/docgen/impact.rs.md` 未出现）。
- 置信度诚实性：15 处 🟢 全部带 `src/docgen/impact.rs:<line>`（`CONFIRMED` 计数 15，与正文一致），3 处 🟡 标 `INFERRED` 并给依据（白名单字面量、grep 无消费点、id 形态），未把推断写成事实。upper 边界 `:510` 未越过 512 行。
- **round 级红门禁**：76 条失败按形状分三类——①行号越界（主体），集中在 `change/session.rs.md` 与 `change/orchestrator.rs.md`（另 `change/testing_gate.rs.md` 2、`change/prepare.rs.md` 1、`change/history.rs.md` 1）；②`change/docs.rs.md:38` 幽灵路径 `src/docgen/inventory.rs`；③`config/render.rs.md:190`/`:195` 未完成标记。成因与 `state.json` 的「全量基线树 0 失败 / EXIT=0」并不冲突：基线树是 `git archive 184abb8`，而本轮门禁量的是**当前工作区**（ARC-R6 拆分在途，`change/session.rs` 已缩短、`inventory.rs`→目录模块），文档行号仍指旧长度 → 引用不存在的实体。
- 归属不构成豁免：门禁衡量整份 docs corpus，引擎已明示「不通过」；归属只影响 doc-fix 处置顺序。
- 附带矛盾：`config/render.rs.md:195` 的注称这两条「仅提示、不判失败、退出码仍为 0」，而引擎本轮把它们列进失败清单 → 文档对门禁行为的描述与确定性审计不符，须改正（或与引擎口径对齐），不能留成「文档说绿、引擎判红」。

## 5. 可跑（本单元 Runbook 从零实跑，EXIT=0）

按文档 Runbook 命令原样执行，仅补齐占位符（`TMPDIR=/var/tmp/r28boot/tmp`、`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`、`CARGO_NET_OFFLINE=true`、仓外空 `CARGO_TARGET_DIR=/var/tmp/r28boot/target`）：

```
cargo test --lib docgen::impact
→ 6 passed; 0 failed; 0 ignored; 0 measured; 592 filtered out
→ EXIT=0（从零编译 36.46s）
```

- 6 个用例名与「测试覆盖」表逐一对应（`parses_name_status_and_renames`、`parses_porcelain_including_untracked_and_renames`、`changed_file_maps_only_to_its_own_unit`、`deleted_symbol_pulls_in_referrers`、`assembly_page_mentioning_changed_unit_is_flagged`、`plan_impact_without_old_inventory_errors`），与文档预期「6 passed / 0 failed」逐字吻合。
- 文档同时说明测试不依赖 `git`——本轮离线（`CARGO_NET_OFFLINE=true`）实跑印证。
- 配置/启停/健康检查/种子数据均 N/A+理由，本单元无进程面，故「boot 不起来」不是本单元阻断项；阻断项是 §4 的门禁。
- 注：命令里的 `<宽裕临时目录>` / `<rustup 根>` / `<仓外独立空目录>` 是占位符，首次执行者需自行填值；建议给出可复制的示例值（非阻塞）。

## 6. 内部自洽 / plan tree（通过）

- 与 `_meta/facts/unit-src-32.md` 无实质冲突（行号表、11 个公开符号、6 个测试、消费方四点位一致）。
- `gaps.md` 本单元 6 条 🟡 在「核心口径」与「证据与缺口」逐条承接（kind 取值比注释宽/C→R、按新路径去重可能丢 `old_path`、删除文件只在整体零命中时回退、单字符名被过滤、汇编页 token 与真实 id 不匹配、引用白名单漏文档配置），无漏项、置信度一致。
- 文档内部无互斥断言：`Impact.units` 的有序去重 ↔ `BTreeSet` 收集；`assembly_review` 顺序 = `ASSEMBLY_PAGES` 顺序；`kind` 取值表 ↔ 两个解析函数的归一规则；「整体兜底而非逐文件兜底」↔ `impacted` 五步描述。
- 与 plan tree（008 T023 / D015 / D021、D016「叶子一变汇编页可能陈旧」）无冲突；`state.json` 的 `review-round`「首稿」描述与文档现状一致。

## 7. 第三方可测

11 个符号的签名/返回/异常/示例、两个结构体的字段约束与空值语义、`kind` 取值来源表、6 例测试的断言面、测试命令与隔离方式齐备；第三方可据此写等价测试，无需追问实现。

## 8. 非阻塞建议（文字质量）

1. `:66` 括号不配平（`……即 src/30 —— `:336` 至 `:339`）` 只有右括号）。
2. `:55`「均从 crate::docgen::impact 全路径 全路径调用」重复词；`state.json` 类笔误。
3. `:68`「`notes` 字段 是过程日志」多余空格。
4. 导语「不做 IO 以外的副作用」自相矛盾（IO 与副作用的关系颠倒），建议改为「除一次 `git` 子进程与只读 IO 外无副作用」。
5. 建议在「证据与缺口」加一行「本单元文档在 28 轮全量 lint 中 0 违规」，便于下一轮把红门禁归因到 `change/*` 的在途重构。

## 结论

覆盖 12/12、必填节与 N-A 合规、置信度诚实（15 🟢 均有 path:line）、本单元 Runbook 从零实跑 6/6（EXIT=0）、无 plan 冲突——本单元文档四面均过。但本轮引擎门禁 **doc-lint 76 失败 / 2 提示 → 不通过**，主因是 `change/session.rs.md`、`change/orchestrator.rs.md` 大量行号越界（引用不存在的实体）与一条幽灵路径、两条未完成标记，属硬 FAIL 面且口径为整份 corpus。据此判 **FAIL**。
