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

## Q002 · Review 结果的形态与回注方式？

Review Agent 输出的是结构化问题清单还是自由文本？回注给 Plan Agent 时走什么通道（文件？直接注入消息？）

更新：通道已定——编排器经 RPC `get_last_assistant_text` 取结果，落盘后作为 prompt 回注 Plan Agent。**形态**借鉴 pi-agents-team 的 `<final_answer>` 协议：约定 Review 末尾输出结构化块（如 `<verdict>PASS|FAIL</verdict>` + 问题清单），同时解决 Q004 的判定解析。格式细节待 T007 定稿。

## Q003 · Plan Agent 的「跟用户沟通」交互面在哪里？【已解决 2026-10-16】

**结论：devforge 编排器做交互中继（relay）。**

RPC 模式下子进程没有 TUI（官方文档：TUI-only 能力在 RPC 下不可用/降级），Plan Agent 无法自带交互面。devforge CLI 承担中继：读用户输入 → `prompt` 发给 Plan Agent 进程 → 流式转发 text_delta 回终端（即 rpc.md 官方 minimal client 的模式）。「编排平台拥有控制台」本就是平台该有的形态。TUI 美化后续再说，MVP 纯文本即可。

## Q004 · 「阶段完成」由谁判定？

Review 通过即阶段结束——但「通过」的判定权在 Review Agent 的输出里，编排器怎么可靠地识别「通过 vs 问题清单」？（结构化输出的可信解析问题）

更新：方向已定——借鉴 pi-agents-team 的 `<final_answer>` 协议，约定 Review 末尾输出结构化 verdict 块（如 `<verdict>PASS|FAIL</verdict>` + 问题清单），与 Q002 同一机制，格式细节 T007 定稿。

## Q005 · 孤儿进程的收养/清理策略？【已解决 2026-10-16】

**结论：直接杀（用户拍板）。** devforge 崩溃后遗留的孤儿 pi，重启对账识别后直接杀——RPC 管道已随父进程断裂，存活上下文救不回来，不留。

配套的预防措施（真正的重点）：spawn ledger 实时落盘 + 编排器正常退出时主动清理所有自家进程——把孤儿产生在源头摁住。
