//! doc-from-zero 编排循环（P008 · D009/D010/D016/D020）。
//!
//! ```text
//! 门 0（引擎侧，run 启动前）：清单 + 闭包 + 单元编号 → 写 _meta/inventory.json
//!   ↓
//! 逐单元：doc-agent 写本单元 → <doc-complete>unit-x</doc-complete>
//!   → doc-review（每轮全新实例）→ PASS → 提交 + 写 state.json → 下一单元
//!                              → FAIL → <issues> + <route> 回注 → 按 route 修
//!   → PASS + route=finish（或单元耗尽）→ 收工
//! ```
//!
//! 零硬限制：单元数、打回轮次、重推次数均无上限；审查协议违例按 C19-R1 口径重跑审查（上限 3）。

use std::path::Path;

use crate::error::{DevforgeError, Result};
use crate::runstate::{RunState, StateStore};
use crate::sop::doc::protocol::{
    MAX_DOC_CONTINUE_NUDGES, MAX_DOC_EMPTY_REPLIES, MAX_DOC_PROTOCOL_VIOLATIONS,
    MAX_DOC_REVIEW_RETRIES, DocVerdict, parse_doc_complete, parse_doc_review,
};
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};

/// 产出目录（D016）：目标项目内。
pub const DOCS_DIR: &str = "docs/contexts";
/// 元数据目录（清单/词表/置信度/缺口）。
pub const META_DIR: &str = "docs/contexts/_meta";
// 检查点常量已删除：运行状态住黑板（plan 010 D001），产物目录不再持有 state.json。
// DOC_CONTINUE_NUDGE 已随串行 runner 删除（ARC-R1）：pipeline 用内联 nudge 话术。

/// 打回回注前缀：必须带回 route，让 agent 回对那一层。
pub const DOC_REMEDIATION_PREFIX: &str = "审查未通过，以下是必须修复的问题（请按 route 回到对应阶段修）：";

/// 进度字段（存在 `RunState.data` 里，引擎不解释；plan 010 D005）。
///
/// - `done`：已通过审查并落盘的单元号（预分配编号，重跑不漂移——P008 D020）
/// - `files`：每个单元当初产出的文件（相对产出根），续跑复核证据用（010 D003）
/// - `review_rounds` / `issues_total`：审查轮次与累计问题（账单用）
fn data_obj(state: &mut RunState) -> &mut serde_json::Map<String, serde_json::Value> {
    if !state.data.is_object() {
        state.data = serde_json::json!({});
    }
    state.data.as_object_mut().expect("data 已确保为对象")
}

/// 已完成的单元列表。
pub fn done_units(state: &RunState) -> Vec<String> {
    crate::runstate::resume::done_units(state)
}

/// 计划总单元数（DR-5：进度聚合端点用；未写入 = None）。
pub fn units_total(state: &RunState) -> Option<usize> {
    state
        .data
        .get("units_total")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
}

/// 是否已完成。
pub fn is_done(state: &RunState, unit: &str) -> bool {
    done_units(state).iter().any(|u| u == unit)
}

/// 把单元从 done 里摘除（重做前用：旧记录已不可信）。
pub fn unmark_done(state: &mut RunState, unit: &str) {
    let kept: Vec<String> = done_units(state).into_iter().filter(|u| u != unit).collect();
    data_obj(state).insert("done".into(), serde_json::json!(kept));
}

/// 标记单元完成，并登记它产出的文件（相对产出根）。
pub fn mark_done(state: &mut RunState, unit: &str, files: &[String]) {
    let mut done = done_units(state);
    if !done.iter().any(|u| u == unit) {
        done.push(unit.to_string());
    }
    let obj = data_obj(state);
    obj.insert("done".into(), serde_json::json!(done));
    let files_map = obj
        .entry("files")
        .or_insert_with(|| serde_json::json!({}));
    if !files_map.is_object() {
        *files_map = serde_json::json!({});
    }
    files_map
        .as_object_mut()
        .expect("files 已确保为对象")
        .insert(unit.to_string(), serde_json::json!(files));
}

/// 审查轮次 +1（包括重试与打回）。
pub fn bump_review(state: &mut RunState) -> usize {
    let round = review_rounds(state) + 1;
    data_obj(state).insert("review_rounds".into(), serde_json::json!(round));
    round
}

pub fn review_rounds(state: &RunState) -> usize {
    state
        .data
        .get("review_rounds")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

/// 累计问题数。
pub fn add_issues(state: &mut RunState, n: usize) {
    let total = issues_total(state) + n;
    data_obj(state).insert("issues_total".into(), serde_json::json!(total));
}

pub fn issues_total(state: &RunState) -> usize {
    state
        .data
        .get("issues_total")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize
}

/// 产出根下所有文件的相对路径与指纹（长度 + mtime 纳秒）——续跑证据的基线快照。
///
/// 用指纹而不是「路径集合」：重做单元时 agent 往往**改写**已有文件，
/// 只看「新出现的文件」会把重写判成没产出，下次续跑又会被踢回重做。
pub fn snapshot_files(root: &Path) -> std::collections::BTreeMap<String, (u64, u128)> {
    let mut out = std::collections::BTreeMap::new();
    fn walk(base: &Path, dir: &Path, out: &mut std::collections::BTreeMap<String, (u64, u128)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let Ok(rel) = p.strip_prefix(base) {
                let stamp = std::fs::metadata(&p)
                    .map(|m| {
                        let nanos = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_nanos())
                            .unwrap_or(0);
                        (m.len(), nanos)
                    })
                    .unwrap_or((0, 0));
                out.insert(rel.to_string_lossy().to_string(), stamp);
            }
        }
    }
    walk(root, root, &mut out);
    out
}

/// 单元跑完之后新增或改写的文件（相对产出根，排除 `_meta/` 下的元数据）。
pub fn produced_files(
    root: &Path,
    before: &std::collections::BTreeMap<String, (u64, u128)>,
) -> Vec<String> {
    snapshot_files(root)
        .into_iter()
        .filter(|(p, _)| !p.starts_with("_meta/"))
        .filter(|(p, stamp)| before.get(p) != Some(stamp))
        .map(|(p, _)| p)
        .collect()
}

/// doc-agent 开场指令：告知仓库、产出目录、本单元、文档档位。
pub fn doc_kickoff(repo: &Path, unit: &str, doc_level: &str) -> String {
    format!(
        "你是文档 Agent：本单元 `{unit}`（档位 `{doc_level}`）。\
\n目标仓库（只读，除了产出目录）：{}\n产出目录：{DOCS_DIR}/（本单元的所有文档写这里；元数据写 {META_DIR}/）\
\n清单：{META_DIR}/inventory.json（先读它，本单元包含哪些文件/符号/入口）\
\n节奏（DR-11 穿插推进，强制）：按源文件粒度边读边写——读完一个源文件，立即在同回合批量写出它的全部文档（多个写调用并行、路径不相交），再进入下一个源文件；本单元的 index/runbook/overview 等枢纽文档压轴最后写（依赖全部叶子）；单回合写入总量 ≤30K tokens 超限拆回合。禁止全部看完再动笔。\
\n纪律：只写 {DOCS_DIR}/ 下；codegraph-first；勘察只记事实、解释标 🟢/🟡/🔴；按 doc-contract 写、不适用写 N/A。\
\n交审前自检后用 <doc-complete>{unit}</doc-complete> 收尾。",
        repo.display()
    )
}

