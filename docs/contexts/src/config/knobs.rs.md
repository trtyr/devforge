---
type: module
module_path: src/config/knobs.rs
unit: pending-inventory
description: F14 旋钮单点声明——零环境变量配置层的唯一声明处（config.json + CLI，env 读取点已清零）
---

# src/config/knobs.rs

T205/F14 的核心：全部运行期旋钮的**唯一声明处**。零环境变量口径（2026-10-30
用户定稿）——优先级链 CLI > config file > 缺省；仓内 `env::var*("DEVFORGE_*")`
读取点已清零（唯一豁免 `DEVFORGE_MANAGED`：engine/ledger.rs 的跨进程标记，
走 /proc/environ 不走进程 env）。

## 职责

**负责**：
- `RuntimeKnobs`：15 键 typed 字段（ui_ask/ui_pending/review/stall/idle/run_timeout/
  audit×2/install/doc_level×2/incremental/base/max_concurrent/today_override/
  auth_loopback_exempt）——编译期防拼错。
- `KNOB_TABLE`：key/缺省/单位/语义（生效值端点与文档的唯一来源）。
- 全局单例：`init`（启动期一次，config.json 装载）+ typed getters（读点唯一入口）+
  `with_mut`（测试独占改）+ `test_lock`（测试串行锁——全局单例并行 init 互相覆盖
  是假红源）。
- `from_config_json`：单键解析失败回落缺省 + WARN（一个手滑值不炸启动，生效值
  端点可见）；timeout 类 0=不限 → None；concurrency clamp 随既有生产语义。
- `max_concurrent_set`：仅 file/cli 覆盖时 Some——保持「显式值>store>缺省」链。
- `effective_values`：生效值清单（key+值+来源 default/file/cli+单位+语义），
  `GET /api/v1/config/knobs` 的数据源。

**不负责**：启动三件套（--home/--addr/--token 在 main.rs + server/http.rs，非本模块
旋钮）；config.json 的加载与损坏处理（main.rs 引导段）；DOC_LEVEL 拆分 rationale
（doc_level_change 缺省 none / doc_level_docgen 缺省 complete——T205 收编裁决）。

## 公开成员

| 成员 | 类型 |
|---|---|
| `RuntimeKnobs` / `KnobSource` / `KnobEffective` | 结构体与枚举 |
| `KNOB_TABLE` | 常量（声明表） |
| `init` / `with_mut` / `effective_values` / `max_concurrent_set` | 函数 |
| typed getters ×16（ui_pending_warn_ms 等） | 函数 |
| `knob_source` / `test_lock` | pub(crate)，限 crate 内与测试 |

## 依赖

- 消费：13 处运行期读点（server/sop/observability/main/audit/installer/doc/change）
  于 F14-2 全量接线；A7/A8/A10 端点与 A1 豁免开关。
- 全仓守卫：`no_devforge_env_read_sites_anywhere_in_src`（knobs_tests.rs）递归扫
  src/**/*.rs 代码行，env::var/env::var_os 的 DEVFORGE 真实调用语法即红（转义
  字面量与注释不算）。

## 测试

`src/config/knobs_tests.rs` 9 例：缺省/覆盖回落/0=不限/全局 init+getter/生效值来源标注/
DOC_LEVEL 拆分回归/零 env 守卫等。
