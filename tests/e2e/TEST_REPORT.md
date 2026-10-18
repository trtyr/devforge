# devforge 端到端测试报告 v4

- 测试时间：2026-10-18 15:40 – 21:05（Asia/Shanghai；campaign 15:40–18:04 + test_14 加窗重跑 18:05–21:05）
- 被测版本：`04b5d5b`（006-coding-hardening 全量落地：F1 spawn 门卫 / F2 安装器硬化 / P3 token CLI / 超时-失活重设计；target/debug，隔离 HOME，不污染真实 ~/.devforge）
- 测试方式：Python 纯标准库脚本逐功能点驱动真实服务器 + 真 pi 子进程 + 真模型（mokeywork/deepseek-flash via newapi.trtyr.top 自建中转）；全部用户交互由 AI 代理（用户委托）
- 约定：发现问题**只记录不修**；测试侧补偿措施显式标注不计为修复
- 本轮性质：006 硬化后的全量验证轮 + **供给升级收口**（F2 定论后 pi-lens 不再剥包，编码链路首次全供给跑通）

## 一、结果总览

| 脚本 | 功能域 | 结果 | issues |
|------|--------|------|--------|
| test_00_server | 服务器生命周期/健康/REST/SSE 冒烟 | 8 PASS / 0 FAIL | 0 |
| test_01_projects | 项目管理 API | 9 PASS / 0 FAIL | 0 |
| test_02_tasks | 任务管理 API（含 coding SOP 正例） | 11 PASS / 0 FAIL | 0 |
| test_03_config | 配置管理与 profile 渲染（四 Agent） | 22 PASS / 0 FAIL | 0 |
| test_04_auth | 鉴权（双实例：默认放行 + DEVFORGE_TOKEN 401/200 矩阵 + SSE 鉴权面） | 6 PASS / 0 FAIL | 0 |
| test_05_events_sse | SSE 事件流 | 6 PASS / 0 FAIL | 0 |
| test_06_artifacts_graph | 产物归集与运行图 | 11 PASS / 0 FAIL | 0 |
| test_07_rerun_serial | 串行守卫/重跑留档（真 pi） | 17 PASS / 0 FAIL | 0 |
| test_08_e2e_planning | 端到端规划流水线（真 pi+真模型） | 15 PASS / 0 FAIL | 0 |
| test_09_run_control | 取消/终态控制 | 8 PASS / 0 FAIL | 0 |
| test_10_timeout_async_render | 异步受理渲染通知+显式超时语义回归 | 8 PASS / 0 FAIL | 0 |
| test_11_coding_config | 编码 profile 渲染面 + **全供给真装** | 25 PASS / 0 FAIL | 0 |
| test_12_sop_override | sop 覆盖触发 + **F1 快速失败回归** | 11 PASS / 0 FAIL | 0 |
| test_13_coding_preflight | 引擎预检（codegraph init）+黑板骨架 | 7 PASS / 0 FAIL | 0 |
| test_14_coding_full_chain | 跨 SOP 全链（真 pi 编码，全供给） | **25 PASS / 0 FAIL**（campaign 首跑 23P/2F，加窗重跑全胜，见 §二） | 0 |
| test_15_coding_run_control | 编码 run 显式超时收割/取消 | 8 PASS / 0 FAIL | 0 |
| test_16_run_liveness | **失活收割（新功能点）**：SIGSTOP 存活沉默构造 | 12 PASS / 0 FAIL | 0 |
| **合计** | | **209 PASS / 0 FAIL** | **0 项在案缺陷（1 环境事件已记录，见 §四）** |

同时：Rust 单测 138 + doctest 1 全绿（04b5d5b 的 006 开发阶段收官门，未回归）。
全部结果 JSON 落盘 `tests/e2e/results/`（本轮 fresh；test_14 为重跑版，commit `a206295`）。

## 二、全链详述（test_14：campaign 首跑 → 加窗重跑全胜）

### 首跑（7200s 窗口，23 PASS / 2 FAIL）

- 机制面全过：规划 Settled、跨 SOP 202、黑板产物、7 轮 review、5 commit、插件工具真实调用、8 问 8 答
- 2 FAIL：7200s 窗口到期 run 仍 Running——coder 自划分 **5 阶段**（比 v3 的 3 阶段更深），review 打到第 8 轮，窗口不够（C14-v4，窗口预算问题重演）

### 重跑（10800s 窗口，25 PASS / 0 FAIL——见证到手）

- **编码 run Settled + 任务 Completed 双见证**（04b5d5b 新 watchdog 路径 + 全供给配置）
- 4 阶段全过审：`a8cc9e3 stage 1` → `bdb1bd0 stage 2` → `f922bc2 stage 3` → `911ce7a stage 4`（D009 连续四次实证）
- review 7 轮（多阶段打回重审，审查者实质严格）；伴写文档 9 篇（D004）；blockers/ 2 条（D012 偏差协议两次触发）
- 493 个代码文件；ui.request 8 问 8 答（AI 代理）；pi-lens 工具真实调用（lens_diagnostics / pi_lens_activate_tools）+ todo 插件
- 观察项：本轮 MCP 调用=无（coder 未用 codegraph explore，agent 自主选择；按契约计观察不计 FAIL）

### 中盘环境插曲（ENV-ENOSPC-01，详见 §四）

