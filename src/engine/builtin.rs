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
use crate::sop::doc::fix::DocFixSopLoop;
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
    // 引擎审计修复 #10：init 不再裸 status()（无超时永久占坑）+ stderr 落 tracing（挂死可诊断）
    let mut child = match std::process::Command::new(codegraph_bin)
        .arg("init")
        .arg(workspace)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                component = "coding-engine",
                bin = %codegraph_bin.display(),
                error = %e,
                "codegraph init spawn 失败，跳过预检（agent 侧优雅降级原生工具）"
            );
            return;
        }
    };
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::warn!(component = "coding-engine", "codegraph init: {line}");
            }
        });
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err("init 超过 10s 未退出，已强杀（挂死不再永久占坑）".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => break Err(format!("wait 失败：{e}")),
        }
    };
    match outcome {
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
        "code-review" => {
            // F1：spawn 前门卫，缺失即快速失败——先于一切副作用（与其他 SOP 同口径，审计修复 #5）
            for p in Self::profiles_for_sop("code-review") {
                self.require_profile(p)?;
            }
            // 第三方 r2 M2：空工作区门卫 + codegraph 预检——两侧其余代码类 SOP
            // （coding/testing/doc-fix/doc-from-zero/change）都做了，本支漏做：
            // 无工作区时切片器盘出零单元（带 m1 后会报「无可审源码」而非“成功”），
            // 但报错语义错位；而 V5 一致性视角依赖 codegraph，索引不在则只能拿旧图审。
            if task.workspace.as_os_str().is_empty() {
                return Err(crate::error::DevforgeError::Store(format!(
                    "任务 `{}/{}` 无代码工作区，code-review 需要项目地址（切片与审查都从仓读源码）",
                    task.project, task.id
                )));
            }
            preflight_codegraph(&task.workspace, &self.codegraph_bin);
            // P013 M1：四阶段审计（盘点切片→规划→批次审查→跨单元→核验→报告）。
            // 会话桥：RpcReviewFactory（code-review profile，会话落 session_root/code-review）。
            // 审计修复 #5：与其他 SOP 同口径——run_with_watchdog 包住整次审计
            // （cancel 端点/墙钟超时/idle 收割全部生效），干预性死亡经 map_intervention
            // 给出明确 Failed 原因；不再裸调 run_audit。
            // 引擎批次 #4：会话存档按 run-id 隔离——README 一直声称按 run-id 组织，
            // 实现却是按角色共享（sessions/code-review），两个 run 的存档互相覆盖。
            let sessions = self.session_root.join(&run.id.0);
            let factory = crate::sop::rpc_bridge::RpcReviewFactory::per_round_in(
                self.profile_root.join("code-review"),
                "code-review",
                sessions.join("code-review"),
                self.plans_root.join(&task.project).join(&task.id),
                // 自审 major：台账显式落 project 层。
                self.plans_root.join(&task.project).join("spawn-ledger.json"),
            );
            let plans_root = self.plans_root.clone();
            let watch_dirs = vec![sessions.join("code-review")];
            // 自审 minor（175）：看门狗强杀走台账精确路径（旧口径恒 None = 全机扫盘）
            let ledger = self.plans_root.join(&task.project).join("spawn-ledger.json");
            let sop_result = run_with_watchdog(
                control,
                run.id.0.clone(),
                // 全仓审计 M4：code-review 长跑批处理——未显式设阈值时给 15 分钟默认值，
                // 否则挂死的批永远不被收割（M3 的 14h 挂死正是这条路径）。
                self.idle_ms.or(Some(
                    crate::audit::runner::DEFAULT_AUDIT_IDLE_MS,
                )),
                watch_dirs,
                Some(ledger),
                Some(self.unblock_hook()),
                move || {
                    crate::audit::runner::run_audit(
                        task,
                        &plans_root,
                        control,
                        &mut crate::audit::pipeline::AuditSessionFactoryBridge(factory),
                    )
                },
            );
            match sop_result {
                Ok(outcome) => Ok(outcome),
                Err(e) => crate::engine::policy::map_intervention(control, "audit-engine", e),
            }
        }
            other => Err(crate::error::DevforgeError::System(format!(
                "SOP `{other}` 未接线编排器（注册表与引擎必须同步）"
            ))),
        }
    }
}

