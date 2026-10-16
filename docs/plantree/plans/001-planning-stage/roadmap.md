# Roadmap — P001 规划阶段

## Done

- [x] (T001) 需求澄清：底座选型（复用 pi）、存储位置（~/.devforge/plans/）、无硬限制哲学（2026-10-16）
- [x] (T002) 验证配置隔离机制：`PI_CODING_AGENT_DIR` 可覆写 pi 配置目录（2026-10-16）
- [x] (T004) 保活/消息机制调研定案：RPC 模式（`pi --mode rpc`），先例 pi-agents-team（2026-10-16，见 open-questions Q001）
- [x] (T003) 编排器设计定稿：状态机 + 保活 + 消息传递 + 进程身份 + 自动流转全定案（2026-10-16）；verdict/完成标记格式细节归 T007
- [x] (T010) 基础架构选型定版：thiserror+anyhow 双层错误（Agent/System 分类）、tracing JSONL 文件层+stderr 双输出（2026-10-16，代码已落 src/error.rs + src/logging.rs，6 单测绿；回填 architecture-principles 落地锚点）
- [x] (T006-引擎核心) RPC 编排引擎（src/engine/）：JSONL 协议编解码（prompt/get_state/response/event/text_delta）、spawn 命令构造（三重身份标记注入+session-dir）、spawn 台账（JSON 落盘+Ours/Foreign/Dead 对账双验证+孤儿直接杀）、生命周期客户端（spawn/prompt/wait_settled/require_success/kill，mock 回放测试不碰真 pi）——55 单测全绿 clippy 干净（2026-10-16）

## Next

- [ ] (T005) 设计两类 Agent 的独立配置目录结构（agent-profiles）——P002 渲染器已落地内置清单（prompt 占位版），完整提示词在实操时细化
- [ ] (T011) 可观测数据层：运行图模型（节点=Agent 实例/边=流转）+ RPC 事件落盘按节点组织 + session 存档归档（--session-dir 按 run-id、台账映射、trace 回溯）（原则 §4，MVP 先数据层、可视化后置）
- [ ] (T006) 落地 orchestrator 服务器骨架（P003 形态：非 CLI）：spawn Plan Agent（含身份标记 + spawn 台账）→ 经 API 面用户交互产出 plan tree → 保活暂停；错误处理与日志随骨架落地，不许后补。注意：ask-user-question 在 RPC 下走 extension UI 子协议（rpc-extension-ui.md），中继经 API 推拉
- [ ] (T007) 落地 Review Agent：读取 plan tree + skills，输出通过/问题清单
- [ ] (T008) 落地 Review 循环：问题清单回注 Plan Agent → 修复 → 新 Review 重查
- [ ] (T009) 端到端跑通：从一句需求到一份通过 review 的 plan tree

## Deferred

- （四阶段扩展不进本 plan）
- **devforge 对接插件（pi extension）**：Agent 侧与编排器的原生对接（完成信号协议化等），方向已拍板（decisions/009），MVP 后按需启动
