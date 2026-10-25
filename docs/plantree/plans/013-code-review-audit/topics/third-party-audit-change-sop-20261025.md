# 第三方审计核实 — change-sop 批次（2026-10-25 第三份）

> 外部第三方对变更 SOP（src/change/** + src/server/change_engine.rs）的 10 条发现，
> 本档为逐条源码核实记录。**判定：10/10 属实**。本档只记录，修复待用户拍板。
> 核实方式：逐条对源码（file:line 实读）。

## 发现清单与逐条判定

### major（6 条，全部属实）

| # | 判定 | 源码证据（实读核实） |
|---|---|---|
| 1 route-plan 早退跳三门 | ✅ 属实 | session.rs:423-431 `<route>plan</route>` → `return CodingTurn { reviews: 0 }`——self-check/step-commits/coding review 全部在 return 之后（:433 marker nudge、:438 self_check）；boundary.rs:149-151 `extra.is_empty()` 直接返回 InBoundary **不看 reply 的 route**——白名单内改动 + route token = 带改动直通 Testing，三个机械门与审查全跳 |
| 2 白名单要求文件已存在 | ✅ 属实 | plan_audit.rs:309-316 `!repo.join(f).exists()` → issue「白名单里的文件不存在」——新增文件在 plan 时必然不存在，必然注入 deterministic issue；测试靠先建文件绕过（「新增文件豁免」workaround），生产规划官被要求不改代码无法预先建文件 |
| 3 essential 文档污染 changed_files | ✅ 属实 | prepare.rs:57 `docs::apply` 在 :65 `ensure_baseline` **之前**——essential 档写 docs/contexts/_meta/* 先于基线；已有 git 仓 ensure_baseline 只记 HEAD 不提交 → docs 文件成 untracked；changed_files 过滤清单只有 docs/plantree/ 与 evidence/（不含 docs/contexts/）→ extras → OutOfBoundary FAIL。裸仓测试 git add -A 折进基线提交掩盖此路径 |
| 4 regression log 无 freshness | ✅ 属实 | testing_gate.rs:45-53 只读文件+解析退出码——无 mtime ≥ turn_started 校验（对比 coding_gate 的 check_baseline_log_fresh/check_self_check_fresh，reverify #6 只修了这两处）；上一轮残留绿 log 可直接满足 claims_green |
| 5 testing→plan 无上限且丢 issues | ✅ 属实 | orchestrator.rs:376-383 RoutePlan 分支只 `phase_change(Plan)` + save + Quality——对比 RouteCoding（:369 附近 persist issues + test_reroutes cap 3）：issues 不进 ChangeState、plan_fix_rounds/roundtrips 不动、无 cap；resume 走 run_plan 的 plan_kickoff 而非 fix_plan(issues)——Plan→Coding→Testing→Plan 可无限循环 |
| 6 testing 不改产品代码仅 prompt 约束 | ✅ 属实 | run_testing（session.rs:540-559）只有 regression log 形状 + 对账表 path:line + test review——无测试前后 HEAD/changed_files 快照对比；「不得改产品代码」只在 prompt（session.rs:134）与 review 项 4，机械面缺失 |

### minor（3 条）+ nit（1 条）

| # | 判定 | 源码证据 |
|---|---|---|
| 7 report diff_stats 吞失败 | ✅ 属实 | orchestrator.rs:388 `diff_stats(...).unwrap_or_default()`——git diff 失败写成 0 files/+0/-0；reverify #1 同类问题在报告路径残留 |
| 8 history.md 读失败静默截断 | ✅ 属实 | history.rs:28-29 `is_file()` 为真但 read 失败 → unwrap_or_default 空串 → header 初始化分支（只在 ！is_file）不走 → 旧往返历史被覆盖丢失 |
| 9 P009 README 漂移 | ✅ 属实 | README.md:67 「testing 回退 `<reroute>coding\|plan</reroute>`」——实现只认尾部 `<route>`；:68 「第 4 次判基础设施故障」——实现是 require_marker_nudged 3 次后 Quality（无自动重启） |
| 10 impl_steps_done 死状态 | ✅ 属实 | state.rs mark_step_done 仅测试调用；编排器活体步对账走 git log vs impl-steps.md；resume 测试断言字段 roundtrip 不证明 SOP 用它（外部复验报告同条已在案，双来源一致） |

## 修复优先级（第三方建议，待用户拍板）

报告未附优先级表；按核实后严重度建议：#1/#5（门禁绕过+无界循环）最重，
#2/#3（白名单可用性/必踩路径）次之，#4/#6（testing 面机械缺口）随后，
#7-#10 清扫。修复方向注意：#1 与 #5 都涉及 route 语义与状态持久化，宜一并设计；
#3 修复应连带审视 changed_files 过滤清单与基线建立顺序。

## 修复状态表（2026-10-25 第二轮，全部落地）

> 10/10 已修复，按 route 语义整体收口首发（#1+#5 同层整体设计不逐条打补丁）；
> 终验 797 绿 0 failed，clippy 无新增（双 EXIT 原始日志 bg_azchpbhp / 重跑确认 bg_7djt07e4：
> TEST_EXIT=0 / CLIPPY_EXIT=0，20 条与修复前持平；preflight 负载偶发一次，隔离 4/4 绿）。

| # | 修复 commit | 落地内容 |
|---|---|---|
| 1 | 7984c47 | boundary::classify 重排：route=plan 优先于 extra 判定——白名单内改动 + route token 走 RoutePlan 回规划（不再借「extra 空→InBoundary」直通 testing 跳三门）；有越界时申报覆盖校验保留；非 plan-route 维持原语义不算申报（首次重写漏掉该门槛被既有守卫 route_other_than_plan 逮住，已对齐） |
| 5 | 7984c47 | testing→plan 回退接入 plan 修复额度管线：issues 持久化进新字段 last_plan_issues（serde default 旧断点兼容）+ plan_fix_rounds 计数（MAX_PLAN_FIXES cap 生效）+ resume 后 plan 循环走 fix_plan(issues) 而非重新 kickoff——无界循环关死 |
| 2 | 3063a18 | 白名单存在性只对「要改的」强制：新增文件（不在「要改的」）plan 时必然存在不了，不再必然注入 issue；clean_plan 测试删除「先建文件」豁免 workaround（实现保证替代测试预设）；回归 modify_target_must_exist |
| 3 | be2ab4c | changed_files 过滤清单加 docs/contexts/——SOP 自产文档域（essential 档 _meta 盘点/词汇表/伴生文档同步）与 docs/plantree 同性质；已有 git 仓基线只记 HEAD，untracked 文档不再必踩范围膨胀 FAIL；回归 untracked 文档不进变更集 |
| 4 | cba2c08 | check_regression_log 加 not_before freshness（mtime 早于本轮开始 = 上一轮残留拒收，与 baseline/self-check 同口径，reverify #6 第三份证据补齐）；fixture 四处+support.rs 补刷 regression.log mtime |
| 6 | cba2c08 | snapshot_tracked_changes（tracked 相对基线改动集，untracked 噪音不纳入；git 不可用降级空集+WARN 不掩盖测试阶段真错误）+ check_no_product_code_edits（tests/docs/evidence 之外新增 tracked 改动 = Quality）——「不改产品代码」从 prompt 升级为机械门；回归两条 |
| 7 | 06c02f2 | diff_stats 失败 Err 上抛（报告不再吞成 no-op 统计）；编排测试环境升级真 git 仓（orchestrator_tests + change_failure_context 的 store() init_git+sha 落文件、cfg() 读 sha，20 处调用点零改动） |
| 8 | 06c02f2 | history.md 读失败 Err 上抛（宁可不写不静默清空既有往返轨迹） |
| 9 | 06c02f2 | P009 README 对齐实现：<route> 非 <reroute>；连续 3 次 nudge 后 Quality 不自动重启 |
| 10 | 06c02f2 | impl_steps_done 死字段删除（编排器从未写入，活体步对账走 git log vs impl-steps.md；mark_step_done 删除；旧断点文件的该键由 serde 忽略未知字段安全兼容，两处 fixture json 保留该键作兼容活验证） |

新增回归测试共 10+ 条：boundary route 优先两条、编排回退持久化/resume 两条、
modify_target 存在性、essential 文档过滤、regression freshness、产品代码编辑 Quality、
kill 前后 expect 消失（编译面）、clean_plan 无豁免；f6/协议 fixture mtime 全量适配。

## 核实总评

- 连续第四份外部报告 0 误报（10/10 实锤）。#1/#3 是本轮 P013/P014 修复时引入或
  遗留的盲区（changed_files 过滤清单是我们自己写的，漏了 docs/contexts/）——
  外部视角对「修复者盲区」的增益再次被证实。
- #5 与 #1 同属「route 语义」层：route 现在同时承担「边界申报」与「阶段回退」
  两个职责，两者的门禁交互（早退跳门、无界回退、issues 丢失）是系统性缺口，
  修复宜整体设计而非逐条打补丁。
