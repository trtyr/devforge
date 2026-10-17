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

## 实证一（23:5x 纠错）：P8-ISSUE-02 原判「双通道断裂」撤回——真因在测试基建

**纠错**：原判称 UiRelay 将 ui.request 广播进私有 requests 通道且无消费者。修复阶段源码复核：
`UiRelay::new(events.clone())` 存的就是共享 events 通道（main.rs:31-32、fixture 同构），
产品侧 SSE 契约无恙，新增回归单测 `sse_delivers_ui_request` 钉死该契约。

**真因**：测试基建 `SseStream` 的 HTTP 连接带 10s socket 读超时，而服务器 SSE keepalive 间隔 15s——
任何静默段先触发 `socket.timeout`（OSError 子类）被 `_pump` 的 `except OSError: pass` 静默吞掉，
**泵线程悄悄退场 = SSE 消费者从首个 >10s 静默段起永久耳聋**。run.started 恰在订阅后首窗口内到达故被捕获，
后续 ui.request/run.finished 全部丢失——历史「autopilot 耳聋」「SSE 流掉线」全部由此解释。
已修：读超时 120s（>15s keepalive）+ TimeoutError 继续读不退泵（common.py）。

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

---

# 第 18 试（2026-10-17 21:43–22:40，探测优先应答器）：史上最深链路 + 第三条根因

## 走到的深度（全部首次）

- 装包 ~14 分钟 → 规划起跑 → **应答器 4s 内接住首问**，ui-1…ui-12+ 全部接住
  （含技术栈决策「接受 Next.js + Prisma + SQLite（推荐）」、范围决策「不加，保持纯认证（推荐）」）；
- **plan tree 首次落约定路径** `~/.devforge/plans/demo-app/planning-run/`：
  README/open-questions/roadmap + baseline（module-map/storage-and-state/runtime-flows/risk-hotspots/test-and-release-gates）
  + decisions（001-tech-stack/002-auth-strategy）+ ideas/inbox + topics（product-scope/architecture/auth-and-session）
  —— 15 文件 5 目录，test_08 脚本级 PASS（文件数=19 含目录）；
- **review-0（grok-4.7）真实 FAIL+问题清单 → 回注修复 → review-1 全新实例重审 →
  会话逐字 `<verdict>PASS</verdict>`**（22:08:34，stopReason=stop，usage 完整，130K 审查会话）；
- 全程 12+ dialog 应答零失败，应答器探测制实战验证。

## 第三条根因（新缺陷，只记录）：verdict 后挂起

PASS verdict 落盘（22:08:34）后 run 迟迟不翻 Settled（至 22:39 脚本兜底仍 Running）：

- 全部 pi 进程自然消失（/proc 实证），**零 stderr**（pi-stderr.log 均未创建 = 清洁退场）；
- 引擎线程堵在 `review() → wait_settled read_line`（rpc_bridge.rs:76）——EOF 应达而未达；
- 无孙进程持 fd（cwd 全扫仅 devforge 自身）；服务端 stderr 无 panic；
- 疑点：pi 回合完成后自行退场（何种触发？真源 blackhole agentMaxTurns=16？）且 EOF/settled 链路
  未按预期唤醒引擎。修复阶段源码级诊断。

黑盒配对证据：test_08 自有 SSE 流 **ui.request 事件数=0**，同期应答器实际接住 12+ dialog——
双通道断裂（实证一）的正反两面同时闭环。

## test_07 重写版（22:24–22:33）：17/17 全绿零缺陷

守卫双 409、cancel 收场 1s 内 run Failed、串行释放验证、重跑留档（runs=2/旧 run 不变/current_run 指向最新）。

## 第 19 试动机

合同要求 Completed 状态实证；verdict 后挂起机制不明，随机性未知（修复前 run 16 曾完整走通），
再赌一次终态翻盘。

---

# 第 21 试（2026-10-18 00:00–00:05，修复后首跑）：全链七环走通，Completed 实证到手

三笔修复（SseStream 读超时 / watchdog scope 自锁 / cancel abort_all）后首次端到端：

- **5 分 02 秒全程**（史上最快；装包 ~2 分钟 + 规划 + 审查）；
- 规划会话 155K，树 5 文件落约定路径（README/roadmap/open-questions/decisions/001-tech-stack/topics/scope）；
- **SSE 时间线全环捕获：run.started → ui.request×5 → config.rendered → run.finished**——
  SseStream 修复后 autopilot 全程在岗，**5 个 dialog 全部经 SSE 应答（answered=5, requested=5）**，
  应答器（会话文件探测制）全程未被需要；
- review-0（grok-4.7，71K 审查会话）逐字 `<verdict>PASS</verdict>`（stopReason=stop，usage 完整）：
  「语义审查完成。规划意图、范围、任务顺序与决策互相引用一致…未发现前后矛盾或语义不通。」
- **run 终态 Settled + 任务终态 Completed（API 实证）**——watchdog 自锁修复生效，
  第 18/19/20 三试的挂起点一去不返；
- test_08 正式 JSON：**15 PASS / 0 FAIL**（含 run.finished ok=True / Settled / Completed / 树路径四项关键断言）。

七环对账：创建项目 ✓ → 任务 ✓ → 真实 pi 规划（多轮 dialog 代理）✓ → 树落约定路径 ✓ →
review 循环 ✓ → 逐字 PASS verdict ✓ → **任务 Completed ✓**。

---

# 第 19 试（2026-10-17 22:33–22:55）：直 PASS 复现 + 挂起二连，黑盒终点确认

- 装包缓慢（~20 分钟，外部克隆抖动）→ 规划起跑 → 首问盲答「完成」（此试会话尾无 toolCall 签名，
  探测制盲答路径实战验证）→ 树 12+ 文件连续落约定路径（README/roadmap/baseline/decisions/D001-language-stack/
  D002-persistence/topics/acceptance/architecture/scope/implementation-status/open-questions/ideas/history——
  含 Provisional 决策、OQ-01…05、Phase 0–2 roadmap 的完整规划结构）；
- **review-0 首审直 PASS**（22:50:34，逐字 `<verdict>PASS</verdict>` + 语义审查结论 + usage 完整，
  stopReason=stop，24.5K tokens）——无 FAIL 轮直接通过；
- **挂起二连**：PASS 后 3 分钟起 run 仍 Running，全部 pi 清洁退场（零 stderr、无孙进程持 fd），
  与第 18 试同签名——**连续两次复现，定性为系统性新缺陷**；
- 第 17/18/19 三试合并结论：前六环（创建→任务→规划→树落约定路径→review 循环→逐字 PASS verdict）
  全部实证且可重复；第七环（Completed 翻转）被 verdict 后挂起阻断，「只记录不修」下为黑盒可达终点。
