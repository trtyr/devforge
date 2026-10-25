# 第三方审计核实 — src/audit/ 批次（2026-10-25）

> 外部第三方对 P013 审计编排域（src/audit/ 六文件）的 12 条发现，本档为逐条源码核实记录。
> **判定：12/12 属实**（#9 为窄条件属实）。本档只记录，修复待用户拍板。
> 核实方式：逐条对源码（file:line 实读），非采信报告原文。

## 发现清单与逐条判定

### major（7 条，全部属实）

| # | 判定 | 源码证据（实读核实） |
|---|---|---|
| 1 实例失败标 Quality 且整次 Completed | ✅ 属实 | pipeline.rs:154-159 `session.ask` 失败一律 `DevforgeError::Quality`（基础设施故障与质量失败不分型，与 review_with_gate 的 System 口径冲突）；runner.rs:106-110 该 Err 只进 `abandoned` 不分型，:171 无条件 `Ok(RunOutcome::Completed)`——RPC 挂死/profile 未就绪表现为「该批放弃、整次成功」 |
| 2 Stage 2 跨单元发现未过 verify | ✅ 属实 | runner.rs:115 verify 只盖 Stage 1 findings；:124 `stage2_cross` 产物 ：146 直接进报告 `cross_analysis`，:147 severity_summary 还把它计入统计——幽灵 path:line 可进最终报告，RepoAudit 无-validator FP +245% 教训对汇总层失效 |
| 3 违例重审测试空洞（LIFO） | ✅ 属实 | pipeline.rs:292 `self.replies.pop()`（Vec LIFO）；测试 :332 压序 `[违例, 成功]` → **第一次 open 就弹出成功回复**，重试循环未走到——删掉 `for attempt in 0..2` 该断言仍绿。「协议违例换实例」实际无测试锁 |
| 4 「零证据不入」无实现无有效测试 | ✅ 属实 | parse 层 finding.rs:94 空 evidence 合法；verify.rs 全文无 evidence 非空/含 path:line 检查；测试 finding.rs:134-138 只断言 `!evidence.is_empty()`（样例本来非空=空洞断言），注释自认「由核验器执行」而核验器没做 |
| 5 run_audit 无 RunControl，接不上 watchdog | ✅ 属实 | runner.rs:18-22 签名仅 (task, plans_root, factory)；builtin.rs:69-74 其余六个 SOP 全带 `control`，唯 :75-89 code-review 分支裸调 run_audit——M3 挂死（bash 14h 无 end）场景下 cancel/超时/idle 收割对审计 run 全部无效 |
| 6 确定性预查仍是占位串 | ✅ 属实 | runner.rs:75 `let precheck = "（引擎预查：…将随 T1.4 落地）";`，:88 原样注入每批——审查官提示词要求「预查是确定性事实直接采信」，实际吃到的是占位串，Stage 1 失去静态面（unwrap/埋点/行数机检全靠 LLM 重扫） |
| 7 path 直接 join 可路径穿越读盘 | ✅ 属实 | verify.rs:29 `workspace.join(&f.path)`、:90 evidence 引用路径同—无 canonical/starts_with/`..` 拒绝；恶意审查回复或被污染黑板可让进程读仓库外文件（读盘副作用真实发生，结果只取行数）。major 定级合理（攻击链需先控制回复/黑板） |

### minor（5 条，全部属实）

| # | 判定 | 源码证据 |
|---|---|---|
| 8 Progress::load 吞损坏 | ✅ 属实 | pipeline.rs:252-256 `.ok().and_then(serde).unwrap_or_default()` 无 WARN——损坏进度整表重跑；与 change::state::load 的 WARN 口径（复验#5 已修的同构问题）不一致 |
| 9 发现行号 0 过存在性检查 | ✅ 属实（窄条件） | verify.rs:40 `*total == 0 \|\| f.line > *total`——line=0 且文件非空时两条件皆 false → 过检查；仅当 evidence 含 `path:0` 形态引用才在 :98 被丢。主行号 0 未显式拒绝 |
| 10 分片标签全局 seq | ✅ 属实 | mod.rs:81 `seq` 跨 group 累加，:96 分母是当前组 `div_ceil`——跨组出现「分片 4/2」式标签 |
| 11 注释与实现不对齐 | ✅ 属实 | finding.rs:51 「上抛给 review_with_gate 换实例重审」；实际 pipeline.rs:149 自制 2 次循环，不经 review_with_gate、错误分类不同 |
| 12 batch_findings 累计值 | ✅ 属实 | runner.rs:96 `findings.extend(fs)` 后 :102 `batch_findings = findings.len()`——M3 中间文档靠差值反推的根因 |

