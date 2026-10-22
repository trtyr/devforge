//! API 服务器（P003 · 服务器形态，非 CLI）。
//!
//! 协议定案（P003 decisions/001）：REST（编排控制/产物/配置/中继输入）+ SSE（事件推送），
//! 路径前缀 `/api/v1` 从第一天；默认绑 127.0.0.1，token 预留位。
//!
//! 五能力域：编排控制 · 产物管理 · 交互中继 · 可观测 · 配置管理。

pub mod change_engine;
pub mod handlers;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use serde_json::json;

use crate::config::installer::PackageInstaller;
use crate::config::ConfigManifest;
use crate::domain::sop::SopRegistry;
use crate::domain::store::Store;
use crate::domain::task::Task;
use crate::domain::run::Run;
use crate::domain::runner::{RunControl, RunEngine, RunOutcome};
use crate::engine::client::RpcClient;
use crate::engine::process::SpawnSpec;
use crate::error::{Context as _, Result};
use crate::sop::coding::CodingSopLoop;
use crate::sop::doc_fix::DocFixSopLoop;
use crate::sop::doc_from_zero::{DOCS_DIR, META_DIR, DocSopLoop};
use crate::sop::planning::{PlanningLoop, ReviewSessionFactory, UserLink};
use crate::sop::rpc_bridge::{RpcPlanSession, RpcReviewFactory};
use crate::sop::testing::{TestingOutcome, TestingSopLoop};

// ---------- 事件总线（可观测域） ----------

/// 单一事件源：序号、环形缓冲、广播三合一。
/// 支撑 SSE 断线续传（Last-Event-ID）：emit 时分配单调序号并留档，
/// 重连客户端带 `Last-Event-ID` 即可回放错过的事件（缓冲窗口内恰一次）。
pub struct EventBus {
    seq: std::sync::atomic::AtomicU64,
    log: Mutex<std::collections::VecDeque<(u64, String)>>,
    tx: tokio::sync::broadcast::Sender<(u64, String)>,
}