impl BuiltinRpcEngine {
    /// 自审 critical：看门狗干预路径（取消/墙钟超时/失活收割）解阻本 run 待答的钩子——
    /// 只杀 pi 子进程解不了阻塞在 ask_blocking recv() 上的 run 线程，串行槽会占死。
    pub(crate) fn unblock_hook(&self) -> crate::observability::watchdog::UnblockHook {
        let ui = self.ui.clone();
        std::sync::Arc::new(move |run_id: &str| {
            ui.abort_run(run_id);
        })
    }

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
        "code-review" => &["code-review"],
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

        // 引擎批次 #4：本 run 的会话根（与 README 口径一致：按 run-id 归档）
        let sessions = self.session_root.join(&run.id.0);
        let ledger_path = self
            .plans_root
            .join(&task.project)
            .join("spawn-ledger.json");
        let plan_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("plan-agent"),
                "plan-agent",
                run.id.0.clone(),
                sessions.join("plan"),
            )
            .with_working_dir(working_dir.clone())
            // 自审 major：台账显式落 project 层。
            .with_ledger_path(ledger_path.clone()),
        )?;
        let plan = RpcPlanSession::new(plan_client);
        // 2026-10-27 自审 major：规划审查的 cwd 必须是 plan root——旧口径 per_round
        // 让审查子进程落在服务器 cwd，实际审的不是 plan-agent 产出的那棵树
        // （与其他 SOP 的 per_round_in 口径对齐：cwd = 被审产物所在目录）。
        let factory = RpcReviewFactory::per_round_in(
            self.profile_root.join("plan-review"),
            "plan-review",
            sessions.join("reviews"),
            working_dir,
            ledger_path,
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
        let ledger = self.plans_root.join(&task.project).join("spawn-ledger.json");
        let sop_result = run_with_watchdog(
            control,
            run_id.clone(),
            self.idle_ms,
            vec![sessions.join("plan"), sessions.join("reviews")],
            Some(ledger),
            Some(self.unblock_hook()),
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
            Err(e) => crate::engine::policy::map_intervention(control, "planning-engine", e),
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

        let sessions = self.session_root.join(&run.id.0);
        let agent_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("coding-agent"),
                "coding-agent",
                run.id.0.clone(),
                sessions.join("coding"),
            )
            .with_working_dir(blackboard.clone())
            // 自审 major：MVP coding 的 blackboard 也是 task 层，台账显式落 project 层。
            .with_ledger_path(
                self.plans_root
                    .join(&task.project)
                    .join("spawn-ledger.json"),
            ),
        )?;
        let agent = RpcPlanSession::new(agent_client);
        let factory = RpcReviewFactory::per_round_in(
            self.profile_root.join("coding-review"),
            "coding-review",
            sessions.join("coding-reviews"),
            blackboard,
            self.plans_root
                .join(&task.project)
                .join("spawn-ledger.json"),
        );
        let mut user = RelayUserLink {
            relay: self.ui.clone(),
        };

        let run_id = run.id.0.clone();
        let workspace = task.workspace.clone();
        let project = task.project.clone();
        let task_id = task.id.clone();
        let ledger = self.plans_root.join(&task.project).join("spawn-ledger.json");
        let sop_result = run_with_watchdog(
            control,
            run_id,
            self.idle_ms,
            vec![sessions.join("coding"), sessions.join("coding-reviews")],
            Some(ledger),
            Some(self.unblock_hook()),
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
            Err(e) => crate::engine::policy::map_intervention(control, "coding-engine", e),
        }
    }

    /// testing run 的活性监视目录（含 doc-fix 移交会话——同 run 的活性整体，D011）。
    /// 引擎批次 #4：会话按 run-id 隔离后，监视目录必须按同一个 run 算。
    pub(crate) fn testing_watch_dirs(&self, run_id: &str) -> Vec<PathBuf> {
        let s = self.session_root.join(run_id);
        vec![
            s.join("testing"),
            s.join("testing-reviews"),
            s.join("doc-fix"),
            s.join("doc-reviews"),
        ]
    }

    /// 独立 doc-fix run 的活性监视目录。
    pub(crate) fn docfix_watch_dirs(&self, run_id: &str) -> Vec<PathBuf> {
        let s = self.session_root.join(run_id);
        vec![s.join("doc-fix"), s.join("doc-reviews")]
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

        let sessions = self.session_root.join(&run.id.0);
        let agent_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("test-agent"),
                "test-agent",
                run.id.0.clone(),
                sessions.join("testing"),
            )
            .with_working_dir(blackboard.clone())
            .with_ledger_path(
                self.plans_root
                    .join(&task.project)
                    .join("spawn-ledger.json"),
            ),
        )?;
        let agent = RpcPlanSession::new(agent_client);
        let test_review_factory = RpcReviewFactory::per_round_in(
            self.profile_root.join("test-review"),
            "test-review",
            sessions.join("testing-reviews"),
            blackboard.clone(),
            self.plans_root
                .join(&task.project)
                .join("spawn-ledger.json"),
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
        let docfix_sessions = sessions.join("doc-fix");
        let doc_review_sessions = sessions.join("doc-reviews");
        let watch_dirs = self.testing_watch_dirs(&run.id.0);
        // 自审 major：doc-fix 交接的 spawn 台账也必须落 project 层（闭包里拿不到 task）。
        let ledger_path_in_scope = self
            .plans_root
            .join(&task.project)
            .join("spawn-ledger.json");

        let sop_result = run_with_watchdog(
            control,
            run_id.clone(),
            self.idle_ms,
            watch_dirs,
            Some(ledger_path_in_scope.clone()),
            Some(self.unblock_hook()),
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
                            // 引擎批次 #5 之二：交接前先验 profile 已渲染——testing 入口只验了
                            // 自己那两个 profile；doc-fix/doc-review 缺 settings.json 时真 pi 会以空
                            // agent-dir 启动并挂死（与 e2e F1 同一形态）。纯路径检查，不需要 self。
                            for (role, dir) in [
                                ("doc-fix", &docfix_profile),
                                ("doc-review", &doc_review_profile),
                            ] {
                                let marker = dir.join("settings.json");
                                if !marker.is_file() {
                                    return Err(crate::error::DevforgeError::Config(format!(
                                        "profile `{role}` 未渲染（缺 {}）——testing→doc-fix 交接中止；先 PUT /api/v1/config/manifest 完成渲染",
                                        marker.display()
                                    )));
                                }
                            }
                            // D004：doc-fix 定点修补（新会话）+ doc-review 内置审查轮
                            let docfix_client = RpcClient::spawn(
                                SpawnSpec::new(
                                    docfix_profile.clone(),
                                    "doc-fix",
                                    run_id.clone(),
                                    docfix_sessions.clone(),
                                )
                                .with_working_dir(blackboard.clone())
                                // 自审 major：台账显式落 project 层（run_id 被 move 前先取 project）。
                                .with_ledger_path(ledger_path_in_scope.clone()),
                            )?;
                            let docfix_agent = RpcPlanSession::new(docfix_client);
                            let doc_review_factory = RpcReviewFactory::per_round_in(
                                doc_review_profile.clone(),
                                "doc-review",
                                doc_review_sessions.clone(),
                                blackboard.clone(),
                                ledger_path_in_scope.clone(),
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
            Err(e) => crate::engine::policy::map_intervention(control, "testing-engine", e),
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

        let sessions = self.session_root.join(&run.id.0);
        let agent_client = RpcClient::spawn(
            SpawnSpec::new(
                self.profile_root.join("doc-fix"),
                "doc-fix",
                run.id.0.clone(),
                sessions.join("doc-fix"),
            )
            .with_working_dir(blackboard.clone())
            .with_ledger_path(
                self.plans_root
                    .join(&task.project)
                    .join("spawn-ledger.json"),
            ),
        )?;
        let agent = RpcPlanSession::new(agent_client);
        let factory = RpcReviewFactory::per_round_in(
            self.profile_root.join("doc-review"),
            "doc-review",
            sessions.join("doc-reviews"),
            blackboard,
            self.plans_root
                .join(&task.project)
                .join("spawn-ledger.json"),
        );
        let mut user = RelayUserLink {
            relay: self.ui.clone(),
        };

        let run_id = run.id.0.clone();
        let workspace = task.workspace.clone();
        let project = task.project.clone();
        let task_id = task.id.clone();
        let watch_dirs = self.docfix_watch_dirs(&run.id.0);

        let sop_result = run_with_watchdog(
            control,
            run_id,
            self.idle_ms,
            watch_dirs,
            Some(self.plans_root.join(&task.project).join("spawn-ledger.json")),
            Some(self.unblock_hook()),
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
            Err(e) => crate::engine::policy::map_intervention(control, "docfix-engine", e),
        }
    }

// 干预性死亡映射（map_intervention）已收敛至 engine/policy.rs（ARC-R5 横切策略单点化）
}
