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
- [ ] T008 e2e 系列（test_17+：配置面/触发链/全链黑盒测试闭环）——**验证轮完成（2026-10-19 04:31，见 `tests/e2e/TEST_REPORT.md` v5）**：
  - [x] 前置：`TESTING_SOP` 注册缺口修复（commit `54e734f`）
  - [x] test_17 配置面 **51P/0F**、test_18 触发链 **15P/0F**
  - [x] **test_20 测试阶段本体 fixture 路线 21P/0F**（650s 三阶段全绿：stages/矩阵/2 条 doc-side 发现/tests 分层/commit 锚/黑盒纪律）
  - [x] C19-R1/R2 审查路由与静默退出修复（commit `7e7200a`，单测 178 全绿）
  - [ ] test_19 三段全链：编码段 3h 窗口两次用尽（环境慢），全链见证留待下一轮（建议 ≥6h 窗口或 fixture 化）
  - [x] **test_21 编码→测试交接 18P/1F**（编码 44min Settled→交接 202→测试段 4 阶段/6 轮审查/4 commit/20 测试文件；仅 3600s 窗口不够）
  - [x] 既有 18 脚本校准+重跑（250P/0F；test_14 沿用 v4 结果）
- [x] **窗口口径结论（写进 TEST_REPORT v5）**：coding ≥10800s、testing ≥10800s、全供给渲染 ≥1800s
- [x] **遗留收尾（2026-10-19 10:40-12:00，goal `mvenh4xy-bga86x`，6/6）**：
  - [x] CAL-19-RENDER 装包提速：`render_all` 串行→有界并发（4 worker，返回值保持清单顺序、失败按序上抛）；渲染幂等（已装齐 + 双记录完整 → 跳过 `pi install`，清单变更/记录缺失 → 重装）；单测 +5，**183 过 0 挂 clippy 0**（commit `c692851`）；**真装包实测**（同机同网络）：首次全量八 profile **65.4s**，同清单二次复用 **0.1s**（旧串行基线 >20min）
  - [x] OBS-TEST-VENV 依赖获取口径：TEST_AGENT 增「隔离环境装（.venv）/不得改实现与运行依赖/进报告/文档没写就记 doc-side」；DOC_FIX+DOC_REVIEW 七件套补「测试依赖获取与隔离方式」；CODING_REVIEW 补「照文档从零 boot」门禁+七件套全文（顺带补齐 P007 设计已定、提示词里却缺的尾巴）；test_17 锚点 +4 → **55P/0F**（commit `b959478`）
  - [x] 窗口预算落地：test_21 两段各 10800s、run_all 预算 25200s，TEST_REPORT 口径同步（同上 commit）
  - [x] 测试侧竞态封口：`common.start_tolerant()`（终态后重跑遇 409 重试 ≤20s），test_07/12/18 落地；**17P/11P/15P 全绿**（commit `1b596a4`）
- [ ] 待办（下一轮）：test_19 三段全链见证（≥6h 窗口或 fixture 化）——唯一未闭合项

## 里程碑口径

- M1（T001-T003）：供给与协议层落地，单测覆盖解析与清单 ✅
- M2（T004-T006）：三个 SOP 状态机 + 引擎接线，mock 测试全绿 ✅
- M3（T007）：全量回归绿 ✅（174+1 全绿 0 警告）；testing 链路具备 e2e 条件，等用户放行 T008

## 实施记录

- 单测增长：139（开工时）→ 174（T006 后）→ 178（C19 修复）→ **183（渲染提速/幂等）** + doctest 1，每任务独立验真、clippy 每步 0 警告
- 实施偏差记录：无——D001-D012 全部按定稿落地，未触发偏离协议
- 备注：coverage-matrix 校验器未在本轮单独实现（矩阵为 Agent 维护+Review 审计的活表格，结构校验随 T008 e2e 取证；协议层锚点已入提示词）
