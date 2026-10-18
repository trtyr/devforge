# P007 Roadmap（定稿）

> 状态：**定稿（2026-10-18）**。Q001-Q009 全部关账 + 三项执行决策拍板（doc-review 新 profile / findings 台账 / 启动沿用现状），决策全文见 decisions/README.md（D001-D012）。
> 开工门槛已满足：coding 阶段（P005+006）收官且 v4 验证全绿（209 PASS / 0 FAIL）。

## 前置

- [x] P005 coding SOP 落地（db262bf）+ 006 硬化（04b5d5b）
- [x] P006 v4 e2e campaign 收官 + TEST_REPORT v4（58a5703，209P/0F）
- [x] 方案确认门：用户过目 P007 定稿计划（2026-10-18 三项执行决策拍板）

## 任务分解

- [ ] T001 清单与渲染扩展：四新 profile（test-agent / test-review / doc-fix / doc-review）+ 四份提示词（黑盒纪律硬禁令+白名单 / 对文档三判据+reroute 标注 / 文档模式剧本 / 一致性审查剧本）+ 供给按 D007
- [ ] T002 黑板 `testing/` 布局与协议：stages.md / todo.md / **findings/**（一条一文件+状态机，D009）/ coverage-matrix.md（D007·Q007）/ reports/（review-N+运行报告）/ blockers/（沿用偏差协议）
- [ ] T003 testing_protocol.rs：`<stage-complete>` 复用 + `<reroute>` 解析（保守：缺失/非法→test-side，D003）+ findings 文件协议 + coverage-matrix 校验
- [ ] T004 TestingSopLoop 状态机：kickoff → 通读文档自划分 N 阶段 → 写测/自起自跑/落报告 → stage-complete → fresh test-review → reroute 路由（test-side 退回 / doc-side 转 doc-fix）→ Finish（D005 完成定义）；run_with_watchdog 活性监视（D011）
- [ ] T005 doc-fix 子 SOP：DOC_FIX_SOP 注册 + 跨 SOP 触发 + kickoff 携带 findings → triage（改码=转 coding / 改档=补文档）→ doc-review 内置审查轮 → 流回 testing（D004）
- [ ] T006 引擎接线：BuiltinRpcEngine 分发 testing/doc-fix + preflight 复用 + session_root 监视目录（testing/test-reviews）+ doc-fix 审查工厂（per_round_in doc-review）
- [ ] T007 全量单测 + clippy 0 警告（编码阶段纪律：**不写 e2e**）
- [ ] T008 e2e 系列（test_17+：配置面/触发链/全链黑盒测试闭环）——**用户放行后的独立验证轮执行**（沿 P005 T008 先例）

## 里程碑口径

- M1（T001-T003）：供给与协议层落地，单测覆盖解析与清单
- M2（T004-T006）：三个 SOP 状态机 + 引擎接线，mock 测试全绿
- M3（T007）：全量回归绿；此时 testing 链路具备 e2e 条件，等用户放行 T008
