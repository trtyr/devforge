# P007 T008 e2e 验证轮 · 阶段记录与发现（2026-10-19 00:35 停止，只记录不修）

> 用户裁定（2026-10-19 00:34）：**停下来记录，这个阶段不要修东西。**
> 本档记录 T008 验证轮已完成部分、中止现场与抓到的缺陷；修复留待下一阶段。

## 一、本轮已完成（代码已提交）

| 项 | 结果 | 证据 |
|----|------|------|
| `TESTING_SOP` 注册缺口修复 | 注册表补齐四 SOP（planning/coding/testing/doc-fix），单元测试改四 SOP 断言；此前引擎分发臂有、注册表漏，`start {"sop":"testing"}` 会被 400 拦死 | commit `54e734f`；`src/domain/sop.rs` |
| 既有脚本校准（八 Agent） | test_03 八 Agent 断言（22P/0F）、test_10 `rendered==8`（8P/0F）、test_11 `rendered==8`+八 profile 断言 | commit `54e734f`、`1436ac8` |
| test_17 测试配置面 | **51 PASS / 0 FAIL**：D007 供给即纪律（test-agent 仅 context7+python skill；test-review/doc-review 极简；doc-fix 双 MCP+四语言）、黑盒硬禁令/白名单锚点、reroute/Runbook/findings/矩阵锚点、mcp 有无矩阵 | `results/test_17_testing_config.json` |
| test_18 触发链 | **15 PASS / 0 FAIL**：testing/doc-fix 创建与触发 + sop 切换 + F1 门禁快失败 + 未知 SOP 拒绝清单含四 SOP | `results/test_18_sop_registry.json` |
| test_19 全链脚本 | 三段跨 SOP 脚本落盘（规划→编码→testing，含 findings/矩阵/doc-review 移交取证与黑盒旁证） | commit `c68e8fd` |

## 二、test_19 中止现场（1h41m，未走完）

- 时间线（Asia/Shanghai）：22:52 起服 → 八 profile 全供给渲染 **>20 分钟**（今晚装包 2-3 倍慢）→ 23:06 编码段点火 → 23:15 首轮审查 → 23:24 review-1.md → 23:41 review-2.md → 23:43:46 第 3 轮审查启动 → 23:44:26 审查进程静默退出（无 assistant 消息）→ 23:44:33 coder 收到错误返工指令 → **coder thinking 打转 49 分钟**（会话冻结 663,552B、进程 59% CPU）→ 00:35 用户裁定停止，`bg_kill` + 清场。
- 隔离 HOME：`/tmp/devforge-e2e-ivs7n9ak`（现场保留，供后续复盘）。

## 三、发现（只记录，不修）

### C19-R1 · 审查空回复/协议违例被误路由给被审 Agent（high）

**现象**：第 3 轮审查 pi 只收到指令、无任何 assistant 回复即退出 → RPC 返回空串 → `parse_review_report` 保守判 `FAIL + Fix`，issue 为「Review 输出缺 `<stage-verdict>` 协议块…（此为审查输出协议问题，非代码问题）」→ **CodingSopLoop 把这条「审查官输出格式问题」当作代码问题回注给 coder**。

**根因**：`conservative()` 的语义是「审查输出违例 → 重审」，但编排层对 `(Fail, Fix)` 只有一条通路——回注被审 Agent。协议违例与真实代码问题的路由没有区分开。

**后果**：被审 Agent 收到自己无法修复的指令（去改别人的输出），无意义返工；与中转长 turn 停滞叠加后整条链 wedged。

**证据**：`/tmp/devforge-e2e-ivs7n9ak/.devforge/sessions/coding/*.jsonl` 末条 user 消息（23:44:33，"审查未通过…Review 输出缺 `<stage-verdict>`…"）；`.../sessions/coding-reviews/review-2/*.jsonl` 仅 system+user 两条、无 assistant。

**建议修法（下一阶段）**：协议违例应触发「重跑本轮审查」（新 fresh reviewer，轮次递增），而非回注被审 Agent；`ReviewReport` 增违例标记位，编排层据此分流。

### C19-R2 · 审查 pi 静默退出，无错误上抛（medium）

**现象**：review-2 会话无 assistant 消息、`pi-stderr.log` 仅有一条无关警告（pi-blackhole peerDependencies）、server 侧无 ERROR；RPC 层把「无回复」当空串交回，未报错。

**后果**：静默死亡被当作「审查通过格式不合规」，掩盖了真实原因（进程为何退出至今无直接证据）。

**证据**：`/tmp/devforge-e2e-ivs7n9ak/.devforge/sessions/coding-reviews/review-2/pi-stderr.log`（335B）；`server-stderr.log` 无 ERROR 行。

**建议（下一阶段）**：RPC 会话在 EOF 且无 assistant 消息时应报错（区别于「有回复但不合规」），错误需带 stderr 尾部。

### OBS-C19-STALL · 中转长 turn 停滞（环境，已知家族 P8-ISSUE-06）

coder 收到返工指令后 49 分钟未完成单个 turn：会话文件冻结、进程 59% CPU。与前几轮观测同族（慢滴式 keepalive 打穿 idle 超时）。**按 D002 哲学不自动杀**（进程活着），本次由用户裁定人工停止。

### CAL-19-RENDER · 八 profile 全供给渲染 >20 分钟（环境/性能，校准点）

今晚装包比 v4 慢 2-3 倍：test_11 v1 的 600s `config.rendered` 预算超时（八 profile 全部落盘但 SSE 未及），预算已提到 900s；test_19 设置期预算 900s→1800s。**下一轮建议**：全供给八 profile 渲染按 ≥1800s 预算，或预装缓存。

## 四、对 P007 结论的影响

- 测试阶段的**配置面与触发面已实证**（test_17 51P、test_18 15P，八 profile 供给契约全绿）。
- 测试阶段的**全链闭环未验证**（test_19 被 C19-R1 阻断）。即：Test Agent 真实跑起来的链路（写测→自起→自跑→审查→doc-side 移交）尚未取证。
- C19-R1 属编码 SOP 的路由缺陷被测试轮暴露，与本轮新增的 testing/doc-fix 代码无关（新增部分单测 174 全绿）。
