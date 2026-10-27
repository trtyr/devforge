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
    // 2026-10-27 自审 major（纵深）：project/id 拼黑板路径——store 创建/加载入口已拦，
    // 执行入口再验一次，防未来绕过入口的调用方。
    crate::domain::store::Store::validate_path_name(&task.project)
        .ctx("审计黑板项目名校验")?;
    crate::domain::store::Store::validate_path_name(&task.id)
        .ctx("审计黑板任务名校验")?;
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
    // 2026-10-25 事故拍板方案：**findings 文件 = 完成事实**——打勾曾是并行 join 后
    // 主线程才写，22:29 的重启把 24 个「文件在盘、勾没打」的批次全部重审（约 40
    // 分钟白烧）。对账以盘上产物为准：可解析 = 已完成；损坏 = 摘出重审；缺失 =
    // 未完成。done_batches 按此重算，有变化才落盘。
    let (mut findings, recovered, bad_batches) =
        load_persisted_findings(&findings_dir, &ordered);
    // 自审 minor（2）：单批超限被截断的条数累计（进报告产物，不只活在 WARN 里）
    let mut truncated_findings: usize = 0;
    {
        let recomputed: std::collections::BTreeSet<String> = recovered.iter().cloned().collect();
        let old: std::collections::BTreeSet<String> =
            progress.done_batches.iter().cloned().collect();
        if recomputed != old {
            progress.done_batches = recovered.clone();
            progress.save(&blackboard)?;
        }
        if !bad_batches.is_empty() {
            tracing::warn!(
                component = "audit-engine",
                bad = ?bad_batches,
                "断点批次发现文件损坏，已摘出并将重审"
            );
        }
        if !recovered.is_empty() {
            tracing::info!(
                component = "audit-engine",
                recovered = recovered.len(),
                "断点对账：以盘上 findings 文件为完成事实"
            );
        }
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
            apply_batch_outcome(
                &unit_id,
                res,
                &mut findings,
                &mut truncated_findings,
                &mut progress,
                &blackboard,
            )?;
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
            if let Ok(parsed) = &res {
                write_batch_findings(&findings_dir, &unit.id, &parsed.findings)?;
            }
            apply_batch_outcome(
                &unit.id,
                res,
                &mut findings,
                &mut truncated_findings,
                &mut progress,
                &blackboard,
            )?;
        }
    }
    // 自审 minor（31）：断点里的 abandoned 可能含历史重复项（旧版本无去重写入过），
    // 读出时按 unit 前缀去重，报告计数才不虚高。
    let mut abandoned_all: Vec<String> = progress.abandoned.clone();
    abandoned_all.sort();
    abandoned_all.dedup_by(|a, b| {
        let key = |s: &String| s.split('：').next().unwrap_or(s).to_string();
        key(a) == key(b)
    });
    let abandoned: Vec<String> = abandoned_all;

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
            // 自审 minor（2）：单批超限被截断的条数——「这次少看了 N 条」必须可对账
            "truncated": truncated_findings,
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
    // 自审 minor（24）：MD 报告也带代码版本（与 JSON 同源），报告文件才能溯到具体 commit
    let head = workspace_sha(&task.workspace);
    let md = super::report::render_markdown(
        &task.project,
        &task.id,
        &merged,
        head.as_deref(),
        &all_dropped,
        &abandoned,
    );
    std::fs::write(blackboard.join("audit-report.md"), md).ctx("写审计报告 MD")?;
    // 自审 minor（28）：收尾日志的计数必须与报告同源（旧口径日志记 kept/abandoned、
    // 报告记 merged/batches_*，两处对不上），并补覆盖度比例——否则仅凭日志无法重建
    // 「这次到底审了多少」。
    let batches_total = progress.done_batches.len() + abandoned.len() + skipped_units.len();
    let coverage_ratio = if plan.units.is_empty() {
        0.0
    } else {
        progress.done_batches.len() as f64 / plan.units.len() as f64
    };
    tracing::info!(
        component = "audit-engine",
        event = "audit.finished",
        units_total = plan.units.len(),
        batches_done = progress.done_batches.len(),
        batches_abandoned = abandoned.len(),
        batches_skipped = skipped_units.len(),
        batches_total,
        coverage_ratio = format!("{:.3}", coverage_ratio),
        merged = merged.len(),
        kept = verified.kept.len(),
        dropped_ghosts = all_dropped.len(),
        cross = cross_kept.len(),
        "全量审计完成（计数与 audit-report.json 同源）"
    );
    // 自审 minor（29）：存在放弃批次时 run 仍是 Completed，状态层没有任何「部分覆盖」
    // 信号（报告里只在 JSON/MD 的 abandoned 列表里）。这里显式落一条 warn 级事件，
    // 让「成功但没审全」在事件流/run 卡片上可辨识。
    if !abandoned.is_empty() {
        tracing::warn!(
            component = "audit-engine",
            event = "audit.partial_coverage",
            abandoned = abandoned.len(),
            units_total = plan.units.len(),
            coverage_ratio = format!("{:.3}", coverage_ratio),
            "审计完成但存在放弃批次：本次为**部分覆盖**，不得当作全量结论"
        );
    }
    Ok(RunOutcome::Completed)
}

