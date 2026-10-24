# ARC-R2 拆解方案 · server/mod.rs 职责分离（2026-10-24，动刀前过目稿）

> 目标：2376 行上帝文件 → 职责单一文件（各 ≤500 行），纯搬移零行为变更。
> 执行人：操作员（底座手术期）。每个职责文件独立 commit（可回滚）。

## 现状职责地图（server/mod.rs 2377 行）

| 行号 | 职责 | 体量 |
| --- | --- | --- |
| 46-107 | EventBus（事件总线） | ~62 |
| 109-190 | UiRelay + RelayUserLink（UI 交互中继） | ~85 |
| 192-218 | preflight_codegraph（预检） | ~27 |
| 219-1075 | **BuiltinRpcEngine**：execute dispatch + profiles_for_sop（唯一事实源表）+ require_profile + run_planning/coding/doc_from_zero/testing/doc_fix（五段 SOP 接线闭包，doc 最重 ~400 行） | ~856 |
| 1076-1152 | RenderGate（渲染闸门，与 profile 门卫同族） | ~77 |
| 1153-1228 | ServerState + SharedState | ~76 |
| 1229-1302 | token 启动令牌（parse/resolve + tests） | ~74 |
| 1303-1359 | build_router（路由组装） | ~57 |
| 1360-1487 | run_in_background + run_phase_for_sop + EngineBox（run 编排） | ~130 |
| 1488-1646 | session_fingerprint + run_with_watchdog（失活收割） | ~160 |
| 1647-1811 | watchdog_tests | ~165 |
| 1812-2377 | coding_engine_tests（dispatch/gate/引擎集成测试混合体） | ~565 |

## 目标文件布局

| 新文件 | 内容 | 预估行数 |
| --- | --- | --- |
| `src/observability/events.rs` | EventBus + session_fingerprint + run_with_watchdog + watchdog_tests（roadmap：watchdog 归 observability） | ~290 ✓ |
| `src/engine/builtin.rs` | BuiltinRpcEngine struct + execute dispatch + profiles_for_sop + require_profile + run_planning/coding/testing/doc_fix（四薄接线） | ~520（贴线，超则四薄接线再分） |
| `src/engine/doc_wiring.rs` | run_doc_from_zero 接线闭包（`impl BuiltinRpcEngine` 跨文件块；R2 边界：先入 engine/，R3 垂直切片时再迁 sop/doc/） | ~420 ✓ |
| `src/server/gate.rs` | RenderGate + preflight_codegraph（门卫簇）+ 相关测试 | ~230 ✓ |
| `src/server/http.rs` | token 启动令牌 + build_router + startup_token_tests | ~130 ✓ |
| `src/server/mod.rs` | mod 声明、UiRelay、ServerState/SharedState、run_in_background/run_phase_for_sop/EngineBox（组装层） | ~440 ✓ |

tests/coding_engine_tests 拆随职责：dispatch/gate 各归其文件（`#[cfg(test)] mod`），不设独立测试文件。

## 关键边界声明

1. **接线闭包落位**：R2 落 `src/engine/doc_wiring.rs`（文件级职责化）；R3 垂直切片时
   随 runner+协议+测试迁 `sop/doc/`——本文件在 R3 计划中标记为「中转站」。
2. **纯搬移纪律**：不改任何行为/签名/可见性（pub 按需最小调整）；每个新文件一个 commit。
3. **acc2 run 不受扰**：拆解期间运行中实例不受影响（inode 替换语义）；task-5 验收才重启 7889。
4. clippy 零新增（存量 6 个不动）；`wc -l` 全部 ≤500。

## 实施顺序（每步独立 commit + cargo test 绿）

observability/events.rs → engine/builtin.rs（含四薄接线）→ engine/doc_wiring.rs →
server/gate.rs → server/http.rs → mod.rs 收尾组装 → 全量验证 + ARC 台账 R2 回写。

## R2 收官回写（2026-10-24，状态 Done，操作员执行）

| commit | 内容 | 行数变化 |
| --- | --- | --- |
| d5a7ab3 | task-2：EventBus+watchdog → observability/events.rs + watchdog.rs | mod.rs 2365→2001 |
| 174eb3a | task-3：BuiltinRpcEngine+SOP 接线 → engine/builtin.rs + doc_wiring.rs | mod.rs 2001→1118 |
| 64dc347 | task-4a：RenderGate → server/gate.rs（含四闸门测试） | mod.rs →1041→809 阶段 |
| 0a38dab | task-4b：token+router+serve → server/http.rs | mod.rs 942→809 |
| f8e3295 | task-4c：coding_engine_tests → engine/builtin_tests.rs；mod.rs 收至组装层 | mod.rs 809→342 |

终态行数：mod.rs **342** / gate.rs 190 / http.rs 137 / builtin.rs 496 / doc_wiring.rs 423 /
builtin_tests.rs 466 / events.rs 66 / watchdog.rs 314——全部 ≤500 ✓。

与预估的偏差（两条，均为实施中发现的技术必然）：
1. preflight_codegraph 未入 gate.rs 而归 engine/builtin.rs——它是引擎的 spawn 前预检，
   与 RenderGate（HTTP start 闸门）非同族；`require_profile`/`map_intervention`/
   `*_watch_dirs` 升 pub(crate)（跨模块 impl 块可见性，行为不变）。
2. 引擎集成测试未拆随职责文件，整体迁 engine/builtin_tests.rs——测试主体是
   BuiltinRpcEngine 行为（被测对象已迁 engine/），拆散反而破坏导航性；
   RenderGate 四闸门测试仍随 gate.rs。修正了原计划「不设独立测试文件」一条。

验收：全量 7 靶绿 exit 0；clippy 零新增（items after a test module 已修——
builtin_tests 声明挪至 mod 列表末尾）；7889 实例换新 binary 重启（pid 3058801），
acc2 resume 正常推进（run 75b962bb，review worker 事件流活跃）。
