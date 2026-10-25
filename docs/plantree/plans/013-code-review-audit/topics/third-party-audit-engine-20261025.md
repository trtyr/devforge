# 第三方审计核实 — src/engine/ 批次（2026-10-25 第二份）

> 外部第三方对 RPC 编排引擎（src/engine/ 七文件）的 10 条发现，本档为逐条源码核实记录。
> **判定：9/10 属实，1/10 已修复**（#1 在本轮 audit 修复线 24fd918 已落地——报告审查
> 基线早于该 commit，其证据引用的 builtin.rs:75-88 裸调形态在当前 HEAD 已不存在）。
> 本档只记录，修复待用户拍板。核实方式：逐条对源码（file:line 实读）。

## 发现清单与逐条判定

### major（5 条）

| # | 判定 | 源码证据（实读核实） |
|---|---|---|
| 1 code-review 控制面旁路 | ✅ **已修复**（24fd918） | 当前 builtin.rs:75-111 = require_profile 门卫（77-79）+ run_with_watchdog（93-97，watch_dirs=session_root/code-review）+ control 传 run_audit（102）+ map_intervention（109）。报告引用的 75-88 裸调形态已不存在；audit 修复线回归测试 cancelled_control_aborts_audit_before_any_session 在档 |
| 2 缺 API key 被当基础设施故障重启 | ✅ 属实 | client.rs:331 `DevforgeError::System` + :332 注释自认「多半是 profile 配置问题：缺 API key / 模型名错误」——配置类失败标 System → is_infra_failure 命中 → 4 次尝试 + 10s/30s/90s 退避，与 require_profile 的 Config 口径打穿 |
| 3 重试判定与死亡分类不一致 | ✅ 属实 | policy.rs:61 `matches!(Err(DevforgeError::System(_)))` 只认裸 System：spawn .ctx() → WithContext(Io) 不重试（client.rs:63）、流提前结束 → RpcProtocol 不重试（client.rs:159）、try_wait 子进程退出 → System 重试（client.rs:165）——「子进程没了」两种表象只有后者重启；注释（:67「基础设施故障」）比实现宽 |
| 4 台账 save 失败被吞 | ✅ 属实 | client.rs:85 `let _ = ledger.save(&ledger_path);`；ledger.rs:109-111 load 失败返回 0——熔断 SIGKILL 与孤儿对账依赖的事实源写失败/读失败均无痕，「先落盘再打勾」铁律在此失效 |
| 5 profiles 单测漏 code-review | ✅ 属实 | builtin_tests.rs:287-318 表驱动断言 planning/coding/testing/doc-from-zero/doc-fix/change + unknown——无 code-review；删掉 builtin.rs:110 表项测试仍绿（门卫与分发可再漂移） |

### minor（5 条）

| # | 判定 | 源码证据 |
|---|---|---|
| 6 损坏台账当空账 | ✅ 属实 | client.rs:78 `SpawnLedger::load(&ledger_path).unwrap_or_default()`——历史 PID 丢失，新 spawn 覆写成空账（注意：与 store.json 持久化的 .bak 口径不一致） |
| 7 kill 后 prompt 会 panic | ✅ 属实 | client.rs:197 `.expect("stdin 仅在 kill 后不可用")`——可失败路径 expect |
| 8 解码错误带整行 JSONL | ✅ 属实 | protocol.rs:99 `.ctx(format!("解析 RPC 记录失败: {line}"))`、:103 同——事件内凭据可能进日志 |
| 9 两套杀进程 | ✅ 属实 | ledger.rs:108 `kill_run_processes`（台账精确命中）与 :153 `scan_and_kill_run`（扫 /proc 按标记）并存，路径未收敛 |
| 10 codegraph init 无超时且丢输出 | ✅ 属实 | builtin.rs:34-35 stdout/stderr 全 null、:36 `.status()` 无超时——init 挂死只有最终 warn，不可诊断 |

## 修复优先级（第三方建议，#1 已落地，其余待用户拍板）

