# devforge 端到端测试报告 v6 · P007 测试阶段验证轮（含 MVP 线收口）

- 测试时间：2026-10-18 22:52 – 2026-10-19 21:21（Asia/Shanghai）
- 被测版本：`ba73dd2`（P007 全部代码 + 提速 P0-P2 + 编码预审门 + C19 修复 + 测试自身可靠性约束）
- 测试方式：Python 纯标准库脚本 + 真服务器 + 真 pi 子进程 + 真模型（mokeywork/deepseek-flash via newapi.trtyr.top）；用户交互全由 AI 代理
- 约定：发现问题记录在案；测试侧校准（非产品改动）显式标注
- 本轮性质：P007 T008 验证轮——**测试阶段首次全链验证 + 既有脚本对八 Agent 现实的校准**

## 一、结果总览

| 脚本 | 功能域 | 结果 | 备注 |
|------|--------|------|------|
| test_00_server | 服务器生命周期/健康/REST/SSE | 8 PASS / 0 FAIL | |
| test_01_projects | 项目管理 API | 9 PASS / 0 FAIL | |
| test_02_tasks | 任务管理 API | 11 PASS / 0 FAIL | |
| test_03_config | 配置管理（**八 Agent**） | 22 PASS / 0 FAIL | 校准：四→八 Agent |
| test_04_auth | 鉴权双实例 | 6 PASS / 0 FAIL | |
| test_05_events_sse | SSE 事件流 | 6 PASS / 0 FAIL | |
| test_06_artifacts_graph | 产物归集与运行图 | 11 PASS / 0 FAIL | |
| test_07_rerun_serial | 串行守卫/取消/重跑留档（真 pi） | 17 PASS / 0 FAIL | 校准：cancel 后串行位异步释放的 20s 重试等待 |
| test_08_e2e_planning | 规划流水线（真模型） | 15 PASS / 0 FAIL | |
| test_09_run_control | 取消/终态控制 | 8 PASS / 0 FAIL | 2 条 OBS 软观察 |
| test_10_timeout_async_render | 异步渲染 + 显式超时 | 8 PASS / 0 FAIL | 校准：rendered 4→8 |
| test_11_coding_config | 编码配置面（全供给真装） | 25 PASS / 0 FAIL | 校准：rendered 4→8 + 八 profile + 1800s 预算 |
| test_12_sop_override | 跨 SOP 触发 + F1 回归 | 11 PASS / 0 FAIL | |
| test_13_coding_preflight | 引擎预检 + 黑板骨架 | 7 PASS / 0 FAIL | |
| test_15_coding_run_control | 编码 run 超时/取消 | 8 PASS / 0 FAIL | |
| test_16_run_liveness | 失活收割（SIGSTOP） | 12 PASS / 0 FAIL | |
| **test_17_testing_config** | **测试配置面（P007 新增）** | **55 PASS / 0 FAIL** | 供给即纪律/mcp 矩阵/skills 矩阵/提示词锚点（依赖获取 +4 锚点） |
| **test_18_sop_registry** | **testing/doc-fix 触发链（P007 新增）** | **15 PASS / 0 FAIL** | 注册/触发/切换/四 SOP 拒绝面 |
| test_19_testing_full_chain | 三段跨 SOP 全链（真模型） | 16 PASS / 10 FAIL | **编码段 3h 窗口未收尾，testing 段未轮到**（见 §四 C19-CODE-TIMEOUT） |
| **test_20_testing_fixture** | **测试阶段本体（P007 新增，fixture 路线）** | **21 PASS / 0 FAIL** | **650s 三阶段全绿，见 §二** |
| **test_21_coding_to_testing** | **编码→测试交接（P007 新增）** | **18 PASS / 1 FAIL** | 编码 44min Settled；testing 60min 窗口未收尾（见 §二·B） |
| test_14_coding_full_chain | 编码全链 | 未跑（沿用 v4 结果 25P/0F） | 本轮新增改动仅协议路由，由单测 + test_12/13/15 覆盖；编码链耗时 3h+ 与 test_19 冲突 |
| **合计（本轮 fresh）** | | **305 PASS / 11 FAIL** | 11 FAIL 全属「窗口预算」同族（test_19 编码段 10 + test_21 测试段 1） |