/// 审查指令：全新实例，带轮次号与单元号。
pub fn doc_review_instruction(unit: &str, round: usize) -> String {
    format!(
        "你是文档审查 Agent（第 {round} 轮，审单元 `{unit}`）。\
\n审查面：覆盖（清单条目 ↔ 文档，按档位集合）、完整性（必填节 / N-A 规则）、真实性（实体存在）、置信度诚实性（标 🟢 必须给得出 path:line）、可跑（**照 Runbook 从零 boot 一遍**：装依赖→配置→启动→健康检查→种子数据，跑不起来 = FAIL）。\
\n引擎审计是确定性事实（覆盖/lint/Runbook 命令块），直接采信并逐条验收，不要重算。\
\n**不读代码（DR-12）**：你的输入只有单元文档与引擎审计文本——事实以审计为准，不需要也不应该打开源码；文字质量/结构/一致性才是你的职责。\
\n**输出协议（强制，裁决前置）**：回复的**第一行**就是完整协议块——\
\n<doc-verdict>PASS|FAIL</doc-verdict> 换行 <route>...</route> 换行 <issues>每行一条</issues>——\
\n先把裁决交了，再写审查报告正文并落盘 {META_DIR}/../_meta/reviews/review-{round}.md。\
\n叙述从简：探索过程不复述，报告正文控制在 4000 字内；协议块被截断 = 本轮作废。"
    )
}

