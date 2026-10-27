# 文档审查报告 — unit-src-25（第 22 轮）

- 单元：`unit-src-25` → `src/config/manifest.rs`；文档 `docs/contexts/src/config/manifest.rs.md`（420 行）
- 引擎审计（确定性，采信不重算）：要求 32 条 / 已覆盖 32 条 / 缺失 0；doc-lint 通过（0 违规）
- 输入边界（DR-12）：不打开源码；事实以 `_meta/facts/unit-src-25.md`、`_meta/gaps.md`、`_meta/glossary.md`、plan tree、引擎审计与本轮实跑输出为准

<doc-verdict>FAIL</doc-verdict>
<route>doc-fix</route>

## 1. 覆盖（32/32 采信）

要求集合 = module ×1 + 公开符号 ×31（5 包常量、2 个 MCP 构造函数、13 提示词常量、4 类型、`builtin` 与 5 个方法）。「公开成员」两张表逐项链到独立小节，锚点与标题 slug 一致；14 个 spec 条目由「测试覆盖」表逐行承接。无悬空条目。inventory 的 46 条 = 上述 32 条 + 14 个 spec，与引擎的 32 条要求集合自洽。

## 2. findings 处置

工作区无 `testing/findings/`，kickoff 未附本单元 doc-side findings → 待处置清单 N/A。`gaps.md` 本单元 3 条：①doc-contract 4/5 差距已在 `:92` 明写并给出行号；②无 schema 版本已在 `:93` 以 🟡 INFERRED 披露并指回 gaps（gaps 标 🔴，严重度口径不一，见建议）；③未锚定散文约束无兜底，文档反向写成「改提示词会先撞红灯」（见问题 4），属未诚实承接。

## 3. 完整性与 N-A

- 模块三件齐备（职责 / 公开成员 / 依赖）；常量给名称·类型·取值·必填语义·影响·示例，函数六件齐备，结构体五件齐备。
- Runbook 七件套：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN`）、依赖获取（`cargo fetch` + `tempfile` + 夹具）、配置项（N/A，模块定义 schema 但无运行时开关）、启停与健康检查（N/A，库内模块无进程）、种子数据（N/A，`builtin()` 即种子）、测试运行与隔离、公开接口清单（成员表；HTTP 归 server）——七项均有落点或理由。
- 无 TODO/TBD、无整段日志、无死链。

## 4. 真实性与置信度（FAIL 主因）

- 🟢 共 20 余处，绝大多数可回指；`doc-contract` 5 持有 / 断言 4、`builtin_has_eleven…` 名字残留、`doc-review` 无 MCP 等均与 facts 一致。
- 但存在 4 类硬伤：
  1. `:87` 「所有 `*-review` 的 `extra_packages` 为空」为假：`coding-review` = `CODING_REVIEW_EXTRA` = `npm:pi-lens`，与本文 `:147`-`:150` 自相矛盾，且被标 🟢 CONFIRMED。
  2. 行号引用偏差：`:81` 的 `Path`/`serde`/`error` 写 `:7/:9/:11`，facts 为 `:8/:10/:12`；`:288` 把 change-test-review 的「缺一条即 FAIL」指到 `:1249`、`:330` 把 doc-agent 模型断言指到 `:1204`-`1207`，而 facts 的 `:1249` 恰是 doc-agent 的 `minimax-m3.1-flash` 断言；`:91` 的 `agents.len()==15` 写 `:1153`-`1155`（facts `:1153`-`1158`）。
  3. `:397` 「14 个 `#[test]` 中 dump…」与 `:23`/`:377` 的「15 个 `#[test]`，1 ignored」矛盾，实跑为 14 passed + 1 ignored。
  4. `:90` 「改提示词会先撞红灯」夸大锚点覆盖，与 gaps 第③条冲突。
  5. `:362` `save` 写「父目录存在则 `create_dir_all`」，facts 为无条件「父目录 `create_dir_all`」，行为语义被写反。
- 🟡 唯一一处（无 schema 版本）给了依据并指回 gaps，态度合格，仅严重度与 gaps 的 🔴 不一致。

## 5. 内部自洽

除问题 1（review 供给）与问题 3（测试计数）外，其余口径一致：两层划分 ↔ 模板层/模型层；`base_packages` 起点 + `extra_packages` 叠加 ↔ 渲染消费点；`resolve_model` 先于 `resolve_provider`；`load` 不校验引用 ↔ PUT 侧 fail-fast；夹具同源与重建命令一致。

## 6. plan tree

- P002 T002（`roadmap.md:6`）：ConfigManifest = 模板层/模型池/agents/api_key、四件套、ask 差异化、DeepSeek flash 池 —— 与文档逐条对应。
- P008 D012/D018 曾要求 `doc-review` 挂 codegraph，文档写空 MCP；`ideas/real-run-doc-issues-20261023.md:29` 明记 DR-12「推翻 D012/D018」，非冲突。
- 余一处待注记：D018 定 `doc-agent` 为 `deepseek-flash`，文档写 `minimax-m3.1-flash`（`:89`/`:330`，facts 确为 minimax），但 plan tree 无对应变更决策；文档的「2026-10-20 起」无出处（selfhost 记录 10-21 起因渠道不可用改用 minimax）。建议补一句依据或同步 plan tree。

## 7. 可跑（照 Runbook 从零）

- 环境：`TMPDIR=/var/tmp/r22t/tmp`、`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`、全新空 `CARGO_TARGET_DIR=/var/tmp/r22t/target`。
- 依赖：`CARGO_NET_OFFLINE=true cargo fetch` → EXIT=0。
- 测试：`CARGO_NET_OFFLINE=true cargo test --lib config::manifest` → 从零编译 30.94s，`running 15 tests`，`test result: ok. 14 passed; 0 failed; 1 ignored; 0 measured; 583 filtered out`，EXIT=0；用例名与文档测试表逐名一致，三条计数与文档预期逐字吻合。
- 启停/健康检查/种子数据：文档 N/A 理由成立（库内模块，无独立进程）；测试即该模块的 boot 面。

## 8. 第三方可测标准

15 Agent × （extra/model/skills/mcp/thinking）供给表、错误语义、`save`→`load` 往返、夹具重建与隔离禁忌，足以让第三方在不读源码的前提下写出等价测试；但问题 1/2/3/5 会直接误导断言对象或定位。

## 结论

覆盖 32/32 与 lint 0 采信；Runbook 从零可跑且输出逐字吻合。**但文档存在一处被标 🟢 的假命题（review 供给）及其自相矛盾、5 处行号/行为偏差、测试计数矛盾、锚点覆盖夸大**，按真实性/自洽面判 **FAIL**。

## 非阻塞建议

1. gaps 第②条标 🔴，文档标 🟡；建议统一为 🔴 或在文档注明「gaps 按部署风险记 🔴」。
2. 编译耗时 45.25s 为机器相关值，建议标「机器相关」或删除。
3. `doc-agent` 模型变更补出处（DR/decision 或 selfhost 记录），并同步 D018。
