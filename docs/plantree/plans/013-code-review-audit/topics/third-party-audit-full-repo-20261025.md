# 第三方审计核实 — 全仓收口批次（2026-10-25 第五份）

> 外部第三方在补齐剩余未读面后做的**六视角全仓收口审查**（workspace / sop 全套 /
> change 全套 / runstate / integration / scheduler / change_engine / engine builtin /
> docgen lint·audit·impact / sop·doc runner·wiring）。9 条发现，本档为逐条源码核实记录。
> **判定：9/9 属实**（其中 2 条属已拍板设计口径的残余风险，1 条有实现内 TODO 自认）。
> 本档只记录，修复待用户拍板。
> 核实方式：逐条对源码（file:line 实读），不采信报告转述。

## 总评（第三方口径，与核实一致）

架构分层清楚，协议解析普遍走「保守判负 / 违例重跑」，确定性门（path:line 核验、
基线/回归 mtime、契约 diff、验收对账）密度高。真正要动的是两类：
**①失败路径的收尾缺口**（panic 与终态无人清扫 → 串行槽位/任务永久卡死）；
**②输入信任与口径分叉**（local_path 无校验、路径拼接无 store 校验、同名枚举分叉）。

## 发现清单与逐条判定

| # | 严重度 | 判定 | 源码证据（实读核实） |
|---|---|---|---|
| 1 doc 流水线 panic 卡死槽位 | major | ✅ 属实 | src/sop/doc/runner.rs:247 `panic!("PASS 到达但单元…无写前快照")`（本轮 #3 修复新加）；调用链 handlers.rs:339 / scheduler.rs:308 `std::thread::spawn(move \|\| run_in_background(...))` → src/server/mod.rs:270 `engine.execute(...)` **无 catch_unwind**；panic 展开后 mod.rs:291-294 `store.finish_run(...)` 不执行 → run 恒为 Running；准入 srс/domain/store.rs:295 `running_run_count() >= max_concurrent_runs` 永久拒绝；tombstone.rs:48-57 panic hook 只写墓碑**不回写 store**。串行槽位与任务状态只能靠重启进程解卡 |
| 2 NeedsRebase/NeedsRepair 终态停滞 | major | ✅ 属实（链路未接线，接线前必修） | src/integration/mod.rs:95 `next_pending` 只挑 Pending；:246 CAS 失败置 NeedsRebase、:185/:209 置 NeedsRepair，**全文件无回到 Pending 的代码**；src/server/scheduler.rs:151「已在队（任何状态）→ continue」不再入队。注：`integrate_next` 目前只有测试调用方（生产链未接线，见 handlers.rs:894 注释），故当前是**待爆的坑**而非现行事故 |
| 3 audit_report 路径无 store 校验 | minor | ✅ 属实 | src/server/handlers.rs:172-176 直接 `plans_root.join(&project).join(&task).join("audit-report.json")` 读盘，未确认项目/任务存在；:178-181 404 文案回显**绝对路径**（信息暴露面）。Path 参数含 `..` 段时可越出 plans_root |
| 4 两个同名 DocLevel 语义分叉 | minor | ✅ 属实 | src/docgen/audit.rs:14 `Essential/Complete/Detailed`（默认 Complete）vs src/change/gate0.rs:24 `None/Essential/Complete`（默认 None）——同名类型、不同值域，跨模块传递无编译期区分 |
| 5 注册项目不校验 local_path | major | ✅ 属实（设计口径内的高代价残余风险） | src/server/handlers.rs:103 `Project::new(req.name, req.local_path…)` 原样采用；src/domain/project.rs:29-35 `create_dir_all`；src/change/gate0.rs:105 无 `.git` 即 `git init`、:115 `git add -A` 后建基线提交——路径误填为已有目录（如 home）即把其全部内容纳入一次基线提交。**注**：local_path 全手动指定是 P004 decisions/001 拍板的设计，本条不推翻设计，只指出缺「空目录或已是 git 仓」的前置校验 |
| 6 token 非常量时间比较 + 回环免鉴权 | minor | ✅ 属实（回环免鉴权是 DR-9 L5 拍板） | src/server/handlers.rs:82 `got != Some(expected.as_str())` 字符串比较（计时侧信道）；src/main.rs:262-267 只在「非回环 且 无 token」时拒绝启动——回环默认无鉴权是 DR-9 L5 的有意设计 |
| 7 Test Review FAIL 空 issues 回注空清单 | minor | ✅ 属实 | src/sop/testing.rs:254 只累加 `parsed.issues.len()`，:279-286 空列表照样拼回注消息（只剩前缀）；src/sop/review_gate.rs:118 只拦空文本。对比 change 侧 src/change/session.rs:174 `MAX_EMPTY_ISSUES_NUDGES=3` + :519-530 空箱拒收（超限 Quality）——**两条测试链路口径不一致** |
| 8 缺口标记检测不看代码围栏 | nit | ✅ 属实 | src/docgen/lint/checks.rs:41-45 只排除「本行以 ``` 开头」，未维护 in_fence 状态——围栏内演示用的 🔴 会进 gap_hits 并被要求 gaps.md 登记（假阳性噪声） |
| 9 集成产物目录恒 _global | minor | ✅ 属实（实现内 TODO 自认） | src/integration/mod.rs:293-296 `fn artifact_dir_name() -> "_global"`，注释明写「M3 先用 flat + task_id 前缀；M4 调度器接线时按项目分目录收紧」；队列本体按项目落盘（:75）——多项目冲突/验证产物混放 |

## 修复优先级建议（第三方建议，待用户拍板）

报告给出的顺序，核实后认同：

1. **run 线程 panic 兜底**（#1）——最高。不止修 runner.rs:247 那处 panic：`run_in_background`
   外层补 `catch_unwind`，panic 分支统一走 Failed 收尾 + 清 runs_control + emit 事件；
   同时把「PASS 无快照」从 `panic!` 降级为 `Err(Quality)`（阶段中止而非进程级爆炸）。
2. **失败路径终态清扫**（#2）——合流/验收链接线（M4）之前必修，否则接线即静默停摆。
3. **local_path 注册校验**（#5）——「空目录 或 已是 git 仓」之外显式报错；不推翻手动指定设计。
4. **路径与 token 加固**（#3 + #6）——先 store 校验再读盘、404 不吐绝对路径；token 常量时间比较。
5. **枚举去重**（#4）——两个 DocLevel 合一（caller 显式区分「不碰文档」与「档位」）。
6. #7 / #8 / #9 随测试链路统一与 docgen 打磨节奏处理。

## 与前四份的关系

- 连续第六份外部报告 **0 误报**（9/9 实锤）。四批 + 本批累计 62 条发现 62 条实锤。
- #1 是本轮我们自己引入的新 panic（doc-from-zero #3 修复快照守卫时用的 `panic!`）——
  防的是「名单写错」，代价却是可让整个串行槽位卡死：**守卫该用 Err 不该用 panic**。
- #2（终态无回转）与 P013 audit 批次 #8「静默进度重置」同族：状态机缺「失败态如何复位」的出口。
- #7 是「同一纪律在两条链路上只落了一条」的第三次复发（前有 changed_files 过滤、freshness 门）。
