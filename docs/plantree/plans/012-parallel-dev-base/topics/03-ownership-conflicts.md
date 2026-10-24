# 03 · 所有权与冲突预防（Ownership & Conflicts）

> 目标：冲突在派发前预防，而不是集成时惊讶。文本锁是第一层，语义所有权是第二层。

## 两档所有权

| 档 | 适用 | 行为 |
| --- | --- | --- |
| **乐观（optimistic）** | 低耦合模块（互不相干的文件面） | 并行放行，集成时检测罕见冲突 |
| **保留（reserved）** | 共享契约/迁移序号/依赖版本/生成物/高争用文件 | 派发前获租约（lease），独占写 |

- 租约有 **TTL**：被弃工作不永久堵路；但集成权威处对过期租约有 fencing（旧持有者 late-write 被拒）。
- 保留权的申请在契约创建时（owned_paths ∩ 已租约集合 ≠ ∅ → 拒绝或排队）。

## 五级冲突与检测点

| 级 | 检测点 | 手段 |
| --- | --- | --- |
| 文本 | 集成时 | git merge 冲突 |
| 结构 | 集成时 | 组合编译 + import/生成物校验 |
| 语义 | 集成时 | 组合后跑全量 lint/coverage/测试（不变量检查） |
| 操作 | 派发时 | 契约的 rollout 前置声明比对 |
| 权威 | 契约创建时 | 所有权注册表互斥 |

**关键认知**：语义冲突文本工具全盲——防线是「组合后全量确定性验证」（devforge 的
lint/coverage/测试工具箱恰好是现成的组合验证器），以及共享契约的单一 owner。

## devforge 自举的首份所有权表（草案，供首批派工用）

```yaml
exclusive:
  "src/config/**":        AF-batch-A   # manifest/render/提示词外置
  "assets/prompts/**":    AF-batch-A
  "src/sop/doc_from_zero.rs": AF-batch-A   # 作业本+kickoff 瘦身
  "src/engine/client.rs": DR-17         # worker 事件落盘
  "src/server/handlers.rs": DR-18       # transcript 映射
  "src/server/mod.rs":    ARC-R2        # 拆解手术（独占窗口）
reserved:
  "Cargo.lock":           integration-only  # 集成权威统一处理
shared-read:
  "**": "*"
```

## 状态机（worker 视角）

```
Normal → blocked{missing: contract/dependency} → (orchestrator 补齐/改派) → Normal
Normal → cancel_requested → persist checkpoint → release lease → teardown
```

worker 缺授权/输入/契约时进入 blocked 并上报缺失项与已收集证据；**禁止围绕缺失需求反复猜测**
（猜出来的分支必然发散）。

## 验收

- 所有权表存在且被调度器消费：相交契约第二个创建被拒
- 租约 TTL 过期后集成权威拒绝旧持有者写入（fencing 测试）
- blocked 事件可查，恢复后自动回 Ready
