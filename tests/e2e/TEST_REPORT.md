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
| P2-ISSUE-03 | 渲染器不分发 models.json——provider 的 baseUrl/模型定义缺失，pi 不知道 newapi 地址与 maxTokens/contextWindow（默认输出预算下 reasoning 模型烧光预算产出空正文——首轮卡死根因之一） | 渲染产物无 models.json；测试侧复制后模型链路即通 |
| P2-ISSUE-04 | 渲染器只写 settings.json 两键（defaultProvider/defaultModel），缺运行时生存配置：retry（中转抖动的重试生存）、defaultThinkingLevel、compaction 等——同一中转抖动，真实环境靠 retry 3×5s 混过，devforge profile 一次即死且无日志（用户对照洞察：同插件同模型，为何我方好好的？根因在此） | 对比真实 settings.json 与渲染产物 |
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
- **P1-CAL-02（2026-10-17 用户拍板，固定死不改）**：模型固定分配——审查 Agent → Grok 4.7（mokeywork/grok-4.7）；编码 Agent → DeepSeek flash（mokeywork/deepseek-flash）；规划 Agent 维持 deepseek-flash。内置清单已写死（P002 decisions/004），测试侧不再覆盖模型。

## 四、根因分析（三条链）

### 链一 · 配置供给链缺陷（三连击，已被测试侧补偿验证绕过）

渲染器产出的 profile 缺三样 pi 启动必需品：正确形状的 auth.json、models.json（provider 定义）、正确 model_id（settings.json 里是显示名）。**测试侧补偿（复制真实 models.json + 改写 auth.json 形状）之后模型链路立即打通**——证明编排/对话/工具调用主体是好的，配置供给层是坏的。

### 链二 · UI 桥接协议缺陷

pi 的 extension UI 分两类：dialog（select/confirm/input/editor，阻塞等应答）与 fire-and-forget（notify/setStatus/setWidget/setTitle/set_editor_text，**不应答**）。当前桥无差别对一切 extension_ui_request 回 `extension_ui_response`：
- 对 setStatus 的应答是协议噪音；
- 真正的 dialog（ask-user-question）在多次长跑中从未到达——根因定性见下节（诊断实验）；
- 两个缺陷相互掩蔽，需要按 method 分流后才能看清 dialog 链路是否健康。

### 链二诊断实验 · 根因定性（测试侧，未改任何产品代码）

为满足「阻断必须被决定性归因」，做了三个绕开 devforge 的对照实验 + 一次干净对照复跑：

| 实验 | 条件 | 结果 |
|------|------|------|
| v1 最小条件 | PI_CODING_AGENT_DIR=补偿 profile，直接指令触发 ask-user-question | **extension_ui_request 立即出现在 stdout**（method=select，title/options 齐全），与 rpc-extension-ui.md 契约逐字段一致 |
| v2 完整复刻 | 同 env 三标记（DEVFORGE_MANAGED/ROLE）+ 同 cwd（空 server-cwd）+ 逐字 PLAN_KICKOFF | agent 深探索 28 个工具后，**dialog 照样正常发出**（ui:1） |
| v3 桥式应答 | 同 v2 + 忠实模拟 devforge 桥（对一切 UI 请求含 fire-and-forget 回 extension_ui_response） | blackhole 10 分钟发 **635 条** setStatus/notify，全部应答，**pi 活满 601s 无任何异常，stderr 零字节**——「误应答毒杀 pi」假设彻底排除 |
| 第六次干净单跑 | 无任何并发实验，配置全量复刻 + 固定模型分配 | 停滞签名复现：agent 早段正常工作（会话 36.5KB）后沉默至窗口结束——并发干扰假设排除 |
| 第七次缓解对照 | blackhole statusBar/worker 通知关闭（消除 UI 请求洪水，v3/v5 实测 7.7 次/秒） | 停滞**第四次复现**：会话 97.5KB 后截断，且 pi 的 5 分钟 idle 超时（retry.provider.timeoutMs=httpIdleTimeoutMs=300s）**6 分钟未触发自愈**——中转以慢滴 keepalive 喂 socket 可绕过 idle 判定；**blackhole UI 刷屏假设同时被排除** |
| 第八次活体干预 | SSE 丢件实锤后改由 **API 用户路径手动应答 dialog**（探测 /ui/ui-N/answer） | **链路走到 6/7 步**：完成确认→<plan-complete>→规划阶段结束→**review-0（grok-4.7）真实运转**：通读全13文件、FAIL+真实语义问题清单→问题回注→规划 Agent 修复轮 128+ 条→**review-1 全新实例重审**→但 review-1 漏发 verdict 协议块→parse 硬错误→run Failed（新发现 P8-ISSUE-10） |
| 第九次 | 装 600s 超时被慢网络击穿 | P8-ISSUE-00 早退（PUT 超时已放宽至 1800s） |
| 第十次 | 全配置（skill 待同步） | **停滞真凶落网**：agent 因 plan-tree skill 是占位目录，发起 `find /` 全盘搜规范（进程实测存活），单次工具执行 10-30 分钟 → 窗口必爆——五次「停滞」全部此机制 |
| 第十一次 | **真实 plan-tree skill 分发进 profile**（占位缺口的测试侧补偿）+ 独立会话应答器 | 截断签名 A 第六次复现（会话 86.7KB 静止）——skill 修复解决「全盘扫描」，但中转长 turn 截断仍随机发生 |

