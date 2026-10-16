# devforge 端到端测试报告

- 测试时间：2026-10-17 00:00 – 02:55（Asia/Shanghai）
- 被测版本：`bc3265e`（target/debug，隔离 HOME 运行，不污染真实 ~/.devforge）
- 测试方式：Python 纯标准库脚本逐功能点驱动真实服务器 + 真 pi 子进程 + 真模型（mokeywork/deepseek-flash via newapi.trtyr.top）；UI 交互由 AI 替用户自动应答（用户委托）
- 约定：发现问题**只记录不修**；测试侧为继续验证下游所做的「补偿措施」均显式标注，不计为修复

## 一、结果总览

| 脚本 | 功能域 | 单测结果 | issues |
|------|--------|----------|--------|
| test_00_server | 服务器生命周期/健康/REST/SSE 冒烟 | 8 PASS / 0 FAIL | 0 |
| test_01_projects | 项目管理 API | 8 PASS / 0 FAIL | 1 |
| test_02_tasks | 任务管理 API | 10 PASS / 0 FAIL | 0 |
| test_03_config | 配置管理与 profile 渲染 | 15 PASS / 1 FAIL | 1 |
| test_04_auth | 鉴权 | 2 PASS / 0 FAIL | 1 |
| test_05_events_sse | SSE 事件流 | 6 PASS / 0 FAIL | 0 |
| test_06_artifacts_graph | 产物归集与运行图 | 11 PASS / 0 FAIL | 0 |
| test_07_rerun_serial | 串行守卫/重跑留档（真 pi） | 6 PASS / 1 FAIL | 4 |
| test_08_e2e_planning | 端到端规划流水线（真 pi+真模型） | 10 PASS / 5 FAIL | 2 |
| **合计** | | **76 PASS / 7 FAIL** | **9 脚本级（去重后 14 项发现）** |

同时：Rust 单测 74 + doctest 1 全绿（开发阶段产物未回归）。

## 二、端到端全链路详述（test_08，900s 长窗口）

### 走通的部分

1. 隔离 HOME 起服务器 → 健康就绪 ✓
2. PUT manifest 注入真实 key + 校准后模型 id → 四件套真实安装（git clone SoL-Pi 成功）→ 双 profile 渲染 ✓
3. 项目/任务创建 ✓ → start 202 ✓ → **spawn 真 pi 成功**（sessions/plan/ 会话文件生成）✓
4. 模型真实响应：Plan Agent 收到 kickoff、执行工具调用（搜索 skill、查看 cwd）——**模型链路真实可用**（经测试侧补偿后）✓
5. SSE 事件流实时可观测（run.started / ui.request 全部捕获）✓
6. AI 自动应答 ui.request 3/3 送达 ✓

### 卡住的部分（900s 长窗口决定性证据）

- 模型首轮生成 >900s 未完成。会话增长探针曲线：`[0, 25134, 25134, …]` 后段仅个位数字节微增——**极慢生成（非死流）**：pi 会话文件仍有微小写入，但 15 分钟内首轮 assistant 回复未完成
- 由此下游全部无法发生：无完成标记、无 Review 循环、无 verdict、**无 plan tree 产物**（约定路径与服务器 cwd 均无）、任务永久 `Running`
- 全程收到的 ui.request 全部是 pi-blackhole 的 `setStatus`（fire-and-forget 状态推送），**真正的 ask-user-question 完成确认从未出现**
- 服务器 stderr 可见 pi install 与渲染日志，但 pi 子进程自身 stderr 被丢弃，模型调用是否发出/卡在哪一步**不可观测**

## 三、发现问题清单（只记录，不修）

### high

| ID | 描述 | 证据 |
|----|------|------|
| P8-ISSUE-01 | 规划 run 240s/420s/900s 三次均未结束，任务永久 Running；**系统无 run 超时与取消端点**，编排器只能无限等 | test_07/test_08 时间线；GET runs 恒 Running |
| P8-ISSUE-02 | 全程 ui.request 均为 pi-blackhole 的 setStatus，**真正的 ask-user-question 完成确认从未出现**——完成确认机制（decisions/008）未运转 | test_08 时间线 ui 事件 3 条全是 setStatus |
| P8-ISSUE-05 | plan tree 零产物（约定路径 plans/<项目>/<任务>/ 与服务器 cwd 均无）——decisions/002 的落盘契约未兑现 | 长窗口结束双路径 glob 均空 |
| P8-ISSUE-06 | 模型首轮生成 >900s（增长探针证实极慢生成非死流）——当前模型/提示词配置下流水线事实上不可用 | 增长曲线 [0, 25134, 25134…] |
| P2-ISSUE-01 | 渲染器写出的 auth.json 形状（`{"api_key":…}`）与 pi 期望（provider 键控）不符——pi 拿不到 key | 对比真实 ~/.pi/agent/auth.json |
| P2-ISSUE-03 | 渲染器不分发 models.json——provider 的 baseUrl/模型定义缺失，pi 不知道 newapi 地址 | 渲染产物无 models.json；测试侧复制后模型链路即通 |
| P8-ISSUE-07 | UI 桥接对 fire-and-forget 类请求（setStatus/notify/setWidget/setTitle/set_editor_text）**误发 extension_ui_response**——rpc-extension-ui.md 明确此类不期待应答，协议违规噪音（对无主 id 的应答可能干扰 dialog 相关性） | 自动应答日志 3 条全是 setStatus |

