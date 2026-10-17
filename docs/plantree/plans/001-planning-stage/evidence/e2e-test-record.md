# Evidence · 端到端测试（测试阶段 T009，十六轮实证，全链路走通）

- 时间：2026-10-17 00:00 – 19:30（Asia/Shanghai）
- 执行者：devforge 会话（AI 替用户应答 UI，用户委托；含 API 用户路径活体应答 + 会话文件独立应答器）
- 结果：**基础 API 面 66 项检查全过；第十六次端到端七环全通，全链路走通**

## 终局：第十六次七环全通（2026-10-17 19:26）

创建项目 → 任务 → 规划流水线（多轮 dialog 逐问应答，15+ 文件树含 D001–D006）→ review-0 FAIL+问题清单 → 修复 → review-1 重审 → **`<verdict>PASS</verdict>`** → run **Settled** → 任务 **Completed**（API 实测）。

- 逐环证据：`tests/e2e/results/RUN16_FULL_CHAIN.md`
- 树落盘在服务器 cwd（P8-ISSUE-04 路径偏差，记录未修）
- 生效补偿（全测试侧）：真实 skill 分发/httpIdleTimeoutMs=60s 流挂死快判自愈/会话应答器/配置全量复刻

## 链路可达终点（已真实观察到）

- 基础 API 五能力域全通；串行守卫 409 双验证 ✓
- 真 pi spawn + 真模型对话 ✓（配置全量复刻后）
- **plan tree 真实产出** ✓（第五/八次：三阶段 13 文件——落在服务器 cwd，P8-ISSUE-04）
- dialog 全环 ✓（第八次：ask-user-question 多连问经 API 用户路径应答，agent 全部消化）
- **review 循环真实运转** ✓（第八次：review-0 通读全 13 文件给出 FAIL+真实语义问题清单→问题回注→规划 Agent 修复轮 128+ 条→review-1 全新实例重审）

## 未观察到（历史轮次，第十六次已解决）

review 通过/任务 Completed 在第 1–15 轮被两层问题阻断（review 漏协议块/中转长 turn 流截断），第十六次经供给链补齐与流快判自愈后全部解决；历史定性详见 TEST_REPORT.md。

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

---

# 修复后复测（第 17 试，2026-10-17 21:36–21:56，修复阶段 commit 991acaf 之后）

## 结果：链路首问被 dialog 挂起阻塞 28 分钟，未走通；但拿到两条重量级根因实证

## 实证一（代码级）：P8-ISSUE-02 真因 = 双通道断裂

`UiRelay::ask_blocking` 把 ui.request 广播进 **UiRelay 私有的 `requests` 通道**（mod.rs:69），
而 `/events` SSE 端点只订阅 `ServerState.events` 领域通道（handlers.rs:289）。
`requests` 通道除单测助手 `subscribe_requests()` 外**无任何消费者**——
ui.request 从未到达任何 SSE 客户端，前端/autopilot 天生耳聋。
run 8 的「autopilot 聋」、修复阶段未覆盖本项，全部由此解释。

## 实证二（行为级）：dialog-pending 变体签名 + 取消半效

- 首问 dialog 挂起期间，**会话文件尾是 blackhole 观察记录而非 assistant toolCall 行**
  ——按会话文件找 toolCall 的应答器对此变体全盲；
- 21:39:30 会话静止后：发 cancel（200）→ pi 被 watchdog 精确击杀（/proc 实证）→
  **run 仍 Running**——引擎线程阻塞在 `ask_blocking`（mpsc recv 无死亡信号），
  杀 pi 解不了 dialog 等待；**应答挂起 dialog（POST /ui/ui-1/answer 200）才解阻** → run 即 Failed；
- 即 P8-ISSUE-01 的 cancel 在 dialog-pending 态只有半效：pi 死 ≠ 引擎解阻。

## 现场数据

- 装包+渲染 124s（修复后异步化，远优于历史 10-15 分钟同步阻塞）；
- 规划 Agent 正常起跑：读真实 plan-tree skill（无全盘扫描）、产出 6 条观察（21:39）、
  判定 greenfield 需澄清 → 发首问 dialog（deepseek-flash）；
- plans/demo-app/planning-run 目录已建（P8-ISSUE-04 修复生效），树未落（阻塞于首问）；
- 三个历史孤儿 pi 进程（test_09/10 批次遗留）已按 PID 清理。

## 对策（第 18 试）

应答器升级探测优先：ui-N 游标轮询（4s）不依赖会话签名也不依赖 SSE（commit 6545429）。
