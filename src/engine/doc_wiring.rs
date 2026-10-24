//! doc-from-zero SOP 的引擎接线（ARC-R2 自 server/mod.rs 迁入）。
//!
//! 门 0（清单 + 闭包 + 单元）→ run_pipeline 流水线滚动 → 终审 → 运行账单。
//! **R3 中转站**：本文件在垂直切片时整体迁 `sop/doc/`（与 runner/协议/测试同居）。

use crate::domain::run::Run;
use crate::domain::runner::{RunControl, RunOutcome};
use crate::domain::task::Task;
use crate::engine::builtin::BuiltinRpcEngine;
use crate::engine::builtin::preflight_codegraph;
use crate::engine::client::RpcClient;
use crate::engine::process::SpawnSpec;
use crate::error::{Context as _, Result};
use crate::observability::watchdog::run_with_watchdog;
use crate::server::RelayUserLink;
use crate::sop::doc::{DOCS_DIR, META_DIR};
use crate::sop::planning::ReviewSessionFactory;
use crate::sop::rpc_bridge::{RpcPlanSession, RpcReviewFactory};

impl BuiltinRpcEngine {
    /// doc-from-zero SOP（P008）：门 0（清单 + 闭包 + 单元）→ run_pipeline 流水线滚动（ARC-R1 后唯一 runner）。
    pub(crate) fn run_doc_from_zero(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome> {
        let started = std::time::Instant::now();
        for p in Self::profiles_for_sop("doc-from-zero") {
            self.require_profile(p)?;
        }
        if task.workspace.as_os_str().is_empty() {
            return Err(crate::error::DevforgeError::Store(format!(
                "任务 `{}/{}` 无代码工作区，doc-from-zero 需要项目地址",
                task.project, task.id
            )));
        }
        let blackboard = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&blackboard).ctx("创建黑板目录")?;
        preflight_codegraph(&task.workspace, &self.codegraph_bin);

        // 门 0（引擎侧，不靠 agent 自证）：清单 + 三道闭包 + 单元编号
        let workspace = task.workspace.clone();
        let inv = crate::docgen::inventory::build_inventory(&workspace)?;
        let meta_dir = workspace.join(META_DIR);
        std::fs::create_dir_all(workspace.join(DOCS_DIR)).ctx("建产出目录")?;
        std::fs::create_dir_all(&meta_dir).ctx("建 _meta 目录")?;
        inv.save(&meta_dir.join("inventory.json"))?;
        std::fs::write(
            meta_dir.join("glossary.md"),
            crate::docgen::inventory::export_glossary(&inv),
        )
        .ctx("写词表")?;
        let findings = crate::docgen::inventory::check_closures(&workspace, &inv)?;
        let mut gate = String::from("# 门 0 闭包校验（引擎侧）\n\n");
        for f in &findings {
            gate.push_str(&format!(
                "- {}:{} [{}] {}\n",
                f.path,
                f.line,
                f.closure.as_str(),
                f.message
            ));
        }
        if findings.is_empty() {
            gate.push_str("三道闭包全过。\n");
        } else {
            gate.push_str("\n> 以上为机器发现：写文档时逐条取舍，确属不可达/不支持的写进 gaps.md，不得静默忽略。\n");
        }
        std::fs::write(meta_dir.join("gate0-closures.md"), gate).ctx("写门0证据")?;

        let mut units: Vec<String> = inv
            .entries
            .iter()
            .filter(|e| e.kind == crate::docgen::inventory::EntryKind::Module)
            .map(|e| e.unit_id.clone())
            .filter(|u| !u.is_empty())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        units.sort();
        if units.is_empty() {
            return Err(crate::error::DevforgeError::System(
                "门 0 未产出任何单元（清单为空或全部未分组）".into(),
            ));
        }

        let agent_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("doc-agent"),
                "doc-agent",
                run.id.0.clone(),
                self.session_root.join("doc"),
            )
            .with_working_dir(blackboard.clone()),
        )?;
        let agent = RpcPlanSession::new(agent_client);
        let factory = RpcReviewFactory::per_round_in(
            self.profile_root.join("doc-review"),
            "doc-review",
            self.session_root.join("doc-reviews"),
            blackboard,
        );
        let mut user = RelayUserLink {
            relay: self.ui.clone(),
        };
        // 审查输入注入（D012）：每轮把覆盖 + lint 的确定性结果算好给审查官
        let audit_inv = inv.clone();
        let audit_ws = workspace.clone();
        let audit_index = crate::docgen::lint::EntityIndex::build(&workspace, Some(&inv))?;

        // 断点与续跑（plan 010）：状态住黑板（不进产物目录）；done 证据 = 产出文件在
        // + 本单元文件内无非 Link 违规（DR-16 单元级归因；Link 死链由末尾全量 lint 兑底）。
        let resume_index = audit_index.clone();
        let resume_docs = workspace.join(DOCS_DIR);
        let resume = crate::sop::doc::resume::DocResume::new(resume_docs.clone()).with_lint(
            move |state: &crate::runstate::RunState| {
                match crate::docgen::lint::lint_docs_with_entities(&resume_docs, &resume_index) {
                    Ok(r) => Some(r.violations().to_vec()),
                    Err(_) => None,
                }
            },
        );
        let store = crate::runstate::StateStore::new(
            self.plans_root.clone(),
            crate::runstate::StateKey::new(&task.project, &task.id, crate::sop::doc::resume::DOC_SOP),
        );
        let loaded = store.load();
        // 供闭包之后推进基线用（store 会被 run_pipeline 吃掉）
        let store_final = crate::runstate::StateStore::new(
            self.plans_root.clone(),
            crate::runstate::StateKey::new(&task.project, &task.id, crate::sop::doc::resume::DOC_SOP),
        );
        let head_now = crate::sop::doc::incremental::head_sha(&workspace);
        let (inc_on, req_base) = crate::sop::doc::incremental::enabled_from_env();
        let mode = crate::sop::doc::incremental::decide_mode(
            loaded.state(),
            head_now.as_deref(),
            req_base.as_deref(),
            inc_on,
        );
        if inc_on && let crate::sop::doc::incremental::Mode::Full { reason } = &mode {
            tracing::warn!(component = "doc-engine", %reason, "增量不可用，本轮按全量跑");
        }
        let mut plan = crate::runstate::resume::plan(
            &self.plans_root,
            &task.project,
            &task.id,
            &resume,
            &units,
            false,
        );
        if plan.resumed {
            tracing::info!(
                component = "doc-engine",
                skip = plan.skip.len(),
                redo = plan.redo.len(),
                "从断点续跑（已完成单元不重写）"
            );
        }
        if let Some(note) = &plan.note {
            tracing::warn!(component = "doc-engine", %note, "断点不可用，按冷启动");
        }
        let mut resume_note = match loaded.state() {
            Some(st) => crate::runstate::resume::Resumable::resume_brief(&resume, st, &plan.redo),
            None => String::new(),
        };

        // 增量（D021）：显式开关 + 状态里的 base_sha；失败一律降级全量并记日志。
        let mut run_units = plan.to_run.clone();
        // T009：报告要记录尝试次数（run_with_retry 在重启前把 rs.attempt 落盘；首次为 1）
        let attempt_now = loaded.state().map(|s| s.attempt).unwrap_or(0).max(1);
        if let crate::sop::doc::incremental::Mode::Incremental { base } = &mode {
            match crate::docgen::impact::changed_since(&workspace, base) {
                Ok(changes) if changes.is_empty() => {
                    run_units = Vec::new();
                    resume_note.push_str("\n增量：自基线以来无代码变更，本轮不派发单元。");
                    tracing::info!(component = "doc-engine", %base, "增量：无变更");
                }
                Ok(changes) => {
                    match crate::docgen::impact::plan_impact(
                        &workspace,
                        &workspace.join(DOCS_DIR),
                        &changes,
                    ) {
                        Ok(impact) => {
                            if let Some(mut st) = loaded.state().cloned() {
                                run_units = crate::sop::doc::incremental::merge_plan(
                                    &mut plan, &mut st, &impact, &units,
                                );
                                if let Err(e) = store.save(&mut st) {
                                    tracing::warn!(component = "doc-engine", error = %e, "增量状态落盘失败");
                                }
                            }
                            resume_note.push_str(&format!(
                                "\n增量：基线 {base}，变更 {} 个文件，受影响单元 {} 个（{}），汇编复核：{}。",
                                changes.len(),
                                impact.units.len(),
                                impact.units.join("、"),
                                if impact.assembly_review.is_empty() {
                                    "无".to_string()
                                } else {
                                    impact.assembly_review.join("、")
                                }
                            ));
                            for n in &impact.notes {
                                resume_note.push_str(&format!("\n- {n}"));
                            }
                            tracing::info!(
                                component = "doc-engine",
                                base = %base,
                                changes = changes.len(),
                                units = impact.units.len(),
                                "增量：只重算受影响单元"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(component = "doc-engine", error = %e, "增量影响面不可算，降级全量");
                            resume_note.push_str(&format!("\n增量降级全量：{e}"));
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(component = "doc-engine", error = %e, "增量 diff 失败，降级全量");
                    resume_note.push_str(&format!("\n增量降级全量：{e}"));
                }
            }
        }

        let final_index = audit_index.clone();
        let audit: Box<dyn Fn(&str) -> String + Send> = Box::new(move |unit: &str| {
            let mut out = String::new();
            let docs = audit_ws.join(DOCS_DIR);
            if let Ok(cov) = crate::docgen::audit::coverage_for_unit(
                &audit_inv,
                &docs,
                crate::docgen::audit::DocLevel::Complete,
                unit,
            ) {
                out.push_str(&cov.render());
            }
            match crate::docgen::lint::lint_docs_with_entities(&docs, &audit_index) {
                Ok(report) => {
                    out.push_str(&report.summary());
                    out.push('\n');
                    for v in report.violations() {
                        out.push_str(&format!("- {}\n", v.render()));
                    }
                }
                Err(e) => out.push_str(&format!("lint 执行失败：{e}\n")),
            }
            if let Ok(cmds) = crate::docgen::audit::runbook_commands(&docs)
                && !cmds.is_empty()
            {
                out.push_str("\nRunbook 可执行命令块（审查时逐条从零 boot）：\n");
                for c in cmds {
                    out.push_str(&format!("- ```\n{c}\n```\n"));
                }
            }
            out
        });
        // ARC-R1：DEVFORGE_DOC_ONLY 单元重派随串行 runner 退役——单单元重跑由续跑计划（doc_resume plan 过滤）承担
        let run_id = run.id.0.clone();
        // 终审（t11）：同一 profile、新实例；捕获所需路径（闭包外先克隆）
        let final_review_root = self.profile_root.join("doc-review");
        let final_session_root = self.session_root.join("doc-reviews");
        let final_blackboard = self.plans_root.join(&task.project).join(&task.id);
        let final_ws = workspace.clone();
        let final_inv = inv.clone();
        let final_run_id = run.id.0.clone();
        let session_root = self.session_root.clone();
        let sop_result = run_with_watchdog(
            control,
            run_id,
            self.idle_ms,
            vec![session_root.join("doc"), session_root.join("doc-reviews")],
            || {
                // DR-14/ARC-R1：pipeline 是唯一 runner（审查(N) 与 写作(N+1) 重叠）；
                // 工厂经 Arc<Mutex> 共享给审查线程。
                let factory = std::sync::Arc::new(std::sync::Mutex::new(factory));
                let report = crate::sop::doc::run_pipeline(
                    agent,
                    factory,
                    store,
                    &mut user,
                    &workspace,
                    &run_units,
                    "complete",
                    resume_note,
                    Some(audit),
                    None,
                )?;

                // ---- 终审（跨模块 / 汇编引用 / 全量 lint 与覆盖）----
                let docs = final_ws.join(DOCS_DIR);
                let mut full = String::new();
                if let Ok(cov) = crate::docgen::audit::coverage(
                    &final_inv,
                    &docs,
                    crate::docgen::audit::DocLevel::Complete,
                ) {
                    full.push_str(&cov.render());
                    full.push('\n');
                }
                match crate::docgen::lint::lint_docs_with_entities(&docs, &final_index) {
                    Ok(r) => {
                        full.push_str(&r.summary());
                        full.push('\n');
                        for v in r.violations() {
                            full.push_str(&format!("- {}\n", v.render()));
                        }
                    }
                    Err(e) => full.push_str(&format!("终审 lint 失败：{e}\n")),
                }
                if let Ok(dups) = crate::docgen::audit::duplicate_mentions(&docs, &final_inv)
                    && !dups.is_empty()
                {
                    full.push_str(
                        "\n同名出现（提示级：实体名在多篇里互相引用是正常写法，不单独判 FAIL）：\n",
                    );
                    for d in dups {
                        full.push_str(&format!("- {d}\n"));
                    }
                }
                if let Ok(dups) = crate::docgen::audit::duplicated_content(&docs)
                    && !dups.is_empty()
                {
                    full.push_str(
                        "\n跨模块重复正文（判据：同一段解释被两篇以上具体描述 → FAIL）：\n",
                    );
                    for d in dups {
                        full.push_str(&format!("- {d}\n"));
                    }
                }

                // 一致性校验（增量 T024）：汇编页引用是否还解析得动、词表/清单还在不在。
                let consistency =
                    crate::sop::doc::incremental::consistency_notes(&docs, &final_ws);
                if !consistency.is_empty() {
                    full.push_str("\n一致性校验（引擎机械检查，陈旧引用要问审查官是否 FAIL）：\n");
                    for c in &consistency {
                        full.push_str(&format!("- {c}\n"));
                    }
                }

                let mut final_factory =
                    RpcReviewFactory::per_round_in(final_review_root, "doc-review", final_session_root, final_blackboard);
                let mut session = final_factory.open()?;
                let final_text = session.review(&crate::sop::doc::prompts::doc_final_instruction(&full))?;
                drop(session);
                let verdict = crate::sop::doc::protocol::parse_doc_review(&final_text)?;

                // 运行账单（T021 验收）：墙钟 / 单元数 / 模块数 / 审查轮次 / token 与花费
                // token 不从 Agent 自述拿，而是扫写方与审方会话（pi 自己写的原始账）。
                let usage = crate::docgen::usage::from_session_dirs(&[
                    session_root.join("doc"),
                    session_root.join("doc-reviews"),
                ]);
                let module_count = final_inv
                    .entries
                    .iter()
                    .filter(|e| e.kind == crate::docgen::inventory::EntryKind::Module)
                    .count();
                let dir = docs.join("_meta/runs").join(&final_run_id);
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(
                    dir.join("report.md"),
                    format!(
                        "# doc-from-zero 运行报告\n\n- run: {}\n- 墙钟: {}s\n- 尝试: {}（1 = 首次；>1 含自动重启）\n- 模块数: {} / 单元数: {}\n- 单元完成: {:?}\n- 审查轮次: {}\n- 累计问题: {}\n- 用量: {}\n- 终审: {:?}\n\n## 终审审计\n\n{}\n",
                        final_run_id,
                        started.elapsed().as_secs(),
                        attempt_now,
                        module_count,
                        report.units_done.len(),
                        report.units_done,
                        report.review_rounds,
                        report.issues_total,
                        usage.render(),
                        verdict.verdict,
                        full
                    ),
                );

                if verdict.protocol_violation {
                    return Err(crate::error::DevforgeError::Quality(format!(
                        "终审输出协议违例：{}",
                        verdict.issues.join("；")
                    )));
                }
                match verdict.verdict {
                    crate::sop::doc::protocol::DocVerdict::Pass => Ok(report),
                    crate::sop::doc::protocol::DocVerdict::Fail => {
                        Err(crate::error::DevforgeError::Quality(format!(
                            "终审未通过（route={}）：{}",
                            verdict.route.as_str(),
                            verdict.issues.join("；")
                        )))
                    }
                }
            },
        );

        match sop_result {
            Ok(report) => {
                // 基线只在成功后推进（D021）：失败保留旧值，下次仍按同一批变更重算
                let head = crate::sop::doc::incremental::head_sha(&workspace);
                if let crate::runstate::Loaded::Found(mut st) = store_final.load()
                    && crate::sop::doc::incremental::advance_base(&mut st, head.as_deref(), true)
                {
                    if let Err(e) = store_final.save(&mut st) {
                        tracing::warn!(component = "doc-engine", error = %e, "基线推进落盘失败");
                    } else {
                        tracing::info!(component = "doc-engine", base = ?head, "基线已推进");
                    }
                }
                tracing::info!(
                    component = "doc-engine",
                    units = ?report.units_done,
                    rounds = report.review_rounds,
                    issues = report.issues_total,
                    "doc-from-zero SOP 通过"
                );
                Ok(RunOutcome::Completed)
            }
            Err(e) => self.map_intervention(control, "doc-engine", e),
        }
    }
}
