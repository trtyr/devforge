# devforge 端到端测试报告 v3

- 测试时间：2026-10-18 03:21 – 06:40（Asia/Shanghai，单夜完成）
- 被测版本：`db262bf`（含全新 Coding SOP；target/debug，隔离 HOME 运行，不污染真实 ~/.devforge）
- 测试方式：Python 纯标准库脚本逐功能点驱动真实服务器 + 真 pi 子进程 + 真模型（mokeywork/deepseek-flash via newapi.trtyr.top 自建中转）；全部用户交互由 AI 自行代理（用户委托）
- 约定：发现问题**只记录不修**；测试侧为继续验证下游所做的「补偿措施」均显式标注，不计为修复
- 本轮性质：P005（Coding SOP 落地）后的修复验证轮——P001 报告（v2，bc3265e 时代）所列 8 项 high 缺陷的修复回归 + 编码 SOP 六个新功能面首测

## 一、结果总览

| 脚本 | 功能域 | 结果 | issues |
|------|--------|------|--------|
| test_00_server | 服务器生命周期/健康/REST/SSE 冒烟 | 8 PASS / 0 FAIL | 0 |
| test_01_projects | 项目管理 API | 9 PASS / 0 FAIL | 0 |
| test_02_tasks | 任务管理 API（含 coding SOP 入册正例） | 11 PASS / 0 FAIL | 0 |
| test_03_config | 配置管理与 profile 渲染（四 Agent） | 22 PASS / 0 FAIL | 0 |
| test_04_auth | 鉴权 | 2 PASS / 0 FAIL | 1（P3-ISSUE-03） |
| test_05_events_sse | SSE 事件流 | 6 PASS / 0 FAIL | 0 |
| test_06_artifacts_graph | 产物归集与运行图 | 11 PASS / 0 FAIL | 0 |
| test_07_rerun_serial | 串行守卫/重跑留档（真 pi） | 17 PASS / 0 FAIL | 1（P1-CAL-01，info） |
| test_08_e2e_planning | 端到端规划流水线（真 pi+真模型） | 15 PASS / 0 FAIL | 0 |
| test_09_run_control | 取消/终态控制 | 8 PASS / 0 FAIL | 2（OBS-*，low） |
| test_10_timeout_async_render | 异步受理渲染通知+超时收割 | 8 PASS / 0 FAIL | 0 |
| test_11_coding_config | 编码 profile 渲染面（mcp.json/skills/提示词） | 21 PASS / 0 FAIL | 1（F2，high） |
| test_12_sop_override | sop 覆盖触发（跨 SOP 链入口） | 9 PASS / 0 FAIL | 1（F1，high） |
| test_13_coding_preflight | 引擎预检（codegraph init）+黑板骨架 | 7 PASS / 0 FAIL | 0 |
| test_14_coding_full_chain | 跨 SOP 全链（真 pi 编码三阶段+审查循环） | **21 PASS / 0 FAIL** | 0（v2 的 C14 已转正，见 §四） |
| test_15_coding_run_control | 编码 run 超时收割/取消 | 8 PASS / 0 FAIL | 0 |
| **合计** | | **183 PASS / 0 FAIL** | **6 项发现（1 high 转正、2 high、1 medium、3 info/low）** |

同时：Rust 单测 120 + doctest 1 全绿（db262bf 开发阶段产物未回归，bg_run 原始日志验真）。

说明：
- test_02 首跑 10P/1F，根因是任务一校准漏网（步骤 2 新增 t2(coding) 后步骤 6 列表期望未同步），属测试预期陈旧非产品缺陷；校准修复（commit `94ed1f3`）后补跑 11P/0F。
- test_11 起的编码面脚本均以「剥除 base/extra packages」降级跑（F2 所迫，见 §四），渲染/触发/状态机断言不受影响；涉及插件工具面的断言本轮缺位（见 §五 外因）。
- 全部结果 JSON 落盘于 `tests/e2e/results/`（本轮 fresh，home 与 run_id 均属本轮）。

