# 文档审查报告 — unit-src-15（第 10 轮）

- 单元：`unit-src-15` → `src/change/prepare.rs`（251 行）
- 文档：`docs/contexts/src/change/prepare.rs.md`（106 行）
- 引擎审计（确定性采信，未重算）：要求 3 条 / 已覆盖 3 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）
- 边界：按 DR-12 不读源码；交叉核对只用文档侧产物（`facts/unit-src-15.md`、`gate0.rs.md`、`docs.rs.md`、`change_id.rs.md`、`mod.rs.md`、`glossary.md`）与 plan tree。

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 doc-fix findings；`_meta/gaps.md` 只登记 unit-src-01 的 GAP，与本单元无关。**N/A，不构成遗漏**。

## 2. 覆盖对账（3/3）

要求集合 = 模块 1（`mod:src/change/prepare.rs`）+ 公开符号 2（`ChangePrep:15`、`prepare:30`）。模块「公开成员」表逐项列名并链到独立小节；清单外 7 条 behavior（测试 96/152/169/187/204/220/236）另由「测试覆盖」表逐例承接，无伪覆盖。与引擎 3/3 一致。

## 3. plan tree 一致性

P009 T015（`roadmap.md:94-96`）：change-id/黑板 → doc_level（none/essential 落地、complete 拒跑指向 doc-from-zero）→ 基线（无 `.git` 则 init+全量提交）→ baseline.txt 与黑板骨架；验收「裸仓有且仅一条 baseline 提交、baseline.txt==sha；重复启动复用 id 且不重建仓库」。文档 §职责/§行为语义/§示例逐条对应（`created_repo=true`→第二次 `false`、sha 不变），D003（计划落目标仓）与 `decisions/README.md:20` 一致。**无硬冲突**。

## 4. 完整性 / 必填节 / N-A

- 模块三件（职责含「不负责」/公开成员/依赖）齐备。
- `ChangePrep`（公开类）：职责、构造/初始化、公开成员、生命周期、典型用法、派生 trait 齐备。
- `prepare`（公开函数）：摘要、行为语义（8 步）、参数、返回、异常、示例齐备；异常按 `Config`/`Quality`/`System` 给触发条件与不回滚语义。
- 「测试运行与环境」承担依赖获取（`tempfile`，`Cargo.toml:27` 核对无误）、运行命令、隔离方式与可并行性。
- 无缺节未声明 N/A，无 TODO/TBD、无日志粘贴。**合规**。

## 5. 真实性 / 内部自洽（FAIL 项）

**FAIL-A（同一行号被指给两组 import，且与兄弟文档冲突）**：`:36` 向下依赖把 `docs` 的 `apply`/`DocAction` 标 `:9`、把 `ensure_baseline`/`Baseline`/`DocLevel` 也标 `:9`——两处不同 `use` 不可能同占一行；且 `gate0.rs.md:38` 明确该 gate0 import 在 `src/change/prepare.rs:10`，`facts/unit-src-15.md` 亦把 gate0/error 归 `:10`。至少一处行号错误（与 round 8 的 FAIL-2 同型）。第三方按行号回指会定位失败。

**FAIL-B（测试输入自相矛盾）**：`:106`「用例都把 `today` 固定为 `(2026, 10, 20)`」与同文 `:82`「同 task 连调两次（需求/日期都不同）→ 复用 `change_id`」直接冲突；`facts` 对 `repeated_prepare_reuses_change_id`（`:204`）明示「需求文本不同、日期不同」。后果：第三方照 `:106` 写重复用例会丢掉「跨日期仍复用 id」这一核心不变量，且无法判定哪条为真。

其余真实性抽查均成立：`mod.rs:18/37`、`change_engine.rs:10/101/102/103/246/279/317`、`change_id.rs.md:49/50`、`gate0.rs.md:65`（`ensure_baseline(repo,&plan_dir)`）、`docs.rs.md:57/58` 与 `:42` 的 `:53/:58/:65` 顺序断言吻合；`Cargo.toml:27` = `tempfile = "3"`；7 个测试行号与 inventory 逐条相同。

## 6. 置信度诚实性

4 处 🟢 均同行给出 `CONFIRMED <path>:<line>`（`:42`、`:61`=`prepare.rs:15`、`:66`=`prepare.rs:30`、`:82`=`prepare.rs:96`），无裸绿。唯一 🟡（`:104` 测试命令）写明推断依据与未复跑原因，诚实；本轮实跑通过，可升级为 🟢（非阻塞）。

## 7. 可跑（从零实跑）

- 环境：审计沙箱无 rustup 默认 toolchain → `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable`；`/tmp` tmpfs 已满（3.9G 用尽，写 target 报 ENOSPC），改用隔离空目录 `CARGO_TARGET_DIR=/home/trtyr/code-work/dfz-r10-target`。
- 装依赖 + 编译：`cargo test --lib change::prepare` → 编译成功（31.5s，13 warning）。
- 健康检查（库内模块以单测代替）：**7 passed; 0 failed; 0 ignored; 593 filtered out; EXIT=0**，测试名与文档表格逐名一致（bare/none/essential/complete/repeated/missing_repo/bad_doc_level）。
- 七件套映射：本单元为库内模块，无独立服务/端口/配置项/种子数据/HTTP 端点 → 相应件 N/A 成立；依赖获取、运行命令、预期输出（7 passed）与隔离（每例独立 tempdir）由「测试运行与环境」承担。文档命令从零可跑，**不构成 FAIL**。
- 注：FAIL-B 是「文档描述的测试输入」问题，不影响命令可跑性。

## 8. 第三方可测标准

公开面（`ChangePrep` 字段语义、`prepare` 七参数/返回/三类异常/不回滚边界）与 7 例测试口径可支撑等价测试编写；但 FAIL-A 的行号回指与 FAIL-B 的日期口径会直接误导第三方，需修订后才能达到「完全基于文档写测试」。

## 结论

引擎覆盖 3/3、lint 0 违规采信通过；findings N/A、plan 无冲突、必填节/N-A 合规、置信度诚实、命令实跑 7/7 通过。但文档存在两处硬伤：`:36` 同一 `:9` 行号承载两组 import 且与 `gate0.rs.md:38` 的 `:10` 冲突；`:106` 的「全部用例固定 2026-10-20」同 `:82` 与 facts 的「日期不同」矛盾。按内部自洽与真实性口径 **FAIL**。

**非阻塞建议（修订时顺带）**：
1. `:104` 的 🟡 升级为 🟢（本轮已实跑 7/7）。
2. `:58` 字段说明「`complete` 在此层不会出现」把 DocLevel 与 DocAction 混写；改为「`FullChainRequired` 在此层不会出现在返回值中（complete 档直接报错）」更准确。
3. §行为语义 8 步未单列 `change_root` 计算黑板目录（现只在第 3 步括注），建议补为独立步骤，与 `change_id.rs.md:41` 的 `:50` 调用点呼应。
4. 测试表 `repeated_prepare_reuses_change_id` 的覆盖口径建议补「需求/日期均不同」。
