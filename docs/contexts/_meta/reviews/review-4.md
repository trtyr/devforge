# 文档审查报告 — unit-src-07（第 4 轮）

- 单元：`unit-src-07` → `src/change/docs.rs`
- 文档：`docs/contexts/src/change/docs.rs.md`
- 轮次：round 4（doc-from-zero 单元轮次）
- 引擎审计（确定性采信）：要求 5 条 / 已覆盖 5 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）

## 0. 审查输入与边界

文档 `contexts/src/change/docs.rs.md`；事实 `_meta/facts/unit-src-07.md`；plan tree 锚点 P009 D002（`plans/009-change-sop/decisions/README.md:13`）及 T002/T015（`roadmap.md:16/94`）。按 DR-12 不读源码，实体存在性以引擎 doc-lint（EXIT=0）与 facts 为准；本报告只审结构/必填节/N-A/一致性/置信度诚实性/第三方可测性。

## 1. findings 处置完整性

本轮 kickoff 未附 doc-fix findings 清单；unit-src-07 本轮为首次审查（review-1/2/3 分别对应 unit-src-01/02/04），`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与 unit-src-07 无关。无待处置的 doc-side finding → **N/A（无遗漏项，不构成 FAIL 条件）**。

## 2. plan tree 一致性

文档锚定 **P009 D002（文档完整度不由引擎探测）**，与 `decisions/README.md:13` 一致；D002 决定「引擎不判断有没有文档，只认调用方传的 `doc_level`，默认 `none`」与文档「引擎不自动探测仓里有没有 docs/」口径完全吻合。T002（`doc_level` 开关 none/essential/complete）与 T015（门 0 预检：none/essential 落地、complete 拒跑并指向 doc-from-zero）均被文档核心口径逐条满足。**无冲突**。

## 3. 内部自洽

- 模块 front matter（type/module_path/unit/description）与目标文件、单元号一致。
- 职责/公开成员/核心口径三处对三档行为表述完全一致：none→Skipped 零动作；essential→真写 `docs/contexts/_meta/inventory.json`+`glossary.md` 后放行 plan、返回 EssentialDone；complete→不写任何东西、返回 FullChainRequired（引擎不假装写完）。
- 公开成员表 4 项（DocAction/apply/essential_ready/inventory_path）与正文小节一一对应，且与 facts「4 公开条目 + 1 模块 = 5」相符（覆盖 5/5）。
- DocAction 变体在「公开成员」与「§DocAction」两处一致（Skipped/EssentialDone/FullChainRequired）；异常面（Context 包装、complete/none 不返回错误）与 facts 口径吻合。
- 测试覆盖表 3 例（:72/:83/:107）与 facts「3 例」一一对应，语义（none 不建 docs / essential 产清单放行且幂等不产正文 / complete 全交 doc-SOP）前后无矛盾。

## 4. 真实性 / 实体存在性

所引实体与 facts 行号逐一吻合：`DocAction:17`、`apply:31`、`essential_ready:52`、`inventory_path:56`；依赖面 `mod.rs:13/30`（声明与转发导出 `apply as apply_doc_level`）、`prepare.rs:9/24/57/58/182`（doc_action 落点）、`gate0.rs` DocLevel、`docgen/inventory.rs`（build_inventory/export_glossary/Inventory）均与 facts 一致。引擎 doc-lint EXIT=0 已逐条校验实体存在性并有行号可回指，无幽灵引用。

## 5. 置信度诚实性

全文 🟢 一律带 `CONFIRMED <path>:<line>`（`docs.rs:17`、`:31`、`:52`、`:56`），与 facts 行号逐一吻合；**无裸 🟢**，标 🟢 均可给出 path:line。

## 6. 完整性（必填节 / N-A 规则）

- 模块条目：职责（负责/不负责）+ 公开成员表 + 依赖（上游消费方/向下依赖/外部 crate）齐备。
- 公开类 `DocAction`：职责/构造（由 apply 分派产生）/公开成员（三变体语义）/生命周期/典型用法齐备。
- 公开函数 3 个（apply/essential_ready/inventory_path）：摘要/行为语义/参数/返回/异常/示例全节齐备。
- N-A 规则：`essential_ready`、`inventory_path` 的「异常：N/A」未附括号理由，但其行为语义已明确「只读检查、无 IO 副作用」/「纯路径拼接、无 IO」，隐含理由成立，沿 round 1/2 同款惯例合规。仅记一条非阻塞建议（见 <notes>）。
- Runbook 七件套：本单元为库内模块（非独立服务，Rust crate 内部），启停/健康检查/种子数据档位按 engine doc-lint EXIT=0 采信（round 1/2 同款惯例）；「公开接口清单」由公开成员表 + 各成员参数/返回/错误节承担。契约面合规。

## 7. 可跑 / 第三方可测

属库内模块（非独立服务），boot 档位以引擎 doc-lint EXIT=0 为准。三个公开函数均有可直接复现的输入/期望输出与 `:line` 锚（`apply(tmp, None)`→Skipped 不建 docs :72；`apply(tmp, Essential)`→EssentialDone 真产清单、`essential_ready()` 为真、不产正文 :83；`apply(tmp, Complete)`→FullChainRequired 不建 docs :107），第三方可据此直接写单测。无 TODO/TBD/日志粘贴/代码复述反模式。

## 结论

引擎审计（覆盖 100%、doc-lint 0 违规）采信通过；人工复核六面：findings 处置 N/A、plan 树无冲突、内部自洽、实体真实、置信度诚实、缺节/N-A 合规、可跑可测——均无阻断问题。**PASS**。

<notes>
- 非阻塞建议：`essential_ready` / `inventory_path` 的「异常：N/A」若补一句括号理由（如「只读无 IO」/「纯路径拼接」）可使 N-A 规则字面更严，与 round 2 已接受的隐含理由惯例无冲突。
</notes>
