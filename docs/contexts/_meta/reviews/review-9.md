# 文档审查报告 — unit-src-12（第 9 轮）

- 单元：`unit-src-12` → `src/change/plan_audit.rs`
- 文档：`docs/contexts/src/change/plan_audit.rs.md`（220 行）
- 轮次：round 9（该单元首轮人工审）
- 引擎审计（确定性采信，未重算）：要求 17 条 / 已覆盖 17 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）

## 0. 输入与边界

单元文档 + 引擎覆盖/lint 文本 + plan tree 锚点（P009 T006/T007/T007b、D012/D013/D024；审查协议见 `plans/009-change-sop/README.md:66`）。按 DR-12 不读源码；交叉核对只使用同为文档侧产物的 `facts/unit-src-12.md`、`mod.rs.md`、`orchestrator.rs.md` 与 `glossary.md`。

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 doc-fix findings；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与本单元无关。**无待处置清单 → N/A（不构成遗漏）**。

## 2. plan tree 一致性

- T007b（`roadmap.md:34-36`）：`path:line` 存在性、白名单路径存在性、「要改的」⊆ 白名单，机械输出注入审查 → 文档 `audit` 的行为语义逐条对应，且 `review_instruction` 标注「勿质疑、勿重算」。
- T007 / D013：验收可观测（输入 → 期望输出）→ 文档 `VAGUE_ACCEPTANCE` 与「含模糊措辞且不含 `→`」判据一致。
- D012：plan 阶段审查不执行验收命令 → `review_instruction` 明文「不跑验收命令」。
- D024：`[必须裁决]` 缺非空 `用户裁决：` 为硬门、未分类/散文/块缺失只进事实、审查侧新增 6–8 条判据 → 文档核心口径、`audit_verdicts` 行为与「8 条判据」分段完全吻合。
- 措辞备注：T007b 正文只枚举三项机械检查（路径存在、白名单存在、集合包含），文档称「四条机械检查」；结合 T007/D013 可解释为第四项「验收可观测」，且 roadmap 证据本身写「四项机械检查」，**非硬冲突**。

## 3. 覆盖对账（17/17）

17 条要求 = 模块 1 + 16 个公开符号（4 常量、2 结构、4 个结构方法、6 个自由函数）。文档「公开成员」表逐项列名并链接到独立小节，方法挂在结构体小节内；逐项有落点，与引擎 17/17 一致。清单外的 19 条 behavior/spec 另由「测试覆盖」表逐例承接。无「只在 Runbook 提一句」式的伪覆盖。

## 4. 完整性 / 必填节 / N-A

- 模块三件套齐备：职责（含「不负责」）、公开成员、依赖（上游消费方 + 向下依赖 + 外部 crate）。
- 4 常量：名称/类型、取值、影响、示例齐备，取值与 facts 字面值逐一相同（六块名、`[必须裁决]`/`[自决]`/`用户裁决：`）。
- 2 结构：职责、构造/初始化、公开成员、生命周期、典型用法、派生 trait 齐备。
- 6 个公开函数：摘要、行为语义、参数、返回、异常、示例齐备；异常不适用处均写 `N/A` 并给理由（纯函数/无 IO/无失败面）。
- 内部 8 个 helper 均给摘要 + 行为语义（超出内部符号要求）。未发现 TODO/TBD、日志粘贴或大段代码复述。

## 5. 真实性 / 内部自洽

