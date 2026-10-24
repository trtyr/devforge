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
