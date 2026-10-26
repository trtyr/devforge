//! 审计 runner（P013 M1）：把四阶段串成一次 run——盘点切片 → Stage0 规划 →
//! Stage1 批次审查（独立会话×N）→ 核验 → Stage2 跨单元 → 报告落黑板。
//!
//! 会话桥：`ReviewSessionFactory`（decisions/004 血统）——每批次/每阶段独立实例。

use std::path::Path;

use crate::domain::runner::{RunControl, RunOutcome};
use crate::domain::task::Task;
use crate::error::{Context as _, Result};

use super::finding::Finding;
use super::pipeline::{AuditSession, AuditSessionFactory, Progress};
use super::{build_plan, AuditUnit};
use super::verify::verify;

/// 干预检查单点（审计修复 #5）：置位即中断——外层 map_intervention 看 control 置位
/// 给出明确的 Failed 原因（已取消/已失活/已超时），audit 层不重复归因。
fn check_intervention(control: &RunControl) -> Result<()> {
    if control.is_cancelled()
        || control.is_stalled()
        || control.timed_out(crate::domain::now_ms())
    {
        return Err(crate::error::DevforgeError::System(
            "审计中断：run 已被干预（cancel/stalled/timeout）".into(),
        ));
    }
    Ok(())
}

/// 全仓审计 M4：code-review 的默认失活阈值——单批 ~6 分钟，agent 流式输出会持续刷新
/// liveness（DR-3 note_agent_event），静默 15 分钟只可能是挂死。
/// 其他 SOP 仍保持「默认不收割」（006 decisions/002），仅 code-review 有默认值；
/// 显式设 DEVFORGE_RUN_IDLE_MS 时以它为准。
pub const DEFAULT_AUDIT_IDLE_MS: u64 = 15 * 60 * 1000;

/// 第三方 r3 M4：单批墙钟（默认 30 分钟）。idle 收割打不中「流式刷活性但不推进」
/// 的活死批（DR-3 之后 text_delta 会刷新 liveness），只能由批次自己的墙钟叫停：
/// 到点 → 会话返回 Quality → 该批记 abandoned，余批继续。
pub const DEFAULT_AUDIT_BATCH_TIMEOUT_MS: u64 = 30 * 60 * 1000;

