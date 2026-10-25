# 第三方审计·code-review 线第二轮（2026-10-25 17:05）

> 第七份外部审计（code-review 线**第二轮**，针对 22 条修复后的新树）。逐条源码核实
> **7/7 属实**（1 critical + 2 major + 4 minor）——连续第八批零误报，累计 **91/91**。
> 本档只记录，不动代码；修复待用户拍板。

## 这批的份量

前六批打的是「实现细节错了」，这一批打的是一句话：**这条线在真仓上从未真正读过代码**。

脚本测试全绿证明的是「编排器自己转得动」；审查官那一侧（session cwd = 黑板、指令只给相对
路径、prompt 也不带工作区）从来没能定位到被审源码。核验器只验 `path:line` 存在（
`verify.rs:55-107`），不验「读过代码」——于是猜一个真实行号就能进报告。

**所以本仓 code-review 跑通只证明调度活着。** 这句话与我的判断一致：M3 那次 14 小时长跑
（我在 013 里当吞吐上限记录）产出的报告，可信度也不成立。

## 逐条核实

### C1（critical）批次指令不含工作区绝对路径，审查官无法定位源码 —— 属实

- `pipeline.rs:153-165` 的 `batch_instruction()` 只拼 `unit.label` / `unit.files`（仓内相对
  路径）/ `precheck_report` / `others`；**没有 workspace 绝对路径**。
- `builtin.rs:121-126`：`RpcReviewFactory::per_round_in(self.profile_root/"code-review",
  "code-review", self.session_root/"code-review", self.plans_root.join(&task.project).join(&task.id))`
  —— 第四个参数是 `working_dir`（见 `rpc_bridge.rs:129-134` 注释：「编码审查的 cwd = 黑板，
  报告与树同址」），即 **session cwd = 黑板目录**。
- `assets/prompts/code-review-agent.md` 全文 70 行，**没有任何工作区路径**（只有角色、六视角
  清单、协议契约）。
- 对照（同骨架的正确做法）：`sop/doc/fix.rs:33-41` 的 `doc_review_instruction()` 第一句就是
  「黑板 = 当前目录…；代码工作区 = {workspace}」；`sop/doc/fix.rs:20-31` 的 docfix_kickoff 同样。
- 结论：相对路径 `src/audit/runner.rs` 在 cwd=黑板 下不可解；没有绝对路径也拼不出来。审查官
  只能靠猜或靠 codegraph（见 M2——那条路的仓路径同样没接）。**这是本仓自审结论不可信的单点
  根因**，不是体验问题。

### M2（major）code-review 分发缺空工作区门卫与 codegraph 预检 —— 属实

- `builtin.rs:111-146`（code-review 分支）：`require_profile` 之后直接 `run_audit`，**没有**
  `task.workspace.as_os_str().is_empty()` 检查、**没有** `preflight_codegraph`。
- 同口径对照：coding `builtin.rs:261-275`、testing `:354`、doc-fix `:475`、
  doc-from-zero `sop/doc/wiring.rs:27-35`、change `server/change_engine.rs:72,130` —— 全部两件都做。
- 连带：`config/manifest.rs:54-60` 的 `codegraph_mcp` 只有 `CODEGRAPH_TELEMETRY`，**不带仓库
  路径**，而 prompt 的 V5 一致性视角依赖 codegraph 查跨模块——即使 V5 想用图，图与被审仓也
  可能不是同一个（本机多仓索引共存时）。

### M3（major）切片只按文件数上限，无行数/token 预算 —— 属实

- `mod.rs:44-45`：`pub const MAX_UNIT_FILES: usize = 7;`，注释里还写着「≤7 可预测」；
  `mod.rs:97` 用 `files.chunks(MAX_UNIT_FILES)`。
- 聚类键 `mod.rs:68-83` 按 `unit:{unit_id}:{dir}` / `dir:{dir}` —— 同目录文件必进同一批。
- 后果对本仓最直观：`src/audit/` 六个源文件（runner.rs 1073 行、pipeline.rs 481、mod.rs 206、
  verify.rs 291、report.rs、finding.rs）会成为一个批次 ≈ 2300+ 行。超载批次的审查质量、
  以及 6 分钟/批的时长估计，都会失真。
- 与 P013 的 M3 并行修复不冲突但也不解决问题：并行提吞吐，不降单批负载。

### m4（minor）预查正文标题仍写「事实直接采信」 —— 属实

- `runner.rs:459`：`String::from("引擎预查（确定性机检，事实直接采信）：\n")`。
- 同轮 m5 修复只改了文档注释（`:455-457`）、`pipeline.rs:161` 的指令文案和 prompt
  `code-review-agent.md:10-12` —— **漏了输出正文这行**。三处口径对撞，且正文这行是 agent
  实际先读到的。

### m5（minor）证据引用读盘绕过 canonicalize 前缀狱 —— 属实

- 主路径 `verify.rs:115-127` 已用 `read_within`（m11 修复）。
- `evidence_refs_out_of_file()` 在 `:157-177`：`path_traversal(path)` 只拦绝对路径与 `..`，
  之后 `workspace.join(path)` + `read_to_string`（`:160,165`）——**仓内符号链接指到仓外仍会
  被打开**（只取行数，不泄露内容，故 minor）。同类缺陷只修了一半。

### m6（minor）零证据测试是空测试 —— 属实

- `finding.rs:148-153`：`zero_evidence_entries_still_parse_but_are_filtered_downstream`
  调 `parse_findings(OK_REPLY)` 并断言 `!f[0].evidence.is_empty()` —— 用的是**带证据**样本，
  零证据样本根本未构造。删掉「零证据可解析」这层行为，测试照样绿。
