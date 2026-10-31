---
type: module
module_path: src/integration/settings.rs
unit: pending-inventory
description: A10 集成开关读写面——integrations/settings.json（ui-9 schema；原子写；缺失/损坏回落全 off）
---

# src/integration/settings.rs

配置台集成开关（P019）的存取面。schema 为 ui-9 裁决形状：
`{"auto_integrate_enabled": bool, "gates": {"clippy": bool, "doc_lint": bool}}`，
缺省全 off（「文件不存在 = 功能不存在」）。

## 职责

**负责**：
- `IntegrationSettings` / `IntegrationGates`（ui-9 schema；Default 全 off）。
- `load`：文件缺失/损坏 = 全 off + WARN（不炸读取方；生效状态在 GET 响应可见）。
- `save`：tmp + rename 原子写 + 父目录自动创建——半写文件永不被消费方读走。
- `settings_path`：`<plans_root>/integrations/settings.json`（ui-11 裁决路径；
  该文件重新热读即生效，无需重启）。

**不负责**：消费方（调度器 tick 热读）——归 gap-E auto-integrate 落地时接线；
settings.json 本体在 master 尚不存在（gap-E 候选在 recovery/sr2-gapE-run3 未合入），
本读写面先行。

## 公开成员

| 成员 | 类型 |
|---|---|
| `IntegrationSettings` / `IntegrationGates` | 结构体（ui-9 schema） |
| `load` / `save` / `settings_path` | 函数 |

## 依赖

- 上游 / 消费方：`GET/PUT /api/v1/integrations/settings`（handlers.rs）；
  消费方（调度器 tick 热读）归 gap-E auto-integrate 落地时接线。
- 向下依赖：标准库（fs/path）；serde。
- 外部依赖：无。

## 消费

`GET/PUT /api/v1/integrations/settings`（handlers.rs）——PUT 挂 A1 写操作审计。

## 测试

`settings_tests.rs` 4 例（缺失缺省/往返+无 tmp 残留/损坏回落/ui-9 schema 形状）；
端点 3 例（tests/integration_settings.rs：GET 缺失全 off/PUT-GET 往返+落盘+审计/
损坏回落+PUT 修复）。