/// 本批墙钟；`0` 或环境变量设 0 = 不限制（旧行为）。
fn batch_deadline() -> Option<std::time::Instant> {
    let ms = std::env::var("DEVFORGE_AUDIT_BATCH_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_AUDIT_BATCH_TIMEOUT_MS);
    batch_deadline_from_ms(ms)
}

fn batch_deadline_from_ms(ms: u64) -> Option<std::time::Instant> {
    if ms == 0 {
        None
    } else {
        Some(std::time::Instant::now() + std::time::Duration::from_millis(ms))
    }
}

/// 全量审计执行（生产入口）。`factory` 每次开新会话（用完即弃）。
pub fn run_audit(
    task: &Task,
    plans_root: &Path,
    control: &RunControl,
    factory: &mut dyn AuditSessionFactory,
) -> Result<RunOutcome> {
    check_intervention(control)?;
    let blackboard = plans_root.join(&task.project).join(&task.id);
    std::fs::create_dir_all(&blackboard).ctx("创建黑板目录")?;
    let mut progress = Progress::load(&blackboard);

    // ---- 盘点 + 切片（确定性零 LLM）----
    crate::observability::set_phase("audit-slice");
    let plan = build_plan(&task.workspace)?;
    // 第三方 r2 m7：批次文件清单落盘——报告只有分母计数，没有「哪批审了哪些文件」，
    // 事后无法重建审查面。此产物是 M6「coverage 可对账」的分子。
    crate::audit::write_units_file(&blackboard, &plan)?;
    // 全仓审计 M5：空盘 = 假成功——指向错目录/空仓时旧实现 Stage 0 空规划→循环 0 次
    // → Ok(Completed) + 0 条发现，最难排查。这里显式失败，不交一个「审了 0 文件」的成功。
    if plan.units.is_empty() {
        return Err(crate::error::DevforgeError::System(format!(
            "无可审源码（盘点 0 个审查单元）：workspace = {}——检查路径是否正确、是否为空仓",
            task.workspace.display()
        )));
    }
    let unit_labels: Vec<String> = plan.units.iter().map(|u| u.label.clone()).collect();
    tracing::info!(
        component = "audit-engine",
        units = plan.units.len(),
        "切片完成，进入四阶段审查"
    );

    // ---- Stage 0 规划 ----
    crate::observability::set_phase("audit-plan");
    // 全仓审计 m1：断点产物损坏不再硬失败——清标记重算（与 Progress::load 的「损坏即重跑」同口径）
    if progress.stage0_done && read_stage0(&blackboard).is_none() {
        tracing::warn!(component = "audit-engine", "Stage 0 断点产物不可用，已清标记重算（m1）");
        progress.stage0_done = false;
        progress.save(&blackboard)?;
    }
    let review_plan = if progress.stage0_done {
        tracing::info!(component = "audit-engine", "断点：Stage 0 已完成，跳过");
        // 自审 major：不再 expect——二次读取失败（磁盘竞态/权限变化）时按损坏处理
        // 清标记重算，与 m1 口径一致，而不是在生产路径上 panic。
        match read_stage0(&blackboard) {
            Some(p) => p,
            None => {
                tracing::warn!(
                    component = "audit-engine",
                    "Stage 0 断点标记在位但产物不可读，清标记重算"
                );
                progress.stage0_done = false;
                progress.save(&blackboard)?;
                check_intervention(control)?;
                let plan0 = with_stage_retry(factory, "Stage 0 规划", |s| {
                    crate::audit::pipeline::stage0_plan_with_deadline(s, &plan.units, batch_deadline())
                })?;
                std::fs::write(
                    blackboard.join("audit-stage0.json"),
                    serde_json::to_vec_pretty(&plan0).ctx("序列化 ReviewPlan")?,
                )
                .ctx("写 Stage0 产物")?;
                progress.stage0_done = true;
                progress.save(&blackboard)?;
                plan0
            }
        }
    } else {
        check_intervention(control)?;
        // 全仓审计 M2：Stage 0 也吃换实例重试（单次格式抖动不应让一行代码都还没审就报废）
        let plan0 = with_stage_retry(factory, "Stage 0 规划", |s| {
            crate::audit::pipeline::stage0_plan_with_deadline(s, &plan.units, batch_deadline())
        })?;
        std::fs::write(
            blackboard.join("audit-stage0.json"),
            serde_json::to_vec_pretty(&plan0).ctx("序列化 ReviewPlan")?,
        )
        .ctx("写 Stage0 产物")?;
        progress.stage0_done = true;
        progress.save(&blackboard)?;
        tracing::info!(
            component = "audit-engine",
            priority = plan0.priority.len(),
            skip = plan0.skip.len(),
            concerns = plan0.cross_concerns.len(),
            "Stage 0 规划完成"
        );
        plan0
    };

    // ---- Stage 1 批次审查（按优先级排序；skip 跳过；断点跳过已完成批次）----
    crate::observability::set_phase("audit-batch");
    // 全仓审计 M6：skip 只能落在 Stage 0 自己声明的单元集合内，且不得跳过全量——
    // 旧实现把 LLM 给的 skip 直接当过滤器，skip 全量就能让整仓零审查还报成功。
    let known_ids: std::collections::BTreeSet<&str> =
        plan.units.iter().map(|u| u.id.as_str()).collect();
    let unknown_skip: Vec<&String> = review_plan
        .skip
        .iter()
        .filter(|s| !known_ids.contains(s.as_str()))
        .collect();
    if !unknown_skip.is_empty() {
        tracing::warn!(
            component = "audit-engine",
            unknown = ?unknown_skip,
            "Stage 0 的 skip 含不存在的单元号，已忽略"
        );
    }
    // 第三方 r3 M2：skip 除了「必须在单元集合内」，还要求**批次内容真是生成物/锁文件**——
    // 旧版照 LLM 说的跳，把 src/audit 整组标 skip 也算「全量成功」。
    // 规则保守：宁多审不少审（不匹配的 skip 一律忽略并 WARN）。
    let mut rejected_skip: Vec<&String> = Vec::new();
    let skipped_units: Vec<String> = review_plan
        .skip
        .iter()
        .filter(|s| {
            let Some(unit) = plan.units.iter().find(|u| &u.id == *s) else {
                return false; // 不存在的单元号已在上面 WARN
            };
            if skippable_unit(unit) {
                true
            } else {
                rejected_skip.push(s);
                false
            }
        })
        .cloned()
        .collect();
    if !rejected_skip.is_empty() {
        tracing::warn!(
            component = "audit-engine",
            rejected = ?rejected_skip,
            "Stage 0 的 skip 指向非生成物/锁文件批次，已忽略（全量审计宁可多审）"
        );
    }
    if skipped_units.len() == plan.units.len() {
        // 防御性兑底：skip 已被限定在「生成物批次」，而切入器只切源码面——
        // 正常路径到不了这里；留着是为了万一以后放开 skip 范围时零审查不能静默成功。
        return Err(crate::error::DevforgeError::Quality(
            "Stage 0 把全部批次都标了 skip——全量审计不允许零审查收工（检查 skip 规则）".into(),
        ));
    }
    let mut ordered: Vec<&AuditUnit> = plan
        .units
        .iter()
        .filter(|u| !skipped_units.contains(&u.id))
        .collect();
    ordered.sort_by_key(|u| {
        (
            !review_plan.priority.contains(&u.id), // 优先级在前
            u.id.clone(),                          // 同级按 id 稳定
        )
    });

    // 确定性预查报告（审计修复 #6，T1.4 落地）：对批次文件做零 LLM 机检——
    // 审查官提示词约定「预查是确定性事实，直接采信并逐条验收」，占位串等于 Stage 1 失去静态面。
    // 文件清单来自切片器（非 LLM 输入），无穿越面；在批次循环内按批生成。
    let findings_dir = blackboard.join("audit-findings");
    std::fs::create_dir_all(&findings_dir).ctx("建 audit-findings 目录")?;
    // 全仓审计 m2：发现文件缺失/损坏的批次必须**摘掉打勾重审**（旧实现只 WARN 跳过，
    // done_batches 仍含该批 → 它的发现永久丢失且永不重审）。
    let (mut findings, bad_batches) =
        load_persisted_findings(&findings_dir, &progress.done_batches);
    if !bad_batches.is_empty() {
        progress.done_batches.retain(|id| !bad_batches.contains(id));
        progress.save(&blackboard)?;
        tracing::warn!(
            component = "audit-engine",
            bad = ?bad_batches,
            "断点批次发现文件不可用，已从已完成列表摘除并将重审"
        );
    }

    // 全仓审计 M3：批次间无共享状态（每批独立会话）→ 可并行；
    // 只有工厂能 fork（生产桥）才走并行，测试替身默认串行（确定性）。
    let concurrency = audit_concurrency();
    let pending: Vec<&AuditUnit> = ordered
        .iter()
        .copied()
        .filter(|u| !progress.done_batches.contains(&u.id))
        .collect();
    if concurrency > 1 && pending.len() > 1 && factory.fork().is_some() {
        tracing::info!(
            component = "audit-engine",
            concurrency,
            batches = pending.len(),
            "Stage 1 并行批次派发"
        );
        let results = run_stage1_parallel(
            factory,
            &pending,
            &unit_labels,
            &task.workspace,
            concurrency,
            control,
            &findings_dir,
        );
        // 工作窃取导致完成序≠派发序：按 idx 排序后串行打勾（断点写入串行化）。
        // 发现已在 worker 内落盘——此处任一兄弟失败不会带走已完成批次（第三方 r3 M1）。
        for (_, unit_id, res) in results {
            check_intervention(control)?;
            apply_batch_outcome(&unit_id, res, &mut findings, &mut progress, &blackboard)?;
        }
    } else {
        for unit in &ordered {
            if progress.done_batches.contains(&unit.id) {
                tracing::info!(component = "audit-engine", unit = %unit.id, "断点：批次已完成，跳过");
                continue;
            }
            check_intervention(control)?;
            // 复验事故修复（M3 缺陷①）：每批 findings 即时落黑板——先落盘再打勾，
            // 断点续跑从黑板回读已完成批次，进程死亡不再丢发现。
            let precheck = precheck_report(&task.workspace, &unit.files, unit.focus_range);
            let res = crate::audit::pipeline::stage1_review_batch_with_deadline(
                factory,
                &task.workspace,
                unit,
                &unit_labels,
                &precheck,
                batch_deadline(),
            );
            if let Ok(fs) = &res {
                write_batch_findings(&findings_dir, &unit.id, fs)?;
            }
            apply_batch_outcome(&unit.id, res, &mut findings, &mut progress, &blackboard)?;
        }
    }
    let abandoned: Vec<String> = progress.abandoned.clone();

    // ---- 核验过滤（确定性零 LLM，RepoAudit validator 血统）----
    let verified = verify(&task.workspace, &findings);
    // ---- 跨单元去重（T2.2：同位置完全重复合并）----
    let deduped = super::report::dedup(&verified.kept);

    // ---- Stage 2 跨单元分析 ----
    crate::observability::set_phase("audit-cross");
    // m1：同 Stage 0——断点产物损坏则清标记重算
    if progress.stage2_done && read_stage2(&blackboard).is_none() {
        tracing::warn!(component = "audit-engine", "Stage 2 断点产物不可用，已清标记重算（m1）");
        progress.stage2_done = false;
        progress.save(&blackboard)?;
    }
    let cross = if progress.stage2_done {
        // 自审 major：同 Stage 0——不再 expect，产物不可读时清标记重算
        match read_stage2(&blackboard) {
            Some(c) => c,
            None => {
                tracing::warn!(
                    component = "audit-engine",
                    "Stage 2 断点标记在位但产物不可读，清标记重算"
                );
                progress.stage2_done = false;
                progress.save(&blackboard)?;
                check_intervention(control)?;
                let out = with_stage_retry(factory, "Stage 2 跨单元分析", |s| {
                    crate::audit::pipeline::stage2_cross_with_deadline(
                        s,
                        &deduped,
                        &review_plan.cross_concerns,
                        batch_deadline(),
                    )
                })?;
                std::fs::write(
                    blackboard.join("audit-stage2.json"),
                    serde_json::to_vec_pretty(&out).ctx("序列化跨单元分析")?,
                )
                .ctx("写 Stage2 产物")?;
                progress.stage2_done = true;
                progress.save(&blackboard)?;
                out
            }
        }
    } else {
        check_intervention(control)?;
        // 全仓审计 M2：Stage 2 在全部批次跑完之后，一次格式抖动不应报废数小时——
        // 同样换实例重试；全仓审计 m8：输入用**去重后**集合（不是验证后未去重的 kept）。
        let out = with_stage_retry(factory, "Stage 2 跨单元分析", |s| {
            crate::audit::pipeline::stage2_cross_with_deadline(
                s,
                &deduped,
                &review_plan.cross_concerns,
                batch_deadline(),
            )
        })?;
        std::fs::write(
            blackboard.join("audit-stage2.json"),
            serde_json::to_vec_pretty(&out).ctx("序列化跨单元分析")?,
        )
        .ctx("写 Stage2 产物")?;
        progress.stage2_done = true;
        progress.save(&blackboard)?;
        out
    };

    // 审计修复 #2：Stage 2 跨单元发现同样过核验——跨单元是 LLM 二次生成 findings 的入口，
    // 绕过核验等于「无幽灵发现」底线对汇总层失效（RepoAudit：无 validator FP +245%）。
    let cross_verify = verify(&task.workspace, &cross.findings);
    let cross_kept = cross_verify.kept.clone();
    let cross_dropped = cross_verify.dropped.clone();
    let mut all_dropped = verified.dropped.clone();
    all_dropped.extend(cross_verify.dropped);

    // ---- 报告组装（JSON + MD，M2）----
    crate::observability::set_phase("audit-report");
    // 全仓审计 M7：JSON 里写**核验后**的 cross_kept，幽灵单独入 cross_dropped——
    // 旧实现把原始 cross 写进 JSON，与 MD/severity_summary 用的 cross_kept 结论不一致。
    // 全仓审计 M8：跨层去重一次（deduped + cross_kept），汇总与渲染都用它，避免同一条算两次。
    let merged = super::report::dedup(
        &deduped
            .iter()
            .cloned()
            .chain(cross_kept.iter().cloned())
            .collect::<Vec<_>>(),
    );
    let report = serde_json::json!({
        "project": task.project,
        "task": task.id,
        "workspace": task.workspace,
        // 全仓审计 m10：报告可溯到具体版本（自审复现的底线）
        "workspace_sha": workspace_sha(&task.workspace),
        "units_total": plan.units.len(),
        "files_total": plan.units.iter().map(|u| u.files.len()).sum::<usize>(),
        // 全仓审计 M6：「全量」必须可对账——盘点到多少文件、切片多少、跳过哪些、
        // 哪些条目根本没进切片（非 Module/Symbol）都要能看见。
        "coverage": {
            "source_files": plan.source_files,
            "sliced_files": plan.units.iter().map(|u| u.files.len()).sum::<usize>(),
            "excluded_entries": plan.excluded_entries,
            "units_total": plan.units.len(),
            "units_file": "audit-units.json",
            "skipped_units": skipped_units,
            "batches_done": progress.done_batches.len(),
            "batches_abandoned": abandoned.len(),
        },
        "batches_done": progress.done_batches.len(),
        "batches_abandoned": abandoned,
        "findings": {
            "kept": deduped,
            "dropped_ghosts": all_dropped,
        },
        "cross_analysis": {
            "findings": cross_kept,
            "dropped_ghosts": cross_dropped,
            "concern_verdicts": cross.concern_verdicts,
            "parse_errors": cross.parse_errors,
        },
        "severity_summary": severity_summary_of(&merged),
    });
    std::fs::write(
        blackboard.join("audit-report.json"),
        serde_json::to_vec_pretty(&report).ctx("序列化审计报告")?,
    )
    .ctx("写审计报告")?;
    let md = super::report::render_markdown(
        &task.project,
        &task.id,
        &merged,
        &[],
        &all_dropped,
        &abandoned,
    );
    std::fs::write(blackboard.join("audit-report.md"), md).ctx("写审计报告 MD")?;
    tracing::info!(
        component = "audit-engine",
        kept = verified.kept.len(),
        dropped = all_dropped.len(),
        cross = cross_kept.len(),
        abandoned = abandoned.len(),
        "全量审计完成"
    );
    Ok(RunOutcome::Completed)
}

/// 全仓审计 M3：批次并发度（DEVFORGE_AUDIT_CONCURRENCY，默认 4，边界 1..=8）。
/// 单批 ~6 分钟，旧串行 87 批 ≈ 9h；四路并发是批次数与 provider 限流间的保守值。
/// 第三方 r3 M2：一个批次能不能被 skip——必须整批都是「生成物 / 锁文件 / vendor 树」。
/// 宁可多审（误判为不可跳 → 照审）也不许把核心目录用一句「生成物」糊过去。
fn skippable_unit(unit: &AuditUnit) -> bool {
    !unit.files.is_empty()
        && unit.files.iter().all(|f| {
            let p = f.as_str();
            p.ends_with(".lock")
                || p.ends_with(".min.js")
                || p.ends_with(".min.css")
                || p.ends_with(".snap")
                || p.contains(".generated.")
                || p.contains("_generated.")
                || p.ends_with("package-lock.json")
                || p.starts_with("target/")
                || p.starts_with("node_modules/")
                || p.starts_with("dist/")
                || p.starts_with("vendor/")
        })
}

fn audit_concurrency() -> usize {
    #[cfg(test)]
    if let Some(n) = tests::concurrency_override() {
        return n;
    }
    std::env::var("DEVFORGE_AUDIT_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .map(|n| n.clamp(1, 8))
        .unwrap_or(4)
}

/// 批次发现落盘（先落盘再打勾的前半）。串行路径与并行 worker **都**调它：
/// 并行时 worker 自己存盘，兄弟批次挂掉也不会带走已完成批次（第三方 r3 M1）。
fn write_batch_findings(findings_dir: &Path, unit_id: &str, fs: &[Finding]) -> Result<()> {
    std::fs::write(
        findings_dir.join(format!("{unit_id}.json")),
        serde_json::to_vec_pretty(&fs).ctx("序列化批次发现")?,
    )
    .ctx("写批次发现")
}

/// 批次结果落地（串行/并行共用）：调用前**发现已落盘**；这里只打勾/记放弃。
/// 质量类失败 = 放弃并记入断点；实例/基础设施失败 = 打断整次审计（不伪装成放弃）。
///
/// 第三方 r3 M1：旧版把「写盘 + 打勾」揃在一起、且并行路径「全跑完才落盘」——
/// 兄弟批次一失败，内存里已审完的批次全丢。现在拆两半，写盘在 worker 内完成。
fn apply_batch_outcome(
    unit_id: &str,
    res: Result<Vec<Finding>>,
    findings: &mut Vec<Finding>,
    progress: &mut Progress,
    blackboard: &Path,
) -> Result<()> {
    match res {
        Ok(fs) => {
            let batch_n = fs.len();
            findings.extend(fs);
            progress.done_batches.push(unit_id.to_string());
            progress.save(blackboard)?;
            tracing::info!(
                component = "audit-engine",
                unit = %unit_id,
                batch_findings = batch_n,
                findings_total = findings.len(),
                "批次审查完成"
            );
            Ok(())
        }
        Err(crate::error::DevforgeError::Quality(msg)) => {
            progress.abandoned.push(format!("{unit_id}：{msg}"));
            progress.save(blackboard)?;
            tracing::warn!(component = "audit-engine", unit = %unit_id, "批次放弃");
            Ok(())
        }
        Err(e) => {
            tracing::error!(
                component = "audit-engine",
                unit = %unit_id,
                error = %e,
                "批次实例失败，打断整次审计（非质量类错误不伪装成放弃）"
            );
            Err(e)
        }
    }
}

/// 全仓审计 M3：并行跑待审批次（固定并发，工作窃取式取下标），结果按原下标排序返回。
/// 线程 panic 映射为 System（不静默丢批）。
/// 并行批次结果：派发下标（排序用） / 单元号 / 该批发现或错误。
type BatchResult = (usize, String, Result<Vec<Finding>>);

fn run_stage1_parallel(
    factory: &dyn AuditSessionFactory,
    units: &[&AuditUnit],
    labels: &[String],
    workspace: &Path,
    concurrency: usize,
    control: &RunControl,
    findings_dir: &Path,
) -> Vec<BatchResult> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<BatchResult> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..concurrency.min(units.len()) {
            let Some(mut fork) = factory.fork() else {
                break;
            };
            let next = &next;
            handles.push(scope.spawn(move || {
                let mut local: Vec<BatchResult> = Vec::new();
                loop {
                    let idx = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(unit) = units.get(idx) else {
                        break;
                    };
                    let unit_id = unit.id.clone();
                    if let Err(e) = check_intervention(control) {
                        local.push((idx, unit_id, Err(e)));
                        break;
                    }
                    let precheck = precheck_report(workspace, &unit.files, unit.focus_range);
                    let deadline = batch_deadline();
                    // 第三方 r3 M1：panic 在该批内部收成 System——
                    // 旧版向主线程扔 usize::MAX 哨兵，主线程 `pending[usize::MAX]` 直接越界 panic。
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::audit::pipeline::stage1_review_batch_with_deadline(
                            fork.as_mut(),
                            workspace,
                            unit,
                            labels,
                            &precheck,
                            deadline,
                        )
                    }))
                    .unwrap_or_else(|_| {
                        Err(crate::error::DevforgeError::System(format!(
                            "批次 {unit_id} 审查线程 panic（已兜底，整次审计失败）"
                        )))
                    });
                    // 成功即落盘：兄弟批次失败不带走本批发现
                    let res = match res {
                        Ok(fs) => match write_batch_findings(findings_dir, &unit_id, &fs) {
                            Ok(()) => Ok(fs),
                            Err(e) => Err(e),
                        },
                        Err(e) => Err(e),
                    };
                    local.push((idx, unit_id, res));
                }
                local
            }));
        }
        for h in handles {
            if let Ok(mut v) = h.join() {
                results.append(&mut v);
            }
        }
    });
    results.sort_by_key(|(i, _, _)| *i);
    results
}

