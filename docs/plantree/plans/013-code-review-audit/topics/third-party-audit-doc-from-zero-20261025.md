# 第三方审计核实 — doc-from-zero 批次（2026-10-25 第四份）

> 外部第三方对出文档 SOP（src/sop/doc/** + src/docgen/**）的 10 条发现，
> 本档为逐条源码核实记录。**判定：10/10 属实**。本档只记录，修复待用户拍板。
> 核实方式：逐条对源码（file:line 实读）。

## 发现清单与逐条判定

### major（6 条，全部属实）

| # | 判定 | 源码证据（实读核实） |
|---|---|---|
| 1 覆盖审计被词表自动盖满 | ✅ 属实 | audit.rs:114/144 `collect_docs(docs_dir)` 底层 collect_markdown 只排除 `.` 开头（:245-247）——`_meta/glossary.md` 进 docs 集合；门 0 wiring.rs:44-49 把全部 Symbol 的 canonical_name 写进 glossary；entry_covered 非 Module 分支（:227）是 `docs.iter().any(\|d\| d.text.contains(&e.canonical_name))`——**Symbol 在单元正文写出前就已 100% 覆盖**（假绿） |
| 2 覆盖/lint 只注入不拦 | ✅ 属实 | wiring.rs:227-242 审计闭包只 `out.push_str(&cov.render())` / `report.summary()`——CoverageReport.passed 与 LintReport.passed 算了却没人消费；单元审与终审都只信 doc-review 的 PASS，缺覆盖/lint 失败的 run 照样 Completed |
| 3 lead=1 快照错单元 | ✅ 属实 | runner.rs:148 `unit_before = None` 只在 :343-347 `Next::Write` 分支重设（Fix 路径不重设）；lead=1 下写 N+1 时覆盖 unit_before，N 的后台审查 PASS 到达时 produced_files 对 N+1 快照算差额——N 的文件已在快照里，mark_done 落错文件名单，续跑 verify_done 按错名单认证据 |
| 4 门 0 闭包失败不拦 | ✅ 属实 | wiring.rs:50-65 check_closures 的 findings 只逐条写进 gate0-closures.md + 一句「写进 gaps.md，不得静默忽略」（:64）——硬失败只有 units.is_empty()；规格「门 0 PASS 才开工」没有对应实现 |
| 5 一单元一提交无实现 | ✅ 属实 | src/sop/doc/ 下 grep 零 git commit 调用；PASS 只 mark_done + save state.json；续跑证据是产出文件存在 + 非 Link lint——单元级 git 回滚与「PASS 才 commit」均不存在（规格 README 写了） |
| 6 终审无协议重试 + 账单丢 IO 错 | ✅ 属实 | wiring.rs:344-347 终审一次 `session.review` + `parse_doc_review(...)?`——协议违例直接 Quality 上抛（对比单元审有 MAX_DOC_PROTOCOL_VIOLATIONS=3 换实例）；wiring.rs:361-362 运行账单 `let _ = create_dir_all` / `let _ = write`——墙钟/用量/终审结论可无声不落盘 |

### minor（3 条）+ nit（1 条）

| # | 判定 | 源码证据 |
|---|---|---|
| 7 doc_level 写死 complete | ✅ 属实 | wiring.rs:233 coverage_for_unit 第三参 `DocLevel::Complete` 硬编码；DocLevel::parse 与 essential/detailed 档位只存在于纯函数层，本 SOP 无读取点 |
| 8 extract_tag 全文 rfind | ✅ 属实 | protocol.rs:139 `text.rfind(&open)` 取最后一次出现、无独占行/尾部约束——正文引用示例块即可盖过尾部协议位（change SOP 已收成尾部位，这边没有） |
| 9 Runbook 命令不执行 | ✅ 属实 | audit.rs:259-294 runbook_commands 只按文件名含 runbook 抽 bash/sh 代码块拼进审查文本——「从零 boot」整段交给审查官，引擎不执行 |
| 10 注释与实现矛盾 | ✅ 属实 | runner.rs:12 「零硬限制：单元数、打回轮次、重推次数均无上限」vs 实现 MAX_DOC_CONTINUE_NUDGES=3 / MAX_DOC_EMPTY_REPLIES=3 / 连续 2 FAIL 断路器（同句后半自己都写了「审查协议违例…上限 3」——一句话内自相矛盾） |

## 修复优先级建议（待用户拍板）

报告未附优先级表；按核实后严重度建议：
- **P0**：#1（覆盖假绿——Complete 档 Symbol 覆盖指标整个失真）+ #2（算了不拦——
  与 #1 叠加后覆盖/lint 双指标形同虚设，SOP 立论「清单可数缺一条即零覆盖」被架空）
- **P1**：#3（断点文件名单写错——续跑证据链受污染）+ #4（门 0 闭包不拦）
- **P2**：#6（终审无协议重试 + 账单丢 IO）+ #8（rfind 尾部协议位，可参考 change 的
  尾部纪律收口）
- **P3**：#5（一单元一提交补实现或改规格）、#7（doc_level 档位接线）、
  #9（Runbook 执行属新能力，建议先立非目标）、#10（注释一句话修正）

修复方向注意：#1+#2 必须一起修（分母修正后不拦等于白修；拦了假分母会误杀）；
#3 的 produced_files 语义需要与 #5（一单元一提交）合并考虑——文件名单的错误
根源是「PASS 认定与文件快照的生命周期错位」。

## 修复状态表（2026-10-25 第二轮，全部落地）

> 10/10 已修复；终验 808 绿 0 failed（TEST_EXIT=0），clippy 本轮新增清零
> （CLIPPY_EXIT=0，lib 剩 13 条均在 audit/pipeline、report、store、change 域、
> resume.rs 等非本轮文件；双 EXIT 原始日志 bg_o36w828e 首验 / bg_04byf8cs 终态）。
> #1+#2 按「分母与拦截缺一不可」捆绑首发；#3+#5 按快照生命周期一体设计
> （提交名单直接复用 #3 快照对的产出集）。

| # | 修复 commit | 落地内容 |
|---|---|---|
| 1 | 66ba19f | collect_markdown 排除 _meta 目录段（门 0 词表含全部 Symbol 名，纳入分母则单元正文零字覆盖已 100% 假绿；对齐 duplicate_mentions 口径，runbook_commands 同源排除无影响）；回归 meta_glossary_is_excluded_from_coverage |
| 2 | 66ba19f | 引擎按 passed() 真拦：run_pipeline 装载清单+EntityIndex 机械门数据（不可读 WARN 降级），Pass 分支 coverage_for_unit+lint 不达标按 FAIL 同款进 fix 队列（带缺失清单）+断路器兜底不 mark_done；终审 parse 后全量机械门不达标 Quality 拒采信；回归 mechanical_gate_rejects_pass_without_coverage |
| 3 | 56f0245 | 快照生命周期收口：unit → (写前, 交卷) 快照对跟单元走（lead=1 下互不冲掉）+ Fix 重入重拍 + produced_files_between 两快照 diff（产出集绑交卷时点，PASS 迟到不混入后续单元产出）+ PASS 无快照守卫宁失败不落错名单；回归 pass_uses_own_units_snapshot_not_latest（修复前实测 A 名单混入 b.md） |
| 4 | 87555de | 门 0 闭包拦截：check_closures findings 非空 run 不开工（Quality 指向 gate0-closures.md，不再放行残清单进滚动）；判定抽 gate0_block 纯函数；回归 gate0_findings_block_the_run + gate0_clean_findings_pass |
| 6 | 237cbcb | 终审协议重试：final_review_with_retry（连续 MAX_DOC_PROTOCOL_VIOLATIONS=3 次违例换实例重试，耗尽交回违例 verdict 走失败路径、账单留证，对齐单审口径）；运行账单 create/write 从 let _ = 吞错改 tracing::error! 含路径；回归 final_review_retries_protocol_violation + final_review_exhausts_violations_returns_last |
| 8 | aa96ca5 | 协议位收口：parse_doc_complete 尾部协议位（close 后全空白才算，正文引用示例块不再盖过尾部真标记，change SOP 同款纪律）+ extract_tag（review 三块）改前置 first find（DR-13 裁决块在第一行）；last_block_wins 按新语义改写 first_block_wins；回归 quoted_example_cannot_override_head_verdict + complete_marker_ignores_quoted_examples |
| 10 | aa96ca5 | runner.rs:12 「零硬限制」注释对齐实现（重推 3/空回复 3/协议违例 3/连续 2 FAIL 断路器；单元数按清单分母无额外上限） |
| 5 | 2482e74 | 一单元一提交补实现：PASS 后按本单元产出名单（#3 快照对的 files）git add+commit -- pathspec——回归实测逮住 lead=1 裹挟（整目录 add 把后续单元提前落盘的 b.md 卷进 A 的提交）改按名单收窄；非 git 仓/空名单/不可用三路降级 WARN 不阻断；回归 unit_pass_commits_docs_per_unit_in_git_repo（真 git 仓两单元两提交） |
| 7 | 2482e74 | doc_level 档位接线：DEVFORGE_DOC_LEVEL（默认 complete）经 DocLevel::parse 归一，单元审计注入/run_pipeline/终审机械门四处同口径（原先 parse 与档位只存在于纯函数层零读取点）；回归 doc_level_parse_defaults_to_complete |
| 9 | 2482e74 | Runbook 引擎代跑立为非目标：P008 README Out 节声明（抽取仅为审查参考文本，「从零 boot」验证交审查官判定与人工复核，引擎不代跑） |

## 核实总评

- 连续第五份外部报告 0 误报（10/10 实锤）。四批累计 53 条发现 53 条实锤。
- #1/#2 是文档 SOP 的立论级缺陷：覆盖审计是这个 SOP 的核心卖点（「清单可数、
  缺一条即零覆盖」），而词表假绿 + 不按 passed 拦让两个核心指标同时失真——
  比前几批的单点 bug 更伤 SOP 的存在理由。
- #3 是 lead=1 流水线化的并发生命周期缺陷（快照变量被下一单元覆盖）——
  与审计批次 #3（audit lead=1 共用 unit_before）是同一类病，跨 SOP 的
  「流水线快照生命周期」值得抽象成共享机制。
