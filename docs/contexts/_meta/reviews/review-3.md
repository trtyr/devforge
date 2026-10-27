# 文档审查报告 — unit-src-04（第 3 轮）

- 单元：`unit-src-04` → `src/change/coding_gate.rs`
- 文档：`docs/contexts/src/change/coding_gate.rs.md`
- 轮次：round 3（doc-from-zero 单元轮次）
- 引擎审计（确定性采信）：要求 17 条 / 已覆盖 17 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）

## 0. 审查输入与边界

文档 `contexts/src/change/coding_gate.rs.md`；事实 `_meta/facts/unit-src-04.md`；plan tree 锚点 P009 T010 / T010b（`plans/009-change-sop/roadmap.md:55/58`）与 D023（`decisions/README.md:161`）。按 DR-12 不读源码，实体存在性以引擎 doc-lint（EXIT=0）与 facts 为准；本报告只审结构/必填节/N-A/一致性/置信度诚实性/第三方可测性。

## 1. findings 处置完整性

本轮 kickoff 未附 doc-fix findings 清单；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与 unit-src-04 无关。unit-src-04 无待处置的 doc-side finding → **N/A（无遗漏项，不构成 FAIL 条件）**。

## 2. plan tree 一致性

文档锚定 **P009 T010（回归前置）** 与 **T010b（实施步骤与按步提交）**，与 roadmap.md:55/58 一致；「先证后报」门与 D023 决策登记吻合。测试覆盖表登记 D023（:566）与文档口径相符。**无冲突**。

## 3. 内部自洽（发现 FAIL 项）

- 模块条目（职责/负责/不负责/公开成员表/依赖）齐备且互不矛盾；核心口径与 16 个公开符号正文一一对应（module + 16 公开符号 = 17 个清单条目，与覆盖数吻合）。
- **FAIL 项（常量取值与事实矛盾）**：facts（确定性）给出三个常量字面值：
  - `BASELINE_LOG = "evidence/baseline-test.log"`
  - `SELF_CHECK_LOG = "evidence/self-check.log"`
  - `IMPL_STEPS = "impl-steps.md"`
  而文档「取值」/「示例」表述为「不写成带斜杠整串」（BASELINE_LOG / SELF_CHECK_LOG）与「不写成带扩展名整串；`impl-steps` 文件」（IMPL_STEPS）。即文档断言常量存储的是**无斜杠前缀、无扩展名**的裸名，与事实（含 `evidence/` 前缀、含 `.md`）直接矛盾。doc-lint 只核实体存在性（EXIT=0 无法捕获字面值语义），仍属**审查面 #3 接口描述与行为语义一致**的违规。

## 4. 真实性 / 实体存在性

所引实体（`src/change/coding_gate.rs` 各符号、`mod.rs:12/29`、`session.rs`、`testing_gate.rs:13/29`、D023 决策台账）均与 facts 一致；doc-lint EXIT=0 逐条校验并有行号可回指，无幽灵引用。仅 §3 所述**常量字面值描述**与 facts 不符（属真实性，非实体存在性）。

## 5. 置信度诚实性

全文 🟢 一律带 `CONFIRMED <path>:<line>` 且与 facts 行号逐一吻合（16 处函数/结构/常量 + 内部 `parse_evidence_log:52`，加上示例锚 :669/:676/:610/:626/:640/:650 等）；**无裸 🟢**。标 🟢 均可给出 path:line。

## 6. 完整性（必填节 / N-A 规则）

- 模块条目、16 个公开符号的分级节齐备：4 常量带类型/取值/含义/影响/示例；2 结构（BaselineEvidence/Step）带职责/构造/成员/生命周期/典型用法；10 函数带摘要/行为语义/参数/返回/异常/示例；内部 `parse_evidence_log` 带摘要+行为语义。
- N-A 规则：`parse_steps` 异常 N/A 带理由；`step_numbers_in`、`impl_steps_path` 异常 `N/A` 未附括号理由，但行为语义皆表明「纯解析/纯路径拼接、无 IO」，隐含理由成立，沿用前轮惯例合规。

## 7. 可跑 / 第三方可测

属库内模块（非独立服务），启停/boot 档位按引擎 doc-lint EXIT=0 采信。10 个公开函数均有可复现输入/期望输出与 `:line` 锚（如 `exit_code: 2`（pytest -q）→ Err 且文案含命令 :393；`step 2 step 2`→`[2]` 去重 :669），第三方可据此直接写单测。测试覆盖表 27 例与 facts「27 例」及其行号逐一吻合。
**但** IMPL_STEPS 取值描述若无 `\.md`，第三方按文档在黑板根建 `impl-steps`（非 `impl-steps.md`），`read_steps` 将命中错误的文件名（缺/空路径对不上），直接破坏可测性；BASELINE_LOG/SELF_CHECK_LOG 同理影响按字面常量写断言的测试。→ 归入 FAIL 项。

## 结论

覆盖 100%、doc-lint 0 违规、findings 处置 N/A、plan 无冲突、置信度诚实、缺节/N-A 合规均通过；但**三个常量（BASELINE_LOG/SELF_CHECK_LOG/IMPL_STEPS）的取值/示例描述与确定性 facts 的字面值矛盾**（含 `evidence/` 前缀与 `.md` 扩展名），属内部自洽与真实性违规，并影响第三方可测性。**FAIL**——需回注 doc-fix 修订三处常量取值与示例。
