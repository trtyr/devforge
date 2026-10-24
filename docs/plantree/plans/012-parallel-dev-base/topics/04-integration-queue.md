# 04 · 集成事务（Integration as a Transaction）

> 目标：影响同一目标的候选变更**串行**集成；每次集成后在组合体上重建证据；
> 目标更新是 compare-and-swap 原子操作。集成是并行的收益兑现处，也是正确性最后防线。

## Merge Queue 流程

```
候选（CandidateFrozen）→ 入队（priority/fairness）
  → 集成工作树：checkout 目标最新 + merge/cherry-pick 候选
  → 组合验证（全量确定性套件）
      ├ 通过 → CAS 更新目标 ref → 标记 Integrated → 通知派工器（解锁依赖者）
      └ 失败 → 冲突/失败产物落盘 → 候选回 needs_repair（修复 = 新候选新证据）
  → 高风险变更（大迁移/策略）独立集成，不 batching
```

- **CAS**：目标 ref 更新用期望旧值守卫（`git update-ref refs/heads/master <new> <expect>`）；
  另一候选抢先集成后，迟到者自动 rebase 重验——**迟到者不能覆盖新目标**。
- **证据重建规则**：候选分支上的单测证据可复用于局部不变量；依赖/生成物/集成/e2e 类证据
  在组合体上重跑。复用边界由「内容与依赖摘要」判定，不由「看起来一样」判定。
- **冲突修复**：不得让模型从控制台文本猜冲突——持久化 base/target/candidate/conflict hunks/
  所有权/相关契约/既往决策，冲突修复作为一个新候选（带新证据）走完整流程。

## 组合验证套件（devforge 现成工具箱）

| 层 | 工具 | 淘汰什么 |
| --- | --- | --- |
| 构建 | cargo build/test 全量 | 结构冲突 |
| 文档确定性 | doc-lint + coverage 对账 | 文档语义冲突（AF 批次产出对不上） |
| 行为 | lib 测试 + 关键 e2e 冒烟 | 行为回归 |
| 边界 | owned_paths ⊆ 实际 diff 复核 | 权威冲突 |

## 容量与公平

- 队列深度上限 + queue-age 上限（base 漂移过大的候选**重生成**比反复 rebase 便宜）
- batching 可提吞吐但失败归因变难——组合验证失败时用二分或缩小批次
- 集成串行性 = 下游硬容量；admission control（06）据此反压派工

## 验收

- 两候选并发就绪：串行集成、第二者经历 rebase-重验、最终双双 Integrated
- 人为构造过期 CAS：被拒且候选回 needs_repair
- 组合验证失败：候选可归因到具体验证层与输出（evidence bundle）

## 实施落点（2026-10-24，M3 开工前设计速记）

新模块 `src/integration/`，复用 M0/M1 已有件（workspace / contract / boundary）：

- **候选**：`IntegrationCandidate { task_id, frozen_sha, base_rev, owned_paths, evidence_paths }`
  —— frozen_sha 来自 worktree freeze（M1 已落黑板 frozen-commit.txt）；owned_paths 来自契约。
- **集成工作树**：目标仓专用 worktree（同 M1 路径约定，`{repo}-worktrees/_integration`）——
  checkout 目标 ref 最新 → 逐候选 `git merge --no-ff`（或 cherry-pick）→ 组合验证。
- **CAS**：`git update-ref refs/<target> <new> <expect>` 三参形态；expect 不符 → 迟到者
  重排（rebase 到新目标重验），不覆盖。
- **组合验证**：cargo test 全量 + doc-lint + owned_paths ⊆ diff 复核（boundary::classify 复用）；
  证据 bundle 落 runstate。
- **冲突产物**：`{plans_root}/integrations/{id}/conflict.json`（base/target/candidate/hunks/
  所有权）——冲突修复 = 新候选走完整流程，不让模型从控制台文本猜。
- **merge queue API**：`POST /projects/{p}/tasks/{t}/queue`（入队 = 候选冻结件登记）、
  `GET .../queue`（队列状态）；集成由后台 worker 串行消费（M4 调度器接入前的手动/内联消费）。

M3 验收（本 topic「验收」段）→ 集成测试承载：两候选并发就绪串行集成 + CAS 过期拒绝 + 组合
验证失败归因（evidence bundle）。