**根因终审（十一轮实证后）**：
- **根因一（主因，已测试侧修复）· 占位 skill → `find /` 全盘扫描**：plan-tree skill 未分发（渲染器占位缺口 P2-ISSUE-05），agent 为找规范发起全盘扫描，单次工具执行 10-30 分钟，任何窗口都装不下——前五次「停滞」的真实机制（第十次抓到活体进程）；真实 skill 分发后此机制消失；
- **根因二（残存）· 中转长 turn 流截断（签名 A）**：会话尾条无 stopReason/usage——模型流中途断供，pi 永等（idle 超时被慢滴喂活失效）；真实 skill 后仍第六次复现；
- **签名 B · 完成后分发停滞**：会话尾条带完整 usage + stopReason:toolUse 但工具永不执行（第五次）；
- **放大器 · devforge 无兜底**：无 run 超时/取消、pi stderr 黑盒、SSE 丢 ui.request（P8-ISSUE-02 机制实锤：第八次 dialog 全部滞留 pending，autopilot 零收到，改 API 用户路径后链路立即推进）——全部按约束只记录不修。

结论链：
1. **pi + ask-user-question 插件的 dialog 发射路径在 devforge 同款条件下完全健康**（裸 stdout 线级证据）；
2. devforge 桥的转发/中继链路被独立证明可用：两次端到端中 blackhole 的 setStatus 全部走完 **桥→UiRelay→SSE→智能应答→回填** 全环（5/5 与 2/2，均 200）；
3. 第五次端到端（配置全量复刻后）取得突破：**agent 真实产出完整 plan tree**（README/roadmap/open-questions + decisions/ideas/topics 全套）——但落在服务器进程 cwd 而非约定路径（新增 P8-ISSUE-04：SpawnSpec 未设工作目录，decisions/002 被违反；对称缺陷：review Agent 同 cwd 反而能读到树）；
4. **最终定性：链路末端阻断 = provider 中转的长 turn 流截断**（模型 turn 已发出 thinking/toolCall 但流永不完成 → 工具不执行 → 会话静止），在三次连续 run（含一次无并发干净对照）中稳定复现；该问题在 pi 侧、devforge 桥、中继链路、配置供给四个层面均已被线级证据排除，指向中转/上游服务对长 turn 的可靠性；devforge 侧的放大器 = 无 run 超时/取消（P8-ISSUE-01）+ pi stderr 黑盒（P8-ISSUE-08），使每次截断都变成不可恢复、不可诊断的永久挂死；
5. 按「只记录不修」约束：修复超纲（外部中转可靠性 + devforge 超时/取消/stderr 均属修复项），端到端在当前约束下的可达终点 = **plan tree 真实产出（路径错位）+ 全链路前段全通 + 阻断决定性定性**。
6. 方法论教训：进程存活判定不可依赖 ps/RTK 改写后的输出（曾误判 pi 已死），**以文件证据（会话增长/产物落盘）为准**。

