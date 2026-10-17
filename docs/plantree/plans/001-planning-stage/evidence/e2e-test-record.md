# Evidence · 端到端测试（测试阶段 T009，含根因定性轮）

- 时间：2026-10-17 00:00 – 16:25（Asia/Shanghai，含上午设计轮 + 傍晚审计驳回后的定性轮）
- 执行者：devforge 会话（AI 替用户应答 UI，用户委托）
- 脚本组：`tests/e2e/test_00…test_08`（9 脚本，纯标准库 Python）+ 三个线级诊断实验（v1/v2/v3）
- 结果：**基础 API 面 66 项检查全过**；端到端六次尝试，阻断根因已决定性定性

## 链路可达终点（已观察到）

- 基础 API 五能力域全通；串行守卫 409 双验证 ✓
- 真 pi spawn + 真模型对话 ✓（配置全量复刻补偿后）
- **plan tree 真实产出** ✓（第五次：README/roadmap/open-questions + decisions/ideas/topics 全套——但落在服务器 cwd 而非约定路径，P8-ISSUE-04）
- UI 中继链路全环验证 ✓（桥→UiRelay→SSE→智能应答→回填，5/5 与 2/2）

## 未观察到（被阻断）

review 循环 → verdict → 约定路径落盘 → 任务 Completed——被「provider 长turn 流截断」阻断：
模型 turn 已发出 thinking/toolCall 但流永不完成，三次连续 run 复现（含一次无并发干净对照）；
pi 侧健康（v1/v2/v3 线级实验：dialog 发射正常、635 次 fire-and-forget 应答无异常、活满 601s、stderr 零字节）、
桥与中继链路健康（UI 全环 200）、配置供给链已按用户拍板全量复刻（models.json/auth.json/settings 运行时键/插件配置原封）。
devforge 侧放大器：无 run 超时/取消 + pi stderr 黑盒（均已记录，按约束不修）。

## 模型固定分配（用户拍板 2026-10-17，P002 decisions/004）

审查 Agent → Grok 4.7（mokeywork/grok-4.7）；编码/规划 Agent → DeepSeek flash（mokeywork/deepseek-flash）。
内置清单已写死，e2e 不再覆盖。

## 完整报告

- 详见 `tests/e2e/TEST_REPORT.md`（根因三链 + 线级诊断实验表 + 最终定性）
- 逐脚本数据：`tests/e2e/results/*.json`；事件时间线：`tests/e2e/results/test_08_timeline.json`

## 对后续阶段的指向

修复顺序建议：①配置供给三补（auth 形状/models.json 分发含 maxTokens/settings 运行时键——渲染器欠账）；②UI 桥按 method 分流（fire-and-forget 只推 SSE 不应答）；③run 超时+取消端点；④pi stderr 落日志；⑤spawn 设置工作目录（plan tree 落约定路径）；⑥SSE 推对话流（可观测）；⑦提示词正文与 plan-tree skill 真实分发；⑧端到端复测（中转长 turn 可靠性需上游配合或重试型客户端）。
