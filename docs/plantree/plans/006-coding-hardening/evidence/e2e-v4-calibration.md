# e2e campaign v4 校准记录（对照 006-coding-hardening 四项改动）

- 日期：2026-10-18
- 基线：`04b5d5b`（P006 全勾后）；本轮为 v4 campaign（goal mvdgvp4m-v7nvs8）的 task-1
- 原则：只校准测试侧，不改产品代码

## 逐项核验结论

| 改动 | 影响面 | 校准动作 |
|------|--------|----------|
| F1 spawn 前 profile 门卫（5d51e29） | test_12 第 3 节旧行为（挂 Running + cancel 解锁 + F1 issue 记录）已不存在 | **重写第 3 节**：断言快速 Failed（实测 0.0s）+ 秒级失败 + 任务终态同步 Failed + Failed 可重跑；移除 F1 issue 记录（缺陷已修，断言翻转为准入）。**实测 11P/0F** |
| P3 token CLI 入口（a73f92f） | test_04 旧版只能测放行路径（P3-ISSUE-03 判旧） | **重写为双实例**：默认实例放行两查 + 强制实例（`DEVFORGE_TOKEN` env_extra）401/200 矩阵 + SSE 面无头 401。**实测 6P/0F** |
| 超时重设计（e18beed） | `DEVFORGE_RUN_TIMEOUT_MS` 显式 opt-in 语义原样保留（deadline 路径未动）；失活收割为新功能点（默认关） | test_10/test_15 **无需改动**（回归由 campaign 真跑验证）；失活收割新脚本 = task-3 |
| F2 根因定论 + 安装器硬化（368fcf7） | 健康盘面下 pi-lens 可真装 → 供给升级（不剥包）成为可能 | 不属校准——供给升级归 task-2 |

## 基建配套改动（tests/e2e/common.py）

- `Server.start()` 健康探针 token 感知：`env_extra` 含 `DEVFORGE_TOKEN` 时探针自动携带
  `x-devforge-token` 头——否则强制模式下无头探针被 401 拦截，服务器活着却被误判未就绪
  （实测踩坑：guarded 服务器 20s 探活超时假阴性）。

## 校准验证（新二进制 cargo build 后实测）

- test_04_auth：**6 PASS / 0 FAIL**（含 401/200 矩阵与 SSE 鉴权面）
- test_12_sop_override：**11 PASS / 0 FAIL**（F1 门卫实测 0.0s 秒杀，无挂起）
- 其余脚本：campaign 真跑（task-4）验证
