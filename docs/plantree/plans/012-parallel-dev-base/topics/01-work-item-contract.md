# 01 · 工作项契约（Work-Item Contract）

> 目标：每个 mutation 工作项自带机器可校验的契约，使「并发而不互踩」成为引擎可执行的不变量，
> 而非约定。

## Schema（草案，落 runstate / task 元数据）

```jsonc
{
  "work_item_id": "AF-batch-A",          // 唯一 id
  "parent_task_id": "devforge/base",      // 归属
  "objective": "…",                        // 目标描述（kickoff 用）
  "requirement_rev": "plantree@<commit>", // 需求修订（plantree commit）
  "base_rev": "git@<sha>",                // pin 的工作区基点
  "owned_paths": ["src/config/**", "assets/prompts/**"],
  "forbidden_paths": ["src/server/mod.rs", "docs/plantree/**"],
  "readable_paths": ["**"],               // 默认全读
  "acceptance": [                         // 验收声明 + 证据 schema
    {"kind": "tests_green", "scope": "lib"},
    {"kind": "no_boundary_violation"}
  ],
  "budget": {"model_tokens": 50_000_000, "wall_minutes": 240},
  "integration": {"prerequisites": [], "consumers": ["AF-5"]}
}
```

## 字段语义

- **owned_paths**（glob）：执行期实际改动 ⊆ 于此；引擎在 commit 前用 `git diff --name-only`
  机械校验，越界 = FAIL（D005 越界判据的并发版）。**这是防文本/结构冲突的第一道闸。**
- **base_rev**：worktree pin 点。契约里显式记录，集成阶段据此判断「目标移动了多少」。
- **forbidden_paths**：读可以（readable_paths 默认全读）、写绝对禁止——保护共享契约与
  他批次的领地（如批 A 期间 `src/server/mod.rs` 冻结给 ARC-R2）。
- **acceptance**：可机械校验的证据声明（tests_green / lint_clean / coverage_match / 自定义命令）。
  集成阶段按声明重建证据，声明之外的质量由评审通道（现有 doc-review 模式）承担。

## 载体与生命周期

- 契约在 **task 创建时**提交（POST tasks 的 body 扩展），不可变；改契约 = 新工作项（旧作废）。
- runstate 持有一份执行期快照（base_rev 实际解析值、owned 匹配缓存），断点续跑时校验一致。
- 契约 owner：`owned_paths` 的交集体 = 隐式争议，创建时由调度器拒绝（冲突预检，见 03）。

## 与 change SOP 的接法

change_plan 的六块计划里「涉及文件」段落升级为 owned_paths 的提案来源；
引擎在 plan-review 阶段校验提案 ⊆ 契约 owned_paths，不覆盖则打回。

## 验收

- 越界写被引擎拦截且事件可查（`boundary.violation`）
- 双任务 owned_paths 相交时第二个创建被拒（冲突预检）
- base_rev 漂移检测在 resume 时触发