/// 全仓审计 m10：workspace 的 HEAD sha（非 git 仓 = None，不阻断审计）。
fn workspace_sha(workspace: &Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .current_dir(workspace)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sha.is_empty() { None } else { Some(sha) }
}

/// 确定性预查报告（审计修复 #6，T1.4）：对批次文件做零 LLM 机检——
/// V2 unwrap/expect/panic/todo/unreachable 与 let _ = 吞错分布、V3 tracing 事件数、V4 行数。
/// 文件清单来自切片器（非 LLM 输入），无穿越面；在批次循环内按批生成。
///
/// 全仓审计 m5：这些是**参考计数**，不是逐行语义级事实——简化扫描不建 AST：
/// 行首/行尾注释已剔，但字符串字面量与 `#[cfg(test)]` 代码仍会计入。
/// 审查官提示词已同步降级为「参考值」（不得当裁决依据，不得据它反驳实际代码）。
fn precheck_report(workspace: &Path, files: &[String], range: Option<(usize, usize)>) -> String {
    let mut out = String::from("引擎预查（参考计数，非裁决依据）：\n");
    for f in files {
        let content = match std::fs::read_to_string(workspace.join(f)) {
            Ok(c) => c,
            Err(e) => {
                out.push_str(&format!("- {f}: 读取失败（{e}）\n"));
                continue;
            }
        };
        // r3 m5 遗留收口：分片只看本段行（否则同一文件的多个分片看到相同计数）。
        let scope: Vec<&str> = match range {
            Some((a, b)) if files.len() == 1 => content
                .lines()
                .skip(a.saturating_sub(1))
                .take(b.saturating_sub(a) + 1)
                .collect(),
            _ => content.lines().collect(),
        };
        let scope_desc = match range {
            Some((a, b)) if files.len() == 1 => format!("（本分片 = 第 {a}–{b} 行）"),
            _ => String::new(),
        };
        let mut unwraps = 0usize;
        let mut swallowed = 0usize;
        let mut events = 0usize;
        for line in &scope {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue; // 注释行不计
            }
            // m5：行尾注释同样剔除（简化规则：`//` 前无引号才当注释截断）
            let code = match line.find("//") {
                Some(pos) if !line[..pos].contains('"') => &line[..pos],
                _ => line,
            };
            if code.contains(".unwrap()")
                || code.contains(".expect(")
                || code.contains("panic!")
                || code.contains("todo!")
                || code.contains("unreachable!")
            {
                unwraps += 1;
            }
            if code.contains("let _ =") {
                swallowed += 1;
            }
            if code.contains("tracing::info!")
                || code.contains("tracing::warn!")
                || code.contains("tracing::error!")
                || code.contains("tracing::debug!")
                || code.contains("tracing::trace!")
            {
                events += 1;
            }
        }
        out.push_str(&format!(
            "- {f}{scope_desc}: 共 {} 行 / unwrap·expect·panic·todo·unreachable {unwraps} 处 / let _ = 吞错 {swallowed} 处 / tracing 事件 {events} 处\n",
            scope.len()
        ));
    }
    out
}