/// 全仓审计 M3：批次并发度（DEVFORGE_AUDIT_CONCURRENCY，默认 4，边界 1..=8）。
/// 单批 ~6 分钟，旧串行 87 批 ≈ 9h；四路并发是批次数与 provider 限流间的保守值。
/// 第三方 r3 M2：一个批次能不能被 skip——必须整批都是「生成物 / 锁文件 / vendor 树」。
/// 宁可多审（误判为不可跳 → 照审）也不许把核心目录用一句「生成物」糊过去。
fn skippable_unit(unit: &AuditUnit) -> bool {
    // 自审 minor（30）：旧口径只看文件后缀/目录名启发式——手写源码放在 dist/ 或 vendor/
    // （或命名为 *.snap）就会被整批静默跳过，与「宁可多审不少审」相悖。现在目录命中也
    // 必须**同时**是生成物标记（后缀/生成器注释）才算可跳；判断不了就照审。
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
                // 目录命中不再单独成立：只有「目录里且带生成物标记」才可跳
                || ((p.starts_with("target/")
                    || p.starts_with("node_modules/")
                    || p.starts_with("dist/")
                    || p.starts_with("vendor/"))
                    && has_generated_marker(p))
        })
}

/// 目录类路径的生成物标记（自审 minor 30）：无标记 = 可能是手写源码 → 不跳。
fn has_generated_marker(p: &str) -> bool {
    p.ends_with(".js")
        && (p.contains(".bundle.") || p.contains(".chunk.") || p.ends_with(".umd.js"))
        || p.ends_with(".d.ts")
        || p.ends_with(".map")
        || p.contains(".generated.")
        || p.contains("/__generated__/")
        || p.ends_with(".tmp")
        || p.ends_with(".log")
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
///
/// 自审 minor 27：串行打勾循环遇错即返回时，**后续已成功且已落盘的批次拿不到 done 勾**，
/// 续跑会重复审查——所以调用方必须把打勾循环做到「逐条应用、遇错记错但不中断」。
/// 质量类失败 = 放弃并记入断点；实例/基础设施失败 = 打断整次审计（不伪装成放弃）。
///
/// 第三方 r3 M1：旧版把「写盘 + 打勾」揃在一起、且并行路径「全跑完才落盘」——
/// 兄弟批次一失败，内存里已审完的批次全丢。现在拆两半，写盘在 worker 内完成。
fn apply_batch_outcome(
    unit_id: &str,
    res: Result<crate::audit::finding::ParsedBatch>,
    findings: &mut Vec<Finding>,
    truncated: &mut usize,
    progress: &mut Progress,
    blackboard: &Path,
) -> Result<()> {
    match res {
        Ok(parsed) => {
            let batch_n = parsed.findings.len();
            // 自审 minor（2）：截断条数累计进报告（旧口径只活在 WARN 日志里）
            *truncated += parsed.truncated;
            findings.extend(parsed.findings);
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
            // 自审 minor（13）：abandoned 是断点字段、会被续跑复用——同一单元只能有一条，
            // 否则报告的 batches_abandoned 计数虚高（旧口径无条件 push）。
            let entry = format!("{unit_id}：{msg}");
            match progress
                .abandoned
                .iter_mut()
                .find(|a| a.starts_with(&format!("{unit_id}：")))
            {
                Some(existing) => *existing = entry,
                None => progress.abandoned.push(entry),
            }
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
type BatchResult = (usize, String, Result<crate::audit::finding::ParsedBatch>);

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
                        Ok(parsed) => match write_batch_findings(findings_dir, &unit_id, &parsed.findings)
                        {
                            Ok(()) => Ok(parsed),
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
            match h.join() {
                Ok(mut v) => results.append(&mut v),
                Err(payload) => {
                    // 自审 minor（32）：worker 在 catch_unwind 覆盖范围外 panic 时，旧口径
                    // `if let Ok` 静默丢弃——该 worker 已完成的批次既不打勾也不进报告，
                    // 而且日志里一个字都没有。现在留痕（已落盘的批次发现仍在盘上，可在
                    // 续跑时按 findings 目录回收）。
                    let msg = crate::server::panic_text(&payload);
                    tracing::error!(
                        component = "audit-engine",
                        event = "audit.worker_panicked",
                        panic = %msg,
                        "批次 worker 线程 panic：其已落盘结果保留，未完成的批次需续跑"
                    );
                }
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
                // 自审 minor（33）：旧口径只把失败写进给 LLM 的提示串——既不 tracing 也不落盘，
                // 而提示词又声明预查是「确定性事实」。留痕 + 报告里保留该行。
                tracing::warn!(
                    component = "audit-engine",
                    event = "audit.precheck_read_failed",
                    file = %f,
                    error = %e,
                    "预查读文件失败（该批计数将不完整）"
                );
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
    // 自审 minor（22）：改用 report::by_severity（旧口径在这里另写一套字符串比对，
    // 同一能力两套实现，改一处必漂移）。
    let grouped = super::report::by_severity(findings);
    let n = |s| grouped.get(&s).map(|v| v.len()).unwrap_or(0);
    serde_json::json!({
        "critical": n(crate::audit::finding::Severity::Critical),
        "major": n(crate::audit::finding::Severity::Major),
        "minor": n(crate::audit::finding::Severity::Minor),
        "nit": n(crate::audit::finding::Severity::Nit),
    })
}

/// 全仓审计 M2：Stage 0/2 的换实例重试（口径同 Stage 1：2 次）——
/// 只重试**输出不合规**（Quality）：实例/基础设施故障（System/Agent）直接上抛，
/// 不伪装成格式问题反复重开。
/// 2026-10-26 补丁：单阶段**空回复**（provider 软断粮：HTTP 200 + 零正文，如 MiniMax
/// Token Plan 到量）与批次同层——转 Quality 走换实例重试，重试耗尽按 Quality 收场，
/// 不再以 System 触发 run 级 4 连重启（每次重启重过切片+对账，纯烧时间）。
fn with_stage_retry<T>(
    factory: &mut dyn AuditSessionFactory,
    label: &str,
    mut run: impl FnMut(&mut dyn AuditSession) -> Result<T>,
) -> Result<T> {
    let mut last = String::new();
    for attempt in 0..2 {
        let mut session = factory.open().ctx(format!("开{label}会话"))?;
        let outcome = run(session.as_mut()).map_err(|e| {
            if e.to_string().contains("未产出一字") {
                tracing::warn!(
                    component = "audit-engine",
                    stage = label,
                    attempt = attempt + 1,
                    "{label}会话空回复（provider 软断粮），按不合规重试"
                );
                crate::error::DevforgeError::Quality(format!(
                    "{label} 会话空回复（进程正常结束但未产出一字）"
                ))
            } else {
                e
            }
        });
        match outcome {
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

/// 断点对账：**盘上 findings 文件 = 完成事实**（2026-10-25 拍板）。对每个切片单元
/// 查 `{id}.json`：可解析 → 发现入库、记入 recovered；损坏 → 记入 bad（将重审）；
/// 缺失 → 未完成（正常首次态，不告警）。旧实现按 done_batches 打勾回读——打勾
/// 曾在并行 join 后才写，崩溃后「文件在盘、勾没打」的批次被整批重审。
fn load_persisted_findings(
    dir: &Path,
    units: &[&AuditUnit],
) -> (Vec<Finding>, Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut recovered: Vec<String> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for u in units {
        let id = &u.id;
        let p = dir.join(format!("{id}.json"));
        let Ok(bytes) = std::fs::read(&p) else {
            continue; // 未完成：正常首次态
        };
        match serde_json::from_slice::<Vec<Finding>>(&bytes) {
            Ok(fs) => {
                out.extend(fs);
                recovered.push(id.clone());
            }
            Err(e) => {
                tracing::warn!(
                    component = "audit-engine",
                    unit = %id,
                    error = %e,
                    "断点批次发现反序列化失败，将重审"
                );
                bad.push(id.clone());
            }
        }
    }
    (out, recovered, bad)
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
#[path = "runner_tests.rs"]
mod tests;