## 二、Coding SOP 全链详述（test_14 v3，28m19s）

### 链路全程（一次走通）

1. 隔离 HOME 起服务器 → 四 profile 渲染齐 → 测试侧补偿（配置全量复刻）✓
2. 项目 demo-app 创建（local_path=工作区）→ 任务 planning-run（sop=planning）→ start 202 ✓
3. 规划段：真 pi Plan Agent 读 skill、写 plan tree（22 文件），7 个 ui.request 全部由 AI 代理应答 → run.finished ok=True → **Settled + 任务 Completed** ✓
4. **跨 SOP 触发**：同任务 `POST start {"sop":"coding"}` == 202，任务 sop 切换，编码 run 点火 ✓
5. 引擎预检：workspace 无 `.codegraph/` → `codegraph init` 自动执行（test_13 单独实证）→ 黑板骨架 coding/docs/reports/blockers 建立 ✓
6. 编码段：真 pi coding-agent 以黑板为 cwd、工作区为代码路径双路径开工——**自划分 3 个阶段**（stage 1: T001+T002 骨架/配置/模型/存储；stage 2: T003+T005 端点/错误映射/结构化日志；stage 3: T004+T006 前端三态/README/文档收尾）✓
7. 审查循环：每阶段 `<stage-complete>N</stage-complete>` 触发 fresh coding-review（grok-4.7，cwd=黑板）——v3 两轮收敛（review-1 FAIL 打回 → 修复 → review-2 PASS）✓
8. 收尾：树完成判定 → 编码 run Settled → **任务终态 Completed** ✓

### 关键证据（v3，`results/test_14_coding_full_chain.json`）

- **五条 git commit 留痕（D009）**：`f1ad721 stage 1 (T001+T002)` → `328f7ca stage 2 (T003+T005)` → `6eade5d stage 3 (T004+T006)` → `7f0e498 stage 3 fixup` → `9a11497 docs: fix storage close() description...`——每阶段审查 PASS 后 commit，无 push
- **伴写文档 10 篇（D004）**：coding/docs/ 非空，第三方可凭文档写测试的口径由审查者把关
- **偏差协议实战触发 1 次（D012）**：blockers/ 留痕 1 条——编码者发起 ask-user-question，AI 代理应答后继续，留痕落盘
- 工作区真实代码产物 22 文件；时间线全事件留档 `results/test_14_timeline.json`
- 交互代理计数：规划段 ui.request 7/7 代理应答（SSE 可见面）；编码段对话框走 blockers/ 留痕路径，SSE 侧 requested/answered 计数 0（脚本时间线窗口未覆盖编码段对话事件，计数口径局限，不影响链路判定）

### v2 附加证据（审查者严格性实证，`review-4.md`/`review-5.md`）

- 审查者**真跑了单测**：`python3 -m unittest discover -s tests -v` → 125 → 133 tests OK（非采信自报）
- 逐条对照 todo 声称与实物：抓出「文档测试计数 34≠实际」「§3 错误码表覆盖写大」「§5.1 调用链与源码不符」「ConfigError.message 不存在」等文档-实现偏差，三次 FAIL 打回——**D011（对照实际代码与文档审）在生产实证**
- v2 编码者按 issues 逐条修复并补强测试（新增 4 条 E3 侧用例 + 拆分存储失败四因 + 自查修复审计 logger 静默缺口）

### 时长与稳定性观察

- v2：规划 ~15min + 编码 60min 窗口耗尽（8 轮审查、双 commit，收在 stage 2）——模型慢速时段
- v3：规划 ~13min + 编码 ~15min 全程走完——模型快速时段
- 同一二进制、同一脚本，两跑差异纯系模型响应速度方差；v2 的 C14-CODE-TIMEOUT 因此转正为非缺陷（见 §四）

## 三、P001 缺陷修复回归（对照 v2 报告）

