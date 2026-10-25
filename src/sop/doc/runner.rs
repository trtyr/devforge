//! doc-from-zero 流水线 runner（P008 · DR-14）：审查(N) 与 写作(N+1) 重叠滚动。
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
use crate::runstate::StateStore;
use crate::sop::doc::evidence::{produced_files, snapshot_files};
use crate::sop::doc::prompts::{
    DOC_REMEDIATION_PREFIX, doc_kickoff, doc_review_instruction,
};
use crate::sop::doc::state::{
    add_issues, bump_review, data_obj, done_units, issues_total, mark_done, review_rounds,
};
use crate::sop::doc::protocol::{
    MAX_DOC_CONTINUE_NUDGES, MAX_DOC_EMPTY_REPLIES, MAX_DOC_PROTOCOL_VIOLATIONS,
    MAX_DOC_REVIEW_RETRIES, DocVerdict, parse_doc_complete, parse_doc_review,
};
use crate::sop::planning::{PlanAgentSession, ReviewSessionFactory, UserLink};

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
    let docs_root = repo.join(crate::sop::doc::prompts::DOCS_DIR);
    // 引擎审计修复 #2：覆盖与 lint 机械门的数据底座——清单不可读时降级跳过（WARN），
    // 生产链路门 0 必写清单，降级只发生在人为构造的半成品环境。
    let gates = match crate::docgen::inventory::Inventory::load(
        &docs_root.join("_meta").join("inventory.json"),
    ) {
        Ok(inv) => match crate::docgen::lint::EntityIndex::build(repo, Some(&inv)) {
            Ok(idx) => Some((inv, idx)),
            Err(e) => {
                tracing::warn!(component = "doc-sop", error = %e, "实体索引构建失败，覆盖/lint 机械门降级跳过");
                None
            }
        },
        Err(e) => {
            tracing::warn!(component = "doc-sop", error = %e, "清单不可读，覆盖/lint 机械门降级跳过");
            None
        }
    };
    let level = crate::docgen::audit::DocLevel::parse(doc_level);
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
       // 引擎审计修复 #3：快照跟单元走，且存「写前 + 交卷」两份——lead=1 下写 N+1
    // 不能冲掉 N 的基线；Fix 重入时重拍；产出集以交卷时点为准（PASS 可能迟到，
    // 不能拿 PASS 处理时点的全局现状做 diff，否则后续单元的产出混入名单）
    let mut unit_snapshots: std::collections::HashMap<
        String,
        (
            std::collections::BTreeMap<String, (u64, u128)>,
            std::collections::BTreeMap<String, (u64, u128)>,
        ),
    > = Default::default();
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
                    // 引擎审计修复 #2：审查官 PASS 不是终点——覆盖与 lint 机械门必须达标，
                    // 不达标按 FAIL 同款进 fix 队列（带缺失清单），断路器兜底防死循环。
                    let gates_ok = match &gates {
                        Some((inv, idx)) => {
                            let cov = crate::docgen::audit::coverage_for_unit(
                                inv, &docs_root, level, &$unit,
                            )?;
                            let lint =
                                crate::docgen::lint::lint_docs_with_entities(&docs_root, idx)?;
                            let mut issues: Vec<String> = cov
                                .missing
                                .iter()
                                .map(|m| {
                                    format!("覆盖缺失：{}（{}）", m.canonical_name, m.id)
                                })
                                .collect();
                            issues.extend(lint.violations().iter().map(|v| format!("lint：{v:?}")));
                            if cov.passed() && lint.passed() {
                                true
                            } else {
                                add_issues(&mut state, issues.len());
                                fix_queue.push_back((
                                    $unit.clone(),
                                    "机械门不达标".to_string(),
                                    issues,
                                    $round,
                                ));
                                consecutive_fails += 1;
                                if consecutive_fails >= 2 {
                                    return Err(DevforgeError::Quality(format!(
                                        "断路器：连续 {} 轮机械门不达标（最近单元 `{}`），停发新单元——修复队列 {} 项（resume 可续）",
                                        consecutive_fails,
                                        $unit,
                                        fix_queue.len()
                                    )));
                                }
                                false
                            }
                        }
                        None => true, // 机械门数据不可用（降级，见 run_pipeline 开头 WARN）
                    };
                    if !gates_ok {
                        // 机械门不达标：本单元未完成（不 mark_done），等 fix 队列消化
                    } else {
                    // 引擎审计修复 #3：produced_files 用本单元自己的「写前→交卷」快照对；
                    // 无快照 = PASS 先于任何写前到达（只可能是 bug），宁可失败不落错名单
                    let (before, committed) = unit_snapshots.remove(&$unit).unwrap_or_else(|| {
                        panic!("PASS 到达但单元 `{}` 无写前快照（引擎审计修复 #3 守卫）", $unit)
                    });
                    let files = crate::sop::doc::evidence::produced_files_between(&committed, &before);
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
                // 引擎审计修复 #3：Fix 重入重拍快照——旧基线是上一单元写前拍的，
                // 拿它算 produced_files 会把上一单元的产出与本次 Fix 的产出混记
                unit_snapshots.insert(u.clone(), (snapshot_files(&docs_root), Default::default()));
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
                unit_snapshots.insert(u.clone(), (snapshot_files(&docs_root), Default::default()));
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
        // 引擎审计修复 #3：交卷时点拍产出快照（PASS 可能迟到到后续单元写完之后）
        unit_snapshots
            .entry(active.clone())
            .or_default()
            .1 = snapshot_files(&docs_root);
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
