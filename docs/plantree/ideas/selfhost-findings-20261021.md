# 自举实测问题台账（2026-10-21）

来源：devforge 自举实验（c1–c5，sop=change，目标仓=devforge 自身）+ t26 五轮 e2e +
10-20 深夜两场崩溃恢复。每条带证据；状态分四档：**已修 / 在修 / 立案待排 / 观察项**。
本文件由操作员（江晚棠会话）维护，用户拍板记录见各条「裁决」注。

---

## 一、已修（有提交）

### F1 · 续跑基线重记自锁
- 症状：coding 中途带提交续跑时，`ensure_baseline` 把 `baseline.txt` 重记为当前
  HEAD，已提交步骤从门禁视野消失，按步提交对账自锁（10-20 深夜服务器死亡后
  续跑实测撞死）。
- 修复：基线记录不可变复用（`src/change/gate0.rs`，`baseline.txt` 已存在且非空
  → 直接复用记录 sha），新增单测
  `existing_baseline_record_is_reused_even_after_head_advances`。
- 提交：`3020873`（step 8，操作员热修，impl-steps 补录）。

### F2 · change-plan-agent 结构性失声（缺 ask 包）
- 症状：两场自举 questions_total=0；plan agent 把「必须用户裁决」的歧义全部按
  默认自决。根因 `src/config/manifest.rs:899` `extra_packages: vec![]`（MVP 的
  plan-agent 在 `:785` 挂了 `PLAN_AGENT_EXTRA`），`:1097` 断言把缺陷锁死。
- 修复：c3 场（提交 `3cf6da7`..`676216c` + `5d7bfd0`）——ask 包供给 + 歧义分类
  契约 + plan_audit 裁决硬门（标了必须裁决却没留痕直接打回）+ e2e 真链证明
  （SSE 出现 ui.request、答复原文落计划）。D024 登记。
- 后果实测：c4 场起 plan agent 真实发问（四问四答，全部用户裁决）。

### F3 · e2e 清单装配耦合（夹具空操作）
- 症状：`tests/e2e/test_26_change_e2e.py:138-143` 的 `have`/`extend` 在宿主清单
  已含 change-* 时变成空操作；`:141-142` 清空所有 `extra_packages` 会把 ask 包
  从 change agent profile 里丢掉（c4 场计划审查发现，引擎日志可查）。
- 修复：c3 step 4，按名 upsert + 夹具保留自身 packages。

## 二、在修（c5 场进行中）

### F4 · 回注消息不送达 + 剥行号话术
- 症状：回注消息只有两个出口（`src/change/orchestrator.rs:266/:274` Quality 文本、
  `:262` WARN 日志）——都是运维面，不是 coding agent 输入面；`ChangeState`
  （`src/change/state.rs:40-62`）无失败字段，`coding_kickoff`
  （`src/change/session.rs:136-152`）无失败上下文。且 `route_back_message`
  （`src/change/testing_gate.rs:115`）剥掉所有 path:line 并写死
  「请自己定位原因（本消息不提供文件位置）」。
- 裁决（2026-10-21 ui-1/ui-2/ui-4）：三出口同源（保留行号、删话术、删
  strip_locations）；决策号 D025；MVP plan-review（`output.rs:60` 同款黑箱）
  同修，doc 侧两处记遗留。
- 状态：c5 场 plan 复审 review-2 进行中（21:35 开庭）。

## 三、立案待排

### F5 · 审查输出协议违例处置（c6 靶，裁决已齐）
- 现状：change 侧遇 protocol_violation 直接 Quality 终止
  （`src/change/session.rs:243/332/400`）；test agent 空箱回注（`<route>coding</route>`
  无 `<issues>`）时引擎合成占位文案回注（`session.rs:355-360`，10-21 上午实测
  黑箱死）；MVP 侧是有界重审×3（`src/sop/coding.rs:81`、`src/sop/testing.rs:75`）。
- 裁决（ui-2/ui-3）：违例有界重审×3（对齐 MVP）；空箱回注拒收+追补×3。
- 范围：change 全部审查角色 + MVP plan-review + doc 排查
  （`testing_protocol.rs:292` 不判违例；`doc_protocol.rs:116` 已判）。