impl EventBus {
    const CAP: usize = 1024;

    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(Self::CAP);
        Self {
            seq: std::sync::atomic::AtomicU64::new(0),
            log: Mutex::new(std::collections::VecDeque::with_capacity(Self::CAP)),
            tx,
        }
    }

    /// 分配序号、留档、广播（无订阅者时广播失败被忽略——留档仍可供回放）。
    pub fn emit(&self, value: serde_json::Value) {
        use std::sync::atomic::Ordering;
        let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let data = value.to_string();
        {
            let mut log = self.log.lock().expect("event log 锁");
            log.push_back((id, data.clone()));
            while log.len() > Self::CAP {
                log.pop_front();
            }
        }
        let _ = self.tx.send((id, data));
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<(u64, String)> {
        self.tx.subscribe()
    }

    /// 回放 `last_id` 之后（不含）的全部留档事件。
    pub fn replay_after(&self, last_id: u64) -> Vec<(u64, String)> {
        self.log
            .lock()
            .expect("event log 锁")
            .iter()
            .filter(|(id, _)| *id > last_id)
            .cloned()
            .collect()
    }

    pub fn last_id(&self) -> u64 {
        self.seq
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- UI 中继（交互中继域） ----------

/// ask-user-question 的 API 中继：请求推 SSE，应答经 POST 端点回填。
pub struct UiRelay {
    next_id: AtomicU64,
    bus: std::sync::Arc<EventBus>,
    pending: Mutex<HashMap<String, std::sync::mpsc::Sender<String>>>,
}

impl UiRelay {
    /// 事件通道：ui.request 与 run 事件同流（单一 SSE 通道，Q002 信封设计）。
    pub fn new(bus: std::sync::Arc<EventBus>) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            bus,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// 阻塞等待用户应答（在引擎的同步线程里调用）。
    pub fn ask_blocking(&self, question: &str, options: &[&str]) -> Result<String> {
        let id = format!("ui-{}", self.next_id.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = std::sync::mpsc::channel();
        self.pending
            .lock()
            .expect("pending 锁")
            .insert(id.clone(), tx);
        // 先登记后广播：应答端点在事件可见后仍能命中 pending
        self.bus.emit(json!({
            "type": "ui.request",
            "id": id,
            "question": question,
            "options": options
        }));
        rx.recv().map_err(|e| {
            crate::error::DevforgeError::System(format!("等待 UI 应答: {e}"))
        })
    }

    /// 回填应答（POST /ui/{id}/answer 调用）。
    pub fn answer(&self, id: &str, value: impl Into<String>) -> Result<()> {
        let tx = self
            .pending
            .lock()
            .expect("pending 锁")
            .remove(id)
            .ok_or_else(|| {
                crate::error::DevforgeError::Store(format!("无此待答 UI 请求 `{id}`"))
            })?;
        tx.send(value.into()).map_err(|e| {
            crate::error::DevforgeError::System(format!("回传 UI 应答: {e}"))
        })
    }

    /// 待答请求数（测试用）。
    pub fn pending_count(&self) -> usize {
        self.pending.lock().expect("pending 锁").len()
    }

    /// 中止全部待答（cancel 端点解阻用）：丢弃 sender 使所有阻塞中的
    /// `ask_blocking` 立刻失败解阻，引擎得以按 cancel 语义收场——
    /// 修复 dialog 挂起态 cancel 半效（杀 pi 不解阻 mpsc recv）。
    pub fn abort_all(&self) -> usize {
        let mut map = self.pending.lock().expect("pending 锁");
        let n = map.len();
        map.clear();
        n
    }
}

/// sop::UserLink 的中继实现（引擎线程内使用）。
pub struct RelayUserLink {
    pub relay: Arc<UiRelay>,
}

impl UserLink for RelayUserLink {
    fn ask(&mut self, question: &str, options: &[&str]) -> Result<String> {
        self.relay.ask_blocking(question, options)
    }
}

// ---------- 生产引擎：内置 SOP 的 RPC 接线 ----------

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
    fn require_profile(&self, role: &str) -> Result<()> {
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

    /// doc-from-zero SOP（P008）：门 0（清单 + 闭包 + 单元）→ DocSopLoop 逐单元滚动。
    fn run_doc_from_zero(&mut self, task: &Task, run: &Run, control: &RunControl) -> Result<RunOutcome> {
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

        // 断点与续跑（plan 010）：状态住黑板（不进产物目录）；done 证据 = 产出文件在 + lint 绿。
        let resume_index = audit_index.clone();
        let resume = crate::sop::doc_resume::DocResume::new(workspace.join(DOCS_DIR)).with_lint(
            move |docs| match crate::docgen::lint::lint_docs_with_entities(docs, &resume_index) {
                Ok(r) => r.violations().is_empty(),
                Err(_) => false,
            },
        );
        let store = crate::runstate::StateStore::new(
            self.plans_root.clone(),
            crate::runstate::StateKey::new(&task.project, &task.id, crate::sop::doc_resume::DOC_SOP),
        );
        let loaded = store.load();
        // 供闭包之后推进基线用（store 会被 DocSopLoop 吃掉）
        let store_final = crate::runstate::StateStore::new(
            self.plans_root.clone(),
            crate::runstate::StateKey::new(&task.project, &task.id, crate::sop::doc_resume::DOC_SOP),
        );
        let head_now = crate::sop::doc_incremental::head_sha(&workspace);
        let (inc_on, req_base) = crate::sop::doc_incremental::enabled_from_env();
        let mode = crate::sop::doc_incremental::decide_mode(
            loaded.state(),
            head_now.as_deref(),
            req_base.as_deref(),
            inc_on,
        );
        if inc_on && let crate::sop::doc_incremental::Mode::Full { reason } = &mode {
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
        if let crate::sop::doc_incremental::Mode::Incremental { base } = &mode {
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
                                run_units = crate::sop::doc_incremental::merge_plan(
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
        let only = std::env::var("DEVFORGE_DOC_ONLY")
            .ok()
            .filter(|s| !s.trim().is_empty());
        let run_id = run.id.0.clone();
        // 终审（t11）：同一 profile、新实例；捕获所需路径（闭包外先克隆）
        let final_review_root = self.profile_root.join("doc-review");
        let final_session_root = self.session_root.join("doc-reviews");
        let final_blackboard = self.plans_root.join(&task.project).join(&task.id);
        let final_ws = workspace.clone();
        let final_inv = inv.clone();
        let final_run_id = run.id.0.clone();
        let sop_result = run_with_watchdog(
            control,
            run_id,
            self.idle_ms,
            vec![self.session_root.join("doc"), self.session_root.join("doc-reviews")],
            || {
                let mut sop_loop = DocSopLoop::new(agent, factory, store)
                    .with_audit(audit)
                    .with_resume_note(resume_note);
                if let Some(unit) = only {
                    sop_loop = sop_loop.with_only(unit);
                }
                let report = sop_loop.run(&mut user, &workspace, &run_units, "complete")?;

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
                    crate::sop::doc_incremental::consistency_notes(&docs, &final_ws);
                if !consistency.is_empty() {
                    full.push_str("\n一致性校验（引擎机械检查，陈旧引用要问审查官是否 FAIL）：\n");
                    for c in &consistency {
                        full.push_str(&format!("- {c}\n"));
                    }
                }

                let mut final_factory =
                    RpcReviewFactory::per_round_in(final_review_root, "doc-review", final_session_root, final_blackboard);
                let mut session = final_factory.open()?;
                let final_text = session.review(&crate::sop::doc_from_zero::doc_final_instruction(&full))?;
                drop(session);
                let verdict = crate::sop::doc_protocol::parse_doc_review(&final_text)?;

                // 运行账单（T021 验收）：墙钟 / 单元数 / 模块数 / 审查轮次 / token 与花费
                // token 不从 Agent 自述拿，而是扫写方与审方会话（pi 自己写的原始账）。
                let usage = crate::docgen::usage::from_session_dirs(&[
                    self.session_root.join("doc"),
                    self.session_root.join("doc-reviews"),
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
                    crate::sop::doc_protocol::DocVerdict::Pass => Ok(report),
                    crate::sop::doc_protocol::DocVerdict::Fail => {
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
                let head = crate::sop::doc_incremental::head_sha(&workspace);
                if let crate::runstate::Loaded::Found(mut st) = store_final.load()
                    && crate::sop::doc_incremental::advance_base(&mut st, head.as_deref(), true)
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

    /// testing run 的活性监视目录（含 doc-fix 移交会话——同 run 的活性整体，D011）。
    fn testing_watch_dirs(&self) -> Vec<PathBuf> {
        vec![
            self.session_root.join("testing"),
            self.session_root.join("testing-reviews"),
            self.session_root.join("doc-fix"),
            self.session_root.join("doc-reviews"),
        ]
    }

    /// 独立 doc-fix run 的活性监视目录。
    fn docfix_watch_dirs(&self) -> Vec<PathBuf> {
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
    fn map_intervention(&self, control: &RunControl, component: &str, e: crate::error::DevforgeError) -> Result<RunOutcome> {
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

// ---------- 服务器状态与路由 ----------

/// 渲染闸门（用户裁决 A）：清单重渲染在途时，`start` 端点等「本 run 所需 profile 的
/// `settings.json` 落盘」或「渲染结束」再开跑。
///
/// - 事实源：闸门与门卫读**同一张** per-SOP profile 表（[`BuiltinRpcEngine::profiles_for_sop`]），
///   清单只有一份，不会漂移；
/// - 等待是 **awaitable** 的（watch 通知 + 短间隔复查），**不占** tokio worker
///   （不得用 `std::thread::sleep` 轮询）；
/// - 闸门只等「所需 profile 的就绪」，**不等整轮渲染**（等整轮会把 `start` 变成分钟级阻塞点）。
pub struct RenderGate {
    /// 在途渲染数（>0 = 有渲染在途）。
    in_flight: Mutex<usize>,
    /// 渲染完成通知（值 = 已完成代数；每次完成自增 → 订阅者的 `changed()` 会醒）。
    notify: tokio::sync::watch::Sender<u64>,
}

impl RenderGate {
    pub fn new() -> Self {
        let (tx, _rx) = tokio::sync::watch::channel(0u64);
        Self {
            in_flight: Mutex::new(0),
            notify: tx,
        }
    }

    /// 登记一次在途渲染（`PUT /config/manifest` 受理后调用）。
    pub fn begin_render(&self) {
        let mut n = self.in_flight.lock().expect("render gate 锁");
        *n += 1;
    }

    /// 结束一次渲染（成功/失败都算）并广播。
    pub fn end_render(&self) {
        {
            let mut n = self.in_flight.lock().expect("render gate 锁");
            *n = n.saturating_sub(1);
        }
        self.notify.send_modify(|v| *v += 1);
    }

    /// 是否有渲染在途。
    pub fn is_in_flight(&self) -> bool {
        *self.in_flight.lock().expect("render gate 锁") > 0
    }

    /// 等「所需 profile 的 `settings.json` 落盘」或「渲染结束」。
    ///
    /// 返回即代表「可以开跑」：所需 profile 已就绪（后续门卫复查会过），
    /// 或渲染已结束（缺 profile 则由门卫明确失败，不再盲重启）。
    pub async fn wait_for_profiles(&self, profiles_root: &Path, required: &[&str]) {
        if required.is_empty() {
            return;
        }
        let mut rx = self.notify.subscribe();
        loop {
            if !self.is_in_flight() {
                return; // 渲染不在途 → 直接开跑
            }
            if required
                .iter()
                .all(|p| profiles_root.join(p).join("settings.json").is_file())
            {
                return; // 所需 profile 已就绪 → 不等整轮渲染
            }
            // 等「渲染结束」信号，或短间隔后重查就绪状态
            // （awaitable：不占 tokio worker；不用 std::thread::sleep 轮询）
            tokio::select! {
                _ = rx.changed() => return,
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        }
    }
}

impl Default for RenderGate {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedState = Arc<ServerState>;

pub struct ServerState {
    pub store: Mutex<Store>,
    pub registry: SopRegistry,
    pub manifest: Mutex<ConfigManifest>,
    /// 清单真源落盘路径（PUT 时同步写盘；None = 仅内存）。Mutex 以便测试注入。
    pub manifest_path: Mutex<Option<PathBuf>>,
    /// 每 run 引擎工厂（decisions/003：可多实例化的单 run 引擎）。
    pub engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
    /// profile 安装器（生产 = PiInstallBridge；测试注入 Fake——单测不走真实网络）。
    pub installer: Arc<dyn PackageInstaller + Send + Sync>,
    /// 运行中 run 的干预面（P8-ISSUE-01：cancel 端点置位，watchdog 杀进程）。
    pub runs_control: Mutex<HashMap<String, Arc<RunControl>>>,
    /// 默认 run 超时 ms（None = 不限时，零硬限制哲学；可由 env DEVFORGE_RUN_TIMEOUT_MS 开启）。
    pub run_timeout_ms: Option<u64>,
    /// 事件总线：序号 + 环形缓冲 + 广播（SSE 源与 Last-Event-ID 回放）。
    pub bus: std::sync::Arc<EventBus>,
    pub ui: Arc<UiRelay>,
    /// 鉴权预留位：Some(token) 时要求 `x-devforge-token` 头匹配（Q003）。
    pub token: Option<String>,
    pub profiles_root: PathBuf,
    pub plans_root: PathBuf,
    pub sessions_root: PathBuf,
    /// 运行级重试策略（plan 010 T005）：生产默认 10/30/90s；测试注入 fast。
    pub retry_policy: crate::engine::retry::RetryPolicy,
    /// 渲染闸门（用户裁决 A）：`PUT /config/manifest` 在途时，`start` 端点等所需 profile 就绪。
    pub render_gate: RenderGate,
}

impl ServerState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: Store,
        registry: SopRegistry,
        manifest: ConfigManifest,
        engine_factory: Box<dyn Fn() -> Box<dyn RunEngine + Send> + Send + Sync>,
        bus: std::sync::Arc<EventBus>,
        ui: Arc<UiRelay>,
        installer: Arc<dyn PackageInstaller + Send + Sync>,
        profiles_root: PathBuf,
        plans_root: PathBuf,
        sessions_root: PathBuf,
    ) -> SharedState {
        Arc::new(ServerState {
            store: Mutex::new(store),
            registry,
            manifest: Mutex::new(manifest),
            manifest_path: Mutex::new(None),
            engine_factory,
            bus,
            ui,
            installer,
            runs_control: Mutex::new(HashMap::new()),
            run_timeout_ms: None,
            token: None,
            profiles_root,
            plans_root,
            sessions_root,
            retry_policy: crate::engine::retry::RetryPolicy::default(),
            render_gate: RenderGate::new(),
        })
    }

    pub fn emit(&self, value: serde_json::Value) {
        self.bus.emit(value);
    }
}

// ---------- 启动入口：token 配置解析（P3：env + CLI 双入口） ----------

/// 从启动参数解析 `--token <v>` / `--token=<v>`（首个命中生效；无则 None）。
pub fn parse_token_flag(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if let Some(v) = a.strip_prefix("--token=") {
            return Some(v.to_string());
        }
        if a == "--token" {
            return iter.next().cloned();
        }
    }
    None
}

/// 启动 token 解析：CLI > env > 无；空串视为未设置（防空 token 锁死）。
pub fn resolve_startup_token(cli: Option<String>, env: Option<String>) -> Option<String> {
    let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
    cli.and_then(non_empty).or_else(|| env.and_then(non_empty))
}

#[cfg(test)]
mod startup_token_tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn token_flag_space_form() {
        assert_eq!(parse_token_flag(&args(&["--token", "abc"])), Some("abc".into()));
    }

    #[test]
    fn token_flag_eq_form() {
        assert_eq!(parse_token_flag(&args(&["--token=xyz"])), Some("xyz".into()));
    }

    #[test]
    fn token_flag_absent() {
        assert_eq!(parse_token_flag(&args(&["--port", "1"])), None);
        assert_eq!(parse_token_flag(&[]), None);
    }

    #[test]
    fn token_flag_dangling_value_is_none() {
        // `--token` 无后继值 → None（不 panic）
        assert_eq!(parse_token_flag(&args(&["--token"])), None);
    }

    #[test]
    fn startup_token_cli_wins_over_env() {
        assert_eq!(
            resolve_startup_token(Some("cli".into()), Some("env".into())),
            Some("cli".into())
        );
    }

    #[test]
    fn startup_token_env_used_when_no_cli() {
        assert_eq!(
            resolve_startup_token(None, Some("env".into())),
            Some("env".into())
        );
    }

    #[test]
    fn startup_token_empty_is_unset() {
        // 空串/纯空白视为未设置——防空 token 把服务锁死
        assert_eq!(resolve_startup_token(Some("  ".into()), None), None);
        assert_eq!(resolve_startup_token(None, Some("".into())), None);
        assert_eq!(resolve_startup_token(None, None), None);
    }
}

pub fn build_router(state: SharedState) -> Router {
    let api = Router::new()
        // 编排控制
        .route("/projects", post(handlers::create_project).get(handlers::list_projects))
        .route("/projects/{name}", get(handlers::get_project))
        .route(
            "/projects/{name}/tasks",
            post(handlers::create_task).get(handlers::list_tasks),
        )
        .route("/projects/{name}/tasks/{task}/start", post(handlers::start_task))
        .route(
            "/projects/{name}/tasks/{task}/cancel",
            post(handlers::cancel_task),
        )
        // 产物管理 + 可观测
        .route("/projects/{name}/tasks/{task}/runs", get(handlers::list_runs))
        .route("/projects/{name}/tasks/{task}/graph", get(handlers::task_graph))
        // 交互中继
        .route("/ui/{id}/answer", post(handlers::answer_ui))
        .route("/events", get(handlers::events))
        // 配置管理
        .route(
            "/config/manifest",
            get(handlers::get_manifest).put(handlers::put_manifest),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handlers::auth_middleware,
        ));
    Router::new().nest("/api/v1", api).with_state(state)
}

/// 生产引导：绑定地址并常驻服务（main 调用）。
pub async fn serve(state: SharedState, addr: std::net::SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(component = "server", %addr, "devforge API 就绪");
    axum::serve(listener, build_router(state)).await
}

// ---------- 后台执行（编排控制的 start 端点用） ----------

/// 台账条目直通（server 侧记录用）。
pub type EngineBox = Box<dyn RunEngine + Send>;

/// 在独立线程跑一个 run（不持锁执行引擎——Store 锁只在 begin/finish 短暂持有）。
pub fn run_in_background(
    state: SharedState,
    project: String,
    task_id: String,
    mut engine: EngineBox,
) {
    let (run, task_snapshot) = {
        let mut store = state.store.lock().expect("store 锁");
        match store.begin_run(&project, &task_id) {
            Ok(run) => {
                let task = store
                    .get_task(&project, &task_id)
                    .cloned()
                    .expect("begin_run 已校验存在");
                (run, task)
            }
            Err(e) => {
                state.emit(json!({"type": "run.error", "project": project, "task": task_id, "error": e.to_string()}));
                return;
            }
        }
    };
    state.emit(json!({"type": "run.started", "project": project, "task": task_id, "run_id": run.id.0}));

    // 干预面登记（P8-ISSUE-01）：cancel 端点置位 / 可选超时，watchdog 杀自家 pi
    let mut control = RunControl::new();
    if let Some(timeout) = state.run_timeout_ms {
        control = control.with_deadline(crate::domain::now_ms() + timeout);
    }
    let control = Arc::new(control);
    state
        .runs_control
        .lock()
        .expect("runs_control 锁")
        .insert(format!("{project}/{task_id}"), control.clone());

    // 基础设施故障自动重启（plan 010 T005/D004）：只重 System 类，质量失败不重试
    let policy = state.retry_policy.clone();
    let sop_for_state = task_snapshot.sop.clone();
    let state_for_retry = state.clone();
    let (project_for_retry, task_for_retry, run_id_for_retry) =
        (project.clone(), task_id.clone(), run.id.0.clone());
    let (outcome, attempts) = crate::engine::retry::run_with_retry(&policy, &control, |attempt| {
        if attempt > 1 {
            // attempt 递增落盘（审计用；断点本体由各 SOP 自己维护）
            let store = crate::runstate::StateStore::new(
                state_for_retry.plans_root.clone(),
                crate::runstate::StateKey::new(&project_for_retry, &task_for_retry, &sop_for_state),
            );
            let mut rs = match store.load() {
                crate::runstate::Loaded::Found(s) => s,
                _ => store.fresh_state(),
            };
            rs.attempt = attempt;
            if let Err(e) = store.save(&mut rs) {
                tracing::warn!(component = "run", error = %e, "attempt 落盘失败");
            }
            tracing::warn!(
                component = "run",
                attempt,
                project = %project_for_retry,
                task = %task_for_retry,
                "基础设施故障，自动重启 run（从断点继续）"
            );
            state_for_retry.emit(json!({
                "type": "run.retry",
                "project": project_for_retry,
                "task": task_for_retry,
                "run_id": run_id_for_retry,
                "attempt": attempt,
            }));
        }
        engine.execute(&task_snapshot, &run, &control)
    });

    state
        .runs_control
        .lock()
        .expect("runs_control 锁")
        .remove(&format!("{project}/{task_id}"));

    let finish = match &outcome {
        Ok(RunOutcome::Completed) => crate::domain::store::FinishOutcome::Completed,
        Ok(RunOutcome::Failed { reason }) => crate::domain::store::FinishOutcome::Failed {
            reason: reason.clone(),
        },
        Err(e) => crate::domain::store::FinishOutcome::Failed {
            reason: e.to_string(),
        },
    };
    let ok = {
        let mut store = state.store.lock().expect("store 锁");
        store.finish_run(&run.id, finish).is_ok()
    };
    state.emit(json!({
        "type": "run.finished",
        "project": project,
        "task": task_id,
        "run_id": run.id.0,
        "attempts": attempts,
        "ok": ok && matches!(outcome, Ok(RunOutcome::Completed)),
    }));
}

// 台账条目类型在本模块的再导出（spawn 侧调用方记录用）
pub use crate::engine::ledger::LedgerEntry as SpawnLedgerEntry;

/// 会话目录指纹：递归累加 (文件长度, mtime 纳秒) 的 FNV-1a 混合。
/// 任一文件被追加/触碰都会改变指纹——活性检测的「进展」信号
/// （006 decisions/002：会话增长=活，平线=失活）。
fn session_fingerprint(dirs: &[PathBuf]) -> u64 {
    fn walk(dir: &Path, hash: &mut u64) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&path, hash);
            } else {
                let nanos = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                for b in meta.len().to_le_bytes() {
                    *hash ^= b as u64;
                    *hash = hash.wrapping_mul(0x100000001b3);
                }
                for b in nanos.to_le_bytes() {
                    *hash ^= b as u64;
                    *hash = hash.wrapping_mul(0x100000001b3);
                }
            }
        }
    }
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for d in dirs {
        walk(d, &mut hash);
    }
    hash
}

/// SOP 执行 + 取消/超时/失活看门狗。
///
/// 契约：`f` 返回后立刻置位 done（在 scope 闭包内），watchdog ≤300ms 退出、scope join 才能返回——
/// 否则 done 置位不可达（在 scope 返回后）而 scope 等 watchdog，正常完成必死锁
/// （第 18/19/20 试 verdict 后挂起的真因）。
///
/// 失活收割（006 decisions/002）：`idle_ms` 为 Some 时，监控 `watch_dirs` 的会话文件指纹；
/// 任一变化即刷新进展时钟，平线超过阈值 → mark_stalled + 杀自家 pi——只杀「真死」的 run，
/// 慢而活着的 run（含长流式生成中的增长）永不误伤；None = 不做活性收割。
fn run_with_watchdog<T>(
    control: &RunControl,
    run_id: String,
    idle_ms: Option<u64>,
    watch_dirs: Vec<PathBuf>,
    f: impl FnOnce() -> T,
) -> T {
    use std::sync::atomic::{AtomicBool, Ordering};
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let done_wd = done.clone();
    let done_main = done.clone();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let mut last_fp = session_fingerprint(&watch_dirs);
            let mut last_progress = crate::domain::now_ms();
            loop {
                if done_wd.load(Ordering::SeqCst) {
                    return;
                }
                let now = crate::domain::now_ms();
                if control.is_cancelled() || control.timed_out(now) {
                    let killed = crate::engine::ledger::scan_and_kill_run(&run_id);
                    if killed > 0 {
                        tracing::warn!(
                            component = "planning-engine",
                            run_id = %run_id,
                            killed,
                            "watchdog 已清场（取消或超时）"
                        );
                    }
                    return;
                }
                if let Some(idle) = idle_ms {
                    let fp = session_fingerprint(&watch_dirs);
                    if fp != last_fp {
                        last_fp = fp;
                        last_progress = now;
                    } else if now.saturating_sub(last_progress) > idle {
                        control.mark_stalled();
                        tracing::warn!(
                            component = "planning-engine",
                            run_id = %run_id,
                            idle_ms = idle,
                            "会话无进展超过闲置阈值，失活收割"
                        );
                        let killed = crate::engine::ledger::scan_and_kill_run(&run_id);
                        if killed > 0 {
                            tracing::warn!(
                                component = "planning-engine",
                                run_id = %run_id,
                                killed,
                                "失活收割：SIGKILL 自家 run 进程"
                            );
                        }
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
        });
        let out = f();
        // 修复点：f 返回后立刻置位（在 scope 闭包内），watchdog 才能退出、join 才能返回；
        // 若在 scope 返回后才置位则不可达，正常完成必死锁（verdict 后挂起真因）
        done_main.store(true, Ordering::SeqCst);
        out
    })
}

#[cfg(test)]
mod watchdog_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn watchdog_scope_returns_after_closure_completes() {
        // 回归（verdict 后挂起）：f 正常返回后 scope join 必须解锁（旧实现此处死锁）
        let control = RunControl::new();
        let start = Instant::now();
        let out = run_with_watchdog(&control, "watchdog-regression".into(), None, vec![], || {
            std::thread::sleep(Duration::from_millis(400));
            42
        });
        assert_eq!(out, 42);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "scope join 死锁：elapsed={:?}",
            start.elapsed()
        );
    }

    #[test]
    fn watchdog_scope_cancel_path_does_not_hang() {
        // cancel 置位 → watchdog 触发清场路径（scan_and_kill_run 对不存在进程为 no-op）→ 退出
        let control = RunControl::new();
        control.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        let start = Instant::now();
        let out = run_with_watchdog(&control, "watchdog-cancel".into(), None, vec![], || 7);
        assert_eq!(out, 7);
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    // ---------- 失活收割（006 decisions/002：默认不杀，显式闲置阈值才判死） ----------

    #[test]
    fn session_fingerprint_changes_on_append() {
        let dir = tempfile::tempdir().unwrap();
        let watch = dir.path().join("coding");
        std::fs::create_dir_all(&watch).unwrap();
        let fp0 = session_fingerprint(std::slice::from_ref(&watch));
        std::fs::write(watch.join("s.jsonl"), b"hello").unwrap();
        let fp1 = session_fingerprint(std::slice::from_ref(&watch));
        assert_ne!(fp0, fp1, "新建文件应改变指纹");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(watch.join("s.jsonl"), b"hello world").unwrap();
        let fp2 = session_fingerprint(&[watch]);
        assert_ne!(fp1, fp2, "追加增长应改变指纹");
    }

    #[test]
    fn idle_configured_growth_keeps_run_alive() {
        // 增长=活：会话文件持续增长时，闲置阈值再小也不收割（慢而活着永不误伤）
        let dir = tempfile::tempdir().unwrap();
        let watch = dir.path().join("coding");
        std::fs::create_dir_all(&watch).unwrap();
        let writer = watch.clone();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_w = stop.clone();
        let grower = std::thread::spawn(move || {
            let mut i = 0u32;
            while !stop_w.load(std::sync::atomic::Ordering::SeqCst) {
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(writer.join("s.jsonl"))
                    .unwrap();
                use std::io::Write;
                let _ = writeln!(f, "line {i}");
                i += 1;
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        });
        let control = RunControl::new();
        run_with_watchdog(
            &control,
            "liveness-growth".into(),
            Some(600),
            vec![watch],
            || std::thread::sleep(Duration::from_millis(1200)),
        );
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        grower.join().unwrap();
        assert!(!control.is_stalled(), "持续增长的会话不应被失活收割");
    }

    #[test]
    fn idle_configured_flatline_gets_harvested() {
        // 平线超阈=死：会话无进展超过闲置阈值 → watchdog 置 stalled 并收割
        let dir = tempfile::tempdir().unwrap();
        let watch = dir.path().join("coding");
        std::fs::create_dir_all(&watch).unwrap();
        let control = RunControl::new();
        run_with_watchdog(
            &control,
            "liveness-flatline".into(),
            Some(400),
            vec![watch],
            || std::thread::sleep(Duration::from_millis(1500)),
        );
        assert!(control.is_stalled(), "平线超阈应被失活收割");
    }

    #[test]
    fn idle_unconfigured_never_harvests() {
        // 未设置不超时：不做活性收割，静默会话也永不自动杀（零硬限制哲学）
        let dir = tempfile::tempdir().unwrap();
        let watch = dir.path().join("coding");
        std::fs::create_dir_all(&watch).unwrap();
        let control = RunControl::new();
        run_with_watchdog(
            &control,
            "liveness-off".into(),
            None,
            vec![watch],
            || std::thread::sleep(Duration::from_millis(900)),
        );
        assert!(!control.is_stalled(), "未配置闲置阈值时永不失活收割");
    }
}

#[cfg(test)]
mod coding_engine_tests {
    use super::*;

    #[test]
    fn preflight_inits_when_no_index() {
        // decisions/013：工作区无索引 → 引擎调用 init（假脚本被调时建 .codegraph 标记）
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let bin = dir.path().join("fake-codegraph.sh");
        std::fs::write(&bin, "#!/bin/sh\nmkdir -p \"$2/.codegraph\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        preflight_codegraph(&ws, &bin);
        assert!(ws.join(".codegraph").exists(), "预检应调用 init 建索引");
    }

    #[test]
    fn preflight_skips_when_index_exists() {
        // 已有索引 → 不调 init（指向不存在的可执行，若被调即报错暴露）
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(ws.join(".codegraph")).unwrap();
        preflight_codegraph(&ws, Path::new("/nonexistent/codegraph"));
        assert!(ws.join(".codegraph").exists());
    }

    #[test]
    fn dispatch_routes_change_to_its_engine() {
        // 注册表与引擎同步：change 已接线，不应再落到「未接线」分支
        let mut engine = BuiltinRpcEngine {
            profile_root: PathBuf::from("/tmp/devforge-nonexistent-profiles"),
            session_root: PathBuf::from("/tmp"),
            plans_root: PathBuf::from("/tmp"),
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        };
        let mut task = Task::new("t", "p", "change", 1);
        task.workspace = PathBuf::from("/tmp");
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("未接线"), "change 必须已接线：{msg}");
        assert!(
            msg.contains("change-plan-agent"),
            "应卡在 profile 门卫（说明真的进了 run_change）：{msg}"
        );
    }

    #[test]
    fn change_without_requirement_file_is_quality_failure() {
        // 门 0 前先要需求：黑板里没 requirement.md 不得开跑
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        for p in crate::server::change_engine::CHANGE_PROFILES {
            let d = profiles.join(p);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        let plans = dir.path().join("plans");
        let mut engine = BuiltinRpcEngine {
            profile_root: profiles,
            session_root: dir.path().join("sessions"),
            plans_root: plans,
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        };
        let ws = dir.path().join("repo");
        std::fs::create_dir_all(&ws).unwrap();
        let mut task = Task::new("t", "p", "change", 1);
        task.workspace = ws;
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("requirement.md"), "{err}");
        assert!(err.is_quality_failure());
    }

    #[test]
    fn change_run_preflights_codegraph_before_plan() {
        // change-plan-agent 被要求 codegraph-first：run_change 必须先建索引（与其他代码类 SOP 同口径）
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        for p in crate::server::change_engine::CHANGE_PROFILES {
            let d = profiles.join(p);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        let ws = dir.path().join("repo");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(ws.join("src/a.rs"), "// a\n").unwrap();
        // 假 codegraph：init 时在当前工作区建 .codegraph 标记
        let bin = dir.path().join("fake-codegraph");
        std::fs::write(&bin, "#!/bin/sh\nmkdir -p \"$2/.codegraph\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let plans = dir.path().join("plans");
        let mut engine = BuiltinRpcEngine {
            profile_root: profiles,
            session_root: dir.path().join("sessions"),
            plans_root: plans.clone(),
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: bin,
            idle_ms: None,
        };
        // 需求故意缺失：preflight 先于需求门，能验到预检且不会 spawn 真 pi
        let task_root = plans.join("p").join("t");
        std::fs::create_dir_all(&task_root).unwrap();
        let mut task = Task::new("t", "p", "change", 1);
        task.workspace = ws.clone();
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("requirement.md"), "{err}");
        assert!(
            ws.join(".codegraph").exists(),
            "run_change 必须先做 codegraph 预检（plan agent 依赖图）"
        );
    }

    #[test]
    fn dispatch_rejects_unwired_sop() {
        // 注册表与引擎同步性：未知/未接线 SOP 在 execute 入口即拒，不 spawn 任何进程
        let mut engine = BuiltinRpcEngine {
            profile_root: PathBuf::from("/tmp"),
            session_root: PathBuf::from("/tmp"),
            plans_root: PathBuf::from("/tmp"),
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        };
        let task = Task::new("t", "p", "ghost", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未接线"));
    }

    // ---------- F1：spawn 前 profile 门卫（e2e F1-MISSING-PROFILE-HANG） ----------

    fn f1_engine(profile_root: std::path::PathBuf, plans_root: std::path::PathBuf) -> BuiltinRpcEngine {
        BuiltinRpcEngine {
            profile_root,
            session_root: PathBuf::from("/tmp"),
            plans_root,
            ui: Arc::new(UiRelay::new(Arc::new(EventBus::new()))),
            codegraph_bin: PathBuf::from("codegraph"),
            idle_ms: None,
        }
    }

    #[test]
    fn profile_gate_passes_when_rendered() {
        // 已渲染（目录 + settings.json 标记）→ 放行
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("coding-agent");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("settings.json"), "{}").unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        engine.require_profile("coding-agent").unwrap();
    }

    #[test]
    fn profile_gate_fails_naming_missing_path() {
        // 缺失 → 报错含角色目录与标记文件路径，指明先渲染
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        let err = engine.require_profile("coding-agent").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("coding-agent"), "报错应含角色目录: {msg}");
        assert!(msg.contains("settings.json"), "报错应含缺失标记路径: {msg}");
        assert!(msg.contains("未渲染"), "报错应说明未渲染: {msg}");
    }

    #[test]
    fn run_coding_fails_before_side_effects_when_profiles_missing() {
        // 编码 run 在 profile 缺失时立即失败（不再真起 pi 挂 Running），
        // 且校验先于一切副作用：黑板骨架不应被创建
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "coding", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(
            !plans.join("p").join("t").exists(),
            "校验应先于副作用：黑板不应被创建"
        );
    }

    #[test]
    fn run_planning_fails_before_side_effects_when_profiles_missing() {
        // 规划路径同样受门卫保护：缺失即失败，plan root 不落盘
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "planning", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(
            !plans.join("p").join("t").exists(),
            "校验应先于副作用：plan root 不应被创建"
        );
    }

    // ---------- P007 T006：testing / doc-fix 引擎接线 ----------

    #[test]
    fn run_testing_fails_before_side_effects_when_profiles_missing() {
        // 测试 run 在 profile 缺失时立即失败，且 testing/ 骨架不应被创建
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "testing", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(
            !plans.join("p").join("t").exists(),
            "校验应先于副作用：黑板不应被创建"
        );
    }

    #[test]
    fn run_doc_fix_fails_before_side_effects_when_profiles_missing() {
        let dir = tempfile::tempdir().unwrap();
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(dir.path().join("profiles"), plans.clone());
        let task = Task::new("t", "p", "doc-fix", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("未渲染"), "应报 profile 未渲染: {err}");
        assert!(!plans.join("p").join("t").exists(), "校验应先于副作用");
    }

    #[test]
    fn testing_start_requires_workspace_after_profile_gate() {
        // profile 已渲染但无工作区 → workspace 门（测试需要项目地址写测/起被测系统）
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        for role in ["test-agent", "test-review"] {
            let p = profiles.join(role);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("settings.json"), "{}").unwrap();
        }
        let plans = dir.path().join("plans");
        let mut engine = f1_engine(profiles, plans.clone());
        let task = Task::new("t", "p", "testing", 1); // workspace 默认空
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(err.to_string().contains("无代码工作区"), "应报 workspace 门: {err}");
        assert!(
            !plans.join("p").join("t").join("testing").exists(),
            "workspace 门也应先于骨架创建"
        );
    }

    // ---------- 010 F6 靶：per-SOP profile 表（门卫与闸门的唯一事实源） ----------

    #[test]
    fn profiles_for_sop_is_the_single_source_for_the_gate_and_the_door() {
        // 错实现形态：闸门另抄一份 profile 清单（改了 SOP 的需求，闸门还在等旧清单）
        // → 本用例必须变红
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("planning"),
            ["plan-agent", "plan-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("coding"),
            ["coding-agent", "coding-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("testing"),
            ["test-agent", "test-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("doc-from-zero"),
            ["doc-agent", "doc-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("doc-fix"),
            ["doc-fix", "doc-review"].as_slice()
        );
        assert_eq!(
            BuiltinRpcEngine::profiles_for_sop("change"),
            crate::server::change_engine::CHANGE_PROFILES.as_slice()
        );
        assert!(
            BuiltinRpcEngine::profiles_for_sop("unknown-sop").is_empty(),
            "未知 SOP 没有 profile 需求"
        );
    }

    #[test]
    fn the_door_checks_every_profile_of_the_sop() {
        // 错实现形态：门卫只查干活 agent（漏掉审查角色）→ 缺审查 profile 也能开跑
        // → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let profiles = dir.path().join("profiles");
        let agent = profiles.join("coding-agent");
        std::fs::create_dir_all(&agent).unwrap();
        std::fs::write(agent.join("settings.json"), "{}").unwrap();
        let mut engine = f1_engine(profiles, dir.path().join("plans"));
        let task = Task::new("t", "p", "coding", 1);
        let run = Run::new("p", "t");
        let control = RunControl::new();
        let err = engine.execute(&task, &run, &control).unwrap_err();
        assert!(
            err.to_string().contains("coding-review"),
            "审查 profile 缺失也要被门卫拦住：{err}"
        );
    }

    // ---------- 010 F6 靶：渲染闸门 ----------

    #[tokio::test]
    async fn the_gate_waits_for_the_required_profiles_and_ignores_the_rest() {
        // 错实现形态：闸门不等所需 profile（start 立刻开跑 → 门卫判未渲染 → 盲重启）
        // 或等整轮渲染（start 变成分钟级阻塞点）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render();
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent", "plan-review"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(!waiter.is_finished(), "所需 profile 未就绪时闸门必须等");
        // 所需 profile 就绪（其它 profile 仍缺、渲染仍在途）→ 放行
        for p in ["plan-agent", "plan-review"] {
            let d = dir.path().join(p);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("settings.json"), "{}").unwrap();
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("所需 profile 就绪后闸门应放行")
            .unwrap();
        assert!(
            gate.is_in_flight(),
            "放行不改变渲染在途状态（闸门不等整轮渲染）"
        );
    }

    #[tokio::test]
    async fn the_gate_releases_when_the_render_finishes_even_if_the_profile_is_missing() {
        // 错实现形态：渲染结束也不放行（start 永远挂着）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let gate = std::sync::Arc::new(RenderGate::new());
        gate.begin_render();
        let root = dir.path().to_path_buf();
        let g = gate.clone();
        let waiter = tokio::spawn(async move {
            g.wait_for_profiles(&root, &["plan-agent"]).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(!waiter.is_finished(), "渲染仍在途且 profile 未就绪 → 继续等");
        gate.end_render();
        tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
            .await
            .expect("渲染结束后闸门应放行（成败由门卫复查决定）")
            .unwrap();
    }

    #[tokio::test]
    async fn the_gate_does_not_wait_when_no_render_is_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let gate = RenderGate::new();
        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            gate.wait_for_profiles(dir.path(), &["plan-agent"]),
        )
        .await
        .expect("无渲染在途时不应等待");
    }

    // ---------- 010 F6 靶：原始错误可见 + profile 门卫的错误种类 ----------

    #[test]
    fn profile_gate_failure_is_a_config_error_not_an_infrastructure_failure() {
        // 错实现形态：仍用 `System`（会被 `is_infra_failure` 判成基础设施故障 → 盲重启 3 次）
        // → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        let err = engine.require_profile("coding-agent").unwrap_err();
        assert!(
            matches!(err, crate::error::DevforgeError::Config(_)),
            "profile 未就绪应是配置错误（不是基础设施故障）：{err:?}"
        );
        assert!(
            !crate::engine::retry::is_infra_failure(&Err(err)),
            "配置错误不得被判为基础设施故障（否则会盲重启）"
        );
    }

    #[test]
    fn profile_gate_failure_keeps_its_text_unchanged() {
        // 错实现形态：改写错误文案（既有单测与 e2e 靠它指路）→ 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().to_path_buf(), PathBuf::from("/tmp"));
        let err = engine.require_profile("coding-agent").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("coding-agent"), "报错应含角色目录: {msg}");
        assert!(msg.contains("settings.json"), "报错应含缺失标记路径: {msg}");
        assert!(msg.contains("未渲染"), "报错应说明未渲染: {msg}");
        assert!(
            msg.contains("先 PUT /api/v1/config/manifest"),
            "报错应带处置建议: {msg}"
        );
    }

    #[test]
    fn quality_failure_in_map_intervention_is_logged_as_warn_with_the_retry_layer_fields() {
        // 错实现形态：①质量失败落 ERROR（应 WARN）②缺 `error_chain` / `attempt` / `retryable`
        // ③只留既有那条无字段的 WARN → 本用例必须变红
        let dir = tempfile::tempdir().unwrap();
        let engine = f1_engine(dir.path().join("profiles"), PathBuf::from("/tmp"));
        let text = crate::observability::test_support::capture_jsonl(|| {
            let outcome = engine
                .map_intervention(
                    &RunControl::new(),
                    "change-engine",
                    crate::error::DevforgeError::Quality("越界改动未申报".into()),
                )
                .unwrap();
            assert!(
                matches!(outcome, RunOutcome::Failed { .. }),
                "质量失败仍是明确的 Failed（不重启）"
            );
        });
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("run.attempt_failed"))
            .collect();
        assert_eq!(lines.len(), 1, "质量失败应落一条同形状条目：{text}");
        let line = lines[0];
        assert!(line.contains("\"level\":\"WARN\""), "质量失败应落 WARN：{line}");
        assert!(line.contains("\"event\":\"run.attempt_failed\""), "{line}");
        assert!(line.contains("\"component\":\"change-engine\""), "{line}");
        assert!(line.contains("\"attempt\":"), "缺 attempt 字段：{line}");
        assert!(line.contains("\"retryable\":false"), "质量失败不重启：{line}");
        assert!(line.contains("越界改动未申报"), "缺原始错误全文：{line}");
        assert!(line.contains("error_chain"), "缺 source 链字段：{line}");
    }

    #[test]
    fn testing_watch_dirs_cover_handoff_sessions() {
        // D011：testing run 的活性监视覆盖移交会话（doc-fix/doc-reviews 同 run 活性整体）
        let engine = f1_engine(PathBuf::from("/tmp/profiles"), PathBuf::from("/tmp"));
        let dirs = engine.testing_watch_dirs();
        let names: Vec<_> = dirs.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, vec!["testing", "testing-reviews", "doc-fix", "doc-reviews"]);
        let names = engine
            .docfix_watch_dirs()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["doc-fix", "doc-reviews"]);
    }
}