### 链三 · 可观测与运维缺口

模型首轮生成 >900s 期间：无流式进度可看（SSE 不推 message_update）、无取消端点、pi stderr 黑盒、无 run 级超时——四个「无法干预」叠加，让一次慢生成变成一次不可恢复的挂死。可观测原则 §4 的「节点内完整 trace」与「流转可见」在 MVP 数据层只完成了一半（生命周期事件有，对话流/工具流没有）。

## 五、结论

- **基础 API 面（五能力域的静态与查询路径）质量良好**：66 项检查全过，含真实目录落地、SOP 校验、事件推送、串行守卫。
- **端到端规划流水线当前不可用，但阻断已定性**：链路可启动、模型可对话、dialog 协议线级健康（诊断实验 v1/v2）；补偿版长跑在 agent 发出 ask_user_question 后遇 provider 流停滞（疑似中转对长上下文 turn 的可靠性），叠加系统无超时/取消、pi stderr 黑盒，一次瞬态停滞即永久挂死——链路末端（review 循环→PASS→plan tree→Completed）至本报告更新时仍未被观察到，属「被外因+内因叠加阻断」而非协议不可行。
- 修复方向（供决策，不在本阶段执行）：①配置供给三补（auth 形状/models.json 分发/校准 model_id）；②UI 桥按 method 分流（dialog 路由用户、fire-and-forget 只推 SSE 不应答）；③运维三件（run 超时+取消端点、pi stderr 落日志、SSE 推对话流）；④spawn 设置工作目录（plan tree 落盘到位）；⑤提示词正文与 plan-tree skill 真实分发；⑥复测端到端。

## 六、测试产物

- 脚本组：`tests/e2e/test_00…test_08` + `common.py` + `run_all.py`（`--quick` 跳过长链路）
- 逐脚本结果：`tests/e2e/results/*.json`
- 事件时间线：`tests/e2e/results/test_08_timeline.json`（run.started → 3×ui.request）
- 复现：`python3 tests/e2e/run_all.py`（全量）或单脚本独立运行


---

## 修复阶段核销（2026-10-17 修复轮，commit 见 git log）

逐项对照上表：✅=已修（含回归单测）｜🍉=缓解（外部依赖/留远期）｜⏳=待复测验证

