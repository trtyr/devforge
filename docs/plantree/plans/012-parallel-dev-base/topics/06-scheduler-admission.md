# 06 · 依赖图调度与准入控制（Scheduler & Admission）

> 目标：把「哪些活能同时开」从操作员脑内聚类变成机器调度——按变更 DAG 就绪派工，
> 按下游容量反压，量串行分数。

## 变更 DAG

```jsonc
{
  "nodes": [
    {"id": "batch-D", "contract_ref": "...", "depends_on": []},
    {"id": "batch-A", "contract_ref": "...", "depends_on": []},
    {"id": "AF-5",    "contract_ref": "...", "depends_on": ["batch-A"]},
    {"id": "ARC-R2",  "contract_ref": "...", "depends_on": ["batch-D"], "exclusive": true}
  ]
}
```

- 首批输入 = plantree 已有的手工聚类（批 A/B/C/D + AF/DR 清单）——**调度器的第一份数据是人肉做的图**，
  之后逐步让 plan 阶段产出机器可读依赖（change_plan 的 owned_paths 交集体自动生成依赖边）。
- `exclusive: true` = 独占窗口（ARC-R2 拆 server/mod.rs 期间冻结其他含该文件的派工）。

## 调度循环

```
loop:
  ready = DAG 中依赖全部 Integrated 且契约就绪的节点
  for node in ready 按优先序:            # 优先序 = 关键路径优先，不是先来先服务
      if admission_ok(node): provision_workspace(); start_run(node)
      else: 挂 blocked{reason: capacity}
  reap: 收割/断路器/超预算 → 状态迁移 → 依赖重算
```

## 准入控制（admission）

| 阀 | 默认 | 理由 |
| --- | --- | --- |
| 并发 mutation 任务 | 2（= worktree × 实例现容量） | 下游组合验证是瓶颈 |
| 组合验证并发 | 1（集成串行性） | CAS 语义要求 |
| queue-age | base 漂移 > N commit 即过期重生成 | 老候选 rebase 成本 > 重做 |
| 每 item 预算 | tokens/wall_minutes 上限 | 复用断路器血统，超限熔断 |

- **下游饱和时停止派发 mutation**，把容量转给验证/集成（业界教训：高利用率可能只是
  在更快地生产过期工作）。
- 串行分数测量（M0 T0.3）持续回灌：spec/implement/verify/integrate 四段占比决定
  「加 worker」还是「优化集成」。

## blocked 语义

worker 缺契约/依赖未集成 → `blocked{missing, evidence}` 事件；调度器三选一：
补齐依赖、转移所有权、升级给人决策。**禁止围绕缺失需求反复猜测。**

## 可观测（并入 events 流）

runnable/blocked/verifying/integrating 时长；关键路径长度与 fanout；
工作区供给与孤儿清理延迟；租约争用；文本/结构/语义冲突率；队列等待与投机验证浪费；
取消与清理延迟；**每个保留集成的总成本**。不优化「活跃 agent 数」。

## 验收

- 批 D + 批 A 双 worktree 并发完成、按序集成（D 先 A 后或按冲突面）
- 人为压低组合验证容量 → 派工自动 blocked{capacity}
- 独占窗口期间含冻结文件的派工被拒
