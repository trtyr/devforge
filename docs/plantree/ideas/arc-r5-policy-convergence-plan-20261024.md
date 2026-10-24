# ARC-R5 收敛方案 · 横切策略单点化（2026-10-24，动刀前拍板稿）

> 目标：重试/干预映射/事件字段三处横切策略收敛为单点模块，接口语义不变。
> 执行人：操作员（底座手术期延续）。**本方案涉及接口设计，过用户确认门后才动代码。**

## 现状地图（R4 收官后实测）

### 病灶：同一形状写两遍 + 策略住错家

| 策略 | 现居所 | 问题 |
| --- | --- | --- |
| 重试策略本体 | `engine/retry.rs`（run_with_retry/RetryPolicy/is_infra_failure/log_attempt_failed） | 本体已单点 ✓ |
| 重试**编排** | `server/mod.rs` run_in_background:256 的闭包（attempt 落盘 + run.retry 事件 emit + engine.execute） | server 层装着编排策略——组装层不该有策略 |
| 干预映射 | `engine/builtin.rs:459` `BuiltinRpcEngine::map_intervention`（方法形态） | 横切策略住在具体引擎类型上；六处调用（planning/coding/testing/docfix/doc-wiring/change_engine） |
| 事件字段同构 | `retry.rs:127` log_attempt_failed（重试层落点①）+ `builtin.rs:470` map_intervention Quality 分支（SOP 层落点②） | **同一 WARN 形状（component/event/attempt/error/error_chain/retryable）写了两遍**——改一处漏一处的经典漂移源 |

## 收敛方案

**新单点模块 `src/engine/policy.rs`**（retry.rs 就地改名扩展，不新建平行文件）：

1. **平移进 policy.rs**（现有单点保持）：
   - RetryPolicy / run_with_retry / is_infra_failure / log_attempt_failed（自 retry.rs，模块改名）
2. **平移进 policy.rs**（从别处收编）：
   - `map_intervention`：自 builtin.rs 挪出，**方法 → 自由函数**
     `pub fn map_intervention(control: &RunControl, component: &str, e: DevforgeError) -> Result<RunOutcome>`
     （不读 self——它本来就不该是引擎的方法；质量 WARN 分支改调单点形状函数）
   - `log_attempt_failed` 保持为形状单点，map_intervention 的 Quality 分支**改调它**
     （两处同构 → 一处构造，字段漂移从机制上不可能）
   - `run_attempt_bookkeeping`（新抽，自 run_in_background 闭包体平移）：
     attempt 落盘（StateStore.rs.attempt）+ run.retry 事件 emit——server 只传参调用
3. **调用面收正**：
   - builtin.rs 五处 + change_engine.rs:240 + doc_wiring.rs:421：`self.map_intervention(...)` →
     `crate::engine::policy::map_intervention(...)`
   - server/mod.rs run_in_background：retry 闭包体的 bookkeeping 两段 → `policy::run_attempt_bookkeeping(...)`
   - 测试面：server/tests.rs（map_intervention 直调三处）、builtin_tests（两处）、retry.rs 内测（模块路径）
4. **engine/mod.rs**：`pub mod retry;` → `pub mod policy;`（模块名消亡，引用面收正
   `crate::engine::retry::` → `crate::engine::policy::`；对外 crate 级路径本就没有 re-export）

## 语义不变的边界

- map_intervention 的三个干预判定（cancel/stalled/timed_out）、Quality 分支判定、
  错误文本**一字不改**（f6/builtin_tests 的文本断言继续过）；
- run.attempt_failed 的字段集与两条落点①②不变（f6:967/1027 两处 capture_jsonl 断言继续过）；
- run_with_retry 的退避档位/次数上限/is_infra_failure 判据不变。

## 实施批次（每批独立 commit + cargo test 绿 + clippy 零新增）

- 批 1：retry.rs → policy.rs 改名 + map_intervention 平移（含形状单点化：Quality 分支改调
  log_attempt_failed）+ 调用面六处收正
- 批 2：run_attempt_bookkeeping 抽取 + run_in_background 瘦身
- 收官：全量验证 + 7889 重启验收 + ARC 台账 R5 回写（R1..R5 全 Done 收官声明）

## 验收口径

- 全量 7 靶 exit 0；clippy 与 R4 收官基线一致零新增；
- grep 验证：`map_intervention` 只出现在 policy.rs（定义）+ 调用点（无第二份实现）；
  `run.attempt_failed` 字段构造只在 log_attempt_failed 一处；
- 7889 resume 推进证据；ARC 台账 R5 Done + R1..R5 收官声明。

## R5 收官回写（2026-10-24，状态 Done，操作员执行）

| commit | 内容 |
| --- | --- |
| 566dd63 | task-1：收敛方案落档（本文件，用户确认门通过后开工） |
| 158b64f | 批 1：retry.rs→policy.rs 单点 + map_intervention 自由函数 + 形状单点化 |
| 0dad282 | 批 1 守卫适配：c7-1 计划 56 处路径指针 + 三副本 + 归一化表加条目 |
| 5f21fdf | 批 2：run_attempt_bookkeeping 抽取（emit 回调注入避免反向依赖） |
| 184abb8 | 批 2 卫生：测试废绑清除（clippy 基线回归一致） |

终态：横切策略单点 engine/policy.rs（468 行：RetryPolicy/run_with_retry/is_infra_failure/
log_attempt_failed_shape/map_intervention/run_attempt_bookkeeping）；grep 验证三类策略
零散落副本；全量 7 靶绿 exit 0；clippy 与 R4 基线一致零新增；验收 pid 3156003 探活 200、
acc2 resume 后 doc-agent 活跃（accept-r5.log 10:09 时段）。

### 流程教训入账

批 1 的验证与 commit 在同一 bg 命令里用 `;` 串联，测试未绿 commit 照进（158b64f 带
未验证状态入历史）——修复后追加 0dad282 入账。后续批次的验证与 commit 必须分开回合，
验证绿是 commit 的前置。