- **新增实战证据（10-21 晚 c5 场，第二次生产命中）**：回归全绿（543/0）+
  验收表 17/17 对账完成的前提下，test-review 实例未写任何会话文件
  （`sessions/change-test-reviews/` 目录根本未创建）即返回 FAIL 空箱 →
  引擎合成占位文案回注 → 回注预算 1 次即 Quality 终止。工作质量与死因完全无关。
  附带发现：审查报错路径被当成「测试判负」处理而非基础设施重试（对照 doc SOP
  的 System 语义），且回注预算实际为 1。

### F6 · 引擎过程不可观测（c7 靶，与死亡诊断合并）
- 证据 A：boot 期基础设施重试**确定性复现**——c1/c2/c3/c4/c5 五场全部在
  start 后 ~10s 打出「基础设施故障，自动重启 run（attempt=2）」WARN（各场
  devforge.log.jsonl 可查；c5 场 20:18:57 第五连）。成因未查，日志不足以归因。
- 证据 B：plan 段 15 分钟空洞（10-21 c5 场 21:20–21:35）：plan agent 交卷后到
  重审开庭之间，引擎无任何可归因活动记录。
- 证据 C：阶段转换/fix 往返/审查开庭均无 milestone 日志；tracing 非阻塞落盘
  在进程死亡时丢缓冲。
- 证据 D：10-20 深夜服务器两次静默死亡（change run testing 阶段；内核零 OOM、
  journal 零痕迹、stderr 零遗言；kprobe 盯梢的第三场未复现）——悬案，
  依赖本条的取证能力（panic hook 落盘、阶段心跳、验尸文件）。

### F7 · 续跑稳健化全家桶（c8 靶）
- F1 只修了重记；边界未穷尽：baseline.txt 缺失/为空/损坏、记录 sha 不在当前
  历史、coding 中途 cancel 后 fresh 语义等。审计时已标注 resume 为稳健性边界
  （T011b 验收点之外）。

### F8 · plan 阶段延迟的结构性优化（待排）
- 每轮修复触发全量重审：review-2 把 109 处 path:line 从头重核（上一轮已核
  ~104 处且未变）。无引用核验缓存、无增量审查。
- flash 模型 800K+ 会话每 turn 全量上下文，单 turn 分钟级。
- 参考实测：c5 plan 段 80+ 分钟（含两问往返 10 分钟、初审 5 分钟、复审 18.5
  分钟、15 分钟引擎空洞→F6）。

### F9 · 宽靶计划必死的结构风险（流程约束，非代码）
- c2（三合一）与 c4（四裁决宽靶）均 3 轮修复打满、第 4 轮终审仍 FAIL →
  Quality 终止；c3/c5 窄靶均过审。审查门本身工作正常（c4 终审意见质量极高，
  含「覆盖了一颗不存在的雷」级发现），约束应在喂靶粒度：**单场单靶**。
- 裁决：拆单重跑（10-21）。

## 四、观察项（不立案，记录在案）

- **模型单一**：全链 minimax-m3.1-flash（mokeywork deepseek 渠道 Invalid token、
  unprefixed 402——10-21 复测仍死）。模型多样性从未实测。
- **docgen 用量累计口径**：run 报告 tokens 含该任务历史所有 session，净成本需差分
  （10-20 T025 已发现，口径未改）。
- **40 字 slug 截断**：change-id 尾部截断，cosmetic（t26 v5 登记未修）。
- **自举驱动器工程化欠账**（操作员侧，非产品）：ui 哨兵一次性语义漏题（ui-2 空转
  ~70 分钟）、答案文件按行偏移消费却被重排 shift（ui-2 卡送 6 分钟）。教训：
  哨兵答完即重挂；答案文件只追加不重排。
- **c4 计划文档幽灵引用**：未落地的 c4 计划用了 D025 号，裁决「编号跟落地现实走」
  ——D025 已由 c5 正式占用（ui-2，10-21）。

---

## 汇总

| 档 | 条目 |
|---|---|
| 已修 | F1 F2 F3 |
| 在修 | F4（c5） |
| 待排 | F5(c6) F6(c7) F7(c8) F8 F9 |
| 观察 | 模型单一 / 用量口径 / slug / 驱动器欠账 / D025 幽灵 |
