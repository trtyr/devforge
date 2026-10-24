//! 内置 SOP 的生产引擎（ARC-R2 自 server/mod.rs 迁入）：按 task.sop 分发，spawn 真 pi 子进程。
//!
//! R3 边界声明：本文件依赖 `crate::server::{UiRelay, RelayUserLink}`（UI 中继暂住组装层）
//! 与 `crate::server::change_engine`（change 接线事实源）——依赖方向在 R5 横切收敛时再理。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::run::Run;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::domain::task::Task;
use crate::engine::client::RpcClient;
use crate::engine::process::SpawnSpec;
use crate::error::{Context as _, Result};
use crate::observability::watchdog::run_with_watchdog;
use crate::server::{RelayUserLink, UiRelay};
use crate::sop::coding::CodingSopLoop;
use crate::sop::doc_fix::DocFixSopLoop;
use crate::sop::planning::PlanningLoop;
use crate::sop::rpc_bridge::{RpcPlanSession, RpcReviewFactory};
use crate::sop::testing::{TestingOutcome, TestingSopLoop};

/// CodeGraph 索引预检（decisions/013）：工作区无 `.codegraph/` 则 init。
/// 引擎独占 init（官方教义：agent 不自行 init）；codegraph 不可用时降级为
/// tracing 警告，不阻塞 run——agent 侧按提示词优雅降级原生工具。
pub fn preflight_codegraph(workspace: &Path, codegraph_bin: &Path) {
    if workspace.join(".codegraph").exists() {
        return;
    }
    match std::process::Command::new(codegraph_bin)
        .arg("init")
        .arg(workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) if s.success() => tracing::info!(
            component = "coding-engine",
            workspace = %workspace.display(),
            "CodeGraph 索引预检完成"
        ),
        other => tracing::warn!(
            component = "coding-engine",
            workspace = %workspace.display(),
            result = ?other,
            "CodeGraph 预检未成功（agent 将降级原生工具）"
        ),
    }
}

/// 内置 SOP 的生产引擎：按 task.sop 分发（planning / coding / testing / doc-fix），spawn 真 pi 子进程。
pub struct BuiltinRpcEngine {
    pub profile_root: PathBuf,
    pub session_root: PathBuf,
    /// plans/<项目>/<任务>/ 根（P8-ISSUE-04：子进程工作目录 = plan root / 黑板）。
    pub plans_root: PathBuf,
    pub ui: Arc<UiRelay>,
    /// codegraph 可执行（测试注入假脚本用；默认 "codegraph"）。
    pub codegraph_bin: PathBuf,
    /// 失活收割阈值 ms（None = 不做活性收割，永不自动杀——零硬限制哲学；
    /// DEVFORGE_RUN_IDLE_MS 显式开启）。006 decisions/002：墙钟超时与失活收割分离。
    pub idle_ms: Option<u64>,
}

impl RunEngine for BuiltinRpcEngine {
    fn execute(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome> {
        match task.sop.as_str() {
            "planning" => self.run_planning(task, run, control),
            "coding" => self.run_coding(task, run, control),
            "testing" => self.run_testing(task, run, control),
            "doc-fix" => self.run_doc_fix(task, run, control),
            "doc-from-zero" => self.run_doc_from_zero(task, run, control),
        "change" => self.run_change(task, run, control),
            other => Err(crate::error::DevforgeError::System(format!(
                "SOP `{other}` 未接线编排器（注册表与引擎必须同步）"
            ))),
        }
    }
}

impl BuiltinRpcEngine {
    /// 每个 SOP 需要的 profile（**唯一事实源**：门卫与渲染闸门读同一张表——清单只有一份，不会漂移）。
    ///
    /// change 一项**引用** `change_engine::CHANGE_PROFILES`（那个文件不属本变更、不进白名单，
    /// 但事实源仍是它，不在这里另抄一份）。
    pub fn profiles_for_sop(sop: &str) -> &'static [&'static str] {
        match sop {
            "planning" => &["plan-agent", "plan-review"],
            "coding" => &["coding-agent", "coding-review"],
            "testing" => &["test-agent", "test-review"],
            "doc-from-zero" => &["doc-agent", "doc-review"],
            "doc-fix" => &["doc-fix", "doc-review"],
            "change" => &crate::server::change_engine::CHANGE_PROFILES,
            _ => &[],
        }
    }