| 优先级 | 项 | 状态 |
|---|---|---|
| P0 | #1 code-review 接门卫 + watchdog + map_intervention | ✅ 已修复（24fd918） |
| P0 | #2 command_failure 配置类用 Config | 待拍板 |
| P1 | #3 统一死亡分类与 is_infra_failure 剥上下文 | 待拍板 |
| P1 | #4 台账 save 失败可见 | 待拍板 |
| P2 | #5 单测锁 code-review 表项 | 待拍板 |
| P3 | #6-#10 | 待拍板 |

建议回归（报告原文）：缺 code-review profile 时 execute 立刻 Config 且不建黑板；缺 API
key 的 success:false 响应 1 次 Failed、0 次重试；spawn IO 失败按文档决定重试与否并有对应
测试；台账目录只读时 spawn 失败或 ERROR 日志含路径。

## 修复状态表（2026-10-25 第二轮，全部落地）

> 9 条属实项全部修复（#1 上轮已落地），按审计报告建议优先级推进；终验 789 绿 0 failed，
> clippy 无新增（双 EXIT 原始日志 bg_azchpbhp：TEST_EXIT=0 / CLIPPY_EXIT=0，20 条与修复前持平）。

| # | 修复 commit | 落地内容 |
|---|---|---|
| 1 | 24fd918（上一轮） | code-review 控制面（门卫/watchdog/map_intervention/RunControl）——本报告审查基线早于该 commit |
| 2 | 5e2d2ff | command_failure 配置类改 Config + map_intervention Config 分支（WARN+明确 Failed 不重启）+ 重试层分级扩展（配置类 WARN「（配置类，不重启）」）；回归：1 次尝试 0 重试 |
| 3 | 52b0e40 | DevforgeError::root_cause 剥包装；is_infra_failure 重写（根因 System\|Io 可重启，决策注释即文档）；client 流 EOF 从 RpcProtocol 改 System（子进程死亡两种表象统一）；f6 断言更新新口径；分类矩阵 8 例回归 |
| 4 | c6a3142 | spawn 台账 save 失败 ERROR 带路径（事实源缺口可见，不回滚的决策注释即文档）；kill_run_processes 不可读 ERROR 留痕；回归不可读强杀返 0 |
| 6 | c6a3142 | SpawnLedger::load_for_record：损坏 WARN + .bak 备份 + 空账起底（替换 unwrap_or_default）；回归损坏备份与恢复写入 |
| 5 | 245d91a | profiles_for_sop 测试锁 code-review 表项（删表项走空切片的门卫放行口子关死） |
| 7 | 245d91a | client 三处可失败路径 expect 改 RpcProtocol（kill 后发 prompt/UI 应答不 panic） |
| 8 | 245d91a | decode_line 错误只露 type/id+len 摘要（summarize_line，凭据不入日志） |
| 10 | 245d91a | preflight_codegraph spawn+截止轮询（10s 超时强杀）+ stderr piped 伴读落 tracing |
| 9 | 789e43b | 统一杀进程入口 kill_run(ledger_path: Option<&Path>)：台账双验证优先，缺失降级扫盘+WARN；scheduler 熔断与 watchdog cancel/失活两处全换接；回归三断言（命中击杀/缺失降级/无路径扫盘） |

新增回归测试共 9 条：config_failures_not_retried、infra_failure_classification_matrix（8 例）、
corrupt_ledger_backed_up、kill_run_processes_unreadable_returns_zero、kill_run_hits_ledger_first、
profiles code-review 断言（并入既有表驱动测试）；f6 runlog 两断言更新到新口径。

## 核实总评

- 连续第三份外部报告保持高精度：10 条中 9 条属实 0 误报，1 条（#1）是基线时差而非误报
  ——审查对象快照早于 24fd918。
- #2/#3 合起来是同一个根因的两面：「什么算基础设施故障」没有单一事实源——command_failure
  把配置塞进 System、is_infra_failure 只认裸 System、spawn 的 .ctx() 又把 Io 包进
  WithContext。修的时候应一次收口（错误分类单点），别再各修各的。
- #4 与 M3 铁律（先落盘再打勾）直接冲突，且 P013 修复线 task-7 刚把 spawn-ledger 建成
  熔断事实源——写失败被吞等于事实源不可信。
