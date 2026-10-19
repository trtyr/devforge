# T006/T007/T008 · SOP 注册 + 协议 + 检查点 证据

- 日期：2026-10-19
- 新增：`src/sop/doc_protocol.rs`、`src/sop/doc_from_zero.rs`、`src/domain/sop.rs`（注册）、`src/server/mod.rs`（`run_doc_from_zero`）

## 一、注册与触发

`SopRegistry` 现为 5 个：`planning / coding / testing / doc-fix / doc-from-zero`。
引擎分发 `task.sop == "doc-from-zero"` → `run_doc_from_zero`。

**API 实测**（mock 引擎）：`POST /api/v1/projects/p/tasks/d/start` → **202**，run 随后 Settled（单测 `start_doc_from_zero_sop_returns_202`）。

## 二、协议三块

| 块 | 解析 |
|---|---|
| `<doc-complete>unit-x</doc-complete>` | 取最后一个块；空内容 = 无标记（重推） |
| `<doc-verdict>PASS\|FAIL</doc-verdict>` | 缺块 → `protocol_violation = true` |
| `<route>phase0..phase3\|finish</route>` | **缺失/非法 → 保守 phase3**（P007 同口径） |
| `<issues>` | 逐行剥 `- `；**FAIL 却零 issues → 协议违例**（不许空口打回） |

单测 9 条覆盖：unit 标记/空标记、PASS+finish、FAIL+route+issues、缺 route、非法 route、缺 verdict、FAIL 无 issues、多块取最后。

## 三、循环语义

```
逐单元（跳过 state.done；--only 时只跑指定单元）
  doc-agent.converse(kickoff) → 无 <doc-complete> → 重推
                              → 单元号不符 → 要求改回（编号不得漂移，D020）
  doc-review（每轮全新实例）
    协议违例 → 重跑审查（不回注 agent），连续 3 次 → 判基础设施故障（C19-R1 口径）
    PASS    → 写 state.json（done + rounds + issues）→ 下一单元；route=finish → 收工
    FAIL    → 回注 <issues> + <route>，按 route 回去修
```

## 四、检查点与幂等

- 状态文件：`docs/contexts/_meta/state.json`（D016：不占项目根）
- 单测：`done_units_are_skipped_on_resume`（已完成单元不再审）、`only_limits_to_single_unit_idempotently`（`--only` 只碰指定单元）
- 环境变量 `DEVFORGE_DOC_ONLY` 可指定单单元重派

## 五、门 0（引擎侧，不靠 agent 自证）

`run_doc_from_zero` 在 spawn agent 前做三件事：
1. `build_inventory` → 写 `_meta/inventory.json`
2. `check_closures` → 门 0 证据写 `_meta/gate0-closures.md`（有发现就写明「逐条取舍，不得静默忽略」）
3. 从 module 条目取唯一 `unit_id` 排序成单元表（空则快速失败）

## 六、门禁

`cargo test` **238 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。