/// 严重度汇总（全仓审计 M8：只接受**已跨层去重**的单一集合——旧的 local+cross 双参数
/// 直接相加，同一条发现被本地与跨单元各算一次，critical 数量虚高）。
fn severity_summary_of(findings: &[Finding]) -> serde_json::Value {
    let count = |sev: &str| {
        findings
            .iter()
            .filter(|f| {
                serde_json::to_value(f.severity)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s == sev))
                    .unwrap_or(false)
            })
            .count()
    };
    serde_json::json!({
        "critical": count("critical"),
        "major": count("major"),
        "minor": count("minor"),
        "nit": count("nit"),
    })
}

/// 全仓审计 M2：Stage 0/2 的换实例重试（口径同 Stage 1：2 次）——
/// 只重试**输出不合规**（Quality）：实例/基础设施故障（System/Agent）直接上抛，
/// 不伪装成格式问题反复重开。
fn with_stage_retry<T>(
    factory: &mut dyn AuditSessionFactory,
    label: &str,
    mut run: impl FnMut(&mut dyn AuditSession) -> Result<T>,
) -> Result<T> {
    let mut last = String::new();
    for attempt in 0..2 {
        let mut session = factory.open().ctx(format!("开{label}会话"))?;
        match run(session.as_mut()) {
            Ok(v) => return Ok(v),
            Err(crate::error::DevforgeError::Quality(msg)) => {
                last = msg;
                tracing::warn!(
                    component = "audit-engine",
                    stage = label,
                    attempt = attempt + 1,
                    "{label}输出不合规，换实例重试"
                );
            }
            Err(e) => return Err(e),
        }
    }
    Err(crate::error::DevforgeError::Quality(format!(
        "{label} 连续 2 次输出不合规：{last}"
    )))
}

