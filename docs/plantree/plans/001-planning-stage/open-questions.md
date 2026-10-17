# Open Questions — P001

未决问题。解决后移除并把结论写进对应 topic / decision。

---

## Q001 · 保活与消息传递用什么机制？【已解决 2026-10-16】

**结论：pi RPC 模式（`pi --mode rpc`，长驻子进程 + JSONL 双向控制）。**

调研结果（官方文档 + 社区先例双确认）：
- SDK 需 Node.js/Bun 进程内嵌，Rust 宿主不可用 → 划掉
- print/JSON 模式是一次性进程（跑完即退），无保活 → 划掉
- RPC 模式为「其他语言编排」而生：进程长驻、上下文在内存，状态机三态原生对应——
  - **暂停保活** = `agent_settled` 事件（pi 明确不会再自动继续）
  - **唤醒续写** = 对同一进程再发 `prompt`（同一会话、完整上下文，非 session 重放）
  - **终止** = 关闭 stdin（体面 shutdown）
- 先例：npm 包 pi-agents-team（2026-10-11 发布）用同构架构（协调者 + `pi --mode rpc --no-session` 后台 worker），其语义可直接借鉴：idle worker 用 fresh prompt 唤醒、`agent_settled` 作为完成判据、`<final_answer>` 块做结构化输出协议
- pi 官方仓库另有 experimental packages/orchestrator（supervisor + rpc-process），方向获官方背书

来源：pi docs rpc.md / cli-integration.md / sdk.md；pi.dev/packages/pi-agents-team；pi repo commit 87ad824

## Q002 · Review 结果的形态与回注方式？【已解决 2026-10-18】

Review Agent 输出的是结构化问题清单还是自由文本？回注给 Plan Agent 时走什么通道（文件？直接注入消息？）

**结论：结构化 verdict 块 + 消息直注回注，已落地并端到端验证。**

- 形态：Review 末尾输出结构化 verdict 块——`<verdict>PASS</verdict>` 或 `<verdict>FAIL</verdict>` + `<issues>` 问题清单；协议段已写入内置提示词模板（src/config/manifest.rs），解析器对缺失/未知块保守按 FAIL（src/sop/output.rs）
- 回注通道：不走文件——问题清单直接作为 prompt 消息回注存活的 Plan Agent（上下文连续，decisions/004）
- 验证：第 18/22 试实测完整 FAIL→回注→修复→全新实例重审循环；第 21 试直 PASS 逐字协议块（stopReason=stop）

## Q003 · Plan Agent 的「跟用户沟通」交互面在哪里？【已解决 2026-10-16】

**结论：devforge 编排器做交互中继（relay）。**

RPC 模式下子进程没有 TUI（官方文档：TUI-only 能力在 RPC 下不可用/降级），Plan Agent 无法自带交互面。交互面归 devforge 的 API 层（形态拍板：devforge 是服务器非 CLI，见 P003）：客户端（前端；MVP 期裸 HTTP 客户端调试）输入 → `prompt` 发给 Plan Agent 进程 → 流式事件经 API 回传，ask-user-question 走 extension UI 子协议经 API 推拉。「编排平台拥有接口层」本就是平台该有的形态。

## Q004 · 「阶段完成」由谁判定？【已解决 2026-10-18】

Review 通过即阶段结束——但「通过」的判定权在 Review Agent 的输出里，编排器怎么可靠地识别「通过 vs 问题清单」？（结构化输出的可信解析问题）

**结论：verdict 块解析 + 保守缺省，已落地并端到端验证。**

- 编排器不做业务判断，只做协议解析：`<verdict>PASS</verdict>` → 阶段收尾（run Settled/任务 Completed）；FAIL + `<issues>` → 问题回注重查
- 可信性处理：verdict 缺失/未知/未闭合一律保守按 FAIL（P8-ISSUE-10 修复），协议违例作 issue 进重查循环而非硬崩——第 18 试实测过该路径
- 端到端实证：第 18 试 FAIL→修复→重审全循环、第 19/21 试直 PASS 逐字协议块；run Settled/任务 Completed 由解析结果驱动

## Q005 · 孤儿进程的收养/清理策略？【已解决 2026-10-16】

**结论：直接杀（用户拍板）。** devforge 崩溃后遗留的孤儿 pi，重启对账识别后直接杀——RPC 管道已随父进程断裂，存活上下文救不回来，不留。

配套的预防措施（真正的重点）：spawn ledger 实时落盘 + 编排器正常退出时主动清理所有自家进程——把孤儿产生在源头摁住。
