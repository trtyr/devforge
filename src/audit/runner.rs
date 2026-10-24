//! 审计 runner（P013 M1）：把四阶段串成一次 run——盘点切片 → Stage0 规划 →
//! Stage1 批次审查（独立会话×N）→ 核验 → Stage2 跨单元 → 报告落黑板。
//!
//! 会话桥：`ReviewSessionFactory`（decisions/004 血统）——每批次/每阶段独立实例。

use std::path::Path;

use crate::domain::runner::RunOutcome;
use crate::domain::task::Task;
use crate::error::{Context as _, Result};

use super::finding::Finding;
use super::pipeline::{stage0_plan, stage1_review_batch, stage2_cross, AuditSessionFactory, Progress};
use super::{build_plan, AuditUnit};
use super::verify::verify;

/// 全量审计执行（生产入口）。`factory` 每次开新会话（用完即弃）。
pub fn run_audit(
    task: &Task,
    plans_root: &Path,
    factory: &mut dyn AuditSessionFactory,
) -> Result<RunOutcome> {
    let blackboard = plans_root.join(&task.project).join(&task.id);
    std::fs::create_dir_all(&blackboard).ctx("创建黑板目录")?;
    let mut progress = Progress::load(&blackboard);

    // ---- 盘点 + 切片（确定性零 LLM）----
    let plan = build_plan(&task.workspace)?;
    let unit_labels: Vec<String> = plan.units.iter().map(|u| u.label.clone()).collect();
    tracing::info!(
        component = "audit-engine",
        units = plan.units.len(),
        "切片完成，进入四阶段审查"
    );

    // ---- Stage 0 规划 ----
    let review_plan = if progress.stage0_done {
        tracing::info!(component = "audit-engine", "断点：Stage 0 已完成，跳过");
        read_stage0(&blackboard)?
    } else {
        let mut session = factory.open().ctx("开规划会话")?;
        let plan = stage0_plan(session.as_mut(), &plan.units)?;
        std::fs::write(
            blackboard.join("audit-stage0.json"),
            serde_json::to_vec_pretty(&plan).ctx("序列化 ReviewPlan")?,
        )
        .ctx("写 Stage0 产物")?;
        progress.stage0_done = true;
        progress.save(&blackboard)?;
        tracing::info!(
            component = "audit-engine",
            priority = plan.priority.len(),
            skip = plan.skip.len(),
            concerns = plan.cross_concerns.len(),
            "Stage 0 规划完成"
        );
        plan
    };

    // ---- Stage 1 批次审查（按优先级排序；skip 跳过；断点跳过已完成批次）----
    let mut ordered: Vec<&AuditUnit> = plan
        .units
        .iter()
        .filter(|u| !review_plan.skip.contains(&u.id))
        .collect();
    ordered.sort_by_key(|u| {
        (
            !review_plan.priority.contains(&u.id), // 优先级在前
            u.id.clone(),                          // 同级按 id 稳定
        )
    });

    // 确定性预查报告（M0 血统的机检结果，注入每批次）——M1 首版给占位摘要，
    // V2 unwrap 扫描/V3 埋点缺口比对随 T1.4 落地。
    let precheck = "（引擎预查：本批次为首批接入，确定性扫描报告将随 T1.4 落地）";

    let mut findings: Vec<Finding> = Vec::new();
    let mut abandoned: Vec<String> = Vec::new();
    for unit in &ordered {
        if progress.done_batches.contains(&unit.id) {
            tracing::info!(component = "audit-engine", unit = %unit.id, "断点：批次已完成，跳过");
            continue;
        }
        match stage1_review_batch(factory, unit, &unit_labels, precheck) {
            Ok(fs) => {
                findings.extend(fs);
                progress.done_batches.push(unit.id.clone());
                progress.save(&blackboard)?;
                tracing::info!(
                    component = "audit-engine",
                    unit = %unit.id,
                    batch_findings = findings.len(),
                    "批次审查完成"
                );
            }
            Err(e) => {
                // 批次放弃不拖垮整审计（诚实失败：记录放弃原因入报告）
                abandoned.push(format!("{}：{e}", unit.id));
                tracing::warn!(component = "audit-engine", unit = %unit.id, "批次放弃");
            }
        }
    }

    // ---- 核验过滤（确定性零 LLM，RepoAudit validator 血统）----
    let verified = verify(&task.workspace, &findings);
    // ---- 跨单元去重（T2.2：同位置完全重复合并）----
    let deduped = super::report::dedup(&verified.kept);

    // ---- Stage 2 跨单元分析 ----
    let cross = if progress.stage2_done {
        read_stage2(&blackboard)?
    } else {
        let mut session = factory.open().ctx("开跨单元会话")?;
        let out = stage2_cross(session.as_mut(), &verified.kept, &review_plan.cross_concerns)?;
        std::fs::write(
            blackboard.join("audit-stage2.json"),
            serde_json::to_vec_pretty(&out).ctx("序列化跨单元分析")?,
        )
        .ctx("写 Stage2 产物")?;
        progress.stage2_done = true;
        progress.save(&blackboard)?;
        out
    };

    // ---- 报告组装（JSON + MD，M2）----
    let report = serde_json::json!({
        "project": task.project,
        "task": task.id,
        "units_total": plan.units.len(),
        "batches_done": progress.done_batches.len(),
        "batches_abandoned": abandoned,
        "findings": {
            "kept": deduped,
            "dropped_ghosts": verified.dropped,
        },
        "cross_analysis": cross,
        "severity_summary": severity_summary(&deduped, &cross.findings),
    });
    std::fs::write(
        blackboard.join("audit-report.json"),
        serde_json::to_vec_pretty(&report).ctx("序列化审计报告")?,
    )
    .ctx("写审计报告")?;
    let md = super::report::render_markdown(
        &task.project,
        &task.id,
        &deduped,
        &cross.findings,
        &verified.dropped,
        &abandoned,
    );
    std::fs::write(blackboard.join("audit-report.md"), md).ctx("写审计报告 MD")?;
    tracing::info!(
        component = "audit-engine",
        kept = verified.kept.len(),
        dropped = verified.dropped.len(),
        cross = cross.findings.len(),
        abandoned = abandoned.len(),
        "全量审计完成"
    );
    Ok(RunOutcome::Completed)
}

fn severity_summary(
    local: &[Finding],
    cross: &[Finding],
) -> serde_json::Value {
    let mut count = |sev: &str, list: &[Finding]| {
        list.iter()
            .filter(|f| {
                serde_json::to_value(f.severity)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s == sev))
                    .unwrap_or(false)
            })
            .count()
    };
    serde_json::json!({
        "critical": count("critical", local) + count("critical", cross),
        "major": count("major", local) + count("major", cross),
        "minor": count("minor", local) + count("minor", cross),
        "nit": count("nit", local) + count("nit", cross),
    })
}

fn read_stage0(blackboard: &Path) -> Result<super::pipeline::ReviewPlan> {
    let bytes = std::fs::read(blackboard.join("audit-stage0.json"))
        .ctx("读 Stage0 断点")?;
    serde_json::from_slice(&bytes).ctx("Stage0 断点反序列化")
}

fn read_stage2(blackboard: &Path) -> Result<super::pipeline::CrossAnalysis> {
    let bytes = std::fs::read(blackboard.join("audit-stage2.json"))
        .ctx("读 Stage2 断点")?;
    serde_json::from_slice(&bytes).ctx("Stage2 断点反序列化")
}
