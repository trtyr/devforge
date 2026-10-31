---
type: module
module_path: src/server/dependencies.rs
unit: pending-inventory
description: A4 依赖管理读面——外部依赖静态清单 + 存活/版本探测 + 日志桥接（只读）
---

# src/server/dependencies.rs

配置台·依赖管理页的数据源（P019）。只读监控口径（用户定稿）：无卸载、无
AI 自动修复；下载/更新是写操作+长任务，留待后续任务化。

## 职责

**负责**：
- `DEPENDENCIES` 静态清单：pi（agent 运行时）/ codegraph（结构索引）/ git
  （版本控制基座）——写死与 SOP 出场表同哲学；devforge 自身不在清单
  （监控者不监控自己，服务状态走 A7 /server/ops）。
- `DependencyProbe` trait：生产 `RealProbe`（Command --version，独立线程 +
  3s 通道超时——探测不卡 HTTP 请求）；测试 FakeProbe 注入（ServerState
  dependency_probe 字段，路由构建前独占期换入）。
- `log_bridge`：候选日志目录的存在性 + 文件数 + 最近文件名；`$HOME` 占位
  展开（HOME 是 OS 层兜底，零 env 红线不涉及）；不存在如实报 absent
  （「没有日志的就算了」——P019 定稿）。
- `list_dependencies`：组装（纯逻辑，probe 注入可测）。

## 公开成员

| 成员 | 类型 |
|---|---|
| `DependencySpec` / `DEPENDENCIES` | 结构体与静态清单 |
| `DependencyProbe`（trait）/ `RealProbe` | 探测抽象与生产实现 |
| `ProbeOutcome` / `LogBridge` | 结果与日志桥接结构 |
| `list_dependencies` / `log_bridge` / `expand_log_dir` | 函数 |

## 依赖

- 上游 / 消费方：`GET /api/v1/dependencies`（handlers.rs::dependencies_list）——
  /api/v1 守门层内；ServerState.dependency_probe 字段持有探测器。
- 向下依赖：标准库（process/fs/path）；`libc` 不经此处（disk 在 A7）。
- 外部依赖：探测目标二进制（pi/codegraph/git）运行期从 PATH 解析；缺失时
  探测如实报 dead（不假装）。

## 消费

`GET /api/v1/dependencies`（handlers.rs::dependencies_list）——/api/v1 守门层内。

## 测试

`src/server/dependencies_tests.rs` 4 例 + 端点测试 `src/server/tests/dependencies_endpoint.rs` 1 例
（Fake 注入验响应结构 + 只读字段守卫：无 uninstall/auto_fix）。