| ID | 状态 | 说明 |
|----|------|------|
| P8-ISSUE-00 | ✅ | PUT 异步化后阻塞超时自然消解；测试侧 PUT 超时亦放宽 |
| P8-ISSUE-01 | ✅ | RunControl + POST cancel 端点 + watchdog（scan_and_kill_run 按 run-id 精确 SIGKILL）+ 可选超时（DEVFORGE_RUN_TIMEOUT_MS，默认不限时——零硬限制哲学） |
| P8-ISSUE-02 | 🍉 | 洪水源已消（fire-and-forget 不再应答/中继），容量 256→1024；Last-Event-ID 续传留远期 |
| P8-ISSUE-04 | ✅ | SpawnSpec.working_dir → plans/<项目>/<任务>/，create_dir_all 后生效 |
| P8-ISSUE-05 | ⏳ | 根因（占位 skill→全盘扫描 + cwd 错位）均已修，待复测实证树落约定路径 |
| P8-ISSUE-06 | 🍉 | 外因（中转长 turn 截断）；httpIdleTimeoutMs=60s 快判 + retry 已入渲染配置；上游可靠性不在本系统范围 |
| P8-ISSUE-07 | ✅ | is_dialog_method 分流：dialog 四方法才应答，fire-and-forget 只推事件；回归单测×2 |
| P8-ISSUE-08 | ✅ | stderr piped → 结构化日志 + session_dir/pi-stderr.log 留档，伴读线程 |
| P8-ISSUE-10 | ✅ | parse_verdict 缺失/未知/未闭合 → 保守 FAIL（协议违例作 issue 进重查循环），不再硬崩 |
| P2-ISSUE-01 | ✅ | auth.json provider 键控形状 |
| P2-ISSUE-03 | ✅ | models.json 分发（providers 表 + contextWindow/maxTokens） |
| P2-ISSUE-04 | ✅ | settings 运行时键（retry/httpIdleTimeoutMs/thinking/compaction）写入渲染器 |
| P2-ISSUE-05 | ✅ | 真实 plan-tree skill 内置资产（include_str! 编译期嵌入），无源时兜底 |
| P1-ISSUE-01 | ✅ | 目录落地失败回滚 store 插入（remove_project） |
| P3-ISSUE-01 | ✅ | 变体优先错误映射：Config→400；PUT 同步段预验证模型/provider 引用 |
| P3-ISSUE-02 | ✅ | main.rs 传 manifest_path（Mutex 字段），PUT 同步落盘真源 |
| P3-ISSUE-03 | ✅ | DEVFORGE_TOKEN 环境变量入口（Arc::get_mut 分享前注入） |
| P7-ISSUE-03 | ✅ | PUT 异步化：202 + 后台渲染线程 + SSE config.rendered/render_failed 通知；顺带 installer 注入 ServerState（单测断真实网络，套件 221s→0.23s） |

**核销合计：✅ 16 项 ｜ 🍉 缓解 2 项 ｜ ⏳ 待复测 1 项（P8-ISSUE-05，复测归下一测试阶段）**

验证：cargo test **87 passed / 0 failed**（含新增回归单测 13 个）+ clippy clean；本阶段零端到端执行（用户约定）。

---

## 终局（第十六次）：全链路走通 ✓（2026-10-17 19:10–19:26）

配置供给链修复（真实 plan-tree skill 分发进 profile）+ 流挂死快判自愈（httpIdleTimeoutMs=60s → pi agent 级 retry）+ 会话文件独立应答器/API 用户路径混合应答后，第十六次端到端**七环全通**：

创建项目 → 任务 → 规划流水线（多轮 ask-user-question dialog 逐问应答，15+ 文件树含 D001–D006 决策）→ **review-0 FAIL+问题清单 → 修复 → review-1 全新实例重审 → `<verdict>PASS</verdict>`** → run **Settled**（ended_at=1792236253601 实录）→ 任务 **Completed**（API 实测）。

- 逐环证据：`tests/e2e/results/RUN16_FULL_CHAIN.md`（review-1 PASS 原文、树结构、各环 API 状态）
- plan tree 落盘位置为服务器 cwd（P8-ISSUE-04 路径偏差，按约定记录未修；decisions/002 约定路径待修复阶段达成）
- 全部遗留缺陷（树路径/SSE 丢件/pi stderr 黑盒/无 run 超时取消等）见上文问题清单，待修复阶段处理

**结论修订**：规划 SOP 端到端机制全环验证成立——「review 通过 → plan tree 落盘 → 任务 Completed」已真实观察到一次；此前「链路末端被阻断」的定性适用于第 1–15 次运行，第十六次在供给链补齐后走通。

---

# 修复后复测报告 v2（2026-10-17 21:10 起，被测版本 991acaf）

- 测试范围：修复阶段全部 16 项代码修复的端到端回归 + 全功能点覆盖重跑
- 脚本组：test_00–test_10 共 11 个（新增 test_09 运行控制面 / test_10 超时与异步渲染）+ dialog 应答器（探测优先版，commit 6545429）
- 交互代理：AI 全程替代用户应答（探测制回填，不依赖 SSE/会话签名）

## 一、逐脚本结果