Rust 单测：**183 passed / 0 failed** + doctest 1，clippy --all-targets **0 警告**（渲染提速/幂等落地后终态，bg_run 原始日志验真）。

## 二、测试阶段本体实证（test_20，11m17s 含渲染）

**方法**：fixture 路线绕开编码段耗时——预置被测系统（`calc.py` CLI，含错误码契约）与文档（`coding/docs/01-calc-spec.md`，含 Runbook/接口/错误路径/边界），直接 `start {"sop":"testing"}`。

**结果：21 PASS / 0 FAIL，testing run Settled，任务 Completed，全程 650s。**

| 设计承诺 | 实证 |
|----------|------|
| 自划分阶段（D006） | `testing/stages.md` + 3 个阶段报告（stage-1/2/3） |
| 逐阶段审查（D003/D008） | `reports/review-1.md` → `review-2.md` → `review-3.md`，末轮 **PASS → finish** |
| 追溯矩阵（Q007） | `testing/coverage-matrix.md` 落盘并被审查官逐节核对 |
| findings 台账（D009） | **2 条 doc-side 发现**（见下） |
| 测试落仓库（D010） | `tests/integration/test_runbook_env.py` + `tests/e2e/test_cli_{add_sub,mul_div,errors,streams,boundaries}.py` + `conftest.py` —— **分层目录真实出现** |
| commit 锚（D009） | 阶段 1 PASS 后 commit `b2fe593`；阶段 2/3 审查时未提交（合规：PASS 后才 commit） |
| 黑盒纪律（D001） | 会话工具 `bash/codemode/edit/read/write`，**无** codegraph/pi-lens 调用 |

**两条发现的原始记录（体现「只记录不归因」纪律）**：

- `F001-unittest-vs-pytest.md`（doc-side，低）：文档 §6 要求 `unittest discover`，但测试阶段指定 pytest 框架 + 分层目录；测试 Agent 明确写下「**不阻塞阶段 1；不改本阶段测试框架来迁就旧文档**，应走 doc-fix」。
- `F002-unspecified-integer-syntax.md`（doc-side，低）：整数书写形式（`+3`/`007`/`1_0`/全角/科学计数法）文档未定义；Agent 把实测行为列为「**仅作证据，不作断言**」，拒绝为未定义行为写断言。

> 这两条正是 D001/D002 设计意图的现场体现：发现系统与文档不符时，测试侧只交中性证据，归因与改档交给后续路由。

**doc-fix 移交未触发**：review 三轮全 PASS，未出现 doc-side 判定——与「发现只在测试/审查阶段触发移交」的预期一致（观察项，非缺陷）。

## 二·B 编码→测试交接实证（test_21，1h44m）

**方法**：预置小 plan tree（`wordcount-lite`：零依赖 CLI，验收点 6 条）+ 空工作区，直接 `start {"sop":"coding"}` → 真编码 → `start {"sop":"testing"}` → 真测试。绕开规划段与大体量编码，专验 P007 最关键的衔接。

**编码段**：Settled，**2620s（44min）**；黑板 `coding/stages.md` 落盘，伴写文档 **4 篇**（01-contract / 02-architecture / 03-testing / 04-runbook —— D004 的 Runbook 约定真实出现）。

**交接**：`start {"sop":"testing"}` → **202**，同任务切 SOP 成功。

**测试段（60min 窗口内未收尾，但工作量大且方向正确）**：

| 证据 | 内容 |
|------|------|
| 阶段推进 | `stages.md` 自划 4 阶段；审查推进到**第 4 阶段第 6 轮** |
| 审查真实在把关 | review-1 判 **FAIL(test-side)**：抓出「追溯矩阵撒谎行」+ `--verbose` 位置越界断言；review-3 判 FAIL：抓出 U+2000–U+200A 缺 8 个码点 + 矩阵撒谎行 → **Q007 矩阵审计设计当场生效** |
| commit 锚（D009） | `20f441e`（阶段1 PASS）→ `4f44ede` / `067d407`（阶段2）→ `8f953e4`（阶段3），逐阶段 PASS 后提交 |
| 测试体量 | 工作区 `test_*.py` **20 个**（含 `tests/integration/` 与 `tests/e2e/` 分层 + `conftest.py`） |
| 发现台账 | `F001-verbose-elapsed-on-output-failure.md`（1 条） |
| 黑盒纪律 | 测试会话无 codegraph/pi-lens 调用 |
| 交接面 | 审查引用 `coding/docs/01-contract.md §x` / `04-runbook.md §x` —— **文档即测试输入**（D001 落到实处） |

