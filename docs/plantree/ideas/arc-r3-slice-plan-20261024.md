# ARC-R3 切片方案 · sop/doc/ 垂直同居（2026-10-24，动刀前定稿稿）

> 目标：doc 域从「散件 + 一坨 1477 行」变为 sop/doc/ 目录同居（runner/协议/断点/增量/修补/提示词/接线/测试），全部 ≤500 行，纯搬移零行为变更。
> 执行人：操作员（底座手术期延续）。分批 commit，每步 cargo test 绿。

## 现状（R2 收官后）

| 文件 | 行数 | 病 |
| --- | --- | --- |
| src/sop/doc_from_zero.rs | **1477** | runner（spawn_review_worker + run_pipeline ~430）与断点辅助（~110）、证据（~60）、提示词（~60）、DocReport（~12）、mock 簇（~270）、24 个测试（~560）混居单文件 |
| src/sop/doc_protocol.rs | 215 | 散件（doc 域，该同居） |
| src/sop/doc_resume.rs | 243 | 散件 |
| src/sop/doc_incremental.rs | 395 | 散件 |
| src/sop/doc_fix.rs | 369 | 散件（doc-fix SOP，doc-review 配套） |
| src/engine/doc_wiring.rs | 423 | R2 中转站（split-plan 预告：R3 迁 sop/doc/） |

## 目标布局（sop/doc/，13 文件全 ≤500）

| 文件 | 内容 | 来源 | 预估 |
| --- | --- | --- | --- |
| sop/doc/mod.rs | 模块声明 + 对外 re-export | 新写 | ~30 |
| sop/doc/state.rs | 断点辅助（data_obj/done_units/mark_done/review_rounds/…） | doc_from_zero 39-112 | ~110 |
| sop/doc/evidence.rs | snapshot_files/walk/produced_files | doc_from_zero 124-167 | ~60 |
| sop/doc/prompts.rs | DOC_REMEDIATION_PREFIX + doc_kickoff/doc_review_instruction/doc_final_instruction（R4 再外置 assets） | doc_from_zero 25-32+168-207 | ~60 |
| sop/doc/runner.rs | DocReport + spawn_review_worker + run_pipeline | doc_from_zero 209-651 | ~440 |
| sop/doc/mocks.rs | 测试 mock 簇（NoUser/MockAgent/PipeAgent/PipeReviewFactory/PipeReviewSession/drive_gates/gate_pair/no_gates/pipe_factory/state_path/seed_done） | doc_from_zero 652-734+838-947 | ~280 |
| sop/doc/runner_tests.rs | pipeline 行为测试（重叠审查/fix 轮/断路器/里程碑/审计注入/协议重试…） | doc_from_zero 测试 | ~450 |
| sop/doc/resume_tests.rs | 断点/证据/续跑测试（checkpoint 黑板/证据归因/legacy 目录…） | doc_from_zero 测试 | ~250 |
| sop/doc/wiring.rs | ← engine/doc_wiring.rs 整迁（task-3） | doc_wiring.rs | 423 |
| sop/doc/protocol.rs | ← doc_protocol.rs 整迁 | | 215 |
| sop/doc/resume.rs | ← doc_resume.rs 整迁 | | 243 |
| sop/doc/incremental.rs | ← doc_incremental.rs 整迁 | | 395 |
| sop/doc/fix.rs | ← doc_fix.rs 整迁 | | 369 |

## 关键边界声明

1. **模块名消亡**：`doc_from_zero` 模块名不复存在（不留别名/转发）——对外路径
   `crate::sop::doc_from_zero::*` 收正为 `crate::sop::doc::*`；`src/sop/mod.rs`
   的 re-export（`DocReport`/`DOCGEN_DOCS_DIR`/`DOC_REMEDIATION_PREFIX`）改指新路径，
   对外 crate 级路径 `crate::sop::DocReport` 保持不变。
2. **纯搬移纪律**：零行为变更、零签名变更；可见性按需最小调整（跨文件引用的
   `pub(crate)`/`pub(super)`），与 R2 同口径；每个职责一个 commit。
3. **迁移顺序**：先整文件 git mv（protocol/resume/incremental/fix——低风险落位），
   再拆 doc_from_zero（state/evidence/prompts → runner → 测试三件套），
   最后 wiring 迁入 + 引用面统一收正。
4. **不动**：rpc_bridge/planning/coding/testing/review_gate/output（各自 SOP 域，
   本期只切 doc）；AF 批 A（协议/提示词面）另行派工。
5. **acc2 不受扰**：7889 运行中实例不动（inode 替换语义），task-5 验收才重启。

## 引用面清单（收正点全查）

- `src/sop/mod.rs`：模块声明 + re-export
- `src/engine/doc_wiring.rs`：DOCS_DIR/META_DIR/run_pipeline/doc_final_instruction/
  doc_resume::DOC_SOP/doc_incremental/docgen 审计调用
- `src/server/mod.rs`：run_phase_for_sop 字符串无涉（"doc-from-zero" SOP id 不变）✓
- `tests/f6_observability.rs`：心跳守卫扫描面（run_doc_from_zero 指 doc_wiring.rs）+
  quality_failure_sites 守卫（终审文案扫描指 doc_wiring.rs）→ 跟迁 wiring.rs
- `src/server/tests.rs`：`doc-from-zero` StateKey 字符串无涉 ✓
- `DOC_SOP` 常量（doc_resume 内）引用面：doc_wiring/retired 注释
