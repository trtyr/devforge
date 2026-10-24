# Roadmap — P012 并行开发底座

> 状态图例：☐ 待做 / ◐ 进行中 / ✅ 完成 / ⛔ 阻塞
> 执行人：操作员（底座手术不派工）。每期收尾：全量测试绿 + 纯 API 验收 + commit。

## M0 · 契约与测量（先行，1-2 天）✅ 2026-10-24 1de960f

- ✅ T0.1 `TaskManifest`/run 契约字段：`owned_paths: Vec<glob>`、`base_rev: Option<String>`、
  `forbidden_paths`、`acceptance: Vec<String>`（evidence schema），入库 runstate
- ✅ T0.2 change SOP 接契约：change_plan 产出 owned paths 提案 → 引擎校验执行期实际改动 ⊆ owned_paths，
  越界即 FAIL（复用越界改动判据，D005 血统）
- ✅ T0.3 串行分数测量：run 记录 spec（用户拍板）/implement/verify/integrate 四段时间戳，
  events 端点透出；攒第一份 devforge 自举的串行分数数据
- ✅ T0.4 现状固化：当前分支直改模式加 `isolation: "none"` 显式标记（向后兼容的枚举起点）

**验收**：单任务带契约跑通；越界改动被拦；串行分数四段在 events 可查。

## M1 · worktree 工作区生命周期（2-3 天）✅ 2026-10-24 afe2c64

- ✅ T1.1 工作区管理模块 `src/workspace/`：create（`git worktree add -b task/{id} @{base_rev}`）、
  freeze（候选 = 冻结 commit + evidence bundle 路径）、remove（含孤儿扫描）
- ✅ T1.2 change SOP 迁入 worktree：spawn cwd = worktree；main 工作树受保护（引擎拒绝在
  main 工作树跑 mutation 任务）
- ✅ T1.3 崩溃清理：server 启动扫描孤儿 worktree/分支，清理所有权转移（按 work-item 记录处置）
- ✅ T1.4 断点续跑与 worktree 对齐：resume 时校验 worktree 仍在且 base 未漂移，漂移则重建

**验收**：change 任务全生命周期在 worktree 内完成；kill -9 server 后重启能清孤儿；
候选冻结 commit 可 diff 归因。

## M2 · 沙箱化 spawn ⛔ 已作废并删除（2026-10-24 用户裁决：沙箱整个不做；隔离 = path-guard 工具级路径权限插件（已落地，4faaa47+d8d85aa）+ worktree + 契约终校验。原 topic 05 已删。）

## M3 · 集成事务（3 天）✅ 2026-10-24 cc5a3ed+64f3bcf（src/integration/ 串行消费者：checkout→merge→注入式验证→CAS 三参，重排/修复产物结构化 + merge queue API；pathguard 渲染接线 64f3bcf）

- ✅ T3.1 集成工作树（integration worktree，保护目标 + 候选的推测合并）
- ✅ T3.2 组合验证管线：cargo test 全量 + doc-lint + （文档任务）coverage 对账——证据 bundle 落 runstate
- ✅ T3.3 CAS 原子更新：目标 ref 以 `update-ref` 期望旧值实现 compare-and-swap；迟到集成者重排
- ✅ T3.4 冲突产物结构化：base/target/candidate/hunks/所有权 落盘，冲突修复 = 新候选带新证据
- ✅ T3.5 merge queue API：`POST .../queue {candidate}` / 队列状态查询 / 优先级

**验收**：两候选并发就绪时，串行集成且第二者经历「目标已动 → rebase → 重验」路径；
CAS 拒绝过期写入。

## M4 · 依赖图调度器 + 准入 ✅ 2026-10-25 e235b47+a1f3e9c（src/server/scheduler.rs：tick 决策/launch 执行分离 + kick 即时；挂账两项同批：forbidden 引擎校验 + 提案校验 audit_with_contract）

- ✅ T4.1 变更 DAG 声明格式：Task.depends_on/exclusive + TaskDeps（创建时存在性校验 + Kahn 环检测）+ TaskStatus::Blocked；首批输入 = API 手工声明
- ✅ T4.2 调度器：tick 纯决策（就绪判定 → 派工计划）+ launch async 执行（渲染闸门等待）；30s 周期 + run 收尾 kick 即时
- ✅ T4.3 准入控制：并发 mutation 上限（SchedulerConfig，store.max_concurrent_runs 硬闸默认 1 兼容 decisions/003）+ 独占窗口 exclusive
- ✅ T4.4 blocked 状态机：capacity / exclusive_window / dependency_failed 三理由 + 恢复联动（条件解除后下轮 tick 自动回 Pending 派工）
- ✅ T4.5 预算：每 work item 墙钟分钟上限（默认 240），超限 cancel 熔断（断路器血统）

**验收（测试级，2026-10-25 用户裁决：端到端长跑作废）**：调度器单测 7 件全绿
（就绪序/依赖失败阻塞/容量拒绝+恢复联动/预算熔断/自动入队/独占窗口/空仓）；
Settled 自动入集成队列 = 集成闭环自动化（单测承载）。

## M5 · 观测与收尾 ✅ 2026-10-25 25093f0（scheduler/status 端点 + docs/api-contract.md 前端对接契约 + 指纹复审结论）

- ✅ T5.1 并行指标入 events：scheduler 事件族（budget_fused/auto_enqueued/blocked 理由）+
  GET /scheduler/status 快照（running+elapsed_ms/runnable/blocked/queue 四态计数/冲突面 needs_repair）
- ✅ T5.2 失活收割语义复审：per-run session 指纹在多 worktree 下成立，无需改动（结论入 api-contract.md）
- ✅ T5.3 文档收尾：本 README 状态翻新 + backlog-20261025 勾销（docs/contexts 对齐随 docs-update 流程）
- ✅ T5.4 API 消费面稳定化：docs/api-contract.md（核心端点表 / SSE 事件 schema 分类 51 种 /
  错误码规范 400-404-409-500 / 变更兼容规则）

**验收（测试级）**：不登机器纯 API 回答「现在几个 worktree 在飞、各自卡在哪、队列多长、
冲突了没有」——status 端点集成测试承载；全量 lib 638 绿。

## 里程碑依赖

M0 → M1 → M3 → M4 → M5（M2 沙箱化 spawn 已作废删除——隔离 = path-guard 插件 + worktree + 契约终校验，2026-10-24 用户裁决）。