/// 断点回读：进度里已完成的批次，发现从黑板加载。缺文件/坏文件的批次跳过
/// （进度打了勾但发现丢了 = 历史遗留数据，如实少记不崩，WARN 留痕）。
fn load_persisted_findings(
    dir: &Path,
    done_batches: &[String],
) -> (Vec<Finding>, Vec<String>) {
    let mut out = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for id in done_batches {
        let p = dir.join(format!("{id}.json"));
        let Ok(bytes) = std::fs::read(&p) else {
            tracing::warn!(
                component = "audit-engine",
                unit = %id,
                "断点批次的发现文件缺失，将重审（m2）"
            );
            bad.push(id.clone());
            continue;
        };
        match serde_json::from_slice::<Vec<Finding>>(&bytes) {
            Ok(fs) => out.extend(fs),
            Err(e) => {
                tracing::warn!(
                    component = "audit-engine",
                    unit = %id,
                    error = %e,
                    "断点批次发现反序列化失败，将重审（m2）"
                );
                bad.push(id.clone());
            }
        }
    }
    (out, bad)
}

fn read_stage0(blackboard: &Path) -> Option<super::pipeline::ReviewPlan> {
    let bytes = match std::fs::read(blackboard.join("audit-stage0.json")) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(component = "audit-engine", error = %e, "读 Stage0 断点失败");
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(component = "audit-engine", error = %e, "Stage0 断点反序列化失败");
            None
        }
    }
}

