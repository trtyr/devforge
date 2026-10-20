# T008 · coding 全链 e2e 证据（005 收官）

- 日期：2026-10-20 落证；运行时间 **2026-10-18 21:06**（`test_14_coding_full_chain` 全供给跑，第 16 次端到端战役的 coding 段）
- 运行快照存档：`evidence/t008-test14-result-snapshot.json`（原 `tests/e2e/results/test_14_coding_full_chain.json`，25 PASS / 0 FAIL / 0 issues）
- 复跑命令：`python3 tests/e2e/test_14_coding_full_chain.py`（真 pi + 真模型 + AI 替答，预算 plan 40min + code 180min）

## 一、验收断言逐条对照（T008 原文）

| T008 要求 | 实测（json 快照） |
| --- | --- |
| demo-app plan tree 输入 | 规划 run Settled，plan tree 落黑板（15+ 文件树，review FAIL→修复→PASS 循环见 RUN16 记录） |
| coding agent 分阶段开发 | `coding/stages.md` 存在；4 个阶段 commit：`a8cc9e3 stage 1 骨架与契约` → `bdb1bd0 stage 2 主流程+单测` → `f922bc2 stage 3 验证路径` → `911ce7a stage 4 证据与状态回写`（D009 阶段 PASS 即 commit） |
| review 循环（打回或直 PASS 均有效） | **真实打回循环**：`reports/review-1.md` … `review-7.md` 七轮审查留痕 |
| 树完成 → run Settled | 编码 run 终态 `Settled` |
| 任务 Completed | `GET /tasks` 实测 `Completed` |
| 黑板产物取证 | stages.md / todo.md / coding/docs/ 伴写 9 篇 / review-*.md 7 份 / blockers/ 2 条（偏差协议真触发过） |
| 工作区留痕 | 493 个真实代码文件 + git 化 + 4 commit |

## 二、观察项：Q001/Q003/Q005 工具级可见性（会话 toolCall 实证）

编码会话扫描到的工具命名空间（agent 真实调用，非注册即算）：

```
ask_user_question, bash, codemode, edit, effective_config, fffind, ffgrep,
lens_diagnostics, module_report, pi_lens_activate_tools, project_report,
read, read_enclosing, read_symbol, recall, symbol_search, todo,
web_fetch, web_search, write
```

| 观察项 | 结论 |
| --- | --- |
| **Q001** todo 插件 RPC 数据面 | ✅ `todo` 工具被真实调用（数据面在 `--mode rpc` 下可用，状态文件可审计） |
| **Q003** web tools 在 RPC profile 暴露 | ✅ `web_fetch` / `web_search` 均在调用集里 |
| **Q005** pi-lens RPC 可用性 | ✅ `lens_diagnostics` / `read_symbol` / `read_enclosing` / `symbol_search` / `module_report` / `project_report` 全被调用——LSP 诊断层真实工作 |
| 交互代理 | `answered=8, requested=8`——ui.request 全部被 AI 应答（含 ask_user_question） |

## 三、Q002（codegraph 多实例 FileLock）——诚实登记

本次 run **agent 自主选择了原生工具**（fffind/ffgrep/read），未调用 codegraph MCP 工具（json 记录「MCP 工具调用=无」）。因此：

- 观察到的事实：**全程零锁冲突事件**（无 FileLock 等待/报错日志），coding agent 与 7 轮 review 并发期内无争用现象；
- 局限：由于 agent 未走 codegraph MCP，多实例并发查询的**主动路径未被压测**。按 Q002 的处置条款（「若出现实质争用再评估单例桥」），现状是无争用证据 → 不触发单例桥评估；
- 后续：009 change SOP 真链（plan agent 强制 codegraph-first）会形成双实例并发查询的自然场景，若有争用会在该 e2e 显形。

## 四、时效声明

- 运行时点（10-18）之后，产品代码的改动集中在 doc-from-zero（008）、change SOP（009）、runstate/resume（010）与增量模式；**coding SOP 本体（CodingSopLoop / coding profile / review 构造）未改**；
- 010 引入的 RetryPolicy 只包 `run_in_background` 外壳（run 失败后的系统级重试），不改变链内行为；
- 因此本快照对 coding 链仍然有效。若需「当前 HEAD 复现」，跑上面的复跑命令即可（预计 30-120 分钟 + 数百万 token）。
