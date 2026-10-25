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

## 核实总评

- 连续第四份外部报告 0 误报（10/10 实锤）。#1/#3 是本轮 P013/P014 修复时引入或
  遗留的盲区（changed_files 过滤清单是我们自己写的，漏了 docs/contexts/）——
  外部视角对「修复者盲区」的增益再次被证实。
- #5 与 #1 同属「route 语义」层：route 现在同时承担「边界申报」与「阶段回退」
  两个职责，两者的门禁交互（早退跳门、无界回退、issues 丢失）是系统性缺口，
  修复宜整体设计而非逐条打补丁。
