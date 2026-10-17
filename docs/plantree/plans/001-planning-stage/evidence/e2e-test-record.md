# Evidence · 端到端测试（测试阶段 T009，含根因定性轮，十一轮实证）

- 时间：2026-10-17 00:00 – 18:35（Asia/Shanghai）
- 执行者：devforge 会话（AI 替用户应答 UI，用户委托；后段含 API 用户路径活体应答 + 会话文件独立应答器）
- 脚本组：`tests/e2e/test_00…test_08`（9 脚本，纯标准库 Python）+ 线级诊断实验 v1–v5 + 十一次端到端
- 结果：**基础 API 面 66 项检查全过**；端到端链路走到 6/7 步，阻断根因全部定性

## 链路可达终点（已真实观察到）

- 基础 API 五能力域全通；串行守卫 409 双验证 ✓
- 真 pi spawn + 真模型对话 ✓（配置全量复刻后）
- **plan tree 真实产出** ✓（第五/八次：三阶段 13 文件——落在服务器 cwd，P8-ISSUE-04）
- dialog 全环 ✓（第八次：ask-user-question 多连问经 API 用户路径应答，agent 全部消化）
- **review 循环真实运转** ✓（第八次：review-0 通读全 13 文件给出 FAIL+真实语义问题清单→问题回注→规划 Agent 修复轮 128+ 条→review-1 全新实例重审）

## 未观察到（被阻断）

verdict PASS → 约定路径落盘 → 任务 Completed——被两层已定性问题阻断：
① review-1 漏发 verdict 协议块 → SOP 硬错误（P8-ISSUE-10，应保守 FAIL）；
② 中转长 turn 流截断（六次复现，含缓解对照），pi idle 超时被慢滴喂活失效。
devforge 侧放大器（无 run 超时/取消、stderr 黑盒、SSE 丢 ui.request）全部记录，按「只记录不修」约束未动。

## 五次「停滞」真凶（已测试侧修复）

plan-tree skill 占位目录 → agent 发起 `find /` 全盘搜规范 → 单次工具 10-30 分钟 → 窗口必爆。
第十一次已把真实 skill 分发进 profile（测试侧补偿），全盘扫描机制消失。

## 模型固定分配（用户拍板 2026-10-17，P002 decisions/004）

审查 Agent → Grok 4.7（mokeywork/grok-4.7）；编码/规划 Agent → DeepSeek flash（mokeywork/deepseek-flash）。
内置清单已写死，e2e 不再覆盖。

## 完整报告

- 详见 `tests/e2e/TEST_REPORT.md`（线级诊断实验表 v1–v5、十一轮复跑表、根因终审）
- 逐脚本数据：`tests/e2e/results/*.json`；事件时间线：`tests/e2e/results/test_08_timeline.json`

## 对后续阶段的指向

修复顺序建议：①渲染器配置债（auth 形状/models.json 分发/settings 运行时键/真实 skill 分发）；②UI 桥按 method 分流；③run 超时+取消端点；④pi stderr 落日志；⑤spawn 设工作目录；⑥verdict 缺失改保守 FAIL；⑦SSE 丢件修复；⑧中转长 turn 重试型客户端；⑨端到端复测。
