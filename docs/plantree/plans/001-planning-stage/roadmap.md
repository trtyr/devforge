# Roadmap — P001 规划阶段

## Done

- [x] (T001) 需求澄清：底座选型（复用 pi）、存储位置（~/.devforge/plans/）、无硬限制哲学（2026-10-16）
- [x] (T002) 验证配置隔离机制：`PI_CODING_AGENT_DIR` 可覆写 pi 配置目录（2026-10-16）
- [x] (T004) 保活/消息机制调研定案：RPC 模式（`pi --mode rpc`），先例 pi-agents-team（2026-10-16，见 open-questions Q001）
- [x] (T003) 编排器设计定稿：状态机 + 保活 + 消息传递 + 进程身份 + 自动流转全定案（2026-10-16）；verdict/完成标记格式细节归 T007
- [x] (T010) 基础架构选型定版：thiserror+anyhow 双层错误（Agent/System 分类）、tracing JSONL 文件层+stderr 双输出（2026-10-16，代码已落 src/error.rs + src/logging.rs，6 单测绿；回填 architecture-principles 落地锚点）
- [x] (T006-引擎核心) RPC 编排引擎（src/engine/）：JSONL 协议编解码（prompt/get_state/response/event/text_delta）、spawn 命令构造（三重身份标记注入+session-dir）、spawn 台账（JSON 落盘+Ours/Foreign/Dead 对账双验证+孤儿直接杀）、生命周期客户端（spawn/prompt/wait_settled/require_success/kill，mock 回放测试不碰真 pi）——55 单测全绿 clippy 干净（2026-10-16）
- [x] (T007+T008) Review Agent + Review 循环（src/sop/）：统一输出协议（完成标记 <plan-complete> + verdict 块 PASS/FAIL/issues 解析）、PlanningLoop 状态机（强制 ask-user-question 完成确认/每轮全新 Review/打回回注/零校验自动重查/无轮次上限）、RPC 桥接（RpcPlanSession 含 extension_ui_request 子协议推拉、RpcReviewFactory 每轮全新 spawn 用完即弃）——66 单测全绿 clippy 干净；注：UI 应答记录格式为联调校准点，端到端验证归测试阶段 T009（2026-10-16）

## 开发阶段收官（2026-10-16）

- [x] (T005) 两类 Agent 独立配置目录：由 P002 渲染器承接落地（内置清单含两 Agent 提示词占位版+差异化包；完整提示词正文待细化，见 src/config/manifest.rs planning_builtin）
- [x] (T006) orchestrator 服务器骨架：axum 常驻 + PlanningRpcEngine 生产引擎接线（src/server/，main.rs 启动；spawn 台账/身份标记随引擎落地；ask-user-question 经 extension_ui_request 子协议 API 推拉）
- [x] (T011-部分) 可观测数据层：SSE 事件流推送（run 生命周期+ui.request 统一信封）+ session 存档机制（SpawnSpec session-dir 按 run-id）；按节点组织的事件落盘归测试阶段完善
- [x] (T009→测试阶段) 端到端跑通：开发阶段按约定跳过（只开发+单测），归测试阶段执行

## 测试阶段（2026-10-17 首轮执行）

- [x] (T009-首轮) 端到端测试已执行：9 脚本/76 PASS/7 FAIL/**14 项发现（high 7）**——全链路可启动、模型可对话，但首轮生成>900s 未完成 → 完成确认未运转/无 review/无 plan tree/任务永久 Running（无取消端点）
- 详见 evidence/e2e-test-record.md 与 tests/e2e/TEST_REPORT.md；问题只记录不修（用户约定），修复决策待用户拍板

## Deferred

- （四阶段扩展不进本 plan）
- **devforge 对接插件（pi extension）**：Agent 侧与编排器的原生对接（完成信号协议化等），方向已拍板（decisions/009），MVP 后按需启动
