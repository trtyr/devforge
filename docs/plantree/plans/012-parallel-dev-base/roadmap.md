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

## M4 · 依赖图调度器 + 准入（2-3 天）

- ☐ T4.1 变更 DAG 声明格式（task 间依赖边 + 契约会合点），首批输入 = AF/ARC 批次聚类（A/B/C/D）
- ☐ T4.2 调度器：就绪判定（依赖候选已集成）→ 准入（下游容量 ≥ 阈值）→ 派工（build worktree + start）
- ☐ T4.3 准入控制：并发 mutation 上限、组合验证容量上限、queue-age 上限（过期候选重生成优于反复 rebase）
- ☐ T4.4 blocked 状态机：缺契约/依赖未集成 → blocked 事件 + 恢复联动
- ☐ T4.5 预算：每 work item 推理/构建/测试分钟数上限，超限熔断（复用断路器血统）

**验收**：批 D + 批 A 在两 worktree 并发完成并按序集成；人为制造下游拥塞时派工自动止住。

## M5 · 观测与收尾（1 天）

- ☐ T5.1 并行指标入 events：runnable/blocked/verifying/integrating 时长、冲突率（文本/结构/语义分类）、
  队列等待、证据重用率、每保留集成的总成本
- ☐ T5.2 失活收割语义复审：idle 判据在多 worktree 下的指纹口径（per-run session 指纹已有，复核即可）
- ☐ T5.3 文档收尾：本 README 状态翻新 + DR 台账联动 + docs/contexts 对齐

**验收**：不登机器纯 API 回答「现在几个 worktree 在飞、各自卡在哪、队列多长、冲突了没有」。

## 里程碑依赖

M0 → M1 → M3 → M4 → M5（M2 沙箱化 spawn 已作废删除——隔离 = path-guard 插件 + worktree + 契约终校验，2026-10-24 用户裁决）。
