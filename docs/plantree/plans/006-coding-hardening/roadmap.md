# P006 · Coding Hardening（缺陷修复与超时机制重设）

- 开工：2026-10-18（用户晨间拍板：F1 自己修 / F2 研究为什么装不上 / P3 搞一搞 / 超时机制重想想——
  「它确实在跑的时候因为超时停掉肯定有问题」）
- 阶段纪律：coding 阶段——只写代码和单测，**绝不写端到端测试**（e2e 归下一阶段）
- 基线：`db262bf`（P005 e2e campaign v3 之后）；e2e 报告 v3 遗留缺陷清单为输入

## Roadmap

- [x] **T001 · F1 修复：spawn 前 profile 门卫** — commit `5d51e29`
  - `BuiltinRpcEngine::require_profile`：校验角色目录 + settings.json 渲染标记，缺失即报错
    （含缺失路径与修复指引）；planning/coding 双 SOP 入口接线，先于一切副作用。
  - 根治 F1-MISSING-PROFILE-HANG（profile 未渲染时真 pi 挂 Running 占串行位）。
  - 单测 ×4（在场放行/缺失报路径/双 SOP 副作用前置）。

- [x] **T002 · F2 根因定论 + 安装器硬化** — commit `368fcf7`（决策档 decisions/001）
  - 根因：**/tmp ENOSPC 被上游 postinstall 吞错伪装**——e2e HOME 在 /tmp（tempfile.mkdtemp），
    昨晚 tmpfs 0 字节；@ast-grep/cli 的 52M 二进制 link/copy 双 catch 丢弃 errno，
    ENOSPC 伪装成 "Failed to move binaries into place"。
  - 实证：干净裸装 5s 成功；真实路径 `PI_CODING_AGENT_DIR=… pi install npm:pi-lens` 6s 成功；
    1M tmpfs 铁证复现（同族谜语报错变体）。
  - **可行安装路径（已验证）**：安装目标盘余量 ≥ ~200M 后正常 `pi install npm:pi-lens` 即可。
  - 产品侧硬化：PiInstallBridge 失败错误附带 stderr 尾部（500B 保尾截断），单测 ×3。

- [x] **T003 · P3 收口：token 配置入口补全 + 判旧勘误** — commit `a73f92f`
  - 勘误：test_04 的 P3-ISSUE-03 大部分判旧——`DEVFORGE_TOKEN` env 入口早在 991acaf 已落，
    401 生效/放行双路径单测本就在；真实缺口仅 CLI 参数。
  - 补 `--token <v>` / `--token=<v>`（CLI > env，空串视为未设），解析函数化 + 单测 ×7。
  - 勘误注记写回 TEST_REPORT.md。

- [x] **T004 · 超时机制重设计：墙钟预算与失活收割分离** — commit `e18beed`（决策档 decisions/002）
  - 哲学：只杀真死的，永不杀慢而活着的。默认两个开关都不开 = **永不自动杀 run**。
  - `DEVFORGE_RUN_TIMEOUT_MS`（墙钟预算，显式 opt-in，语义不变）；
    `DEVFORGE_RUN_IDLE_MS`（失活阈值，显式 opt-in）——会话目录指纹
    （Σ len+mtime 纳秒，FNV-1a）平线超阈才收割，流式生成中的增长计为活着。
  - `RunControl.stalled` → map_intervention 给出明确 Failed 原因（不再伪装成 RPC EOF）。
  - 单测 ×6（指纹变化/增长=活/平线=死/未设不杀/取消与超时回归不变）。
  - 下轮 e2e 校准点：test_15 可增失活路径用例；e2e HOME 迁 /var/tmp（decisions/001 运维防线）。

- [x] **T005 · 全量回归 + 回写** — 终局回归 bg_run 原始日志验真（`/tmp/final-regression.log`）：
  - cargo test **138 passed / 0 failed**（+1 doctest）；
  - cargo clippy --all-targets **0 warnings**。
  - 本文件即计划树状态回写；commit 证据如上。

## 缺陷清单终态

| ID | 原严重度 | 终态 |
|----|----------|------|
| F1-MISSING-PROFILE-HANG | high | ✅ 已修（5d51e29） |
| F2-PI-LENS-NPM-FAIL | high | ✅ 根因定论非上游缺陷 + 安装器硬化（368fcf7）；实战安装待下轮 e2e 在健康盘面复跑 |
| P3-ISSUE-03 | medium | ✅ 勘误判旧 + CLI 入口补齐（a73f92f） |
| C14-CODE-TIMEOUT | high→转正 | ✅ 非缺陷 + 机制重设计根治同类风险（e18beed） |
| P1-CAL-01 / OBS-* | info/low | 无行动项（记录在案） |

## 遗留（外因未验证，非本轮范围）

- 插件工具级可见性（rpiv-todo/pi-lens RPC 实战）与 agent 真实调用 codegraph/context7 工具——
  待下一轮 e2e（健康盘面 + 全供给）。
- 真人 UI 弹窗交互路径未人工走过。