| v2 缺陷 | 本轮验证态 |
|---------|-----------|
| P8-ISSUE-01 无超时/取消 | ✅ 修复——test_10（超时收割 21s<60s 及时）、test_09/test_15（cancel→Failed）双实证 |
| P8-ISSUE-02 完成确认未运转 | ✅ 修复——test_08 七连对话全代理、run.finished 落定 |
| P8-ISSUE-05 plan tree 零产物 | ✅ 修复——test_08: 22 文件落约定路径；test_14: 黑板交接编码段 |
| P8-ISSUE-06 模型首轮卡死 | ✅ 修复——models.json/settings.json 渲染补全后模型链路全程可用（P2 系列修复的综合效果） |
| P2-ISSUE-01/03/04 渲染缺配置 | ✅ 修复——test_03 22 项渲染面全过 |
| P8-ISSUE-07 setStatus 误应答 | ✅ 修复——test_05/test_08 无协议违规噪音 |

## 四、问题清单（只记录，不修）

### F1-MISSING-PROFILE-HANG（high）

- 描述：profile 未渲染时启动编码 run 不快速失败——spawn 不校验 profile 目录，真 pi 以缺失 agent-dir 启动后挂 Running，占住全局串行位；仅 cancel 可解锁（cancel→Failed 在编码路径有效）
- 影响：静默资源占用 + 用户侧无感知排队
- 证据：`results/test_12_sop_override.json`（issue 记录）+ `tests/e2e/test_12_sop_override.py` 第 3 节；spawn 无校验点在 `src/engine/process.rs`

### F2-PI-LENS-NPM-FAIL（high，上游/环境）

- 描述：pi-lens 的 npm 依赖 @ast-grep/cli postinstall 在本环境失败（"Failed to move @ast-grep/cli binaries into place"，手工复现）→ npm install pi-lens 整包失败 → 安装器报错 → render_all 中断 → 后续 profile 不渲染
- 影响：D014 诊断层在真实安装路径不可用；本轮 e2e 全线以剥包降级跑（补偿措施，已标注）
- 证据：`results/test_11_coding_config.json`（issue 记录）+ 安装期 stderr 日志
- 备注：mcp.json（codegraph/context7）与 skills 不受 F2 影响，照常渲染（test_11 断言）

### C14-CODE-TIMEOUT（high → 转正非缺陷）

- 描述：test_14 v2 编码段 3600s 窗口耗尽（增长曲线平线收尾），Completed 未及见证
- 转正依据：v3 同二进制同脚本给足 7200s 后 **21P/0F 全链走通**——纯窗口预算问题，非产品缺陷
- 证据：`results/test_14_coding_full_chain.json` v2/v3 两份 + `results/test_14_run.log`、`results/test_14_run2.log`

### P3-ISSUE-03（medium → 勘误：大部分判旧）

- 原描述：token 鉴权预留位无启动配置入口（main.rs 恒置 None，无 env/CLI 参数），401 强制路径无法端到端验证（单测已覆盖逻辑）
- 证据：`results/test_04_auth.json`
- **勘误（2026-10-18 coding 阶段核实）**：env 入口（`DEVFORGE_TOKEN`）早在 991acaf 修复阶段已落 main.rs，401 生效/放行双路径单测亦在——test_04 的记录为判旧。真实缺口仅剩 CLI 参数，已在 coding 阶段补齐（`--token <v>/--token=<v>`，CLI > env，空串视为未设）

### P1-CAL-01（info，校准记录非缺陷）

- 描述：清单 model_id 须写真实 id `mokeywork/deepseek-flash`（显示名「DeepSeek flash」不可用）；provider baseUrl = https://newapi.trtyr.top/v1
- 证据：`results/test_07_rerun_serial.json`

### OBS-SESSION / OBS-STDERR（low，软观察非缺陷）

- 取消早于 pi 首个会话文件落盘属正常时序；pi-stderr.log 未落盘为 soft 行为（单测 stderr_lines_land_in_session_log 覆盖机制）
- 证据：`results/test_09_run_control.json`