### medium

| ID | 描述 | 证据 |
|----|------|------|
| P7-ISSUE-03 | PUT manifest 同步执行包安装阻塞 47–71s（四件套含 git clone），客户端常规超时必炸；应异步化或返回任务句柄 | 首跑 10s 客户端超时崩溃；v2 实测 47s |
| P1-ISSUE-01 | 目录创建失败的项目残留列表（半注册：store 已插入、ensure_local_dir 失败不回滚） | POST local_path=/dev/null/impossible → 500，但 GET 列表含 badpath |
| P3-ISSUE-03 | token 鉴权预留位无启动配置入口（main.rs 恒 None），401 强制路径无法端到端验证（单测已覆盖） | main.rs 无 token 配置读取 |
| P8-ISSUE-08 | pi 子进程 stderr 被 Stdio::null 丢弃——pi 侧故障完全不可观测，违反可观测原则精神 | RpcClient::spawn；排障时无法看到 pi 诊断 |

### low

| ID | 描述 | 证据 |
|----|------|------|
| P3-ISSUE-01 | 未知模型引用返回 500 而非 400（Config 错误被归 internal） | PUT model=ghost-model → 500 |
| P3-ISSUE-02 | PUT manifest 未落盘真源（manifest_path 恒 None），重启配置丢失 | PUT 后 manifest.json 内容未变 |

### 校准结论（info）

- **P1-CAL-01**：清单 model_id 照录显示名「DeepSeek flash」不可用；真实 id = `mokeywork/deepseek-flash`；provider baseUrl = `https://newapi.trtyr.top/v1`（自建中转，本机直连可达）。curl 实测该 id 200。
- 注意：`api.newapi.top` 公共域在本机被 TLS 阻断（000），自建域直连正常——端到端必须用后者。

## 四、根因分析（三条链）

### 链一 · 配置供给链缺陷（三连击，已被测试侧补偿验证绕过）

渲染器产出的 profile 缺三样 pi 启动必需品：正确形状的 auth.json、models.json（provider 定义）、正确 model_id（settings.json 里是显示名）。**测试侧补偿（复制真实 models.json + 改写 auth.json 形状）之后模型链路立即打通**——证明编排/对话/工具调用主体是好的，配置供给层是坏的。

### 链二 · UI 桥接协议缺陷

pi 的 extension UI 分两类：dialog（select/confirm/input/editor，阻塞等应答）与 fire-and-forget（notify/setStatus/setWidget/setTitle/set_editor_text，**不应答**）。当前桥无差别对一切 extension_ui_request 回 `extension_ui_response`：
- 对 setStatus 的应答是协议噪音；
- 真正的 dialog（ask-user-question）从未到达——Plan Agent 还没走到确认环节就卡在模型生成；
- 两个缺陷相互掩蔽，需要按 method 分流后才能看清 dialog 链路是否健康。

### 链三 · 可观测与运维缺口

模型首轮生成 >900s 期间：无流式进度可看（SSE 不推 message_update）、无取消端点、pi stderr 黑盒、无 run 级超时——四个「无法干预」叠加，让一次慢生成变成一次不可恢复的挂死。可观测原则 §4 的「节点内完整 trace」与「流转可见」在 MVP 数据层只完成了一半（生命周期事件有，对话流/工具流没有）。

## 五、结论

- **基础 API 面（五能力域的静态与查询路径）质量良好**：66 项检查全过，含真实目录落地、SOP 校验、事件推送、串行守卫。
- **端到端规划流水线当前不可用**：可以启动、模型可以对话，但首轮生成 >15 分钟不完成 + 完成确认未运转 + 无 plan tree 产物，三者叠加使「从需求到通过 review 的 plan tree」无法达成。
- 修复方向（供决策，不在本阶段执行）：配置供给链三补（auth 形状/models.json 分发/校准 model_id）→ UI 桥按 method 分流（dialog 路由用户、fire-and-forget 只推 SSE 不应答）→ 运维三件（run 超时+取消端点、pi stderr 落日志、SSE 推对话流）→ 提示词正文与 plan-tree skill 真实分发 → 放开端到端复测。

## 六、测试产物

- 脚本组：`tests/e2e/test_00…test_08` + `common.py` + `run_all.py`（`--quick` 跳过长链路）
- 逐脚本结果：`tests/e2e/results/*.json`
- 事件时间线：`tests/e2e/results/test_08_timeline.json`（run.started → 3×ui.request）
- 复现：`python3 tests/e2e/run_all.py`（全量）或单脚本独立运行