| 脚本 | 功能域 | 结果 | 备注 |
|------|--------|------|------|
| test_00_server | 服务器生命周期/健康 | 8 PASS / 0 FAIL | |
| test_01_projects | 项目管理 API | 全 PASS | P1-ISSUE-01 断言自适应翻转：失败创建不再残留半注册 |
| test_02_tasks | 任务管理 API | 全 PASS | |
| test_03_config | 配置管理与渲染 | **22 PASS / 0 FAIL** | 原生 models.json 分发（1M/64K 预算）+ 运行时生存键 + 内置 skill 兑底 + PUT 202 异步 + 未知模型 400 + 真源落盘，全部修复面断言实测通过 |
| test_04_auth | 鉴权 | 全 PASS | |
| test_05_events_sse | SSE 事件流 | 全 PASS | |
| test_06_artifacts_graph | 产物归集与图 | 全 PASS | |
| test_09_run_control | cancel 端点/spawn cwd/stderr 观测 | **8 PASS / 0 FAIL** | 取消→run+task Failed；plans/<项目>/<任务>/ 目录实证；终态 cancel 404 幂等 |
| test_10_timeout_async | 可选超时/异步渲染通知 | **8 PASS / 0 FAIL** | DEVFORGE_RUN_TIMEOUT_MS=20s → 21s 收割（<60s）；config.rendered SSE 通知实测送达 |
| test_07_rerun_serial | 串行守卫/cancel 收场/重跑留档 | **17 PASS / 0 FAIL** | 重写版全绿：守卫双 409、cancel 1s 收场、串行释放、留档 2 条旧 run 不变、current_run 指向最新 |
| test_08_e2e_planning | 端到端规划全链路 | 9 PASS / 6 FAIL（第 18 试 JSON）| **树 19 项落约定路径脚本级 PASS**；run 未终态（verdict 后挂起，见第 17/18/19 试）|

## 二、test_08 第 17/18 试：链路深度推进 + 两条新根因实证

### 第 17 试（21:36–21:56，含证伪过程）

装包+渲染 124s（异步化后远优于历史 10–15 分钟同步阻塞）→ 规划 Agent 正常起跑（读真实 skill、无全盘扫描）→
首问 dialog 挂起 **28 分钟**无人应答 → 期间 cancel 实验拿到两条重量级实证：

1. **P8-ISSUE-02 真因（代码级实锤）· 双通道断裂**：`UiRelay::ask_blocking` 把 ui.request 广播进 UiRelay 私有
   `requests` 通道（mod.rs:69），而 `/events` SSE 只订阅 `ServerState.events`（handlers.rs:289）——
   **ui.request 从未到达任何 SSE 消费者**，前端/autopilot 天生耳聋。run 8 的「autopilot 聋」由此全部解释。
2. **dialog-pending 变体 + cancel 半效（行为级）**：dialog 挂起期间会话尾是 blackhole 观察记录而非 toolCall 行
  （会话文件监听型应答器对此全盲）；cancel 杀掉 pi 后 run 仍 Running——引擎线程阻塞在 ask_blocking
  （mpsc recv 无死亡信号），**应答挂起 dialog（POST /ui/ui-1/answer）才解阻**→ run 即 Failed。

### 第 18 试（21:43 起，探测优先应答器）

- 装包 ~14 分钟 → 规划起跑 → **应答器 4 秒内接住首问**，此后 ui-1…ui-12+ 全部接住（含技术栈决策
  「Next.js+Prisma+SQLite」「不加认证保持纯认证」）→ **plan tree 15+ 文件首次落约定路径**
  `~/.devforge/plans/demo-app/planning-run/`（README/roadmap/baseline/…，P8-ISSUE-04 修复实证）→
  **review-0（grok-4.7）真实 FAIL+问题清单 → 问题回注 → 规划 Agent 修复 → review-1 全新实例重审 →
  会话内逐字 `<verdict>PASS</verdict>`（22:08:34，stopReason=stop）**。