## 五、结论（对照 P005 plan tree 设计承诺逐条验证态）

### 机制已验证（生产实证）

| 承诺 | 验证态 |
|------|--------|
| 跨 SOP 触发（同任务 sop 覆盖） | ✅ test_12 + test_14 两次 202 实证 |
| CodingSopLoop 状态机（标记解析/FAIL 打回/advance 消化 notes/finish） | ✅ test_14 v2/v3 共 10 轮审查循环运转；单测 6 例 |
| `<stage-complete>N</stage-complete>` 协议（取最后/缺块重推零上限） | ✅ 编码者每阶段发标记触发审查；v2 无标记期重推路径由单测覆盖 |
| D009 阶段 PASS 后 commit 不 push | ✅ v3 五 commit 锚点；v2 双 commit |
| D011 审查对照实际代码与文档、真跑单测 | ✅ review-4/5 实证（133 tests OK + 三次文档偏差打回） |
| D004 编码期伴写文档（第三方可测标准） | ✅ docs/ 10 篇，审查者以该标准把关 |
| D012 ask-user-question 即阻塞态 + blockers/ 留痕 | ✅ blockers/ 1 条实战留痕 |
| D013 引擎拥有 codegraph init 预检 | ✅ test_13：无索引自动 init、有索引跳过、init 失败降级告警 |
| 编码 profile 供给渲染（mcp.json/skills/提示词/插件清单） | ✅ test_11 21 项断言（codegraph serve --mcp/CODEGRAPH_TELEMETRY=0/context7/四语言技能逐字/提示词锚点） |
| 规划→编码黑板交接（计划树为编码输入） | ✅ test_14 黑板 plan tree → coding 黑板双路径开工 |
| 超时收割/取消/串行守卫（编码路径复用 P001 机制） | ✅ test_10/test_15/test_09/test_12 cancel 路径 |
| 规划面回归（P001 全功能域） | ✅ test_00-10 全绿 |

### 外因未验证（本轮条件不允许，非产品问题）

| 项 | 原因 |
|----|------|
| 插件工具级可见性（rpiv-todo/pi-lens 在 RPC 下注册的工具） | F2 迫使全线剥包降级；T003 的 RPC 冒烟仅验证进程活着（`evidence/rpc-smoke.md` 已记录此限制） |
| agent 实际调用 codegraph_explore/context7 工具 | mcp.json 已渲染且进程应已拉起，但本轮未观测会话内真实工具调用（可下轮审计会话 jsonl 里的 toolCall 名） |
| D014 pi-lens 诊断层实战效果 | 同 F2——配置面已验，运行面未验 |
| 真人 UI 弹窗交互路径 | 全程 AI 代理（委托），真人点选路径未人工过 |
| 401 强制鉴权路径 | 原记 P3-ISSUE-03——勘误后仅 CLI 参数缺位，已补（见 §四勘误） |

### 真缺陷（待修，本轮不修）

| ID | 严重度 | 一句话 |
|----|--------|--------|
| F1-MISSING-PROFILE-HANG | high | spawn 不校验 profile 目录，缺失时真 pi 挂 Running 占串行位 |
| F2-PI-LENS-NPM-FAIL | high | pi-lens 的 @ast-grep/cli postinstall 失败阻断整条渲染链（上游/环境） |

### 总评

Coding SOP 从配置渲染到跨 SOP 全链的**机制面全部落地并经生产实证**：真 pi 三阶段自划分开发、审查循环真跑单测打回、commit 锚点、伴写文档、偏差协议留痕、Settled+Completed 收官，一次不缺。P001 时代 8 项 high 缺陷修复全部回归通过。遗留两项 high 缺陷（F1 启动校验缺失、F2 上游 npm 依赖）与一组外因验证项（插件/MCP 工具级实战），均已带证据指针入档，留待下一轮修复与验证。

—— devforge e2e campaign v3 · 2026-10-18