结论：交接与测试阶段在真编码产物上成立；未收尾属**窗口预算**（详见 §四 C21-TEST-TIMEOUT），同 C14 家族。

## 三、C19-R1/R2 修复与验证

| 缺陷 | 修复（commit `7e7200a`） | 验证 |
|------|--------------------------|------|
| C19-R1 审查协议违例被误路由给被审 Agent | 三个报告类型增 `protocol_violation`；coding/testing/doc-fix 三循环改为「违例→重跑本轮审查（上限 3）」，违例不计入 issues_total | 单测 5 条（重试不误路由 ×3 / 超限快速失败 ×2）；test_20 全程未见违例回注 |
| C19-R2 审查静默退出当空结论 | `RpcReviewSession` 增 role，空正文报基础设施错误 | 单测 `empty_review_reply_is_infrastructure_error`（静默退出回放脚本） |

附带修复：`TESTING_SOP` 注册缺口（引擎有分发臂、注册表漏项，`start sop=testing` 曾 400）——commit `54e734f`；test_17/18 印证注册与触发面全通。

## 四、本轮记录项（只记录不修）

1. **C19-CODE-TIMEOUT（环境/窗口）**：`test_19` 编码段 3h 窗口用尽仍未收尾（今晚真模型明显慢于 v4：编码会话 90 分钟仍在 Stage 1 修复轮）。依 v2→v3→v4 先例属窗口预算问题，非产品缺陷；**但连挂两次（3h/3h）值得下一次换 ≥6h 窗口或改用更快的模型档位单独验一次全链**。证据：`results/test_19_testing_full_chain.json`、`/var/tmp/p007-e2e-t008/test19/`。
2. **CAL-19-RENDER（已修，commit `c692851`）**：八 profile 全供给渲染 >20 分钟（装包比 v4 慢 2-3 倍）——`render_all` 改有界并发（4 worker）+ 渲染幂等（已装齐跳过装包），单测 +5；e2e 预算仍保留 1800s 兜底。
3. **OBS-C19-STALL（环境，已知家族 P8-ISSUE-06）**：单 turn 长停滞（会话冻结 + 进程高 CPU）。本轮 C19-R2 修复后，同类静默消失会以明确错误暴露，不再污染审查结论。
4. **OBS-TEST-VENV（已定口，commit `b959478`）**：test_20 中测试 Agent 为跑 pytest 在仓库内建 `.venv` 并装依赖。现口径：隔离环境可行，但**不得改动被测系统实现与运行依赖**，装了什么必须进阶段报告；文档 Runbook 没写清测试依赖获取方式 → 记 doc-side 发现（不再自行拍口径）。
5. **C21-TEST-TIMEOUT（窗口预算）**：test_21 测试段 60min 窗口用尽时仍在第 4 阶段第 6 轮（已落 4 个 commit / 20 个测试文件 / 矩阵审计打回两次），非停滀非故障；**生产口径 confirmation：测试阶段窗口应 ≥10800s**（与 D011「最慢阶段」一致，本轮 3600s 是 e2e 预算压缩所致）。
6. **测试侧竞态清单（2026-10-19 扫查 22 个脚本，非产品缺陷）**：「终态后重跑 start」与「cancel 受理→run 转 Failed→串行守卫可见」之间存在亚秒级窗口，早到的 start 会吃 409「已有运行中的任务」。影响面：test_07（首个踩中，已修）、test_12（F1 回归后无 body start）、test_18（testing/doc-fix 触发链两处）；其余脚本无「终态后重跑」动作（test_09/13/15/16 只收场不重跑，test_08/10/11/14/17/19/20/21 单次 start，test_21 交接前已等编码 Settled）。统一收敛为共享 helper `common.start_tolerant()`（409 重试 ≤20s，超时才把原响应交给断言）——不改产品语义（MVP 串行守卫 + 409 本就可重试）。

## 五、结论

