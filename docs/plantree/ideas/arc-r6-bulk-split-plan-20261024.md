# ARC-R6 · 大文件一揽子拆解方案（2026-10-24）

> 还债路线 R1..R5 的收官延续。用户裁决：**一次打包拆完，不再一期期磨**。
> 本文一次过确认门，之后批次连续推进。

## 目标口径

8 个 >1000 行文件全部拆到 **≤800 行**；纯移动零行为变化；每批 commit；
验证绿才 commit（R5 教训）；clippy 与基线 worktree 逐条 diff 零差异（R5 审计口径）。

## 现状结构图（wc -l @ 0f219cb）

| 文件 | 行数 | 结构侦察结论 | 拆法 |
| --- | --- | --- | --- |
| tests/f6_observability.rs | 2394 | 平铺单文件：采集器/mocks/夹具/MVP 契约/change 侧/panic 组/源码守卫七段 | 目录化 `tests/f6_observability/`（main.rs + support/mocks/fixture/mvp/change/guards 六 mod），target 名不变 |
| src/change/session.rs | 1771 | 生产码 ~587 行 + **tests mod 1184 行** | tests mod → `session_tests/`（plan_baseline/runtime 两 mod）；本体 587 ✓ |
| src/docgen/inventory.rs | 1589 | 数据模型 145 + 支持文件扫描 245 + build_inventory 167 + 源码扫描 166 + units 92 + closures 127 + imports 181 + tests 431 | 垂直切：`inventory/{mod,data,support,scanner,units,closure,imports,tests}.rs` |
| src/server/tests.rs | 1555 | server/mod.rs 内联测试 mod：mocks+夹具 ~200 / retry 主题 / heartbeat / run 生命周期 / HTTP 守卫 | 目录化 `server/tests/`（mod.rs 共享夹具 + mocks/retry/heartbeat/lifecycle/guards 五 mod） |
| tests/change_review_protocol.rs | 1531 | 守卫常量（双白名单）+ 测试函数平铺 | 目录化 `tests/change_review_protocol/`（main + whitelists/guards 两 mod） |
| src/docgen/lint.rs | 1485 | 类型+EntryType 224 + lint 主体+检查器 465 + EntityIndex+实体检查 250 + tests 418 | 垂直切：`lint/{mod,report,checks,entities,tests}.rs` |
| src/change/orchestrator.rs | 1184 | 类型+trait ~129 + run() 264 + tests mod 791 | tests mod → `orchestrator_tests.rs`（791 ✓）；本体 393 ✓ |
| tests/change_failure_context.rs | 1095 | 守卫白名单 + 测试函数平铺 | 目录化 `tests/change_failure_context/`（main + guards 两 mod） |

其余 800-1000 档（manifest 948 / render 829 / client 810）**本 scope 不动**——低于阈值。

## 已知牵连面（R2..R5 实证）

1. **tracing target 断言**：mod 拆分会改 `module_path!()` → SOP 测试的 target 字面量断言机械更新
   （session/inventory/lint/orchestrator 的 tests 若断言 target，逐处核对）。
2. **plan_audit 白名单三副本 + 归一化表**：tests/f6_observability.rs → 目录化后路径变化，
   change_review_protocol / change_failure_context 同理——plan_audit PATH_MIGRATIONS
   加条目 + 三守卫副本（ask_ring/failure_context/review_protocol）+ c7-1 计划内路径指针同步。
3. **f6 内部 include_str**：f6 源码守卫用 include_str 读生产码（相对本文件路径）——目录化后
   include_str 路径层级变化（`../..` 前缀）。
4. **mod 内 use 路径**：拆出后原 `use super::*` 失效，改显式导入（builtin_tests 先例：trait
   方法需要 trait 在 scope）。
5. **visibility**：跨文件共享的夹具函数/结构升 `pub(crate)` 或 `pub(super)`。

## 批次划分（确认后连续推进，每批验证绿才 commit）

| 批 | 内容 | 预期 commit |
| --- | --- | --- |
| 1 | change 域两件：session tests 拆分 + orchestrator tests 外迁 | 2 commits |
| 2 | docgen 两件：inventory 垂直切 8 件 + lint 垂直切 5 件 | 2 commits |
| 3 | server/tests.rs 目录化 + f6 目录化（含 plan_audit 牵连面） | 2 commits |
| 4 | change_review_protocol + change_failure_context 目录化（含守卫副本） | 2 commits |
| 终验 | 行数表复扫（8 件全 ≤800）+ 全量 7 靶 + clippy worktree 逐条 diff + 7889 重启 resume + 台账 R6 Done | 1-2 commits |

## 验收口径

- `wc -l` 复扫：原 8 件（含拆出件）全部 ≤800；
- 全量 cargo test exit 0，测试总数不减少（mod 化不减测试）；
- clippy 与当前基线（0f219cb）worktree 逐条 diff 零差异；
- 7889 新 binary 重启 + acc2 resume 推进证据；
- 台账：本文件收官段 + arch-ai-friendly 台账加 R6 行（Done）。

## 边界

- 不动 800-1000 档三件（另立后案）；不动 AF 批 A / P012；
- 纯移动：不改任何函数体、不改断言语义（target 字面量除外——机械更新）；
- 批次间不回头重拆已关账文件。

## R6 收官回写（2026-10-24，状态 Done，操作员执行）

| commit | 内容 |
| --- | --- |
| b43adc4 | task-1：本方案落档（用户确认门通过后开工） |
| ed97564 / f9383b7 | 批 1：session 589 + session_tests 三件 / orchestrator 395 + tests 790 |
| 636ebec / 0ce4fd9 | 批 2：inventory 七件最大430 / lint 五件最大563（pub use 层保外部路径） |
| a9bcd2c | 批 3+终验：f6 八件 / server-tests 六mod / review_protocol 四件 / failure_context 四件 + 牵连面全适配 |

终态：8 件全 ≤800（最大 orchestrator_tests 790）；全量 7 靶 exit 0、707 测试数持平；
clippy 与批 1 前基线 worktree 逐条 diff 零差异；验收 pid 3222514 探活 200、acc2 resume
后 doc-agent 活跃（accept-r6.log 11:11 时段）。

### 批 3 教训入账

1. **mod 声明截断事故**：main.rs 尾部悬空 doc 清理脚本用硬编码行号截断，把已追加的
   mod 声明一并切掉 → target 0 测试静默通过（cargo test 不报错）——**测试数对账**
   （707 vs 707）是唯一的暴露面。教训：行号截断类脚本禁止用于追加过内容的文件；
   每批验收必须做测试总数对账。
2. **glob 互引 unused 清理**：clippy 驱动删行脚本的路径解析 bug 错删到不相干文件 →
   统一重写 use 头（标准互引块 + `#[allow(unused_imports)]`）一次收敛。
3. **守卫测试自指**：源码守卫读自身/兄弟文件解析常量——目录化后指针跟随常量所在文件；
   doc_comment_above 加可见性前缀兼容（pub(super) 声明同命中）。
