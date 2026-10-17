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

## 测试阶段（2026-10-17 首轮执行 + 根因定性轮，十一轮实证）

- [x] (T009-首轮) 端到端测试已执行：9 脚本/基础 API 66 项全过/真 pi 真模型十一轮驱动
- [x] (T009-定性) 链路走到 **6/7 步**：规划全对话+plan tree 产出（13 文件，路径错位已记录）+review-0 真实 FAIL+问题清单+修复轮+review-1 重审——全环机制验证；终末步被双层阻断（review 漏发 verdict 协议块 P8-ISSUE-10 + 中转长 turn 流截断六次复现），根因全部线级定性
- [x] (T009-真凶) 五次「停滞」真凶=占位 skill→`find /` 全盘扫描（第十次抓活体进程）；真实 skill 已测试侧分发，机制消失
- 详见 evidence/e2e-test-record.md 与 tests/e2e/TEST_REPORT.md；问题只记录不修（用户约定），修复决策待用户拍板

## 修复阶段收官（2026-10-17，commit 991acaf）

- [x] (T009-修复) TEST_REPORT 全部记录在案缺陷修复完毕：✅16 / 🍉缓解 2 / ⏳待复测 1——渲染器配置债（auth 形状/models.json 分发/settings 运行时键/真实 skill 内置/协议段入模板）、UI 桥 method 分流、运行控制面（cancel 端点+watchdog 精确清场+可选超时）、pi stderr 落日志、spawn 工作目录、verdict 保守 FAIL、半注册回滚、manifest 落盘、token 入口、PUT 异步化
- [x] 修复全部带回归单测：cargo test 87 过 0 挂 + clippy 清；零端到端执行（用户约定，复测归下一阶段）

## 复测阶段收官（2026-10-17，第 17-21 试）

- [x] (T009-复测) 修复后全量端到端：11 脚本全功能点覆盖（新增 test_09 运行控制面/test_10 超时与异步渲染）
- [x] 第 18-20 试连续暴露三新缺陷并全部定位：①SSE 消费者超时耳聋（误诊双通道后纠正：真因=测试 SseStream 10s 读超时 vs 15s keepalive）；②verdict 后挂起（watchdog scope 自锁死锁，活体取证实锤）；③dialog 挂起态 cancel 半效
- [x] 第 21 试（修复后首跑）：**5 分 02 秒全链七环走通**——15 PASS/0 FAIL，run Settled+任务 Completed+SSE 全环（ui.request×5 全应答）+树落约定路径，逐字 PASS verdict

## 收官闭环（2026-10-18，本轮）

- [x] 收官清点：9 项尾巴枚举（evidence/closure-checklist.md），零待定
- [x] A1 Last-Event-ID 断线续传落地（EventBus 序号+环形留档+回放）；B1 run_all 补 09/10+时限；C4 补 README；D1 中转可靠性显式划为外部依赖（缓解已内置）
- [x] 收官全量回归（第 22 试）：run_all 全套 11 脚本 RUNALL_RC=0——93 单测全绿；07 17/17；08 **15 PASS/0 FAIL 含完整 FAIL→修复→重审循环与 Settled+Completed**；09 8/0；10 8/0；clippy 0 警告
- [x] decisions/010 收官模式入档；Q002/Q004 关闭；closure-checklist 零待定——**规划阶段闭环**

## Deferred

- （四阶段扩展不进本 plan）
- **devforge 对接插件（pi extension）**：Agent 侧与编排器的原生对接（完成信号协议化等），方向已拍板（decisions/009），MVP 后按需启动
