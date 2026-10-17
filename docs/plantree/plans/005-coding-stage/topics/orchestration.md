# Topic · 编排（CodingSopLoop 与跨 SOP 衔接）

## 状态机

```
spawn coding-agent（长驻，cwd=黑板）
  → 初始 prompt：读树 → 自判体量 → N 阶段 → stages.md/todo.md → 阶段 1 开工
  → [阶段循环]
       coding agent 自认阶段完成（结构化标记）
         → spawn review-N（用完即弃）→ Review Report
             → FAIL+fix   → report 回注同一 coding 进程 → 修复 → 重审（回到循环头）
             → PASS+advance → 同一进程继续阶段 N+1（回循环头）
             → PASS+finish → 全树完成
  → run Settled + 任务 Completed
```

- 与 planning SOP 同构的骨架：长驻主 Agent + 短命 Review + verdict 驱动 + 零校验自动流转（decisions/008：完成确认在 Agent 环节内，编排层只解析结构化块）
- 阶段轮数与修复轮数无上限（零硬限制，decisions/003）
- **无标记停转处置**：agent settled 但未发阶段完成标记 → 编排器回注「继续当前阶段」重推（零上限；模型自然收敛，与零硬限制同源）
- **阻塞语义**（decisions/012）：ask-user-question 挂起即系统可见阻塞态（dialog 通道现有）；blockers/ 纯留痕；cancel 解除挂起能力直接复用 P001 修复
- **PASS 附带意见**：advance 的 report 允许携非阻塞建议，回注后 coding agent 先消化建议再进下一阶段

## 与 planning 的衔接（同任务跨 SOP）

- 任务目录即交接面：planning run 的树留在黑板，coding run 从同一目录起步
- 触发：`POST /projects/{p}/tasks/{t}/start` 接受 sop 覆盖（缺省用任务注册 sop）；run 归档按 run-id 天然区分两阶段历史
- 任务状态机沿用：新 run → Running → Completed（允许同任务多次 Completed，每 SOP 一次）

## 工作目录模型

- spawn cwd = 黑板（沿用 P8-ISSUE-05 修复的 `plans_root/<project>/<task>`）
- 代码工作区 = `project.local_path`，提示词注入告知；CodeGraph 索引与项目代码同址

## 风险与观察点

- rpiv-todo 在 RPC 模式的可用性（Q001）
- codegraph 多实例对同一索引的 FileLock 争用——e2e 观察，异常才评估单例桥（Deferred）
- 长链路上下文膨胀——coding agent 跨 N 阶段单进程，依赖 pi 的 compaction；真实树体量下 e2e 观察轮次与时长