/// 终审指令（同一 doc-review profile，全新实例）：
/// 跨模块重复、总览/汇编引用一致性、全量 lint 与覆盖——单元审看不到的东西只在这里查。
pub fn doc_final_instruction(audit: &str) -> String {
    format!(
        "你是文档审查 Agent（**终审轮**，全库范围）。\
\n审查面（单元审没看到的部分）：\
1. **跨模块重复**：引擎分两段给——「同名出现（提示级）」只是提示（实体名在多篇里互相引用是正常写法，**不单独判 FAIL**）；「跨模块重复正文」才是判据（同一段解释被两篇以上具体描述 = FAIL，应改成引用）。\
2. **汇编层引用**：总览/架构/数据流里的每个引用是否指向存在的叶子文档；上层断言能否溯源到叶子证据。\
3. **全量 lint 与覆盖**：引擎审计已给退出码与缺口清单，逐条验收（禁止重算）。\
4. **置信度诚实性抽样**：抽 3 条标 🟢 的断言，看能不能真给 path:line。\
\n## 引擎审计（确定性，勿质疑）\n\n{audit}\n\n写 {META_DIR}/../_meta/reviews/final.md，然后输出协议块：<doc-verdict>PASS|FAIL</doc-verdict> + <route>...</route> + <issues>...</issues>。"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocReport {
    pub units_done: Vec<String>,
    pub review_rounds: usize,
    pub issues_total: usize,
}

/// 引擎侧审计提供者：输入单元号，返回确定性审计文本（覆盖 + lint）。
pub type AuditProvider = Box<dyn Fn(&str) -> String + Send>;



/// DR-14：审查 worker——独立线程上开全新审查会话，空响应/异常有界重试（C19-R1 口径），
/// 结果（含错误）经 channel 回主循环。
fn spawn_review_worker<F>(
    factory: &std::sync::Arc<std::sync::Mutex<F>>,
    unit: &str,
    round: u32,
    instruction: &str,
    tx: std::sync::mpsc::Sender<(String, u32, Result<String, DevforgeError>)>,
) where
    F: ReviewSessionFactory + Send + 'static,
{
    let factory = factory.clone();
    let unit = unit.to_string();
    let instruction = instruction.to_string();
    std::thread::spawn(move || {
        let result = (|| -> Result<String> {
            let mut retries = 0usize;
            // 协议违例独立计数（原串行 runner 口径：与空回复/异常重试分开）
            let mut violations = 0usize;
            loop {
                let mut session = {
                    let mut f = factory.lock().expect("审查工厂锁");
                    f.open()?
                };
                match session.review(&instruction) {
                    Ok(t) if !t.trim().is_empty() => {
                        // ARC-R1（自串行 runner 移植）：审查协议违例（有内容但缺 doc-verdict/route 块）
                        // → 就地重跑本轮审查（不回注被审 Agent）；耗尽按质量失败收场
                        let report = parse_doc_review(&t)?;
                        if !report.protocol_violation {
                            return Ok(t);
                        }
                        violations += 1;
                        if violations > MAX_DOC_PROTOCOL_VIOLATIONS {
                            return Err(DevforgeError::Quality(format!(
                                "审查连续 {violations} 轮输出协议违例（缺 doc-verdict/route 块），判定为质量失败（不自动重启）：{}",
                                report.issues.join("；")
                            )));
                        }
                    }
                    Ok(_) => {
                        retries += 1;
                        if retries > MAX_DOC_REVIEW_RETRIES {
                            return Err(DevforgeError::System(format!(
                                "审查会话连续 {retries} 次空回复"
                            )));
                        }
                    }
                    Err(e) => {
                        retries += 1;
                        if retries > MAX_DOC_REVIEW_RETRIES {
                            return Err(e);
                        }
                    }
                }
            }
        })();
        let _ = tx.send((unit, round, result));
    });
}

/// DR-14 流水线 runner：审查(N) 与 写作(N+1) 重叠（lead=1，在途审查槽 = 1），
/// 断路器 = 连续 2 FAIL（andon），fix 队列优先于新单元。事件与串行版同套。
/// 工厂经 Arc<Mutex<F>> 共享给审查线程；被审 Agent 仍为单会话顺序推进。
#[allow(clippy::too_many_arguments, unused_assignments)]
pub fn run_pipeline<P, F>(
    mut agent: P,
    factory: std::sync::Arc<std::sync::Mutex<F>>,
    store: StateStore,
    ui: &mut dyn UserLink,
    repo: &Path,
    units: &[String],
    doc_level: &str,
    resume_note: String,
    audit: Option<AuditProvider>,
    first_unit_deadline_ms: Option<u64>,
) -> Result<DocReport>
where
    P: PlanAgentSession,
    F: ReviewSessionFactory + Send + 'static,
{
    let docs_root = repo.join(DOCS_DIR);
    let mut state = match store.load() {
        crate::runstate::Loaded::Found(s) => s,
        crate::runstate::Loaded::Missing => store.fresh_state(),
        crate::runstate::Loaded::Unusable(reason) => {
            tracing::warn!(component = "doc-sop", %reason, "断点不可用，按冷启动");
            store.fresh_state()
        }
    };
    // DR-5：总单元数入断点 + 立即落盘
    data_obj(&mut state)
        .entry("units_total".to_string())
        .or_insert_with(|| serde_json::json!(units.len()));
    store.save(&mut state)?;

    let mut pending: Option<(usize, String, u32, std::sync::mpsc::Receiver<(String, u32, Result<String, DevforgeError>)>)> = None;
    // lead=1 写超前：审查槽被占时交卷的单元先排队，槽空出时补派
    let mut written_unreviewed: std::collections::VecDeque<(usize, String, u32)> = Default::default();
    let mut fix_queue: std::collections::VecDeque<(String, String, Vec<String>, u32)> = Default::default();
    let mut consecutive_fails = 0u32;
    let mut frontier = 0usize;
    let mut active = String::new();
    let mut active_idx = 0usize;
    let mut active_is_fix = false;
    let mut active_fix_round: Option<u32> = None;
    let mut last_unit: Option<String> = None;
    let mut unit_before = None;
    let mut resume_note = resume_note;
    let mut first_kickoff_sent = false;
    let mut nudges = 0usize;
    let mut nudged_for = String::new();
    // TODO(DR-10③)：流水线路径的 glue 计时（stage）尚未接线；协议违例计数已下沉到 spawn_review_worker
    let mut _stage: Option<(&'static str, std::time::Instant)> = None;
    // DR-4：首单元逾期守卫——开跑超阈值仍无任何单元完成 → 告警一次（假活/单元过重）
    let run_started = std::time::Instant::now();
    let first_unit_deadline =
        first_unit_deadline_ms.unwrap_or(crate::observability::stall_warn_ms() * 2);
    let mut first_unit_warned = false;

    enum Next {
        Fix(String, String, Vec<String>, u32),
        Write(String),
    }

    // 裁决处理：Pass → mark_done；Fail → fix 队列 + 断路器
    macro_rules! process_verdict {
        ($unit:expr, $report:expr, $round:expr) => {{
            let report = $report;
            match report.verdict {
                DocVerdict::Pass => {
                    let files = produced_files(
                        &docs_root,
                        unit_before.as_ref().expect("写前快照应在"),
                    );
                    mark_done(&mut state, &$unit, &files);
                    store.save(&mut state)?;
                    consecutive_fails = 0;
                    tracing::info!(
                        component = "doc-sop",
                        event = "doc.unit_done",
                        unit = %$unit,
                        done = done_units(&state).len(),
                        "单元完成（流水线）"
                    );
                }
                DocVerdict::Fail => {
                    add_issues(&mut state, report.issues.len());
                    fix_queue.push_back((
                        $unit.clone(),
                        report.route.as_str().to_string(),
                        report.issues.clone(),
                        $round,
                    ));
                    consecutive_fails += 1;
                    if consecutive_fails >= 2 {
                        return Err(DevforgeError::Quality(format!(
                            "断路器：连续 {} 轮审查 FAIL（最近单元 `{}`），停发新单元——修复队列 {} 项（resume 可续）",
                            consecutive_fails,
                            $unit,
                            fix_queue.len()
                        )));
                    }
                }
            }
        }};
    }

    // 审查槽空出时，把排队中的已交卷单元补派审查（与 F 的首次派发同一口径）
    macro_rules! spawn_queued_review {
        () => {{
            if pending.is_none() {
                if let Some((qidx, qu, qround)) = written_unreviewed.pop_front() {
                    let mut instruction = doc_review_instruction(&qu, qround as usize);
                    if let Some(audit_fn) = &audit {
                        instruction.push_str("\n\n## 引擎审计（确定性，勿质疑）\n\n");
                        instruction.push_str(&audit_fn(&qu));
                    }
                    let (qtx, qrx) = std::sync::mpsc::channel();
                    spawn_review_worker(&factory, &qu, qround, &instruction, qtx);
                    pending = Some((qidx, qu, qround, qrx));
                }
            }
        }};
    }

    loop {
        // DR-4：首单元逾期检查（每轮查一次，只告警一次）
        if !first_unit_warned
            && done_units(&state).is_empty()
            && run_started.elapsed().as_millis() as u64 >= first_unit_deadline
        {
            first_unit_warned = true;
            tracing::warn!(
                component = "doc-sop",
                event = "doc.first_unit_overdue",
                deadline_ms = first_unit_deadline,
                elapsed_ms = run_started.elapsed().as_millis() as u64,
                "首单元逾期：开跑超阈值仍无任何单元完成——agent 可能假活或单元过重"
            );
        }

        // ── A. 非阻塞清账：在途裁决到了就处理 ──
        if let Some((idx, unit, round, rx)) = pending.take() {
            match rx.try_recv() {
                Ok((u, round, text)) => {
                    let text = text.map_err(|e| {
                        if e.is_quality_failure() {
                            e
                        } else {
                            DevforgeError::System(format!("审查重试耗尽：{e}"))
                        }
                    })?;
                    let report = parse_doc_review(&text)?;
                    process_verdict!(u, report, round);
                    let _ = (idx, round);
                    spawn_queued_review!();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    pending = Some((idx, unit, round, rx));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(DevforgeError::System("审查线程失联".into()));
                }
            }
        }

        // ── B. 决定下一步 ──
        let next = if let Some((u, route, issues, fix_round)) = fix_queue.pop_front() {
            Next::Fix(u, route, issues, fix_round)
        } else if frontier < units.len() {
            // lead=1（用户拍板 10-23）：审查 N 在途期间允许写 N+1（写超前 1）；
            // 要写 N+2（写超前将到 2）才必须先等 N 的裁决落地。
            let allowed = match &pending {
                None => true,
                Some((pidx, _, _, _)) => frontier <= *pidx + 1,
            };
            if allowed {
                let u = units[frontier].clone();
                active_idx = frontier;
                frontier += 1;
                Next::Write(u)
            } else {
                // 写超前已满 → 等在途裁决落地（可能回填 fix 队列），回循环头重新决定
                let (_, _, _, rx) = pending.take().expect("pending 已判存在");
                let (vu, vround, vtext) = rx
                    .recv()
                    .map_err(|_| DevforgeError::System("审查线程失联".into()))?;
                let vtext = vtext.map_err(|e| {
                    if e.is_quality_failure() {
                        e
                    } else {
                        DevforgeError::System(format!("审查重试耗尽：{e}"))
                    }
                })?;
                let report = parse_doc_review(&vtext)?;
                process_verdict!(vu, report, vround);
                let _ = vround;
                spawn_queued_review!();
                continue;
            }
        } else if let Some((_, _, _, rx)) = pending.take() {
            // 无新活但审查在途 → 等它（可能产生 fix 或补派排队审查）；
            // 终止由循环头收敛：裁决清完 + 队列空 + fix 空 → else break
            let (vunit, vround, vtext) = rx
                .recv()
                .map_err(|_| DevforgeError::System("审查线程失联".into()))?;
            let vtext = vtext.map_err(|e| {
                if e.is_quality_failure() {
                    e
                } else {
                    DevforgeError::System(format!("审查重试耗尽：{e}"))
                }
            })?;
            let report = parse_doc_review(&vtext)?;
            process_verdict!(vunit, report, vround);
            spawn_queued_review!();
            continue;
        } else {
            break;
        };

        // ── C. 组消息 + 交回合 ──
        let (unit, mut message, _is_fix) = match next {
            Next::Fix(u, route, issues, fix_round) => {
                active = u.clone();
                active_idx = units.iter().position(|x| *x == u).unwrap_or(active_idx);
                active_is_fix = true;
                active_fix_round = Some(fix_round);
                tracing::info!(
                    component = "doc-sop",
                    event = "fix.start",
                    unit = %u,
                    round = fix_round,
                    "单元重写开始（流水线）"
                );
                let mut msg = format!("{DOC_REMEDIATION_PREFIX}\n<route>{route}</route>\n");
                for i in &issues {
                    msg.push_str(&format!("- {i}\n"));
                }
                (u, msg, true)
            }
            Next::Write(u) => {
                active = u.clone();
                active_is_fix = false;
                active_fix_round = None;
                unit_before = Some(snapshot_files(&docs_root));
                tracing::info!(
                    component = "doc-sop",
                    event = "doc.unit_start",
                    unit = %u,
                    index = frontier,
                    total = units.len(),
                    "单元开始（流水线）"
                );
                let mut msg = doc_kickoff(repo, &u, doc_level);
                if !first_kickoff_sent && !resume_note.is_empty() {
                    msg.push_str("\n\n");
                    msg.push_str(&resume_note);
                    resume_note.clear();
                }
                first_kickoff_sent = true;
                (u, msg, false)
            }
        };
        // DR-2：阶段推进（上一个单元 → 当前单元；fix 轮单元未变不重复落）
        if !active_is_fix {
            tracing::info!(
                component = "doc-sop",
                event = "phase.change",
                from = last_unit.clone().unwrap_or_else(|| "start".into()),
                to = unit.as_str(),
                "doc 单元推进（流水线）"
            );
            last_unit = Some(unit.clone());
        }

        // ── D. 交回合（空响应 + 标记守卫原地重试）──
        // ARC-R1 修：nudge 不回主循环——lead=1 状态机下 continue 回循环头会命中
        // B 段 break/重新 kickoff，话术永远到不了模型（四大失败用例同根因）
        let mut empty_replies = 0usize;
        loop {
            let t = agent.converse(&message, ui)?;
            // DR-8：空响应 = provider 侧失败信号，有界重试后升级基础设施故障
            if t.trim().is_empty() {
                empty_replies += 1;
                if empty_replies > MAX_DOC_EMPTY_REPLIES {
                    return Err(DevforgeError::System(format!(
                        "单元 `{unit}` 连续 {empty_replies} 次返回空内容（0 tokens，provider 侧失败），判定为基础设施故障，可重试"
                    )));
                }
                tracing::warn!(
                    component = "doc-sop",
                    event = "agent.empty_reply",
                    unit = %unit,
                    count = empty_replies,
                    "空回复（provider 失败信号），重新提示"
                );
                continue;
            }
            // 标记守卫（nudge 同串行，按 active 单元计；原地重试）
            if nudged_for != active {
                nudges = 0;
                nudged_for = active.clone();
            }
            let Some(done_unit) = parse_doc_complete(&t) else {
                nudges += 1;
                if nudges > MAX_DOC_CONTINUE_NUDGES {
                    return Err(DevforgeError::Quality(format!(
                        "单元 `{active}` 连续 {nudges} 次未输出 <doc-complete>，判定为质量失败（agent 不配合，重启也无意义）"
                    )));
                }
                message = format!(
                    "连续 {nudges} 次未输出完成标记。请补交：<doc-complete>{active}</doc-complete>（单独一行）。"
                );
                continue;
            };
            if done_unit != active {
                message = format!(
                    "你输出的是 `{done_unit}`，但当前单元是 `{active}`。请确认编号（不得自行改号）并重新输出 <doc-complete>{active}</doc-complete>。"
                );
                continue;
            }
            nudges = 0;
            break t;
        };
        // fix 回注已随本回合交卷 → 修复轮收尾（串行口径：fix.end 落在交卷回合）
        if active_is_fix {
            tracing::info!(
                component = "doc-sop",
                event = "fix.end",
                round = active_fix_round.unwrap_or(0),
                "单元重写结束（流水线）"
            );
            active_is_fix = false;
            active_fix_round = None;
        }

        // ── F. 交卷 → 派审查（占 pending 槽；槽被占则排队，空出时补派）──
        let round = bump_review(&mut state) as u32;
        tracing::info!(
            component = "doc-sop",
            event = "review.open",
            role = "doc-review",
            round,
            "审查开庭（流水线）"
        );
        if pending.is_some() {
            written_unreviewed.push_back((active_idx, active.clone(), round));
        } else {
            let mut instruction = doc_review_instruction(&active, round as usize);
            if let Some(audit_fn) = &audit {
                instruction.push_str("\n\n## 引擎审计（确定性，勿质疑）\n\n");
                instruction.push_str(&audit_fn(&active));
            }
            let (tx, rx) = std::sync::mpsc::channel();
            spawn_review_worker(&factory, &active, round, &instruction, tx);
            pending = Some((active_idx, active.clone(), round, rx));
        }

        // 回到循环头：非阻塞清账 → 决定下一个（fix 优先 / 新单元 kickoff）
        // lead=1：pending 占位期间不开新审查；写手与审查的时间重叠由此产生。
    }

    Ok(DocReport {
        units_done: done_units(&state),
        review_rounds: review_rounds(&state),
        issues_total: issues_total(&state),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sop::planning::ReviewSession;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    struct NoUser;
    impl UserLink for NoUser {
        fn ask(&mut self, q: &str, _: &[&str]) -> Result<String> {
            Err(DevforgeError::System(format!("测试不应有用户交互: {q}")))
        }
    }

    struct MockAgent {
        turns: VecDeque<String>,
        received: Rc<RefCell<Vec<String>>>,
    }
    impl MockAgent {
        fn new(turns: &[&str]) -> Self {
            Self {
                turns: turns.iter().map(|s| s.to_string()).collect(),
                received: Rc::new(RefCell::new(Vec::new())),
            }
        }
    }
    impl PlanAgentSession for MockAgent {
        fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
            self.received.borrow_mut().push(message.into());
            Ok(self.turns.pop_front().expect("MockAgent 轮次耗尽"))
        }
    }

    /// 无闸门审查工厂：裁决立即返回（行为类测试用）
    fn pipe_factory(replies: &[&str]) -> PipeReviewFactory {
        PipeReviewFactory {
            replies: Arc::new(Mutex::new(
                replies.iter().map(|s| s.to_string()).collect(),
            )),
            instructions: Arc::new(Mutex::new(Vec::new())),
            gates: no_gates(),
        }
    }

    /// 行为类测试的 pipeline 调用（闸门/注入类用例用全参数 run_pipeline）
    macro_rules! run_pipe {
        ($agent:expr, $f:expr, $tmp:expr, $units:expr) => {
            run_pipeline(
                $agent,
                Arc::new(Mutex::new($f)),
                state_path(&$tmp),
                &mut NoUser,
                $tmp.path(),
                &$units,
                "complete",
                String::new(),
                None,
                None,
            )
        };
    }

    fn state_path(tmp: &tempfile::TempDir) -> StateStore {
        StateStore::new(
            tmp.path().join("plans"),
            crate::runstate::StateKey::new("p", "t", crate::sop::doc::resume::DOC_SOP),
        )
    }

    /// 种一个「已有 done」的断点（续跑类用例公用）。
    fn seed_done(store: &StateStore, done: &[&str]) {
        let mut state = store.fresh_state();
        for u in done {
            mark_done(&mut state, u, &[]);
        }
        if !done.is_empty() {
            bump_review(&mut state); // 与旧 fixture 一致：已完成单元落过一次审查轮次
        }
        store.save(&mut state).unwrap();
    }

    #[test]
    fn two_units_pass_and_checkpoint_written() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "写完 A\n<doc-complete>unit-a-01</doc-complete>",
            "写完 B\n<doc-complete>unit-b-02</doc-complete>",
        ]);
        let f = pipe_factory(&[
            "<doc-verdict>PASS</doc-verdict>",
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
        let rep = run_pipe!(agent, f, tmp, units).unwrap();
        assert_eq!(rep.units_done, units);
        assert_eq!(rep.review_rounds, 2);
        let saved = state_path(&tmp).load();
        assert_eq!(done_units(saved.state().expect("断点应已落盘")), units);
    }

    #[test]
    fn empty_review_reply_is_retried_with_fresh_instance() {
        // 2026-10-20 实测：doc-review 返回空回复（模型一字未出），旧行为把整个 run 判死
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["写完了\n<doc-complete>unit-a-01</doc-complete>"]);
        let f = pipe_factory(&["", "<doc-verdict>PASS</doc-verdict>"]);
        let instructions = f.instructions.clone();
        let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
        assert_eq!(rep.units_done, vec!["unit-a-01"]);
        assert_eq!(instructions.lock().unwrap().len(), 2, "空回复应换实例重试一次");
    }

    #[test]
    fn repeated_empty_reviews_fail_fast() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["写完了\n<doc-complete>unit-a-01</doc-complete>"]);
        let f = pipe_factory(&["", "", "", ""]);
        let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("空回复"), "{msg}");
    }

    #[test]
    fn missing_marker_is_bounded_not_infinite() {
        // 零上限的旧口径在这里就是无限空转：连续不发标记必须报错
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["想想", "再想想", "还在想", "就是不写"]);
        let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
        let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
        assert!(format!("{err}").contains("doc-complete"), "{err}");
    }

    #[test]
    fn first_unit_overdue_warns_exactly_once() {        // DR-4：开跑超阈值仍无任何单元完成 → doc.first_unit_overdue 告警一次
        // （模拟假活：deadline 注入 0；agent 永不交卷）
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["想想", "再想想", "还在想", "就是不写"]);
        let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
        let log = crate::observability::test_support::capture_jsonl(|| {
            // mock 回合微秒级完成：不给时钟前进的机会就永远到不了 deadline
            std::thread::sleep(std::time::Duration::from_millis(5));
            let err = run_pipeline(
                agent,
                Arc::new(Mutex::new(f)),
                state_path(&tmp),
                &mut NoUser,
                tmp.path(),
                &["unit-a-01".to_string()],
                "complete",
                String::new(),
                None,
                Some(0),
            );
            assert!(err.is_err(), "agent 永不交卷应报错收场");
        });
        assert!(log.contains("doc.first_unit_overdue"), "应告警: {log}");
        assert_eq!(
            log.matches("doc.first_unit_overdue").count(),
            1,
            "只告警一次，不刷屏: {log}"
        );
    }

    #[test]
    fn kickoff_carries_interleave_and_batch_rules() {
        // DR-11/DR-10④：kickoff 必须带穿插节奏与批量写预算护栏（合同 §七 同款）
        let k = doc_kickoff(Path::new("/repo"), "unit-x", "complete");
        assert!(k.contains("穿插推进"), "要带穿插节奏条款: {k}");
        assert!(k.contains("批量写出"), "要带同回合批量写: {k}");
        assert!(k.contains("30K tokens"), "要带输出预算护栏: {k}");
        assert!(k.contains("压轴最后写"), "枢纽压轴规则要在: {k}");
    }

    #[test]
    fn review_instruction_puts_verdict_first() {
        // DR-13：裁决块前置——防思考型输出把协议块挤到截断线外；DR-12：不读码条款
        let ins = doc_review_instruction("unit-x", 1);
        assert!(ins.contains("第一行"), "要强制裁决块前置: {ins}");
        assert!(ins.contains("<doc-verdict>"), "要带协议块格式: {ins}");
        assert!(ins.contains("4000 字"), "要带叙述预算: {ins}");
        assert!(ins.contains("本轮作废"), "要写明截断后果: {ins}");
        assert!(ins.contains("不读代码"), "DR-12 不读码条款要在: {ins}");
    }

    #[test]
    fn empty_reply_is_infra_not_quality() {
        // DR-8：空响应 = provider 侧失败 → System（可重试），非 Quality；不计入 doc-complete nudges
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["", "", "", ""]);
        let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
        let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
        assert!(!err.is_quality_failure(), "空响应不得判质量失败: {err}");
        assert!(format!("{err}").contains("基础设施故障"), "{err}");
    }

    // DR-9 L3 glue 计时（engine.glue）在流水线路径尚未接线（TODO DR-10③），专项用例随串行 runner 退役

    // ── DR-14 流水线并行测试（Send 版 mock）──

    #[derive(Clone)]
    struct PipeAgent {
        replies: Arc<Mutex<VecDeque<String>>>,
        received: Arc<Mutex<Vec<String>>>,
    }
    impl PipeAgent {
        fn new(replies: &[&str]) -> (Self, Arc<Mutex<Vec<String>>>) {
            let received = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    replies: Arc::new(Mutex::new(
                        replies.iter().map(|s| s.to_string()).collect(),
                    )),
                    received: received.clone(),
                },
                received,
            )
        }
    }
    impl PlanAgentSession for PipeAgent {
        fn converse(&mut self, message: &str, _: &mut dyn UserLink) -> Result<String> {
            self.received.lock().unwrap().push(message.into());
            let popped = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default();
            Ok(popped)
        }
    }

    #[derive(Clone)]
    struct PipeReviewFactory {
        replies: Arc<Mutex<VecDeque<String>>>,
        instructions: Arc<Mutex<Vec<String>>>,
        // 闸门：每个审查会话开庭后等放行再出裁决（模拟真实审查耗时，测试才能确定性断言重叠）
        gates: Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>>,
    }
    impl ReviewSessionFactory for PipeReviewFactory {
        fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
            Ok(Box::new(PipeReviewSession {
                replies: self.replies.clone(),
                instructions: self.instructions.clone(),
                gate: self.gates.clone(),
            }))
        }
    }
    struct PipeReviewSession {
        replies: Arc<Mutex<VecDeque<String>>>,
        instructions: Arc<Mutex<Vec<String>>>,
        gate: Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>>,
    }
    impl ReviewSession for PipeReviewSession {
        fn review(&mut self, instruction: &str) -> Result<String> {
            self.instructions.lock().unwrap().push(instruction.into());
            if let Some(rx) = self.gate.lock().unwrap().pop_front() {
                let _ = rx.recv();
            }
            Ok(self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default())
        }
    }

    /// 事件驱动放行：每个闸门等 watch 闭包达标后放行；10s 死线强制全放（防测试悬挂）
    fn drive_gates(
        watch: Vec<(Box<dyn Fn() -> usize + Send>, usize)>,
        gate_txs: Vec<std::sync::mpsc::Sender<()>>,
    ) {
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            for (tx, (count_fn, threshold)) in gate_txs.into_iter().zip(watch) {
                while std::time::Instant::now() < deadline && count_fn() < threshold {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                let _ = tx.send(());
            }
        });
    }

    fn gate_pair() -> (
        std::sync::mpsc::Sender<()>,
        std::sync::mpsc::Receiver<()>,
    ) {
        std::sync::mpsc::channel()
    }

    fn no_gates() -> Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>> {
        Arc::new(Mutex::new(Default::default()))
    }

    #[test]
    fn pipeline_runs_units_with_overlapped_reviews() {
        // DR-14 lead=1：审查 a 在途期间，b 的 kickoff 必须先行（写审重叠）；
        // 串行退化 = 闸门死等 10s 后强放 + 时长断言失败
        let tmp = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let (agent, received) = PipeAgent::new(&[
            "A\n<doc-complete>unit-a-01</doc-complete>",
            "B\n<doc-complete>unit-b-02</doc-complete>",
        ]);
        let (tx1, rx1) = gate_pair();
        let received_w = received.clone();
        let factory = PipeReviewFactory {
            replies: Arc::new(Mutex::new(
                [
                    "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
                    "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            )),
            instructions: Arc::new(Mutex::new(Vec::new())),
            gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1]))),
        };
        drive_gates(
            vec![(
                Box::new(move || received_w.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
                2,
            )],
            vec![tx1],
        );
        let report = run_pipeline(
            agent,
            Arc::new(Mutex::new(factory)),
            state_path(&tmp),
            &mut NoUser,
            tmp.path(),
            &["unit-a-01".to_string(), "unit-b-02".to_string()],
            "complete",
            String::new(),
            None,
            None,
        )
        .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "写审应重叠（串行退化会死等闸门到死线）：{:?}",
            started.elapsed()
        );
        let got = received.lock().unwrap().join("|||");
        assert!(got.contains("unit-a-01"), "收到的消息: {got}");
        assert_eq!(report.units_done, vec!["unit-a-01", "unit-b-02"]);
    }

    #[test]
    fn pipeline_fix_round_requeues_failed_unit() {
        // DR-14 lead=1 语义：a 写完即开审（在途），b 立刻 kickoff（写超前 1）；
        // a 的 FAIL 裁决在下一个空闲点（b 交卷后）以 fix 优先送达——fix 不打断在写单元
        let tmp = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let (agent, received) = PipeAgent::new(&[
            "W1\n<doc-complete>unit-a-01</doc-complete>",
            "W2\n<doc-complete>unit-b-02</doc-complete>",
            "FIX1\n<doc-complete>unit-a-01</doc-complete>",
        ]);
        let (tx1, rx1) = gate_pair();
        let (tx2, rx2) = gate_pair();
        let (tx3, rx3) = gate_pair();
        let instructions = Arc::new(Mutex::new(Vec::new()));
        let (received_w, instr_w2, instr_w3) = (received.clone(), instructions.clone(), instructions.clone());
        let factory = PipeReviewFactory {
            replies: Arc::new(Mutex::new(VecDeque::from(vec![
                "<doc-verdict>FAIL</doc-verdict>\n<route>finish</route>\n<issues>\n- 描述与行为不符\n</issues>"
                    .to_string(),
                "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>".to_string(),
                "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>".to_string(),
            ]))),
            instructions: instructions.clone(),
            gates: Arc::new(Mutex::new(VecDeque::from(vec![rx1, rx2, rx3]))),
        };
        drive_gates(
            vec![
                (
                    Box::new(move || received_w.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
                    2,
                ),
                (
                    Box::new(move || instr_w2.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
                    2,
                ),
                (
                    Box::new(move || instr_w3.lock().unwrap().len()) as Box<dyn Fn() -> usize + Send>,
                    3,
                ),
            ],
            vec![tx1, tx2, tx3],
        );
        let report = run_pipeline(
            agent,
            Arc::new(Mutex::new(factory)),
            state_path(&tmp),
            &mut NoUser,
            tmp.path(),
            &["unit-a-01".to_string(), "unit-b-02".to_string()],
            "complete",
            String::new(),
            None,
            None,
        )
        .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "写审应重叠：{:?}",
            started.elapsed()
        );
        let mut done = report.units_done.clone();
        done.sort();
        assert_eq!(
            done,
            vec!["unit-a-01", "unit-b-02"],
            "修复后的 unit 1 与 unit 2 都应过审（过审标记顺序随裁决到达序）"
        );
        let msgs = received.lock().unwrap();
        assert_eq!(msgs.len(), 3, "消息序应为 [kickoff a, kickoff b, fix a]: {msgs:?}");
        assert!(
            msgs[1].contains("unit-b-02"),
            "第 2 条应是 unit 2 的 kickoff（审 a 期间写 b）: {msgs:?}"
        );
        assert!(
            msgs[2].contains(DOC_REMEDIATION_PREFIX),
            "FAIL 裁决在 b 交卷后的空闲点送达 fix: {msgs:?}"
        );
    }

    #[test]
    fn pipeline_breaker_stops_after_two_consecutive_fails() {
        // DR-14 断路器：连续 2 FAIL → andon 熔断（Quality），不再无限重跑
        let tmp = tempfile::tempdir().unwrap();
        let (agent, _) = PipeAgent::new(&[
            "W1\n<doc-complete>unit-a-01</doc-complete>",
            "FIX1\n<doc-complete>unit-a-01</doc-complete>",
        ]);
        let factory = PipeReviewFactory {
            replies: Arc::new(Mutex::new(VecDeque::from(vec![
                "<doc-verdict>FAIL</doc-verdict>\n<route>finish</route>\n<issues>\n- 问题一\n</issues>"
                    .to_string(),
                "<doc-verdict>FAIL</doc-verdict>\n<route>finish</route>\n<issues>\n- 问题依旧\n</issues>"
                    .to_string(),
            ]))),
            instructions: Arc::new(Mutex::new(Vec::new())),
            gates: no_gates(),
        };
        let err = run_pipeline(
            agent,
            Arc::new(Mutex::new(factory)),
            state_path(&tmp),
            &mut NoUser,
            tmp.path(),
            &["unit-a-01".to_string()],
            "complete",
            String::new(),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.is_quality_failure(), "断路器应判质量失败: {err}");
        assert!(format!("{err}").contains("断路器"), "{err}");
    }

    #[test]
    fn pass_with_finish_route_does_not_end_the_run() {
        // 2026-10-19 实测：审查官对第一个单元就回 PASS+finish，旧逻辑直接 return，
        // 第二个单元压根没写，run 却报 Settled。
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "写完 A\n<doc-complete>unit-a-01</doc-complete>",
            "写完 B\n<doc-complete>unit-b-02</doc-complete>",
        ]);
        let f = pipe_factory(&[
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
        let rep = run_pipe!(agent, f, tmp, units).unwrap();
        assert_eq!(rep.units_done, units, "finish 只收口本单元，不得结束整个 run");
        assert_eq!(rep.review_rounds, 2);
    }

    #[test]
    fn fail_message_contains_route_and_issues() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "v1\n<doc-complete>unit-a-01</doc-complete>",
            "v2\n<doc-complete>unit-a-01</doc-complete>",
        ]);
        let received = agent.received.clone();
        let f = pipe_factory(&[
            "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错\n</issues>",
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
        let msgs = received.borrow();
        assert!(msgs.len() >= 2);
        assert!(msgs[1].contains("<route>phase1</route>"));
        assert!(msgs[1].contains("事实错"));
    }

    #[test]
    fn review_open_fix_round_and_unit_advance_are_logged_as_milestones() {
        // 错实现形态：①审查开庭不落 review.open（或缺角色名/轮次）
        // ②单元判负后的重写不落 fix.start/fix.end ③单元推进不落 phase.change → 本用例必须变红
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "v1\n<doc-complete>unit-a-01</doc-complete>",
            "v2\n<doc-complete>unit-a-01</doc-complete>",
        ]);
        let f = pipe_factory(&[
            "<doc-verdict>FAIL</doc-verdict>\n<route>phase1</route>\n<issues>\n- 事实错\n</issues>",
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let units = vec!["unit-a-01".to_string()];
        let text = crate::observability::test_support::capture_jsonl(|| {
            run_pipe!(agent, f, tmp, units).unwrap();
        });

        // 审查开庭：两轮各一条，角色名与轮次（首审 = 1，轮次 = bump_review 的 1-based 计数）
        let opens: Vec<&str> = text.lines().filter(|l| l.contains("review.open")).collect();
        assert_eq!(opens.len(), 2, "两轮审查应各落一条：{text}");
        assert!(
            opens[0].contains("\"role\":\"doc-review\""),
            "开庭要带角色名：{}",
            opens[0]
        );
        assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{}", opens[0]);
        assert!(opens[1].contains("\"round\":2"), "次审轮次 = 2：{}", opens[1]);
        assert!(
            opens[0].contains("\"target\":\"devforge::sop::doc_from_zero\""),
            "里程碑的 target 必须是发出它的模块：{}",
            opens[0]
        );
        // 全部里程碑条目：级别 INFO + 带 component 字段（计划 §5）
        for line in text.lines().filter(|l| l.contains("\"event\":")) {
            crate::observability::test_support::assert_milestone_shape(line);
        }

        // 单元判负后的重写：一对 fix.start / fix.end（含轮次）
        let starts: Vec<&str> = text.lines().filter(|l| l.contains("fix.start")).collect();
        let ends: Vec<&str> = text.lines().filter(|l| l.contains("fix.end")).collect();
        assert_eq!(starts.len(), 1, "判负重写应落一条 fix.start：{text}");
        assert_eq!(ends.len(), 1, "判负重写应落一条 fix.end：{text}");
        assert!(starts[0].contains("\"round\":1"), "重写轮次要带上：{}", starts[0]);
        assert!(ends[0].contains("\"round\":1"), "重写轮次要带上：{}", ends[0]);

        // 单元推进：start → unit-a-01
        let phases: Vec<&str> = text.lines().filter(|l| l.contains("phase.change")).collect();
        assert_eq!(phases.len(), 1, "一个单元应落一条推进：{text}");
        assert!(
            phases[0].contains("\"from\":\"start\"")
                && phases[0].contains("\"to\":\"unit-a-01\""),
            "单元推进要从 start 到 unit-a-01：{}",
            phases[0]
        );
    }

    #[test]
    fn audit_text_is_injected_into_review_instruction() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
        let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
        let instructions = f.instructions.clone();
        run_pipeline(
            agent,
            Arc::new(Mutex::new(f)),
            state_path(&tmp),
            &mut NoUser,
            tmp.path(),
            &["unit-a-01".to_string()],
            "complete",
            String::new(),
            Some(Box::new(|unit: &str| {
                format!("覆盖审计：单元 {unit} 缺 2 条")
            })),
            None,
        )
        .unwrap();
        let ins = instructions.lock().unwrap();
        assert!(ins[0].contains("引擎审计（确定性，勿质疑）"));
        assert!(ins[0].contains("覆盖审计：单元 unit-a-01 缺 2 条"));
    }

    #[test]
    fn protocol_violation_retries_review_without_touching_agent() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
        let received = agent.received.clone();
        let f = pipe_factory(&[
            "我审完了，写得不错。", // 缺协议块 → 违例（worker 层就地重跑，不重新开庭）
            "<doc-verdict>PASS</doc-verdict>\n<route>finish</route>",
        ]);
        let rep = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
        assert_eq!(rep.units_done.len(), 1);
        assert_eq!(rep.review_rounds, 1, "违例重跑在同一轮内（round = 开庭次数）");
        assert_eq!(received.borrow().len(), 1, "agent 只被调用一次（不回注违例）");
    }

    #[test]
    fn violation_cap_aborts_run() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["v1\n<doc-complete>unit-a-01</doc-complete>"]);
        let f = pipe_factory(&["无协议块", "无协议块", "无协议块", "无协议块"]);
        let err = run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap_err();
        assert!(err.to_string().contains("协议违例"), "{err}");
    }

    #[test]
    fn done_units_are_skipped_on_resume() {
        // 引擎侧（plan）剔除证据成立的 done 单元；loop 只跑 to_run
        let tmp = tempfile::tempdir().unwrap();
        let store = state_path(&tmp);
        let docs = tmp.path().join(DOCS_DIR);
        std::fs::create_dir_all(docs.join("a")).unwrap();
        std::fs::write(docs.join("a/x.md"), b"x").unwrap();
        let mut state = store.fresh_state();
        mark_done(&mut state, "unit-a-01", &["a/x.md".to_string()]);
        bump_review(&mut state);
        store.save(&mut state).unwrap();

        let all = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
        let resume = crate::sop::doc::resume::DocResume::new(&docs);
        let plan = crate::runstate::resume::plan(
            &tmp.path().join("plans"),
            "p",
            "t",
            &resume,
            &all,
            false,
        );
        assert_eq!(plan.to_run, vec!["unit-b-02".to_string()]);

        let agent = MockAgent::new(&["B\n<doc-complete>unit-b-02</doc-complete>"]);
        let seen = agent.received.clone();
        let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>\n<route>finish</route>"]);
        let rep = run_pipe!(agent, f, tmp, plan.to_run).unwrap();
        assert_eq!(rep.units_done, all);
        assert_eq!(rep.review_rounds, 2, "已完成的单元不再审");
        assert_eq!(seen.borrow().len(), 1, "只应跑 to_run 里的那个单元");
        assert!(seen.borrow()[0].contains("unit-b-02"));
    }

    #[test]
    fn rewritten_file_counts_as_produced() {
        // 重做单元常常是改已有文件：只看「新文件」会把重写误判为无产出
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join(DOCS_DIR);
        std::fs::create_dir_all(docs.join("a")).unwrap();
        std::fs::write(docs.join("a/x.md"), b"old").unwrap();
        let before = snapshot_files(&docs);
        std::fs::write(docs.join("a/x.md"), b"new content, longer").unwrap();
        let files = produced_files(&docs, &before);
        assert_eq!(files, vec!["a/x.md".to_string()], "改写也算产出");
    }

    #[test]
    fn untouched_files_are_not_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let docs = tmp.path().join(DOCS_DIR);
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("keep.md"), b"same").unwrap();
        let before = snapshot_files(&docs);
        assert!(produced_files(&docs, &before).is_empty());
    }

    #[test]
    fn checkpoint_lives_on_blackboard_not_in_output_dir() {
        // plan 010 T007：状态住黑板 `plans/<p>/<t>/<sop>/state.json`，不得再写进产物目录
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&["A\n<doc-complete>unit-a-01</doc-complete>"]);
        let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>"]);
        run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();

        let store = state_path(&tmp);
        assert!(store.path().exists(), "断点应落在黑板：{}", store.path().display());
        assert!(
            store.path().starts_with(tmp.path().join("plans")),
            "断点不得住在产物目录：{}",
            store.path().display()
        );
        assert!(
            !tmp.path().join(DOCS_DIR).join("_meta/state.json").exists(),
            "旧位置（产物目录）不得再出现 state.json"
        );
    }

    #[test]
    fn legacy_output_dir_state_does_not_participate_in_resume() {
        // 旧路径的断点作废：不得因此跳过任何单元
        let tmp = tempfile::tempdir().unwrap();
        let legacy = tmp.path().join(DOCS_DIR).join("_meta");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(
            legacy.join("state.json"),
            serde_json::json!({"done": ["unit-a-01"], "review_rounds": 9}).to_string(),
        )
        .unwrap();

        let agent = MockAgent::new(&[
            "A\n<doc-complete>unit-a-01</doc-complete>",
            "B\n<doc-complete>unit-b-02</doc-complete>",
        ]);
        let f = pipe_factory(&[
            "<doc-verdict>PASS</doc-verdict>",
            "<doc-verdict>PASS</doc-verdict>",
        ]);
        let rep = run_pipe!(
            agent,
            f,
            tmp,
            ["unit-a-01".to_string(), "unit-b-02".to_string()]
        )
        .unwrap();
        assert_eq!(rep.units_done, vec!["unit-a-01", "unit-b-02"], "旧断点不得生效");
        assert_eq!(rep.review_rounds, 2, "旧轮次计数不得被采用");
    }

    #[test]
    fn broken_evidence_unit_is_reincluded_and_rewritten() {
        // plan 010 T004：断点说 done，但证据不成立 → 必须重新进 kickoff 集合
        let tmp = tempfile::tempdir().unwrap();
        let store = state_path(&tmp);
        seed_done(&store, &["unit-a-01"]); // 未登记任何产出文件

        let resume = crate::sop::doc::resume::DocResume::new(tmp.path().join(DOCS_DIR));
        let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
        let plan = crate::runstate::resume::plan(
            &tmp.path().join("plans"),
            "p",
            "t",
            &resume,
            &units,
            false,
        );
        assert!(plan.resumed);
        assert_eq!(plan.redo, vec!["unit-a-01".to_string()]);
        assert!(
            plan.to_run.contains(&"unit-a-01".to_string()),
            "证据失效的单元必须重跑：{:?}",
            plan.to_run
        );

        // 引擎按 to_run 派发：agent 必须真的收到 unit-a-01 的 kickoff 并重写
        let agent = MockAgent::new(&[
            "重写 A\n<doc-complete>unit-a-01</doc-complete>",
            "写 B\n<doc-complete>unit-b-02</doc-complete>",
        ]);
        let seen = agent.received.clone();
        let f = pipe_factory(&[
            "<doc-verdict>PASS</doc-verdict>",
            "<doc-verdict>PASS</doc-verdict>",
        ]);
        let rep = run_pipeline(
            agent,
            Arc::new(Mutex::new(f)),
            state_path(&tmp),
            &mut NoUser,
            tmp.path(),
            &plan.to_run,
            "complete",
            crate::runstate::resume::Resumable::resume_brief(
                &resume,
                store.load().state().expect("断点应在"),
                &plan.redo,
            ),
            None,
            None,
        )
        .unwrap();
        // 硬断言：kickoff 回路真的发生，而不是只信 plan 与收尾名单（审计指出的假阳性）
        let kicks = seen.borrow();
        assert_eq!(kicks.len(), 2, "两个单元都应收到 kickoff：{kicks:?}");
        assert!(
            kicks[0].contains("unit-a-01"),
            "第一个 kickoff 必须是被重做的单元：{}",
            kicks[0]
        );
        assert!(kicks[0].contains("必须整单元重写") || kicks[0].contains("断点复核"));
        assert_eq!(rep.units_done, units);
    }

    #[test]
    fn intact_evidence_is_not_redispatched() {
        let tmp = tempfile::tempdir().unwrap();
        let store = state_path(&tmp);
        // 登记 done + 真实存在的产物文件
        let docs = tmp.path().join(DOCS_DIR);
        std::fs::create_dir_all(docs.join("a")).unwrap();
        std::fs::write(docs.join("a/x.md"), b"x").unwrap();
        let mut state = store.fresh_state();
        mark_done(&mut state, "unit-a-01", &["a/x.md".to_string()]);
        store.save(&mut state).unwrap();

        let resume = crate::sop::doc::resume::DocResume::new(&docs);
        let units = vec!["unit-a-01".to_string(), "unit-b-02".to_string()];
        let plan = crate::runstate::resume::plan(
            &tmp.path().join("plans"),
            "p",
            "t",
            &resume,
            &units,
            false,
        );
        assert_eq!(plan.skip, vec!["unit-a-01".to_string()]);
        assert_eq!(plan.to_run, vec!["unit-b-02".to_string()]);
    }

    // DEVFORGE_DOC_ONLY 单元重派随串行 runner 退役（ARC-R1）：单单元重跑由续跑计划承担
    //（见 done_units_are_skipped_on_resume / broken_evidence_unit_is_reincluded_and_rewritten）

    #[test]
    fn unit_mismatch_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let agent = MockAgent::new(&[
            "写完\n<doc-complete>unit-wrong-99</doc-complete>",
            "改正\n<doc-complete>unit-a-01</doc-complete>",
        ]);
        let received = agent.received.clone();
        let f = pipe_factory(&["<doc-verdict>PASS</doc-verdict>\n<route>finish</route>"]);
        run_pipe!(agent, f, tmp, ["unit-a-01".to_string()]).unwrap();
        assert!(received.borrow()[1].contains("不得自行改号"));
    }
}