## 修复优先级（第三方建议，待用户拍板）

| 优先级 | 项 | 理由 |
|---|---|---|
| P0 | #1 错误分类 + Completed | 基础设施故障被写成成功审计 |
| P0 | #5 RunControl/watchdog | 长跑 SOP 无法取消、无法收割失活 |
| P1 | #2 Stage 2 过 verify | 「无幽灵发现」底线对汇总层失效 |
| P1 | #6 预查 T1.4 | 审查官提示词的前提（预查=事实）不成立 |
| P1 | #7 path allowlist | 核验器按 LLM 路径读盘 |
| P2 | #3 重试测试改 FIFO | 锁住换实例逻辑 |
| P2 | #4 空 evidence 过滤 | 兑现「零证据不入」 |
| P3 | #8-#12 | 断点 WARN/line 0/分片标签/注释/日志字段 |

## 修复状态表（2026-10-25 第二轮，全部落地）

> 12/12 已修复，按审计报告建议的优先级批次推进；终验 784 绿 0 failed，
> clippy 无新增（双 EXIT 原始日志 bg_gdgf3g7r：TEST_EXIT=0 / CLIPPY_EXIT=0）。

| # | 修复 commit | 落地内容 |
|---|---|---|
| 1 | 24fd918 | pipeline ask 失败保持原分类上抛（去 Quality 包装）；runner Err 分流：Quality=协议违例放弃记录继续，System 等=打断整次审计（无「该批放弃整次成功」） |
| 5 | 24fd918 | run_audit 签名加 &RunControl + check_intervention 单点（入口/Stage0/每批/Stage2 四检查点）；builtin code-review 分支对齐其他六 SOP：require_profile 门卫 + run_with_watchdog（cancel/timeout/idle 收割生效）+ map_intervention |
| 2 | f27059d | Stage 2 跨单元发现同过 verify：cross_verify.kept 进报告与 severity 统计，dropped 并入 dropped_ghosts（Stage2 夹具幽灵 path 回归） |
| 6 | f27059d | 预查 T1.4 落地：precheck_report 零 LLM 机检（unwrap/expect/panic/todo/unreachable、let _ = 吞错、tracing 事件数、行数，注释行不计）替换占位串，批次循环内按批注入 |
| 7 | f27059d | 路径穿越静态拒绝：绝对路径或含 .. 段的发现路径与证据引用路径不读盘直接丢弃（堵 join 整体替换与 ../ 爬出） |
| 3 | 6f959cd | ScriptFactory LIFO→FIFO + opens 计数断言：违例→成功恰好 2 次 open（空洞测试关死）；无违例 1 次/双违例 2 次后放弃 |
| 4 | 6f959cd | verify 兑现「零证据不入」：has_evidence_ref 同源启发式，空 evidence/散文无引用在读盘前丢弃计「零证据」 |
| 8 | 240bd4f | Progress::load 损坏 WARN 留痕（文件不存在静默冷启动；存在但坏 WARN 后作废整表重跑，与 change::state::load 口径一致） |
| 9 | 240bd4f | verify 行号 0 丢弃（与 evidence 分支对齐，1-based 约定 0 必非法） |
| 10 | 240bd4f | 分片标签组内局部计数（chunk_i+1/n_chunks），全局 seq 不再出现「分片 4/2」 |
| 11 | 240bd4f | finding.rs 注释对齐实现（stage1 自制 2 次循环不经 review_with_gate，实例失败原分类上抛） |
| 12 | 240bd4f | batch_findings 改本批产出值（batch_n），另留 findings_total 累计值——M3 差值反推成历史 |

新增回归测试共 10 条：System 分类保持、实例失败打断整次（无报告假象+无发现落盘）、
cancel 入口即断不碰 factory、Stage2 幽灵进 dropped、穿越拒绝不读盘、precheck 计数、
opens 计数 ×2（隐含在重试用例）、零证据/散文无引用丢弃、line 0 丢弃。

## 核实总评

- 第三方审计 12/12 全中（含一处窄条件 #9），定位精确到行、无一条误报——连续第二次
  外部审查质量高于内部产出（上次 reverify-20261025 同结论）。
- 报告自述方法论与本仓审查官提示词的三步法/六视角同构，且预查缺失（#6）由报告自己
  点名——报告是在没有预查注入的条件下靠纯源码阅读做到的。
- 四条 major 里 #1/#5 与 M3 事故直接相关（挂死 14h 无人收割、失败被写成功），
  #2 与 RepoAudit 教训直接相关，#3 说明 M1 验收时有测试造假空间——修复前重跑自审
  意义有限（报告结论：先把 P0/P1 补上再重跑）。