- 所有公开实体行号与 facts 吻合：`PLAN_BLOCKS:13`、`VERDICT_*:32/35/38`、`VerdictAudit:42`、`is_clean:50`、`render_facts:55`、`audit_verdicts:75`、`PlanAudit:203`、`is_clean:213`、`render:218`、`audit:238`、`path_refs:295`、`plan_facts:364`、`review_instruction:386`、`audit_plan_file:419`；内部 helper `119/138/159/190/327/341/355/313` 亦逐条对得上。
- 消费方锚点与 facts 一致：`mod.rs:17/36`（只转发 `PlanAudit`、`audit as audit_plan`、`review_instruction`）、`session.rs:18/304/310/319/540/569/574`、`change_engine.rs:126`、`manifest.rs:1045/1066-1068/1084-1085`、`testing_gate.rs:234`。
- 跨文档：`mod.rs.md:13/43/61/75` 的「选定清单转发、其余走子模块路径」与本单元「其余公开函数不在转发面内」一致；`orchestrator.rs.md` 的 `plan_facts` 默认方法与本单元自由函数职责相容；`glossary.md:81-106` 的行号一致。
- 引用的源文件、`Cargo.toml`、plantree 文档均真实存在；无幽灵引用。测试表 19 例的名称、行号与测试运行结果逐名吻合。
- 未发现前后矛盾：`audit` 的五步行为与「四条机械检查 + 六块」总述自洽；`audit_verdicts`「最后一个歧义标题生效」与 `last_ambiguity_heading_wins` 自洽；`render`/`render_facts` 的空值形态与结构体小节一致。

## 6. 置信度诚实性

16 处 🟢 全部同行给出 `CONFIRMED <path>:<line>`，无裸绿；行号与 facts、测试行号一致。唯一 🟡 在 `:218`（测试命令未实跑复现），写明推断依据与环境限制，未冒充事实。**本轮已实跑通过（见 §7），该 🟡 可升级为 🟢（非阻塞建议）**。

## 7. 可跑（从零 boot 实跑）

- 环境：审计沙箱 `HOME=/var/tmp/devforge-accept` 无 rustup 默认 toolchain；改用仓主已装 toolchain：`RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`（cargo/rustc 1.99.0）。
- 装依赖：`cargo fetch` → **EXIT=0**。
- 隔离：`CARGO_TARGET_DIR=/tmp/dfz-r9-target`（空目录，未复用仓库 `target/`）。
- 启动/健康检查（库内模块以单测代替）：`cargo test --lib change::plan_audit` → **19 passed; 0 failed; 581 filtered out; EXIT=0**；19 个测试名与文档测试表逐条一致。
- 七件套映射：本单元为库内模块，无独立服务/端口/配置项/种子数据/HTTP 端点，相应件按 N/A 成立；「测试运行与环境」承担依赖获取（`tempfile`，`Cargo.toml:27`）、运行命令、预期输出（19 passed）与隔离方式（每例独立 `tempdir`、可并行）。文档命令从零可跑，**不构成 FAIL**。
- 建议（非阻塞）：在「测试运行与环境」补一行环境前提「edition 2024 ⇒ Rust 1.85+；本机需已安装可用 toolchain」，避免无默认 toolchain 的环境直接执行失败。

## 8. 第三方可测标准

每个公开函数均有可复现输入/期望输出与 `:line` 锚；裁决门覆盖缺留痕/空留痕/同行/续行留痕/全自决/未分类/散文/无块/多标题九类边界；`path_refs` 给出路径形状与弃用规则；结构体给出字段语义与 `render` 形态。第三方可据文档直接写出与现有 19 例等价的测试。

## 结论

引擎审计（覆盖 17/17、lint 0 违规）采信通过；findings N/A、plan 无硬冲突、必填节/N-A 完整、实体与行号真实、内部与跨文档自洽、置信度诚实；文档命令在选定 toolchain 后从零实跑 19/19 通过。**PASS**。

**非阻塞建议（供汇编/后续轮参考）**：
1. `:218` 的 🟡 升级为 🟢（本轮已验证）。
2. 补 Rust toolchain 版本前提（edition 2024 → 1.85+）。
3. `:180`「失败再包一层 `Quality`」与 `:45`「读失败回空对」在本单元 facts/审计中无行号锚，建议补 `path:line` 或标 🟡，避免第三方按断言写断言。
4. T007b 正文与文档「四条机械检查」的措辞差异，建议在 plan 证据或文档中互相点明来源，减少汇编时误判。