中盘 /tmp tmpfs 被历史 e2e 家目录塞至 0 字节，编码 agent 的写入静默丢失；**agent 自诊自愈**——发现 contract.ts 未落盘后主动 `df -h /tmp` 确诊并逐文件补写；测试侧清盘 2.7G 后立刻恢复全速（会话 245KB→400KB/2min→1.25MB）。

## 三、006 四项修复回归核验

| 修复 | 验证脚本 | 核验结论 |
|------|----------|----------|
| F1 spawn 门卫（require_profile） | test_12 §3 | 未渲染 profile 时编码 start → run **秒级 Failed**（实测 0.0s）+ 任务同步 Failed + 可重跑；不再挂起占串行位 |
| F2 安装器硬化（stderr 尾巴） | test_11 + decisions/001 | 全供给真装 25P/0F：pi-lens/rpiv-todo 双 profile 登记、@ast-grep/cli 二进制落盘（ENOSPC 修复回归过）；根因定论档已入库 |
| P3 token CLI | test_04 双实例 | DEVFORGE_TOKEN 场景 401（无头/错头）+ 200（正头）+ SSE 401 矩阵全过；默认实例无 token 放行不回归 |
| 超时-失活重设计（D002） | test_10/15/16 | 显式 DEVFORGE_RUN_TIMEOUT_MS 语义保持（test_15：20s → run Failed + 任务同步 + 终态后 cancel 404）；**失活收割新实证**（test_16：SIGSTOP 构造「进程存活但沉默」，IDLE_MS=6s 实测 since_stop=6.0s 正中阈值；未配置 IDLE_MS 平线不自动收割；cancel 恢复路径可用）——「只杀真死的，永不杀慢而活着的」哲学双面实证 |

## 四、新发现清单（只记录不修）

1. **C14-v4-CODE-TIMEOUT**（窗口预算，非缺陷）：campaign 首跑 7200s 窗口不足（慢模型 5 阶段自划分）。终分类同 v2→v3 先例：加窗即通（10800s 全胜），机制无缺。证据：`results/test_14_coding_full_chain.json`（重跑版）、`/tmp/v4c_14.log`、`/tmp/v4-t14-rerun.log`。校准点：编码窗口建议生产默认 ≥10800s。
2. **ENV-ENOSPC-01**（环境事件，非产品缺陷）：/tmp tmpfs 历史家目录堆积 → 0 字节 → agent 写入静默丢失。agent 侧自诊自愈（df 确诊+补写）；测试侧清盘解封。再次实证 006 decisions/001 校准点：**e2e HOME 应迁 /var/tmp**（下轮执行）；运维面建议：长跑前检查 df。
3. **OBS-CONTEXT7-NOCALL**（观察，info）：v4 两轮全链 run 均未观测到 context7 MCP 真实调用——被测产品为 Node+TS 零运行时依赖，无外部库文档查询需求，agent 不调用属合理自主选择；见 §五 收口状态。

## 五、外因未验证项收口状态（v3 遗留四项）

| v3 遗留项 | v4 状态 | 证据 |
|-----------|---------|------|
| 插件工具真实调用（pi-lens/todo） | ✅ **关闭** | 两轮 run 的会话 toolCall 实证：lens_diagnostics / pi_lens_activate_tools / todo；@ast-grep/cli 二进制落盘 |
| codegraph MCP 真实调用 | ✅ **关闭** | campaign 首跑 toolCall：`mcp__codegraph__codegraph_explore`；重跑未调用属 agent 自主选择（观察项） |
| context7 MCP 真实调用 | ⚠️ 维持未观测 | 两轮均无调用（OBS-CONTEXT7-NOCALL）——供给已验证渲染正确（test_11），真实调用待有外部库依赖的被测产品出现时自然收口 |
| 人机 UI 路径（真人浏览器操作） | ⚠️ 维持 | 全程 AI 代理（8/8 + 8/8 对话代理应答）；真人 UI 操作面不在 AI 代理能力内，留待用户人工验收 |

## 六、对照 plan tree 承诺的验证态

| 006 roadmap 任务 | 承诺 | v4 验证 |
|------------------|------|---------|
| T001 F1 | spawn 前门卫，缺失即快速失败 | ✅ test_12（秒级 Failed + 重跑可用） |
| T002 F2 | 安装失败根因定论 + 错误带 stderr | ✅ test_11 全供给真装 + decisions/001 |
| T003 P3 | token 配置入口完备 | ✅ test_04 双实例矩阵 |
| T004 超时重设计 | 默认永不杀 + 失活收割 opt-in | ✅ test_10/15/16（含新功能点脚本） |
| T005 回写 | 006 roadmap 落 commit 证据 | ✅ 04b5d5b + 本报告 |

## 七、结论

- **真缺陷：0**。209 PASS / 0 FAIL；v3 的 F1/F2 两项 high 缺陷修复经本轮回归实证；P3 勘误归档。
- **机制全验证**：planning → coding 跨 SOP 全链在硬化后版本上完整走通（Settled+Completed 双见证），审查循环、D009 commit 锚、D004 伴写、D012 偏差协议全部实战触发。
- **外因收口 2/4**：插件工具与 codegraph 真实调用关闭；context7 调用与人机 UI 两项留待后续（前者需合适被测产品，后者需真人验收）。
- **环境事件 1 起**（ENOSPC）已记录并给出校准点；测试基础设施三轮演进（v2→v3→v4）的窗口预算经验已回写脚本注释。
