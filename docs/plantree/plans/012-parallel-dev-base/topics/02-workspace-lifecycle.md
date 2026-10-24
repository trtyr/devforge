# 02 · 工作区生命周期（Workspace Lifecycle）

> 目标：每个 mutation 工作项获得唯一分支 + linked worktree，pin 到已知 base；
> 崩溃后清理所有权转移给平台。清理是工作流，不是 best-effort 命令。

## 布局

```
{HOME}/workspaces/{project}/{task}/{run}/     # worktree 根（devforge HOME 内，随实例隔离）
  branch: task/{project}/{task}/{run短id}
  base:   base_rev（契约 pin）
{repo}/                                        # main 工作树 = 受保护目标，禁止 mutation 任务落此
integration/                                   # M3 的集成工作树（独立）
```

- linked worktree 共享对象库（.git/objects），隔离 index/检出文件/分支——**开销 ≈ 每路径 0.4KiB 元数据**，
  不是整仓拷贝。
- worktree 内的 pi 会话目录、`_meta/`、evidence bundle 全部落在 worktree 内 → 候选可整体归因。

## 生命周期状态机

```
Provisioning → Ready → (agent 干活: Running) → CandidateFrozen
    → (集成 M3) Integrated → Cleanup → Gone
         ↘ Failed/Cancelled → Cleanup → Gone（产物按策略保留 N 天）
```

1. **Provisioning**：解析契约 base_rev → `git worktree add -b task/... <dir> <sha>` →
   写 work-item 记录（worktree 路径、分支、pin、创建时刻）→ Ready。
2. **CandidateFrozen**：run 达成验收后，冻结 = 记录 HEAD sha + `git diff base..HEAD` 的 stat 摘要
   + evidence bundle 路径；此后 worktree 只读（引擎拒绝新 converse）。
3. **Integrated**（M3）：集成事务提交成功；worktree 保留至「目标包含该候选」确认后清理。
4. **Cleanup**：`git worktree remove` + 分支删除 + 记录闭合。清理 checklist：
   worktree 目录、分支 ref、pi 子进程（进程组）、临时凭证 env、_meta 里的 pidfile。

## 崩溃恢复

- server 启动时扫描：runstate 里状态 ∈ {Running, CandidateFrozen} 但 worktree 缺失/分支悬空的记录
  → 标记 `orphaned` → 按策略清理（候选若已冻结则保留分支待集成，未冻结则丢弃并通知）。
- **resume 对齐**：续跑前校验 worktree 存在且 HEAD 属于 task 分支且 base_rev 未漂移；
  base 已动 → 重建 worktree（旧目录保留为 `.bak-<ts>` 供取证）。

## 与现有件的交互

- **失活收割**：idle 指纹当前按 run 的 session 文件——worktree 化后口径不变（per-run session），
  收割动作从「杀 run」扩展为「杀 run + 工作区转 Cleanup」。
- **事件**：`workspace.provisioned / frozen / cleanup_done / orphaned` 四事件入流（DR-7 纯 API 观测）。

## 验收

- 全生命周期纯 API 可视；kill -9 server → 重启 → 孤儿被标记并按策略处置
- 候选冻结后引擎拒绝继续 converse；diff 可归因（base..candidate）
