# D002 · 超时机制重设计：墙钟预算与失活收割分离，默认永不自动杀

- 日期：2026-10-18
- 状态：已实现（本轮 coding 阶段）
- 关联：用户裁决 2026-10-18——「给超时是非常错误的做法，它确实在跑的时候因为超时停掉肯定有问题；
  一个项目跑一两小时很正常，甚至跑两三天/一晚上四五个小时都有可能」

## 哲学

**只杀真死的，永不杀慢而活着的。** 墙钟时长不是死亡证据——一个真在推流的 run 跑一夜也是健康的；
只有「会话无进展」才是死亡信号。因此：

1. **墙钟超时（预算上限）**：保留但永远是显式 opt-in——`DEVFORGE_RUN_TIMEOUT_MS`，默认不开。
2. **失活收割（活性检测）**：新增 opt-in——`DEVFORGE_RUN_IDLE_MS`，默认不开。
   判死信号不是时间，是**会话文件指纹平线**。
3. **两个都不开（默认态）= 永不自动杀 run**；恢复手段只剩人工 cancel 端点。
   挂死风险由 F1 修复（spawn 前 profile 门卫）在源头削掉一大类。

## 机制

- **进展信号**：watchdog（300ms 轮询）对 run 的会话目录
  （planning: `sessions/plan` + `sessions/reviews`；coding: `sessions/coding` + `sessions/coding-reviews`）
  递归计算**指纹** = Σ(文件长度, mtime 纳秒) 的 FNV-1a 混合。任一文件被追加/触碰 → 指纹变化 →
  刷新进展时钟。pi 在流式生成期间会话文件持续增长（e2e 增长探针曲线实证），故单轮超长生成也计为「活着」。
- **判死**：`now - last_progress > DEVFORGE_RUN_IDLE_MS` → `RunControl.mark_stalled()` +
  按 run-id 精确 SIGKILL 自家 pi（scan_and_kill_run，红线不变）→ run Failed，原因
  「已失活（会话无进展超过闲置阈值，被 watchdog 终止）」——不再伪装成底层 RPC EOF 噪音。
- **优先级**：cancel > 失活 > 墙钟超时（map_intervention 检查序）。
- **配置**：
  - `DEVFORGE_RUN_TIMEOUT_MS`（>0 整数）：墙钟预算，到点收割，原语义不变。
  - `DEVFORGE_RUN_IDLE_MS`（>0 整数）：失活阈值。建议生产值 ≥ 900_000（15 分钟）——
    模型偶发长尾思考 + 单轮大生成也远不到；e2e 测试口径可给几百 ms。
  - 两者独立开关，可同时用（预算兜底 + 失活精杀）。

## 实现

- `domain/runner.rs`：`RunControl` 增 `stalled: AtomicBool`（watchdog 置位，map_intervention 读）。
- `server/mod.rs`：`session_fingerprint()`（递归 FNV-1a）；`run_with_watchdog` 增
  `idle_ms + watch_dirs` 参数与失活分支；`BuiltinRpcEngine.idle_ms` 字段（main.rs 从 env 注入，
  Arc 分享前捕获）；`map_intervention` 增 stalled 分支。
- 单测 6 条：指纹随建文件/追加变化；增长=活（150ms 持续追加 + 600ms 阈值 → 不收割）；
  平线超阈=死（400ms 阈值 + 静默 1.5s → stalled 置位）；未设置不超时（静默 900ms → 永不收割）；
  原有 cancel/超时 watchdog 回归不变。

## 下轮 e2e 校准点（本轮 e2e 脚本不动）

- test_10 / test_15 用 `DEVFORGE_RUN_TIMEOUT_MS` 显式开预算——语义不变，无需改。
- 可选增强（下轮）：test_15 增加失活路径用例（临时目录会话停写 + `DEVFORGE_RUN_IDLE_MS` 小值 →
  run Failed 且 reason 含「已失活」）；e2e HOME 迁 /var/tmp（006 decisions/001 运维防线）。