- **测试阶段（P007）核心机制已实证**：配置面 55P、触发链 15P、本体闭环 21P——Test Agent 真的读文档写测试、自起自跑、维护追溯矩阵、产出中性发现，Test Review 按文档三判据审完并 finish。
- **真缺陷 0**（本轮）；C19-R1/R2 两项产品缺陷已在轮内修复并由单测 + 现场回归覆盖。
- **遗留**：三段全链（规划→编码→testing 一次走通）未在本轮取得见证——卡在编码段耗时上；但**「编码→测试」交接已在真编码产物上拿到见证（test_21：编码 44min Settled → 交接 202 → 测试段 4 阶段/6 轮审查/4 commit/20 测试文件）**，测试阶段本体另由 fixture 路线独立取证（`test_20` 650s 全绿）。
- **窗口口径结论**：coding ≥10800s（v4 先例）、testing ≥10800s（本轮 test_21 实证 3600s 不够）；全供给渲染预算 ≥1800s。本轮已将这些值落进脚本默认值（test_19 三段 2400/10800/10800，test_21 两段各 10800，run_all 预算同步 28800/25200）。
- **单测门禁**：185 passed / 0 failed + clippy 0 警告（`ba73dd2`）。

## 六、2026-10-19 全链重跑（第 2 次，人工终止）

**本轮目的**：用提速后的配置（reviewer medium thinking + 批量调用 + 报告密度 + 编码预审门）重跑三段全链，取得 16:32 那轮的对照。

| 段 | 结果 | 细节 |
|---|---|---|
| 规划 | ✅ 4m24s | 无异常 |
| 编码 | ✅ 2h07m 收尾 | **10 轮审查、5 次 FAIL**（与上轮同形：Runbook 七件套不全 / 非 2xx 信封不统一 / 空态在 1280x720 不可见）；预审门的首次实战效果**未达预期** |
| 测试 | ⚠️ S1-S4 均 PASS（含 review-6），**S5 发生熔断** | 详见 C22 |

### C22 · 测试套件自指递归（fork bomb，已修）

- **现象**：4h49m 未结束；52 个 `python -m pytest -q` 同时存活，每 ~21s 新增一层（两条链）
- **根因**：S5 写的「门禁测试」`tests/e2e/test_s5_ui.py:268-315` 在套件内跑 `python -m pytest -q`（cwd = 仓库根），把包含自身的套件重新收进来 → 无限递归
- **放大**：每层 `subprocess.run(timeout=600)`；每层真起一套服务（端口/目录泄漏）；pytest 捕获输出 → **运行期日志为空**，只能从进程树看出异常
- **处置**：人工终止（用户授权）；4h49m 作废
- **修复**（commit `ba73dd2`）：TEST_AGENT_PROMPT 新增「测试自身的可靠性」7 条（禁自指跑整套测试 / 短超时 / 失败最多重跑 1 次 / 进程组回收 + 临时端口 + `tmp_path` / 禁假绿必须报三数并反向验证 / 禁交互式无界等待 / 报告耗时）；TEST_REVIEW_PROMPT 新增**一票否决**审查面 + 「发现挂死风险先停跑」
- **状态**：约束已写入并有单测/锚点断言，但**从未在真实任务上跑过**（下一轮才会验证）

### 本轮观察（不修，只记）

1. **首次 PASS 率仍为 0/4 阶段**（编码 10 轮审、5 次 FAIL）。预审门没能拦住“把自测全绿当交付标准”的惯性——证据：S4 之前逐轮打回的问题与上轮同族
2. **运行期可观测性缺口**：`test_19` 与 pytest 都缓冲到结束，长跑出事时外部零信号（本次只能靠进程树发现）
3. **审查面盲区**：审查只盖「被测系统行为」，没盖「测试工具自身的失效模式」——C22 正是从这里漏过去的

## 七、MVP 线收口（用户拍板 2026-10-19 21:23）

用户判定：**MVP 流程已通，不再补跑全链验证**。本报告即作为 P007 验证线的终态记录。

| 已证 | 未证 |
|---|---|
| 规划→编码 完整跑通（两轮，含 10 轮审查循环） | 三段一次性全链（本次已过编码，卡在测试 S5） |
| 编码→测试 交接（test_21） | C22 新约束的真实效果 |
| 测试阶段本体（test_20 fixture 650s 全绿；本轮 S1-S4 均 PASS） | 首次 PASS 率是否真的提升 |
| 配置/触发/注册面（test_17 55P、test_18 15P） | 验收/部署（不在本系统范围，归调用方） |
