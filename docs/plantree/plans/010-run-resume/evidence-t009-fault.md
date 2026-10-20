# T009 · 真实故障自动重启实证（e2e，2026-10-20）

- 脚本：`tests/e2e/test_25_fault_retry.py`（可复跑；故障代理内置为测试进程内守护线程）
- 终版运行：2026-10-20 19:0x–19:5x，**48m02s，17 PASS / 0 FAIL / 0 issues**（日志 `/tmp/bg-run/917c35/bg_7xqzlqcj.log`；结果快照 `results/test_25_fault_retry.json`）
- fixture：`fault-app`（alpha/beta 两包两单元）；doc-review profile 的 baseUrl 指向本地故障代理

## 一、故障注入设计（自整定，不猜内部重试次数）

代理规则：
1. 第一个 POST /chat/completions 必回 500；
2. 之后只要与上一请求间隔 ≤120s 也回 500（覆盖 pi 内部 HTTP 重试 + SOP 内 3 次换实例重试的风暴期）；
3. 一旦出现 >120s 静默（= SOP 抛 System → run 级重启 → doc-agent 重写期不产生代理流量），**永久放行**透传真上游。

为什么不是「前 N 个失败」：v1 实测（22:xx）注入 3 个 500 被 **pi 的 provider 客户端内部重试全部吞掉**——会话未死、0 次 run.retry、attempt=0、run 正常 Settled。故障必须跨过内部重试 AND 耗尽 SOP 实例重试才能触发 run 级重启，固定 N 无法两 iff.

## 二、实测时间线（终版）

| 时刻 | 事件 |
| --- | --- |
| t=0 | start 202，attempt-1 开跑（doc-agent 直连 MiniMax，不经代理） |
| t≈12min | unit-alpha 交审 → 代理连续回 500：**共注入 16 次**（pi 内部重试 ×N + 3 个审查实例各一次首请求）→ SOP 抛 System |
| +10s | 退避后 **run 级自动重启**：WARN + SSE `run.retry`（attempt=2）+ 黑板 `rs.attempt=2` 落盘 |
| t≈27min | attempt-2：doc-agent 重写（代理静默期无流量）→ 首个审查请求距上次 >120s → **代理永久放行** |
| t=2860s | 终审过 → **Settled**，7 篇正文文档齐全 |

## 三、验收断言逐条对照（T009 原文）

| T009 要求 | 实测 |
| --- | --- |
| 可控方式制造上游故障 | 代理注入 **16 次 HTTP 500**（`500 (injected)` 逐条在代理日志） |
| 自动重启真实发生 | SSE 事件 `{'type': 'run.retry', 'attempt': 2, ...}`；黑板 state.json `attempt==2` |
| 最终 Settled | 2860s 后 Settled，两单元 7 篇文档齐全 |
| 报告记录尝试次数 | `report.md` 含 **`- 尝试: 2（1 = 首次；>1 含自动重启）`**（产品补丁 commit `1ec8433`） |
| 重启后审查走通 | 代理放行后透传 **76 次**，全部 200 |

## 四、配套产品改动

- `1ec8433`：doc 运行报告补「尝试」行（run_with_retry 落盘的 `rs.attempt` 直接进 report.md）——T009 验收点「report.md 记录尝试次数」的载体；
- 质量失败不重启的边界未受扰动：本轮故障走的是 System 路径；Quality 路径（协议违例/审查打回）不重试已由 010 T006 单测与 e2e 锁死。

## 五、诚实边界

1. 重启只观测到 **1 次**（attempt 2 即恢复）；「连续多次重启」的极端路径由单测 `persistent_infra_failure_stops_at_four_attempts` 覆盖（4 次尝试封顶）；
2. 故障只注入在 review 渠道（doc-agent 直连未动）——若 agent 渠道故障，行为同构（同一 retry 环路），未单独实证；
3. 代理的 120s 静默阈值是经验值：低于它可能误吞 attempt-2 的首个审查请求（多一轮重启，仍能收敛），高于它可能提前放行（退化为 v1 行为，测不到重启）。