- 这是 M1 那条（同位置不同主张的测试用不同 view 绕开缺陷）同族的第二例：**测试名承诺的行为
  从未被断言**。审自己的测试先长这样，值得当成模式化缺陷看待。

### m7（minor）批次文件清单不落盘，报告无法重建分母 —— 属实，且比报告说的更严重

- `run_audit`（`runner.rs:38`）`:51` `build_plan` 之后**不落盘**；报告 `coverage`
  （`runner.rs:274-282`）只有计数与 `skipped_units`。
- `audit-units.json` 唯一写入点在 `mod.rs:127-138` 的 `run_skeleton`——**该函数生产环境无调用方**
  （全仓 grep 只有定义）。即：不仅「报告缺清单」，而是「清单产物在现网从未生成过」。
- 影响：M6 说的「coverage 可对账」只对了一半——有分母没有分子（哪批审了哪些文件）。

## 修复建议（待拍板）

按依赖排序，前三条不做完，这条线不该再用于自审：

1. **C1 工作区入指令**：`batch_instruction()` 增 `workspace: &Path` 参数，首行写
   「代码工作区（绝对路径）= …；当前目录是共享黑板」，口径对齐 `doc/fix.rs:34-41`。
   同时 prompt 补一句「文件路径均相对代码工作区」。——**这是让自审成立的单点修复。**
2. **M2 门卫对齐**：code-review 分支补空工作区 `Err(Store)` + `preflight_codegraph`；
   顺带审视 codegraph MCP 是否该带仓路径（否则 V5 要么不用、要么用错仓）。建议与 C1 同 commit
   ——两者都是「审查官能不能看到正确的仓」。
3. **M3 切片预算**：切片器加行数上限（建议单批 ≤800 行源码，超限按文件再二分），
   `MAX_UNIT_FILES` 保留作硬上限。回归：构造「同目录多个大文件」断言切成 ≥2 批。
4. **m7 清单落盘**：`run_audit` 在 `build_plan` 后写 `audit-units.json`（复用 `run_skeleton`
   的写法），并把 `run_skeleton` 本身处理掉（删或标注为仅测试）；报告 `coverage` 增
   `units_file` 字段。这条做完 M6 的「可对账」才闭环。
5. **m4 口径**：`runner.rs:459` 文案改「参考计数，非裁决依据」；顺带做一次三处口径的对撞检查
   （prompt / batch_instruction / 预查正文）。
6. **m5 收口**：`evidence_refs_out_of_file` 改走 `read_within`。
7. **m6 重写测试**：构造真·零证据回复断言「可解析且 evidence 为空」，另加一条「零证据在核验
   阶段被丢弃并计入噪声」的端到端断言。

## 修复状态表（2026-10-25，7/7 全部落地）

> 终验 831 绿 0 failed（lib 721 + integration 19/16/31/43 + doctest 1，TEST_EXIT=0），
> clippy lib 11 条**与修复前持平**（CLIPPY_EXIT=0，日志 `bg_rbwkyonb`）。新增回归 4 条。

| # | 落地内容 |
|---|---|
| C1 | `batch_instruction()` 增 `workspace: &Path`，首行写「代码工作区（绝对路径）= …；当前目录是共享黑板」，并声明 `path:line` 以工作区为基准；`stage1_review_batch` 两个调用点（串行/并行）传 `&task.workspace` / `workspace`；prompt 新增硬约束「只报读过的代码……没打开过文件就不报」。回归：`batch_instruction_carries_workspace_absolute_path` |
| M2 | code-review 分支补空工作区 `Err(Store)` + `preflight_codegraph`；回归：`code_review_start_requires_workspace_after_profile_gate`（并断言门卫先于黑板创建）。注：codegraph MCP 是否带仓路径未动（那是全仓共用的 MCP 配置面，需单独拍板） |
| M3 | 切片改为双预算：`MAX_UNIT_FILES = 7` + 新增 `MAX_UNIT_LINES = 800`，`split_by_budget()` 顺序切批（不重排，同批仍同目录）；单文件自身超限就独占一批。回归：`line_budget_splits_same_directory_big_files`（3×600 行同目录 → ≥3 批） |
| m4 | `runner.rs` 预查正文标题改「引擎预查（参考计数，非裁决依据）」——与 prompt / batch_instruction 三处口径一致 |
| m5 | `evidence_refs_out_of_file` 改走 `read_within`（符号链接越界同样拒读）；失败文案改为「文件不存在或经符号链接越出仓库」 |
| m6 | 测试重写为 `zero_evidence_parses_at_protocol_layer`（真·零证据样本 + 断言 evidence 为空）；核验层的丢弃行为原本就有真测试（`verify::tests::empty_or_referenceless_evidence_is_dropped`），两份不再重复声称 |
| m7 | 新增 `write_units_file()`，`run_audit` 切片后即落 `audit-units.json`；报告 `coverage` 增 `units_file`；**`run_skeleton` 已删**（生产零调用方，清单产物从未生成过） |

未动（记录在案）：codegraph MCP 配置不带仓路径（M2 连带项，属全仓 MCP 配置面）；真 LLM e2e 验收未做。

## 与本档前几份的关系

- 2026-10-25 第一轮 22 条（M1-M8 + m1-m14）已全部落地（commit `6134b71`，状态表在
  `third-party-audit-code-review-20261025.md`）。本批没有推翻那些修复，但指出**修复面止于
  编排器**——审查官侧（指令/门卫/切片预算/可见性）不在上一轮范围内。
- 上轮我记录的「M4 未做单批级 abandon」「M3 并行后单批仍 ~6 分钟」两条，在本批 C1 面前优先级
  都靠后：先让审查官看得见代码，再谈快慢。
