# T008 · 续跑实证（e2e，2026-10-20）

- 脚本：`tests/e2e/test_24_doc_resume.py`（可复跑）
- 运行：2026-10-20 17:00–17:35，**34m47s，15 PASS / 0 FAIL / 0 issues**（日志 `/tmp/bg-run/917c35/bg_r61nu3zl.log`；结果快照 `evidence-t008-result.json`）
- fixture：`resume-app`（alpha/beta 两包两单元，无 git——与 T008 验收无关，T008 只验续跑）

## 一、时间线（关键数字）

| 时刻（相对） | 事件 |
| --- | --- |
| t=0 | `start` == 202，doc-from-zero 全量开跑 |
| t≈17min | `unit-alpha-01` 写完（4 篇产物）→ review PASS → **done 落黑板** |
| t≈17min | 脚本读 state 确认 `done=[unit-alpha-01]`、4 个产物登记 → **POST cancel** |
| +5s | run 终态 **Failed**（cancel 语义） |
| +5s | **第二次 `start`（无 fresh）== 202，回显 `resumed: true, sop: doc-from-zero`** |
| t=1025s | 第 2 轮 **Settled**，`done=[unit-alpha-01, unit-beta-02]` |

## 二、验收断言逐条对照（T008 原文）

| T008 要求 | 实测 |
| --- | --- |
| 第一个单元完成后杀掉 run | `cancel` 受理（200），run Failed 收场，断点已先行落盘 |
| 第二次 start 不重写第一单元 | 第一单元 4 个产物 **mtime_ns + size 逐字节不变**（如 `alpha-sum.md` mtime `1792487637392805744` / 2863B 两轮完全一致） |
| 只推后续 | 第二轮新会话只写 beta（`beta-*.md` 5 篇新增，alpha 零触碰） |
| 两轮产物 diff 为空（第一单元部分） | ✅（同上，脚本 `snap1 == snap2` 断言） |
| 会话里无该单元 kickoff | 第二轮**新增/增长**的 doc 会话增量文本里搜 `本单元 \`unit-alpha-01\`` **零命中**（`leaks=[]`） |

## 三、配套证据

- `start` 响应体：`{'note': None, 'resumed': True, 'sop': 'doc-from-zero'}`——断点认账的 API 级证据；
- 第 2 轮审查 PASS：`<doc-verdict>PASS</doc-verdict>` + `<route>advance</route>`（review-0 会话 46 行，含对 alpha 文档的 6 条非阻塞 notes——审查真的在干活，不是橡皮章）；
- state.json 两阶段形态：cancel 时 `done=[unit-alpha-01]` → 终态 `done=[unit-alpha-01, unit-beta-02]`、`review_rounds: 1→2`。

## 四、诚实边界

1. **第一轮的模型渠道教训**：第一次运行（16:2x）在 alpha 第 4 轮审查时撞上 newapi deepseek-flash 渠道 402（余额耗尽），run Failed——这不是产品缺陷（fail-fast 正确），但把 doc-review 切到 MiniMax-M3.1-Flash 后重跑才通过。两次运行的 cancel/resume 机制行为一致；
2. resume 后 `plan.resumed` 的 WARN 日志（skip/redo 计数）在服务端 stderr，脚本只验了 API 回显与产物，没断言日志行——证据链已够，不再加跑；
3. 本实证只覆盖 doc-from-zero 的续跑（010 T007 接入的 SOP）；change SOP 的断点续跑（ChangeState）另有单测覆盖（`checkpoint_path_is_change_state_on_blackboard`），不在本项范围。
