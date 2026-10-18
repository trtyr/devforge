# P007 Roadmap（实施中）

> 状态：**代码+单测轮完成（T001-T007 全勾，2026-10-18）**；T008 e2e 留用户放行后的独立验证轮。决策全文见 decisions/README.md（D001-D012）。
> 开工门槛已满足：coding 阶段（P005+006）收官且 v4 验证全绿（209 PASS / 0 FAIL）。

## 前置

- [x] P005 coding SOP 落地（db262bf）+ 006 硬化（04b5d5b）
- [x] P006 v4 e2e campaign 收官 + TEST_REPORT v4（58a5703，209P/0F）
- [x] 方案确认门：用户过目 P007 定稿计划（2026-10-18 三项执行决策拍板）

## 任务分解

- [x] T001 清单与渲染扩展：四新 profile（test-agent / test-review / doc-fix / doc-review）+ 四份提示词（黑盒纪律硬禁令+白名单 / 对文档三判据+reroute 标注 / 文档模式剧本 / 一致性审查剧本）+ 供给按 D007 —— commit `9135772`
- [x] T002 黑板 `testing/` 布局与协议：stages.md / todo.md / **findings/**（一条一文件+状态机，D009）/ coverage-matrix.md（D007·Q007）/ reports/（review-N+运行报告）/ blockers/（沿用偏差协议） —— commit `7b4562f`
- [x] T003 testing_protocol.rs：`<stage-complete>` 复用（re-export 单一实现）+ `<reroute>` 解析（保守：缺失/非法→test-side，D003）+ TestReviewReport/DocReviewReport 协议 + findings 文件协议 —— commit `887291b`
- [x] T004 TestingSopLoop 状态机：kickoff → 通读文档自划分 N 阶段 → 写测/自起自跑/落报告 → stage-complete → fresh test-review → reroute 路由（test-side 退回 / doc-side 转 doc-fix）→ Finish（D005 完成定义）；账本跨移交累计 —— commit `0f350c3`
- [x] T005 doc-fix 子 SOP：DOC_FIX_SOP 注册（planning/coding/doc-fix）+ DocFixSopLoop（单轮工作→doc-review 内置审查→PASS 出口/FAIL 回注零上限）+ kickoff FINDINGS_DIR 常量化 —— commit `f4e32ce`
- [x] T006 引擎接线：BuiltinRpcEngine 分发 testing/doc-fix + run_testing（doc-side 闭环编排：触发 doc-fix→过审→resume 同阶段重审，账本跨移交累计）+ run_doc_fix（独立触发）+ 监视目录（testing/testing-reviews/doc-fix/doc-reviews）+ 无 codegraph 预检（D007） —— commit `94f6a9c`
- [x] T007 全量回归：bg_run 原始日志验真 cargo test **174 passed / 0 failed** + doctest 1 + clippy --all-targets **0 警告**（TEST_EXIT=0 / CLIPPY_EXIT=0，/tmp/p007-t007-regression.log）
- [ ] T008 e2e 系列（test_17+：配置面/触发链/全链黑盒测试闭环）——**用户放行后的独立验证轮执行**（沿 P005 T008 先例）

## 里程碑口径

- M1（T001-T003）：供给与协议层落地，单测覆盖解析与清单 ✅
- M2（T004-T006）：三个 SOP 状态机 + 引擎接线，mock 测试全绿 ✅
- M3（T007）：全量回归绿 ✅（174+1 全绿 0 警告）；testing 链路具备 e2e 条件，等用户放行 T008

## 实施记录

- 单测增长：139（开工时）→ 174（T006 后）+ doctest 1，每任务独立验真、clippy 每步 0 警告
- 实施偏差记录：无——D001-D012 全部按定稿落地，未触发偏离协议
- 备注：coverage-matrix 校验器未在本轮单独实现（矩阵为 Agent 维护+Review 审计的活表格，结构校验随 T008 e2e 取证；协议层锚点已入提示词）