    /// F1 修复（e2e F1-MISSING-PROFILE-HANG）：spawn 前校验目标 profile 已渲染——
    /// 目录存在且含 settings.json（渲染器必然写出的最小标记）。缺失 → 立即报错
    /// （run 将 Failed，错误指明缺失路径），杜绝真 pi 以空 agent-dir 启动后挂
    /// Running 占住全局串行位、只能 cancel 解锁的静默卡死。
    pub(crate) fn require_profile(&self, role: &str) -> Result<()> {
        let dir = self.profile_root.join(role);
        let marker = dir.join("settings.json");
        if !marker.is_file() {
            // 「profile 未就绪」是**配置问题**不是基础设施故障：重启不会让它变好
            // （`is_infra_failure` 只认裸 `System` → 这里不进入自动重启分支，010 D004 口径的修正）。
            // 错误**文本一字不改**（既有单测与 e2e 靠它指路）。
            return Err(crate::error::DevforgeError::Config(format!(
                "profile `{}` 未渲染（缺 {}）——先 PUT /api/v1/config/manifest 完成安装渲染，再启动 run",
                dir.display(),
                marker.display()
            )));
        }
        Ok(())
    }

    fn run_planning(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome> {
        // F1：spawn 前门卫（agent + review 双角色），缺失即快速失败
        for p in Self::profiles_for_sop("planning") {
            self.require_profile(p)?;
        }
        // P8-ISSUE-04 修复：子进程工作目录 = plans/<项目>/<任务>/（decisions/002）
        let working_dir = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&working_dir).ctx("创建 plan root 目录")?;

        let plan_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("plan-agent"),
                "plan-agent",
                run.id.0.clone(),
                self.session_root.join("plan"),
            )
            .with_working_dir(working_dir),
        )?;
        let plan = RpcPlanSession::new(plan_client);
        let factory = RpcReviewFactory::per_round(
            self.profile_root.join("plan-review"),
            self.session_root.join("reviews"),
        );
        let mut user = RelayUserLink {
            relay: self.ui.clone(),
        };

        // watchdog（P8-ISSUE-01）：取消置位或超时 → 按 run-id 精确 SIGKILL 自家 pi。
        // scope 借用外部 control 原件（非快照——cancel 端点置位必须实时可见）；
        // SOP 结束后在闭包内置 done，watchdog ≤300ms 内退出（修复：原实现 done 在
        // scope 返回后才置位，而 scope 要等 watchdog 退出——正常完成必死锁，
        // 第 18/19/20 三试 verdict 后挂起的真因）。
        let run_id = run.id.0.clone();
        let sop_result = run_with_watchdog(
            control,
            run_id.clone(),
            self.idle_ms,
            vec![self.session_root.join("plan"), self.session_root.join("reviews")],
            || {
                let mut sop_loop = PlanningLoop::new(plan, factory);
                sop_loop.run(&mut user)
            },
        );

        match sop_result {
            Ok(report) => {
                tracing::info!(
                    component = "planning-engine",
                    rounds = report.rounds,
                    issues = report.issues_total,
                    "规划 SOP 通过"
                );
                Ok(RunOutcome::Completed)
            }
            Err(e) => self.map_intervention(control, "planning-engine", e),
        }
    }

    /// 编码 SOP（P005）：黑板骨架 + CodeGraph 预检 + CodingSopLoop。
    fn run_coding(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome> {
        // F1：spawn 前门卫（agent + review 双角色），缺失即快速失败——先于一切副作用
        for p in Self::profiles_for_sop("coding") {
            self.require_profile(p)?;
        }
        if task.workspace.as_os_str().is_empty() {
            return Err(crate::error::DevforgeError::Store(format!(
                "任务 `{}/{}` 无代码工作区（项目登记的 local_path 为空），编码 SOP 需要项目地址",
                task.project, task.id
            )));
        }
        let blackboard = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&blackboard).ctx("创建黑板目录")?;
        // 黑板骨架（topics/shared-blackboard.md）：stages/todo 由 Agent 自建
        for d in [crate::sop::coding_protocol::CODING_DIR, crate::sop::coding_protocol::DOCS_DIR, crate::sop::coding_protocol::REPORTS_DIR, crate::sop::coding_protocol::BLOCKERS_DIR] {
            std::fs::create_dir_all(blackboard.join(d)).ctx("建黑板骨架")?;
        }

        // decisions/013：引擎预检 init（agent 不自行 init）
        preflight_codegraph(&task.workspace, &self.codegraph_bin);

        let agent_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("coding-agent"),
                "coding-agent",
                run.id.0.clone(),
                self.session_root.join("coding"),
            )
            .with_working_dir(blackboard.clone()),
        )?;
        let agent = RpcPlanSession::new(agent_client);
        let factory = RpcReviewFactory::per_round_in(
            self.profile_root.join("coding-review"),
            "coding-review",
            self.session_root.join("coding-reviews"),
            blackboard,
        );
        let mut user = RelayUserLink {
            relay: self.ui.clone(),
        };

        let run_id = run.id.0.clone();
        let workspace = task.workspace.clone();
        let project = task.project.clone();
        let task_id = task.id.clone();
        let sop_result = run_with_watchdog(
            control,
            run_id,
            self.idle_ms,
            vec![
                self.session_root.join("coding"),
                self.session_root.join("coding-reviews"),
            ],
            || {
                let mut sop_loop = CodingSopLoop::new(agent, factory);
                sop_loop.run(&mut user, &workspace, &project, &task_id)
            },
        );

        match sop_result {
            Ok(report) => {
                tracing::info!(
                    component = "coding-engine",
                    stages = ?report.stages_completed,
                    rounds = report.review_rounds,
                    issues = report.issues_total,
                    "编码 SOP 通过"
                );
                Ok(RunOutcome::Completed)
            }
            Err(e) => self.map_intervention(control, "coding-engine", e),
        }
    }

    /// testing run 的活性监视目录（含 doc-fix 移交会话——同 run 的活性整体，D011）。
    pub(crate) fn testing_watch_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.session_root.join("testing"),
            self.session_root.join("testing-reviews"),
            self.session_root.join("doc-fix"),
            self.session_root.join("doc-reviews"),
        ]
    }

    /// 独立 doc-fix run 的活性监视目录。
    pub(crate) fn docfix_watch_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.session_root.join("doc-fix"),
            self.session_root.join("doc-reviews"),
        ]
    }

    /// 测试 SOP（P007）：testing/ 骨架 + TestingSopLoop（doc-side 移交时编排 doc-fix 子 SOP，D004）。
    fn run_testing(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome> {
        // F1：spawn 前门卫（agent + review 双角色），缺失即快速失败——先于一切副作用
        for p in Self::profiles_for_sop("testing") {
            self.require_profile(p)?;
        }
        if task.workspace.as_os_str().is_empty() {
            return Err(crate::error::DevforgeError::Store(format!(
                "任务 `{}/{}` 无代码工作区，测试 SOP 需要项目地址（写测 + 起被测系统）",
                task.project, task.id
            )));
        }
        let blackboard = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&blackboard).ctx("创建黑板目录")?;
        crate::sop::testing_protocol::create_testing_skeleton(&blackboard).ctx("建 testing 骨架")?;
        // 无 codegraph 预检——测试 Agent 不配代码事实层（D007），不需要索引

        let agent_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("test-agent"),
                "test-agent",
                run.id.0.clone(),
                self.session_root.join("testing"),
            )
            .with_working_dir(blackboard.clone()),
        )?;
        let agent = RpcPlanSession::new(agent_client);
        let test_review_factory = RpcReviewFactory::per_round_in(
            self.profile_root.join("test-review"),
            "test-review",
            self.session_root.join("testing-reviews"),
            blackboard.clone(),
        );
        let mut user = RelayUserLink {
            relay: self.ui.clone(),
        };

        let run_id = run.id.0.clone();
        let workspace = task.workspace.clone();
        let project = task.project.clone();
        let task_id = task.id.clone();
        let docfix_profile = self.profile_root.join("doc-fix");
        let doc_review_profile = self.profile_root.join("doc-review");
        let docfix_sessions = self.session_root.join("doc-fix");
        let doc_review_sessions = self.session_root.join("doc-reviews");
        let watch_dirs = self.testing_watch_dirs();

        let sop_result = run_with_watchdog(
            control,
            run_id.clone(),
            self.idle_ms,
            watch_dirs,
            move || {
                let mut sop_loop = TestingSopLoop::new(agent, test_review_factory);
                let mut outcome = sop_loop.run(&mut user, &workspace, &project, &task_id)?;
                loop {
                    match outcome {
                        TestingOutcome::Completed(final_report) => return Ok(final_report),
                        TestingOutcome::DocSideHandoff { stage, issues, report } => {
                            let mut acc = report;
                            acc.doc_fix_handoffs += 1;
                            tracing::info!(
                                component = "testing-engine",
                                stage,
                                findings = ?issues,
                                "doc-side 判定 → 触发 doc-fix 子 SOP"
                            );
                            // D004：doc-fix 定点修补（新会话）+ doc-review 内置审查轮
                            let docfix_client = RpcClient::spawn(
                                SpawnSpec::new(
                                    docfix_profile.clone(),
                                    "doc-fix",
                                    run_id.clone(),
                                    docfix_sessions.clone(),
                                )
                                .with_working_dir(blackboard.clone()),
                            )?;
                            let docfix_agent = RpcPlanSession::new(docfix_client);
                            let doc_review_factory = RpcReviewFactory::per_round_in(
                                doc_review_profile.clone(),
                                "doc-review",
                                doc_review_sessions.clone(),
                                blackboard.clone(),
                            );
                            let mut fix_loop = DocFixSopLoop::new(docfix_agent, doc_review_factory);
                            let fix_report =
                                fix_loop.run(&mut user, &workspace, &project, &task_id)?;
                            tracing::info!(
                                component = "testing-engine",
                                rounds = fix_report.review_rounds,
                                issues = fix_report.issues_total,
                                "doc-fix 过审，恢复测试同阶段重审"
                            );
                            outcome = sop_loop.resume_after_docfix(
                                &mut user,
                                &workspace,
                                stage,
                                acc.clone(),
                            )?;
                        }
                    }
                }
            },
        );

        match sop_result {
            Ok(report) => {
                tracing::info!(
                    component = "testing-engine",
                    stages = ?report.stages_completed,
                    rounds = report.review_rounds,
                    issues = report.issues_total,
                    handoffs = report.doc_fix_handoffs,
                    "测试 SOP 通过"
                );
                Ok(RunOutcome::Completed)
            }
            Err(e) => self.map_intervention(control, "testing-engine", e),
        }
    }

    /// 文档修补 SOP（P007 D004）：testing/ 骨架 + DocFixSopLoop（doc-review 内置审查）。
    fn run_doc_fix(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome> {
        // F1：spawn 前门卫（agent + review 双角色），缺失即快速失败——先于一切副作用
        for p in Self::profiles_for_sop("doc-fix") {
            self.require_profile(p)?;
        }
        if task.workspace.as_os_str().is_empty() {
            return Err(crate::error::DevforgeError::Store(format!(
                "任务 `{}/{}` 无代码工作区，doc-fix triage 需要读代码核实事实",
                task.project, task.id
            )));
        }
        let blackboard = self.plans_root.join(&task.project).join(&task.id);
        std::fs::create_dir_all(&blackboard).ctx("创建黑板目录")?;
        crate::sop::testing_protocol::create_testing_skeleton(&blackboard).ctx("建 testing 骨架")?;

        let agent_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("doc-fix"),
                "doc-fix",
                run.id.0.clone(),
                self.session_root.join("doc-fix"),
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

        let run_id = run.id.0.clone();
        let workspace = task.workspace.clone();
        let project = task.project.clone();
        let task_id = task.id.clone();
        let watch_dirs = self.docfix_watch_dirs();

        let sop_result = run_with_watchdog(
            control,
            run_id,
            self.idle_ms,
            watch_dirs,
            move || {
                let mut sop_loop = DocFixSopLoop::new(agent, factory);
                sop_loop.run(&mut user, &workspace, &project, &task_id)
            },
        );

        match sop_result {
            Ok(report) => {
                tracing::info!(
                    component = "docfix-engine",
                    rounds = report.review_rounds,
                    issues = report.issues_total,
                    "文档修补 SOP 通过"
                );
                Ok(RunOutcome::Completed)
            }
            Err(e) => self.map_intervention(control, "docfix-engine", e),
        }
    }

    /// 干预性死亡映射为明确的 Failed 原因（而非底层 RPC EOF 噪音）。
    pub(crate) fn map_intervention(&self, control: &RunControl, component: &str, e: crate::error::DevforgeError) -> Result<RunOutcome> {
        if control.is_cancelled() {
            Ok(RunOutcome::Failed {
                reason: "已取消（cancel 端点）".into(),
            })
        } else if control.is_stalled() {
            Ok(RunOutcome::Failed {
                reason: "已失活（会话无进展超过闲置阈值，被 watchdog 终止）".into(),
            })
        } else if control.timed_out(crate::domain::now_ms()) {
            Ok(RunOutcome::Failed {
                reason: "已超时（run 超时，被 watchdog 终止）".into(),
            })
        } else if let crate::error::DevforgeError::Quality(msg) = &e {
            // 质量失败（审查打回/协议违例达上限/越界申报）：明确的 Failed，**不进自动重启**（010 D004）
            tracing::warn!(component, reason = %msg, "SOP 质量失败（不重试）");
            // 原始错误全量可见（010 F6 靶 · 需求 1）：质量类失败落 **WARN**，
            // 字段与重试层那条**同构**（`event` / `attempt` / `error` / `error_chain` / `retryable`）——
            // 质量失败在生产链路上到不了重试层（这里被转成 `Ok(Failed)`），不在这儿落就永远看不见。
            let error_chain = crate::observability::error_chain(&e);
            tracing::warn!(
                component,
                event = "run.attempt_failed",
                attempt = crate::observability::attempt(),
                error = %e,
                error_chain = %error_chain,
                retryable = false,
                "SOP 质量失败（原始错误已落盘，不重启）"
            );
            Ok(RunOutcome::Failed {
                reason: msg.clone(),
            })
        } else {
            tracing::warn!(component, error = %e, "SOP 非干预性失败");
            Err(e)
        }
    }
}
