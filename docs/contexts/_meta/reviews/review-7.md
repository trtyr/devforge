# 文档审查报告 — unit-src-08（第 7 轮）

- 单元：`unit-src-08` → `src/change/gate0.rs` + `src/change/history.rs`
- 文档：`docs/contexts/src/change/gate0.rs.md`、`docs/contexts/src/change/history.rs.md`
- 轮次：round 7（首轮人工审；此前无该单元的有效审查记录与 findings）
- 引擎审计（确定性采信，未重算）：要求 10 条 / 已覆盖 10 条 / 缺失 0 条 → 覆盖 100%；doc-lint 通过（0 违规）

## 0. 输入与边界

文档 + `_meta/facts/unit-src-08.md` + plan tree 锚点（P009 T013、D001/D002/D003）+ 引擎覆盖/lint 文本。按 DR-12 不读源码；实体存在性以引擎 lint 与 facts 为准，本报告只审结构/必填节/N-A/一致性/置信度诚实性/可跑可测。

## 1. findings 处置完整性

- 工作区无 `testing/findings/`，kickoff 未附 doc-fix findings；`_meta/gaps.md` 仅登记 unit-src-01 的 GAP，与 unit-src-08 无关。
- 本单元无待处置的 doc-side finding → **N/A（无遗漏项，不构成 FAIL）**。

## 2. plan tree 一致性

- T013（`plans/009-change-sop/roadmap.md:52-54`）：往返上限 3、`history.md` 逐次落盘由 `change::history` 实现——history 文档的路径、表头、逐 gap 追加与之一致。
- D001（`decisions/README.md:6-11`）：无 `.git` → `git init` + 一次 `devforge: baseline` 提交，基线 sha 落 `docs/plantree/<change-id>/baseline.txt`——gate0 的 `ensure_baseline` 行为与 `BASELINE_FILE` 口径吻合（记录文件相对 `out_dir`）。
- D002（`:13-18`）：`none`/`essential`/`complete`、默认 `none`——gate0 的 `DocLevel`/`parse`/`needs_docs` 完全一致。
- D003（`:20` 起）：计划/历史落目标仓 `docs/plantree/`——history 文件布局与其一致。**无冲突**。

## 3. 内部自洽

- gate0：公开成员表 6 项 ↔ 逐节详述一一对应；基线四分支（无 git / 有 git 有 HEAD / 空仓 / 已记录）与测试表四例对齐；`created_repo=true` 仅在新建仓分支，空仓补提交仍为 `false`，前后无矛盾；错误面 Config（缺仓/未知档）与 System（git/IO）分层清晰。
- history：`history_path` 纯拼接、`append_roundtrip` 建父目录→读或建表头→逐 gap 追加→写回；「读失败按空串继续、不再补表头」与「仅文件不存在才建表头」自洽；「不判重、重复追加会累积」为诚实披露。
- 仅在命名层面有一处轻微不一致：`parse`/`as_str`/`needs_docs` 实际是 `DocLevel` 的方法，公开成员表标为「函数」（非阻塞，见 notes）。

## 4. 真实性 / 实体存在性

- gate0 六个公开实体行号与 facts 逐条吻合：`Baseline:17`、`DocLevel:24`、`parse:34`、`as_str:45`、`needs_docs:54`、`ensure_baseline:66`；内部 `BASELINE_FILE`、四个 helper 在 facts 中有据。
- history 两个公开实体吻合：`history_path:10`、`append_roundtrip:17`；依赖 `ImpactGap`（`boundary.rs:17`）、error（`history.rs:7`）。
- 消费方：`prepare.rs:10/22/45/65`、`session.rs:16/41/83/749`、`mod.rs:14/15/31/32`、`orchestrator.rs:246/681`、`change_engine.rs:199/252` 均与 facts/清单结构相符；`docs/plantree/changes` 目录真实存在。
- 引擎 lint 0 违规（确定性），无幽灵引用。

## 5. 置信度诚实性

- 全部 🟢 均同行给出 `CONFIRMED path:line`，无裸绿；行号与 facts 吻合。
- 两处测试命令标 🟡 INFERRED 并写明「未实跑复现」，未冒充事实；本轮已实跑，见 §7，可升级为 🟢。

## 6. 完整性（必填节 / N-A）

- 模块三件套（职责/公开成员/依赖）齐备；`Baseline`、`DocLevel` 的职责/构造/成员/生命周期/典型用法齐备；公开函数与 `history_path`/`append_roundtrip` 的摘要/行为语义/参数/返回/异常/示例齐备。
- N/A 均附理由（纯路径无 IO、纯判断无失败面等），无裸省略。
- 内部 helper（`create_baseline_commit`/`head_sha`/`run_git`/`write_baseline`）未单列「摘要+行为语义」——按契约属内部符号，**只记 finding、不 FAIL**（见 notes）。
- Runbook 七件套：本单元是库内模块，无独立服务/端口/配置项/种子数据；「测试运行与环境」承担依赖（`tempfile`、`git`）、工具链前提与隔离方式，项目级 `10-runbook.md` 属汇编阶段。本轮引擎未注入 Runbook 命令块，按单元审范围以文档测试命令实跑代替 → N/A 理由成立。

## 7. 可跑 / 第三方可测（实跑验证）

- 审计环境 `RUSTUP_HOME` 指向空 toolchain，直接 `cargo` 报「no default toolchain」。按已装 toolchain 运行文档命令：
  - `RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable cargo test --lib change::gate0` → **6 passed; 0 failed; 594 filtered out**，与文档 6 例逐名吻合。
  - 同方式 `cargo test --lib change::history` → **3 passed; 0 failed; 597 filtered out**，与文档 3 例逐名吻合。
- 结论：文档命令在选定 toolchain 后从零可跑；测试依赖与隔离（`tempfile` 临时目录、gate0 用临时 git 仓、显式提交身份）与文档一致，第三方可据此写测试。
- 建议（非阻塞）：把两处 🟡 升级为 🟢，并补一句工具链前提（edition 2024 → Rust 1.85+；本机需指定已装 toolchain），避免读者在无默认 toolchain 的环境直接执行失败。

## 结论

引擎审计（覆盖 10/10、lint 0 违规）采信通过；findings N/A、plan 无冲突、内部自洽、实体真实、置信度诚实、缺节/N-A 合规；文档测试命令实跑 9/9 通过。**PASS**。

<notes>
- `parse`/`as_str`/`needs_docs` 在公开成员表标为「函数」，建议统一为 `DocLevel` 的方法，减少同名异写。
- gate0 的 `create_baseline_commit`/`head_sha`/`run_git`/`write_baseline` 可各补一行「摘要+行为语义」（内部符号 finding 级，不阻塞）。
- 两处 🟡 可升级 🟢，并补 Rust toolchain 版本/选择前提。
- 汇编 `10-runbook.md` 时把两模块的测试依赖（`tempfile`、`git`、工具链）并入环境要求。
</notes>
