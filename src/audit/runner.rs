//! 审计 runner（P013 M1）：把四阶段串成一次 run——盘点切片 → Stage0 规划 →
//! Stage1 批次审查（独立会话×N）→ 核验 → Stage2 跨单元 → 报告落黑板。
//!
//! 会话桥：`ReviewSessionFactory`（decisions/004 血统）——每批次/每阶段独立实例。

use std::path::Path;

use crate::domain::runner::{RunControl, RunOutcome};
use crate::domain::task::Task;
use crate::error::{Context as _, Result};

use super::finding::Finding;
use super::pipeline::{stage0_plan, stage1_review_batch, stage2_cross, AuditSessionFactory, Progress};
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
        check_intervention(control)?;
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

    // 复验事故修复（M3 缺陷①）：每批 findings 即时落黑板——先落盘再打勾，
    // 断点续跑从黑板回读已完成批次，进程死亡不再丢发现。
    let findings_dir = blackboard.join("audit-findings");
    std::fs::create_dir_all(&findings_dir).ctx("建 audit-findings 目录")?;
    let mut findings: Vec<Finding> = load_persisted_findings(&findings_dir, &progress.done_batches);
    let mut abandoned: Vec<String> = Vec::new();
    for unit in &ordered {
        if progress.done_batches.contains(&unit.id) {
            tracing::info!(component = "audit-engine", unit = %unit.id, "断点：批次已完成，跳过");
            continue;
        }
        check_intervention(control)?;
        match stage1_review_batch(factory, unit, &unit_labels, precheck) {
            Ok(fs) => {
                // 先落盘批次发现，再打勾进度（顺序不可反：进程死在中间也不丢）
                std::fs::write(
                    findings_dir.join(format!("{}.json", unit.id)),
                    serde_json::to_vec_pretty(&fs).ctx("序列化批次发现")?,
                )
                .ctx("写批次发现")?;
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
                // 审计修复 #1：错误分流——协议违例放弃（Quality）= 记录后继续；
                // 实例失败（System/Agent 等基础设施故障）= 打断整次审计（run 层重试/重启接手），
                // 不许「该批放弃、整次成功」把故障写成成功审计。
                if matches!(e, crate::error::DevforgeError::Quality(_)) {
                    abandoned.push(format!("{}：{e}", unit.id));
                    tracing::warn!(component = "audit-engine", unit = %unit.id, "批次放弃");
                } else {
                    tracing::error!(
                        component = "audit-engine",
                        unit = %unit.id,
                        error = %e,
                        "批次实例失败，打断整次审计（非质量类错误不伪装成放弃）"
                    );
                    return Err(e);
                }
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
        check_intervention(control)?;
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

/// 断点回读：进度里已完成的批次，发现从黑板加载。缺文件/坏文件的批次跳过
/// （进度打了勾但发现丢了 = 历史遗留数据，如实少记不崩，WARN 留痕）。
fn load_persisted_findings(dir: &Path, done_batches: &[String]) -> Vec<Finding> {
    let mut out = Vec::new();
    for id in done_batches {
        let p = dir.join(format!("{id}.json"));
        let Ok(bytes) = std::fs::read(&p) else {
            tracing::warn!(
                component = "audit-engine",
                unit = %id,
                "断点批次的发现文件缺失，跳过回读（历史遗留批次）"
            );
            continue;
        };
        match serde_json::from_slice::<Vec<Finding>>(&bytes) {
            Ok(fs) => out.extend(fs),
            Err(e) => tracing::warn!(
                component = "audit-engine",
                unit = %id,
                error = %e,
                "断点批次发现反序列化失败，跳过"
            ),
        }
    }
    out
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
        let got = load_persisted_findings(
            &findings_dir,
            &["unit-001".to_string(), "unit-002".to_string()],
        );
        assert_eq!(got.len(), 1, "存在的批次回读，缺失的跳过");
        assert_eq!(got[0].batch, "unit-001");
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
}