fn read_stage2(blackboard: &Path) -> Option<super::pipeline::CrossAnalysis> {
    let bytes = match std::fs::read(blackboard.join("audit-stage2.json")) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(component = "audit-engine", error = %e, "读 Stage2 断点失败");
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(component = "audit-engine", error = %e, "Stage2 断点反序列化失败");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 复验 M3 缺陷①回归：断点续跑时已落盘的 findings 从黑板回读，
    /// 不随进程死亡丢失；进度里但文件缺失的历史批次跳过不崩。
    #[test]
    fn persisted_findings_are_read_back_on_resume() {
        let dir = tempfile::tempdir().unwrap();
        let findings_dir = dir.path().join("audit-findings");
        std::fs::create_dir_all(&findings_dir).unwrap();
        let f = Finding {
            path: "src/x.rs".into(),
            line: 1,
            view: crate::audit::finding::View::Correctness,
            severity: crate::audit::finding::Severity::Major,
            claim: "测试发现".into(),
            evidence: "src/x.rs:1".into(),
            batch: "unit-001".into(),
        };
        std::fs::write(
            findings_dir.join("unit-001.json"),
            serde_json::to_vec_pretty(&vec![f]).unwrap(),
        )
        .unwrap();
        let (got, bad) = load_persisted_findings(
            &findings_dir,
            &["unit-001".to_string(), "unit-002".to_string()],
        );
        assert_eq!(got.len(), 1, "存在的批次回读，缺失的进重审名单");
        assert_eq!(got[0].batch, "unit-001");
        assert_eq!(bad, vec!["unit-002".to_string()], "缺失批次必须重审（m2）");
    }

    // ---------- 审计修复 #1/#5 回归 ----------

    /// Stage 0 成功后，批次会话 ask 失败（System 类）：整次审计必须 Err，
    /// 错误不得伪装成 Quality，批次发现不落盘、报告不生成（无「成功审计」假象）。
    #[test]
    fn batch_instance_failure_aborts_whole_run_not_abandoned() {
        use std::sync::atomic::Ordering;
        struct PlanOnlySession;
        impl super::super::pipeline::AuditSession for PlanOnlySession {
            fn ask(&mut self, _: &str) -> crate::error::Result<String> {
                Ok("```json\n{\"priority\": [], \"skip\": [], \"cross_concerns\": []}\n```"
                    .into())
            }
        }
        struct FailAskSession;
        impl super::super::pipeline::AuditSession for FailAskSession {
            fn ask(&mut self, _: &str) -> crate::error::Result<String> {
                Err(crate::error::DevforgeError::System("RPC 挂死模拟".into()))
            }
        }
        /// 第一次 open 给 Stage 0 会话（成功），之后全部给 ask 失败的批次会话。
        struct Stage0OkThenFailFactory { opens: usize }
        impl super::super::pipeline::AuditSessionFactory for Stage0OkThenFailFactory {
            fn open(&mut self) -> crate::error::Result<Box<dyn super::super::pipeline::AuditSession>> {
                self.opens += 1;
                if self.opens == 1 {
                    Ok(Box::new(PlanOnlySession))
                } else {
                    Ok(Box::new(FailAskSession))
                }
            }
        }
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut task = crate::domain::task::Task::new("audit-self", "p-audit", "code-review", 1);
        task.workspace = ws.path().to_path_buf();
        let control = crate::domain::runner::RunControl::new();
        assert!(!control.cancel.load(Ordering::SeqCst));
        let mut factory = Stage0OkThenFailFactory { opens: 0 };
        let err = run_audit(&task, root.path(), &control, &mut factory).unwrap_err();
        assert!(
            !matches!(err, crate::error::DevforgeError::Quality(_)),
            "实例失败不得伪装成 Quality：{err:?}"
        );
        let blackboard = root.path().join("p-audit").join("audit-self");
        assert!(
            !blackboard.join("audit-report.json").exists(),
            "实例失败打断时不得生成报告（无成功假象）"
        );
        let findings_dir = blackboard.join("audit-findings");
        let leaked: usize = std::fs::read_dir(&findings_dir)
            .map(|d| d.flatten().count())
            .unwrap_or(0);
        assert_eq!(leaked, 0, "被打断的批次不得有发现落盘（abandoned 不含该批的文件面证据）");
    }

    /// 审计修复 #5：cancel 置位 → run_audit 入口即中断，不碰 factory。
    #[test]
    fn cancelled_control_aborts_audit_before_any_session() {
        struct PanicFactory;
        impl super::super::pipeline::AuditSessionFactory for PanicFactory {
            fn open(&mut self) -> crate::error::Result<Box<dyn super::super::pipeline::AuditSession>> {
                panic!("cancel 置位后不得开任何会话");
            }
        }
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut task = crate::domain::task::Task::new("audit-self", "p-audit", "code-review", 1);
        task.workspace = ws.path().to_path_buf();
        let control = crate::domain::runner::RunControl::new();
        control
            .cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let mut factory = PanicFactory;
        let err = run_audit(&task, root.path(), &control, &mut factory).unwrap_err();
        assert!(err.to_string().contains("干预"), "应报干预中断：{err:?}");
    }

    /// 审计修复 #2 回归：Stage 2 跨单元发现（LLM 二次生成）同样过核验——
    /// 穿越路径进 dropped_ghosts，不进报告 kept；真实跨单元发现照常保留。
    #[test]
    fn stage2_findings_pass_verify_ghosts_go_to_dropped() {
        use super::super::pipeline::{AuditSession, AuditSessionFactory};
        struct ScriptedSession {
            replies: std::collections::VecDeque<&'static str>,
        }
        impl AuditSession for ScriptedSession {
            fn ask(&mut self, _: &str) -> crate::error::Result<String> {
                self.replies
                    .pop_front()
                    .map(|s| s.to_string())
                    .ok_or_else(|| crate::error::DevforgeError::System("回复耗尽".into()))
            }
        }
        /// 按会话序切换脚本：会话 1 = Stage 0，会话 2 = 批次，会话 3 = Stage 2。
        struct SequencedFactory {
            scripts: std::collections::VecDeque<std::collections::VecDeque<&'static str>>,
        }
        impl AuditSessionFactory for SequencedFactory {
            fn open(&mut self) -> crate::error::Result<Box<dyn AuditSession>> {
                let s = self
                    .scripts
                    .pop_front()
                    .ok_or_else(|| crate::error::DevforgeError::System("会话耗尽".into()))?;
                Ok(Box::new(ScriptedSession { replies: s }))
            }
        }
        const PLAN: &str = "```json\n{\"priority\": [], \"skip\": [], \"cross_concerns\": []}\n```";
        const FINDINGS: &str = r#"<audit-findings>
[{"path":"src/a.rs","line":1,"view":"correctness","severity":"major","claim":"c","evidence":"src/a.rs:1"}]
</audit-findings>"#;
        const CROSS_GHOST: &str = "```json\n{\"findings\": [\n{\"path\":\"../../etc/passwd\",\"line\":1,\"view\":\"correctness\",\"severity\":\"critical\",\"claim\":\"越界发现\",\"evidence\":\"../../etc/passwd:1\"},\n{\"path\":\"src/a.rs\",\"line\":1,\"view\":\"security\",\"severity\":\"minor\",\"claim\":\"真实跨单元发现\",\"evidence\":\"src/a.rs:1\"}], \"concern_verdicts\": []}\n```";
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut task = crate::domain::task::Task::new("audit-self", "p-audit", "code-review", 1);
        task.workspace = ws.path().to_path_buf();
        let control = crate::domain::runner::RunControl::new();
        let mut factory = SequencedFactory {
            scripts: [PLAN, FINDINGS, CROSS_GHOST]
                .into_iter()
                .map(|s| [s].into_iter().collect())
                .collect(),
        };
        run_audit(&task, root.path(), &control, &mut factory).unwrap();
        let blackboard = root.path().join("p-audit").join("audit-self");
        let v: serde_json::Value = serde_json::from_slice(
            &std::fs::read(blackboard.join("audit-report.json")).unwrap(),
        )
        .unwrap();
        let kept = v["findings"]["kept"].as_array().unwrap();
        assert!(
            !kept.iter().any(|f| f["path"].as_str().unwrap().contains("..")),
            "穿越发现不得进 kept：{kept:?}"
        );
        let dropped = v["findings"]["dropped_ghosts"].as_array().unwrap();
        assert!(
            dropped.iter().any(|d| d[0].as_str().unwrap().contains("..")),
            "Stage 2 穿越发现应进 dropped_ghosts：{dropped:?}"
        );
    }

    /// 审计修复 #6：预查报告含本批文件的 unwrap/吞错/埋点/行数机检事实。
    #[test]
    fn precheck_report_counts_machine_facts() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(
            ws.path().join("a.rs"),
            "fn f() {\n    let x = vec![1].pop().unwrap(); // 行尾注释不跳过\n    let _ = std::fs::read_to_string(\"x\");\n    tracing::info!(\"hello\");\n}\n",
        )
        .unwrap();
        let report = precheck_report(ws.path(), &["a.rs".to_string()], None);
        assert!(report.contains("a.rs"), "{report}");
        assert!(report.contains("unreachable 1 处"), "{report}");
        assert!(report.contains("吞错 1 处"), "{report}");
        assert!(report.contains("tracing 事件 1 处"), "{report}");
        assert!(report.contains("共 5 行"), "{report}");
    }

    // ---------- 全仓审计（第六份）回归 ----------

    struct SSess {
        replies: std::collections::VecDeque<&'static str>,
    }
    impl crate::audit::pipeline::AuditSession for SSess {
        fn ask(&mut self, _: &str) -> crate::error::Result<String> {
            self.replies
                .pop_front()
                .map(|s| s.to_string())
                .ok_or_else(|| crate::error::DevforgeError::System("回复耗尽".into()))
        }
    }
    struct SFactory {
        scripts: std::collections::VecDeque<Vec<&'static str>>,
    }
    impl crate::audit::pipeline::AuditSessionFactory for SFactory {
        fn open(&mut self) -> crate::error::Result<Box<dyn crate::audit::pipeline::AuditSession>> {
            let s = self
                .scripts
                .pop_front()
                .ok_or_else(|| crate::error::DevforgeError::System("会话耗尽".into()))?;
            Ok(Box::new(SSess {
                replies: s.into_iter().collect(),
            }))
        }
    }
    fn mk(scripts: Vec<Vec<&'static str>>) -> SFactory {
        SFactory {
            scripts: scripts.into_iter().collect(),
        }
    }

    /// 测试用并发度覆盖（避免改环境变量影响并行跑的其他用例）。
    static CONCURRENCY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    pub(super) fn concurrency_override() -> Option<usize> {
        match CONCURRENCY.load(std::sync::atomic::Ordering::SeqCst) {
            0 => None,
            n => Some(n),
        }
    }
    fn set_concurrency(n: usize) {
        CONCURRENCY.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    /// 支持并行的工厂：脚本队列共享，fork 出的子工厂取下一个脚本（模拟真实多进程）。
    struct SharedScriptFactory {
        scripts: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Vec<&'static str>>>>,
    }
    impl crate::audit::pipeline::AuditSessionFactory for SharedScriptFactory {
        fn open(&mut self) -> crate::error::Result<Box<dyn crate::audit::pipeline::AuditSession>> {
            let s = self
                .scripts
                .lock()
                .expect("scripts 锁")
                .pop_front()
                .ok_or_else(|| crate::error::DevforgeError::System("会话耗尽".into()))?;
            Ok(Box::new(SSess {
                replies: s.into_iter().collect(),
            }))
        }
        fn fork(&self) -> Option<Box<dyn crate::audit::pipeline::AuditSessionFactory + Send>> {
            Some(Box::new(SharedScriptFactory {
                scripts: self.scripts.clone(),
            }))
        }
    }

    fn audit_task() -> (tempfile::TempDir, tempfile::TempDir, crate::domain::task::Task) {
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut task = crate::domain::task::Task::new("audit-x", "p-audit", "code-review", 1);
        task.workspace = ws.path().to_path_buf();
        (ws, root, task)
    }

    const P0: &str = "```json\n{\"priority\": [], \"skip\": [], \"cross_concerns\": []}\n```";
    const F1: &str = r#"<audit-findings>
[{"path":"a.rs","line":1,"view":"correctness","severity":"major","claim":"c","evidence":"fn main() {} （a.rs:1）"}]
</audit-findings>"#;
    const C0: &str = "```json\n{\"findings\": [], \"concern_verdicts\": []}\n```";
    const JUNK: &str = "我先想一下……（这里没有 json 块）";

    /// 全仓审计 M5：空仓/错目录不得 Completed + 空报告（假成功最难排查）。
    #[test]
    fn empty_workspace_fails_instead_of_fake_success() {
        let empty = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut task = crate::domain::task::Task::new("t", "p", "code-review", 1);
        task.workspace = empty.path().to_path_buf();
        let control = crate::domain::runner::RunControl::new();
        let mut f = mk(vec![vec![P0]]);
        let err = run_audit(&task, root.path(), &control, &mut f).unwrap_err();
        assert!(format!("{err}").contains("无可审源码"), "{err}");
    }

    /// 全仓审计 M2：Stage 0 首次输出没 json 块 → 换实例重试后继续，不报废整次。
    #[test]
    fn stage0_format_jitter_is_retried() {
        let (_ws, root, task) = audit_task();
        let control = crate::domain::runner::RunControl::new();
        let mut f = mk(vec![vec![JUNK], vec![P0], vec![F1], vec![C0]]);
        run_audit(&task, root.path(), &control, &mut f).unwrap();
    }

    /// 全仓审计 M2：Stage 2（全部批次跑完之后）同理。
    #[test]
    fn stage2_format_jitter_is_retried() {
        let (_ws, root, task) = audit_task();
        let control = crate::domain::runner::RunControl::new();
        let mut f = mk(vec![vec![P0], vec![F1], vec![JUNK], vec![C0]]);
        run_audit(&task, root.path(), &control, &mut f).unwrap();
    }

    /// 第三方 r3 M2：skip 指向非生成物批次 → 被忽略，该批仍进审查（自审不能再
    /// 用一句「生成物」把 src/audit 跳过）。旧的「skip 全量拒收」用例已并到这里：
    /// skip 只能命中生成物批次，而切入器只切源码面，那条 100% 分支现在只是防御性兑底。
    #[test]
    fn skip_of_core_unit_is_ignored_and_still_reviewed() {
        let (_ws, root, task) = audit_task();
        let control = crate::domain::runner::RunControl::new();
        let skip_core =
            "```json\n{\"priority\": [], \"skip\": [\"unit-001\"], \"cross_concerns\": []}\n```";
        // 先 skip 后正常审：若 skip 被采纳，第二/三次会话不会被消费 → 脚本耗尽时报错
        let mut f = mk(vec![vec![skip_core], vec![F1], vec![C0]]);
        run_audit(&task, root.path(), &control, &mut f)
            .expect("核心批次被 skip 时应忽略 skip 并照审");
        let progress: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                root.path()
                    .join("p-audit")
                    .join("audit-x")
                    .join("audit-progress.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            progress["done_batches"].as_array().unwrap().len(),
            1,
            "unit-001 必须真被审（不能被 skip 掉）：{progress}"
        );
    }

    /// 全仓审计 M7/M8：JSON 的 cross_analysis 只写核验后集合（幽灵单放到 dropped），
    /// 跨层重复只算一次（summary 不因子与本地重复而虚高）。
    #[test]
    fn cross_layer_duplicates_count_once_and_json_excludes_ghosts() {
        let (_ws, root, task) = audit_task();
        // 本地与跨层各一条同位置同主张（evidence 必须引原文，否则被核验器当猜行号丢掉）
        let cross_dup = r#"```json
{"findings": [{"path":"a.rs","line":1,"view":"correctness","severity":"major","claim":"c","evidence":"fn main() {} （a.rs:1）"},{"path":"../x.rs","line":1,"view":"correctness","severity":"critical","claim":"ghost","evidence":"../x.rs:1"}], "concern_verdicts": []}
```"#;
        let control = crate::domain::runner::RunControl::new();
        let mut f = mk(vec![vec![P0], vec![F1], vec![cross_dup]]);
        run_audit(&task, root.path(), &control, &mut f).unwrap();
        let v: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                root.path()
                    .join("p-audit")
                    .join("audit-x")
                    .join("audit-report.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let cross = v["cross_analysis"]["findings"].as_array().unwrap();
        assert!(
            !cross.iter().any(|f| f["path"].as_str().unwrap().contains("..")),
            "JSON 不得含未核验幽灵：{cross:?}"
        );
        assert_eq!(v["severity_summary"]["major"], 1, "跨层重复不得重复计数：{v}");
        assert_eq!(v["severity_summary"]["critical"], 0, "幽灵不得入汇总：{v}");
        assert!(
            v["cross_analysis"]["dropped_ghosts"]
                .as_array()
                .unwrap()
                .len()
                == 1,
            "幽灵应单列在 cross_analysis.dropped_ghosts：{v}"
        );
    }

    /// 全仓审计 m2：断点批次发现文件损坏 → 摘掉打勾**重审**（不得永久丢）。
    #[test]
    fn corrupt_findings_file_is_rereviewed() {
        let (_ws, root, task) = audit_task();
        let blackboard = root.path().join("p-audit").join("audit-x");
        std::fs::create_dir_all(blackboard.join("audit-findings")).unwrap();
        std::fs::write(
            blackboard.join("audit-progress.json"),
            r#"{"stage0_done":true,"done_batches":["unit-001"],"stage2_done":false}"#,
        )
        .unwrap();
        std::fs::write(
            blackboard.join("audit-stage0.json"),
            r#"{"priority":[],"skip":[],"cross_concerns":[]}"#,
        )
        .unwrap();
        std::fs::write(blackboard.join("audit-findings/unit-001.json"), b"{broken").unwrap();
        let control = crate::domain::runner::RunControl::new();
        // Stage 0 走断点；批次因发现损坏而重审（消费 1 个会话）；Stage 2 正常
        let mut f = mk(vec![vec![F1], vec![C0]]);
        run_audit(&task, root.path(), &control, &mut f).unwrap();
        let findings: serde_json::Value = serde_json::from_slice(
            &std::fs::read(blackboard.join("audit-findings/unit-001.json")).unwrap(),
        )
        .unwrap();
        assert!(
            findings.as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "损坏的发现文件应被重审结果覆盖：{findings}"
        );
    }

    /// 全仓审计 M3：工厂支持 fork 时批次并行，两批都落盘。
    #[test]
    fn parallel_batches_review_every_unit() {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join("src")).unwrap();
        std::fs::create_dir_all(ws.path().join("tools")).unwrap();
        std::fs::write(ws.path().join("src/a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(ws.path().join("tools/b.rs"), "fn b() {}\n").unwrap();
        let root = tempfile::tempdir().unwrap();
        let mut task = crate::domain::task::Task::new("audit-par", "p-audit", "code-review", 1);
        task.workspace = ws.path().to_path_buf();
        set_concurrency(2);
        let control = crate::domain::runner::RunControl::new();
        let scripts = std::sync::Arc::new(std::sync::Mutex::new(
            std::collections::VecDeque::from(vec![vec![P0], vec![F1], vec![F1], vec![C0]]),
        ));
        let mut f = SharedScriptFactory { scripts };
        let outcome = run_audit(&task, root.path(), &control, &mut f);
        set_concurrency(0);
        outcome.unwrap();
        let progress: serde_json::Value = serde_json::from_slice(
            &std::fs::read(
                root.path()
                    .join("p-audit")
                    .join("audit-par")
                    .join("audit-progress.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            progress["done_batches"].as_array().unwrap().len(),
            2,
            "并行路径下两批都应完成：{progress}"
        );
    }

    /// 第三方 r3 M1：worker 内 panic 收成该批 System（不再向主线程扔 usize::MAX 哨兵），
    /// 且同批已成功的兄弟批次发现已落盘（兄弟失败不带走已完成批次）。
    #[test]
    fn parallel_panic_is_contained_and_successful_batch_persisted() {
        struct LabelSession;
        impl crate::audit::pipeline::AuditSession for LabelSession {
            fn ask(&mut self, instruction: &str) -> crate::error::Result<String> {
                // 只匹「本批」那一行——指令里的「其他批次」列表也会列出别的 label
                if instruction.contains("审查批次 `label-unit-002`") {
                    panic!("模拟审查线程 panic");
                }
                Ok(F1.to_string())
            }
        }
        struct LabelFactory;
        impl crate::audit::pipeline::AuditSessionFactory for LabelFactory {
            fn open(
                &mut self,
            ) -> crate::error::Result<Box<dyn crate::audit::pipeline::AuditSession>> {
                Ok(Box::new(LabelSession))
            }
            fn fork(
                &self,
            ) -> Option<Box<dyn crate::audit::pipeline::AuditSessionFactory + Send>> {
                Some(Box::new(LabelFactory))
            }
        }

        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("a.rs"), "fn a() {}\n").unwrap();
        let findings_dir = tempfile::tempdir().unwrap();
        let units = vec![
            AuditUnit {
                id: "unit-001".into(),
                files: vec!["a.rs".into()],
                label: "label-unit-001".into(),
                focus: Vec::new(),
                focus_range: None,
            },
            AuditUnit {
                id: "unit-002".into(),
                files: vec!["a.rs".into()],
                label: "label-unit-002".into(),
                focus: Vec::new(),
                focus_range: None,
            },
        ];
        let refs: Vec<&AuditUnit> = units.iter().collect();
        let labels: Vec<String> = units.iter().map(|u| u.label.clone()).collect();
        let control = crate::domain::runner::RunControl::new();
        set_concurrency(2);
        let results = run_stage1_parallel(
            &LabelFactory,
            &refs,
            &labels,
            ws.path(),
            2,
            &control,
            findings_dir.path(),
        );
        set_concurrency(0);
        assert_eq!(results.len(), 2, "两批都要有结果（panic 不得吞掉其他批）");
        let panic_res = results
            .iter()
            .find(|(_, id, _)| id == "unit-002")
            .expect("unit-002 必须在结果里");
        let msg = format!(
            "{:?}",
            panic_res.2.as_ref().err().expect("panic 批必须是 Err")
        );
        assert!(msg.contains("panic"), "panic 要收成带说明的 System：{msg}");
        assert!(
            findings_dir.path().join("unit-001.json").is_file(),
            "兄弟批失败不得带走已完成批次的发现（r3 M1 回潮点）"
        );
    }

    /// 第三方 r3 M4：0 = 不限时限（旧行为），非 0 = 给未来截止点。
    #[test]
    fn batch_deadline_zero_means_unlimited() {
        assert!(batch_deadline_from_ms(0).is_none());
        let at = batch_deadline_from_ms(1_000).expect("非 0 应有截止点");
        assert!(at > std::time::Instant::now());
    }

    /// r3 m5 遗留收口：分片预查只数本段行（否则同文件多分片看到相同计数）。
    #[test]
    fn precheck_counts_only_the_focus_range() {
        let ws = tempfile::tempdir().unwrap();
        // 第 1–3 行有 unwrap，第 4–6 行干净
        std::fs::write(
            ws.path().join("a.rs"),
            "let a = x.unwrap();\nlet b = y.unwrap();\nlet c = z.unwrap();\nlet d = 1;\nlet e = 2;\nlet f = 3;\n",
        )
        .unwrap();
        let whole = precheck_report(ws.path(), &["a.rs".to_string()], None);
        assert!(whole.contains("共 6 行"), "{whole}");
        assert!(whole.contains("unwrap·expect·panic·todo·unreachable 3 处"), "{whole}");
        let tail = precheck_report(ws.path(), &["a.rs".to_string()], Some((4, 6)));
        assert!(tail.contains("第 4–6 行"), "{tail}");
        assert!(tail.contains("共 3 行"), "{tail}");
        assert!(tail.contains("unreachable 0 处"), "分片不得看到整文件计数：{tail}");
    }

    /// 全仓审计 m1：Stage 0 断点产物损坏 → 清标记重算，不硬失败。
    #[test]
    fn corrupt_stage0_checkpoint_is_recomputed() {
        let (_ws, root, task) = audit_task();
        let blackboard = root.path().join("p-audit").join("audit-x");
        std::fs::create_dir_all(&blackboard).unwrap();
        std::fs::write(blackboard.join("audit-stage0.json"), b"not json").unwrap();
        std::fs::write(
            blackboard.join("audit-progress.json"),
            r#"{"stage0_done":true,"done_batches":[],"stage2_done":false}"#,
        )
        .unwrap();
        let control = crate::domain::runner::RunControl::new();
        let mut f = mk(vec![vec![P0], vec![F1], vec![C0]]);
        run_audit(&task, root.path(), &control, &mut f).unwrap();
    }
}
