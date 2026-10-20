# 置信度三色证据表（D006）

规则：正文用 🟢/🟡/🔴 极简标记，证据挂在本表；🟢 必须能落到 `path:line` 或可重放命令；🔴 必须同时登记在 `_meta/gaps.md`。
本文件按单元追加（每单元一节），勿覆盖既有小节。

---

## unit-src-01（`src/config/installer.rs` / `src/config/manifest.rs`）

### 🟡 推断清单（正文里标 🟡 的每条）

| # | 结论 | 依据 | 影响面 | 落点 |
|---|---|---|---|---|
| 1 | 运行期 `pi` 必须在 `PATH` 上，否则真桥起进程即失败。 | 命令程序名写死为 `pi`（`src/config/installer.rs:13`），进程查找按 `PATH` 规则；本机未装 pi，未实测。 | 生产装包链路；假桥不受影响。 | `docs/contexts/src/config/installer.md`「依赖」节 |
| 2 | 装包失败不区分「包不存在」与「网络/权限问题」。 | 真桥只判退出码是否成功，无错误码分类（`src/config/installer.rs:60`），失败原因只能从 stderr 文本读。 | 排障时依赖 stderr 尾部；已由错误信息带出。 | `docs/contexts/src/config/installer.md`「行为规格」补充事实 |
| 3 | 清单反序列化时，`Option` 字段缺失按 `None` 处理、其余字段缺失报 missing field。 | serde 派生语义；本仓无「缺字段加载」单测覆盖。 | 手改 `<home>/.devforge/manifest.json` 的容错边界。 | `docs/contexts/src/config/manifest.md`「`ConfigManifest` → 约束」 |
| 4 | 新增可选字段应配 `#[serde(default)]`，否则旧清单读不出来。 | serde 派生语义 + 本模块既有写法（`mcp` / `thinking_level` / `env` 三个可选字段都带 `default`）。 | 未来 schema 演进的迁移注意。 | `docs/contexts/src/config/manifest.md`「`ConfigManifest` → 迁移注意」 |
| 5 | 「审查类 → Grok、干活方 → DeepSeek/MiniMax」是用户拍板写死的终态配置，不是自动推导。 | 字段旁注记「审查 Agent 固定 Grok 4.7（用户拍板 2026-10-17）」（`src/config/manifest.rs:547`）与决策记录 `docs/plantree/plans/002-config-management/decisions/README.md` 的 D004。 | 改动模型分配等于改决策，需走 plan tree。 | `docs/contexts/src/config/manifest.md`「内置清单快照」读表要点 |
| 6 | 提示词按「定稿即定死」维护，改动先改 plan tree。 | 常量注释「勿手改——改决策先改 plan tree」（`src/config/manifest.rs:73`）。 | 提示词维护流程。 | `docs/contexts/src/config/manifest.md`「提示词常量」 |

### 🟢 证据锚点抽样（正文 🟢 结论的可回指位置）

| 结论 | 证据 |
|---|---|
| 命令程序名/参数/环境变量的固定形状 | `src/config/installer.rs:12` |
| 假桥双记录落点 | `src/config/installer.rs:76`、`src/config/installer.rs:95` |
| stderr 尾部截断阈值 500 字节 + UTF-8 边界回退 | `src/config/installer.rs:24`、`src/config/installer.rs:27` |
| 空 stderr 占位文案 | `src/config/installer.rs:33` |
| 失败错误消息模板 | `src/config/installer.rs:38` |
| 清单 5 个结构的字段与 serde 属性 | `src/config/manifest.rs:42`、`src/config/manifest.rs:400`、`src/config/manifest.rs:415`、`src/config/manifest.rs:426`、`src/config/manifest.rs:449` |
| 引用解析的两个错误分支 | `src/config/manifest.rs:655`、`src/config/manifest.rs:666`、`src/config/manifest.rs:677` |
| 内置清单规模（1 provider / 3 模型 / 9 Agent / 4 包） | `src/config/manifest.rs:465`、`src/config/manifest.rs:503`、`src/config/manifest.rs:510`、`src/config/manifest.rs:533` |
| 落盘/读盘位置与时机 | `src/main.rs:151`、`src/main.rs:155`、`src/main.rs:156`、`src/server/handlers.rs:359` |

### 🔴 缺口

见 `_meta/gaps.md` 的 unit-src-01 小节（4 条：单测未执行、pi 未装真机未验、提示词出处、清单缺字段容错未测）。

### 门禁证据

- `devforge doc-lint docs/contexts --repo .` → 退出码 0，摘要「doc-lint: 通过（0 违规）」，原始输出见 `_meta/evidence/unit-src-01-doc-lint.txt`。
- `devforge doc-audit docs/contexts --repo . --level complete`：unit-src-01 的 44 条清单条目零缺失；跨模块重复正文：无；同名出现提示均为通用词（`main` / `error` / `config` / `skills` 等子串误命中），本单元的实体符号（`PackageInstaller` / `ConfigManifest` 等）未出现在重复清单中。