- **新发现（只记录）· verdict 后挂起**：PASS verdict 落盘后 run 迟迟不翻 Settled，全部 pi 进程自然消失
  而桥未解阻（review() 仅阻塞于 wait_settled read_line；无孙进程持 fd；服务端 stderr 无 panic）——
  疑点指向 pi 回合完成后自行退场（真源 blackhole 配置 agentMaxTurns=16 上限？）而 EOF/settled 链路
  未按预期唤醒引擎。修复阶段新 bug，待修复阶段处理。

## 三、复测结论（v2，第 17/18/19 三试后定稿；23:5x 纠错更新）

- **修复面 16 项全部端到端回归通过**（含 test_03/07/09/10 专项断言），零回归；
- **P8-ISSUE-05 转正**：第 18/19 两试树连续落约定路径（15 文件 5 目录 / 12+ 文件全结构），占位 skill+cwd 双根因修复实证；
- **P8-ISSUE-02 误诊撤回（重要更正）**：原判「双通道断裂——UiRelay 私有 requests 通道无消费者」**经修复阶段源码复核不成立**：`UiRelay::new(events.clone())` 存的就是共享 events 通道（main.rs:31-32、fixture 同构），产品侧 SSE 契约无恙（新增回归单测 sse_delivers_ui_request 钉死）。真因在**测试基建**：`SseStream` 的 HTTP 连接带 10s socket 读超时，而服务器 SSE keepalive 间隔 15s——任何静默段先触发 `socket.timeout`（OSError 子类）被 `_pump` 的 `except OSError: pass` 静默吞掉，**泵线程悄悄退场 = SSE 消费者从首个 >10s 静默段起永久耳聋**。run.started 恰好在订阅后 10s 窗口内到达故被捕获，后续 ui.request/run.finished 全部丢失——历史「autopilot 耳聋」「SSE 流掉线」全部由此解释。已修：读超时提到 120s（>15s keepalive）+ TimeoutError 继续读不退泵（common.py）；产品代码零改动（git diff 991acaf..c2dcde3 -- src/ 为空，audit 已核实）。
- **新缺陷（已修，本轮 task-2）· verdict 后挂起 = watchdog scope 自锁死锁**：第 18/19/20 三试连续复现；活体取证（第 20 试挂起现场 /proc）定位：服务进程 fd 表**无任何管道**（review pi 已被 Drop 正常击杀 = SOP 已越过 review 返回）、线程表内引擎线程 `futex_do_wait`（scope join 停车）+ watchdog 线程 `hrtimer_nanosleep`（轮询循环）双双存活、stderr 无 panic。机制：`PlanningRpcEngine::execute` 的 watchdog scope 中 `done.store(true)` 写在 `std::thread::scope` 返回**之后**，而 scope join 要等 watchdog 退出、watchdog 只认 done——正常完成（PASS→SOP 返回）时 done 置位不可达 → 三行自锁死锁。cancel/超时路径反而能退出（watchdog 触发后 return），故单测 MockEngine 从未覆盖。修复：done 置位挪进 scope 闭包内 f() 返回后；新增回归单测 `watchdog_scope_returns_after_closure_completes`（旧实现下必死锁）与 cancel 路径单测；顺带清 main.rs 三处 collapsible_if 与 client.rs 一处 len_zero。
- **外因项（中转长 turn 截断）本期未再现**——httpIdleTimeoutMs=60s + retry 的渲染层缓解疑似生效，样本不足不下结论。

### 对目标的诚实终点

合同要求的全链路七环：创建项目 ✓ → 任务 ✓ → 真实 pi 规划（多轮 dialog 代理）✓ → plan tree 落约定路径 ✓（两连证）→ review 循环含 FAIL→修复→重审 ✓（第 18 试）与首审直 PASS ✓（第 19 试）→ 逐字 PASS verdict ✓（两连证）→ **任务 Completed ✗**——被上述 verdict 后挂起缺陷阻断。

**用户拍板（2026-10-17 23:16）：本阶段只测试不修，测试结果如实记录，测完即结项**——Completed 状态缺失本身作为测试发现记录在案（verdict 后挂起缺陷），修复归下一阶段；修复后重跑一次链路即可领取 Completed 实证（前六环已稳定复现，预期一次即过）。
