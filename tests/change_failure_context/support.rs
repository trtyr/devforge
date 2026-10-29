//! （ARC-R6 自 change_failure_context.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{context::*, guards::*};

// ---------------- 常量 ----------------

/// 本变更的基线提交（计划 §1.1）。
/// 10-22 运维：与 change_ask_ring 保持同值（a16 互检）；两场之间守卫 ignore（F10/F13）。
/// 口径：**本场基线**（`docs/plantree/changes/<change-id>/baseline.txt`）= `a36f6b7…`
/// → 守卫的扫描范围与引擎的越界门同口径（同一个基线）。
pub(super) const BASELINE: &str = "a36f6b79dec2e681db8daed0da067f9abd47a9f5";

/// 计划 §4 的白名单去掉唯一的测试阶段文件 `tests/f6_observability.rs`
/// = **coding 步的 23 个文件**。
pub(super) const WHITELIST: &[&str] = &[
    "src/observability/mod.rs",
    "src/observability/heartbeat.rs",
    "src/observability/tombstone.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/engine/policy.rs",
    "src/engine/client.rs",
    "src/server/mod.rs",
    "src/server/handlers.rs",
    "src/server/tests/mod.rs",
    "src/change/orchestrator.rs",
    "src/change/session.rs",
    "src/sop/planning.rs",
    "src/sop/coding.rs",
    "src/sop/testing.rs",
    "src/sop/doc/runner.rs",
    "src/sop/doc/fix.rs",
    "tests/change_ask_ring.rs",
    "tests/change_failure_context/main.rs",
    "tests/change_review_protocol/main.rs",
    "src/change/coding_gate.rs",
    "docs/plantree/plans/010-run-resume/decisions/README.md",
    "docs/plantree/plans/010-run-resume/roadmap.md",
];

/// 本场 change-id（守卫常量区声明一处）——F10：本场守卫只判定**本场**的提交。
///
/// 病根（finding F10）：变化期断言被铸成永久测试——旧口径拿**旧场基线**当扫描区间，
/// 把区间里所有含 ` step ` 的提交都当成「本场 coding 步提交」，外场（其他 change-id）
/// 的步提交因此被当成越界 → 守卫恒红 → 只能靠 `#[ignore]` 压住。
/// 收敛口径：**提交 subject 含本场 change-id** 才是本场的提交，外场步提交不参与判定。
pub(super) const CHANGE_ID: &str = "change-20261029-change-failure-context-finding-f10-sr2-g";

/// 本场白名单（change-plan §6：仓内改动上限四条）——本场步提交动过的文件逐个必须落在其中。
///
/// 与上面的旧场 `WHITELIST`（23 个文件、值与注释都冻结）**不是**同一份：旧场那份是
/// c7-1 场的历史快照，白名单外的守卫逐字核对它的值与注释，改不得；本场这份才是
/// F10 之后步提交对账的依据。
pub(super) const CHANGE_WHITELIST: &[&str] = &[
    "tests/change_failure_context/guards.rs",
    "tests/change_failure_context/support.rs",
    "tests/change_failure_context/context.rs",
    "tests/change_failure_context/main.rs",
];

/// 一条带 `path:line` 的失败明细（验收标准里的样例原文）。
pub(super) const ISSUE_WITH_LOCATION: &str = "tests/change_ask_ring.rs:557 台账计数断言失败";
/// 一条不带位置的失败明细。
pub(super) const ISSUE_PLAIN: &str = "接口返回 500";
/// 引擎**不再**合成的占位文案（裁决 B：空箱拒收 + 追补，不再拿占位文案当失败明细）。
pub(super) const PLACEHOLDER_ISSUE: &str = "测试阶段判定失败（未给出 issues 明细）";
/// 本场计划批准的开工语协议契约（Q13）：声明块放在回复末尾。
/// a1 的「逐字等于基线那版」只在这句上让路（其余措辞仍逐字锁死）。
pub(super) const KICKOFF_PROTOCOL_CLAUSE: &str = "（**声明块放在回复末尾**，其后不要再写别的）";

/// 三处「零信息 / 阉割痕迹」话术（验收标准 5、6、7 逐条点名）。
pub(super) const ZERO_INFO_TRACES: &[&str] = &["请自己定位原因", "本消息不提供文件位置", "<位置略>"];

// ---------------- 本场（F10）扫描口径 ----------------

/// 一条提交（sha + subject）——扫描面只认 subject：change-id 判归属、` step ` 判步提交。
#[derive(Debug, Clone)]
pub(super) struct Commit {
    pub(super) sha: String,
    pub(super) subject: String,
}

/// 命中的类别——文案按类别不同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FindingKind {
    /// 步提交动了白名单外的文件。
    StepOutsideWhitelist,
}

/// 守卫命中的一条明细：出错文件 + 所属提交的 subject + 命中类别。
#[derive(Debug, Clone)]
pub(super) struct Finding {
    /// 出错文件（相对仓根；已按迁移表归一化）。
    pub(super) file: String,
    /// 所属提交的 subject（文案要点名它，读者才知道是哪笔提交越界）。
    pub(super) subject: String,
    /// 命中类别。
    pub(super) kind: FindingKind,
}

impl Finding {
    /// 命中文案——点名出错文件与所属提交。
    pub(super) fn message(&self) -> String {
        match self.kind {
            FindingKind::StepOutsideWhitelist => format!(
                "coding 步提交越界（白名单外）：{}（提交 {}）",
                self.file, self.subject
            ),
        }
    }
}

/// 仓内提交历史（**全仓历史**，不按区间裁剪）。
///
/// 口径理由（F10）：扫描面用「全仓历史 + change-id 过滤」而不是「某基线区间 +
/// change-id 过滤」——区间口径把「扫描范围」绑在会漂移的基线 sha 上（F10 的病因
/// 就是旧场基线常量 `a36f6b7…` 早已不是本场基线），change-id 过滤已足够定位本场提交。
pub(super) fn commit_history(root: &Path) -> Vec<Commit> {
    git(root, &["log", "--format=%H%x09%s"])
        .lines()
        .filter_map(|line| {
            let (sha, subject) = line.split_once('\t')?;
            Some(Commit {
                sha: sha.to_string(),
                subject: subject.to_string(),
            })
        })
        .collect()
}

/// 本场的提交（subject 含本场 change-id）——外场提交不参与判定。
pub(super) fn change_commits(root: &Path, change_id: &str) -> Vec<Commit> {
    commit_history(root)
        .into_iter()
        .filter(|c| c.subject.contains(change_id))
        .collect()
}

/// 步提交（subject 含 ` step `）——沿用现状口径，不扩大到「步骤 N」。
pub(super) fn step_commits(commits: &[Commit]) -> Vec<Commit> {
    commits
        .iter()
        .filter(|c| c.subject.contains(" step "))
        .cloned()
        .collect()
}

/// 本场的步提交——a16 / a17 的扫描面（F10 的收敛点）。
pub(super) fn change_step_commits(root: &Path, change_id: &str) -> Vec<Commit> {
    step_commits(&change_commits(root, change_id))
}

/// 某提交动过的文件（按迁移表归一化到现状路径）。
///
/// 归一化不可省（ARC-R3 起仓库手术会迁移文件）：历史步提交是**事实**不可改，
/// 白名单登记的是**现状**，两侧对账前必须先过迁移表。
pub(super) fn files_of_commit(root: &Path, sha: &str) -> Vec<String> {
    git(root, &["show", "--name-only", "--format=", sha])
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            devforge::change::plan_audit::normalize_path(l.trim()).to_string()
        })
        .collect()
}

/// 步提交的文件逐个必须落在白名单内——越界即命中（守卫变红）。
///
/// 白名单是**参数**：变异探测靠传一份删掉某项的白名单实现（不在运行时改源文件）。
pub(super) fn step_whitelist_violations(
    root: &Path,
    commits: &[Commit],
    whitelist: &[&str],
) -> Vec<Finding> {
    let mut out = Vec::new();
    for c in commits {
        for f in files_of_commit(root, &c.sha) {
            if !whitelist.contains(&f.as_str()) {
                out.push(Finding {
                    kind: FindingKind::StepOutsideWhitelist,
                    file: f,
                    subject: c.subject.clone(),
                });
            }
        }
    }
    out
}

// ---------------- 夹具 ----------------

pub(super) fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// 真基线 sha（store() 已把它落到 `.git/head-sha`）。
pub(super) fn head_sha(repo: &Path) -> String {
    std::fs::read_to_string(repo.join(".git/head-sha"))
        .map(|s| s.trim().to_string())
        .expect("store() 已初始化 git 仓并落 head-sha")
}

pub(super) fn store(root: &Path) -> StateStore {
    // 引擎审计修复 #7：Done 路径的 diff_stats 不再吞失败——测试环境升级为真 git 仓，
    // 基线 sha 落文件供 cfg 读取（与 orchestrator_tests 同方案）。
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    git(root, &["init"]);
    git(root, &["add", "-A"]);
    git(root, &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "init"]);
    let sha = git(root, &["rev-parse", "HEAD"]);
    std::fs::create_dir_all(root.join(".git")).ok();
    std::fs::write(root.join(".git/head-sha"), sha.trim()).unwrap();
    StateStore::new(root, StateKey::new("proj", "task", CHANGE_SOP))
}

pub(super) struct NoUser;

impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

/// 脚本化 session：按队列吐出各阶段结果，并记录每轮 coding 收到的失败上下文。
pub(super) struct Scripted {
    pub(super) plans: VecDeque<PlanTurn>,
    pub(super) amends: VecDeque<PlanTurn>,
    pub(super) codings: VecDeque<CodingTurn>,
    pub(super) testings: VecDeque<TestingTurn>,
    /// coding 轮直接报错（用来观察「coding 过门之前」的断点状态）。
    pub(super) coding_error: Option<String>,
    /// testing 轮直接报错（用来观察「coding 过门那一刻」的断点状态）。
    pub(super) testing_error: Option<String>,
    /// 每轮 coding 收到的失败上下文（`None` = 无上下文）——送达面的观测点。
    pub(super) seen_failures: Vec<Option<TestFailureContext>>,
}

impl Scripted {
    pub(super) fn new() -> Self {
        Self {
            plans: VecDeque::new(),
            amends: VecDeque::new(),
            codings: VecDeque::new(),
            testings: VecDeque::new(),
            coding_error: None,
            testing_error: None,
            seen_failures: Vec::new(),
        }
    }
}

impl ChangeSession for Scripted {
    fn run_plan(&mut self, _ui: &mut dyn UserLink, _rt: usize) -> Result<PlanTurn> {
        Ok(self.plans.pop_front().expect("plan 脚本耗尽"))
    }

    fn amend_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _gaps: &[ImpactGap],
        _rt: usize,
        _reason: &str,
    ) -> Result<PlanTurn> {
        Ok(self.amends.pop_front().expect("amend 脚本耗尽"))
    }

    fn fix_plan(
        &mut self,
        _ui: &mut dyn UserLink,
        _issues: &[String],
        _round: usize,
    ) -> Result<PlanTurn> {
        Err(DevforgeError::Quality("fix_plan 不该被调用".into()))
    }

    fn run_coding(
        &mut self,
        _ui: &mut dyn UserLink,
        _rt: usize,
        last_failure: Option<&TestFailureContext>,
    ) -> Result<CodingTurn> {
        self.seen_failures.push(last_failure.cloned());
        if let Some(msg) = &self.coding_error {
            return Err(DevforgeError::Quality(msg.clone()));
        }
        Ok(self.codings.pop_front().expect("coding 脚本耗尽"))
    }

    fn run_testing(&mut self, _ui: &mut dyn UserLink) -> Result<TestingTurn> {
        if let Some(msg) = &self.testing_error {
            return Err(DevforgeError::Quality(msg.clone()));
        }
        Ok(self.testings.pop_front().expect("testing 脚本耗尽"))
    }
}

pub(super) fn in_boundary() -> CodingTurn {
    CodingTurn {
        reply: "干完了\n<stage-complete>1</stage-complete>".into(),
        changed: vec!["src/a.rs".into()],
        reviews: 1,
        review_issues: Vec::new(),
    }
}

pub(super) fn pass_testing() -> TestingTurn {
    TestingTurn::Pass {
        acceptance_done: vec!["x=1 → 2".into()],
        reviews: 1,
    }
}

/// 造一个 change 上下文（目标仓 + 黑板都在临时目录里）。
pub(super) fn ctx_in(tmp: &Path) -> ChangeCtx {
    let repo = tmp.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    // 真 git 仓 + 真基线 sha：testing 阶段的产品代码快照要能真跑 `git diff <baseline>`，
    // 假基线（"base123"）会让它直接失败。注意仓里必须先有文件，否则 commit 空仓会失败。
    std::fs::write(repo.join("src/a.rs"), "fn a() {}\n").unwrap();
    git(&repo, &["init"]);
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "init"],
    );
    let baseline = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    let blackboard = tmp.join("bb");
    std::fs::create_dir_all(&blackboard).unwrap();
    let change_dir = repo.join("docs/plantree/changes/change-20261020-x");
    std::fs::create_dir_all(&change_dir).unwrap();
    ChangeCtx {
        repo,
        blackboard,
        change_dir,
        change_id: "change-20261020-x".into(),
        baseline,
        requirement: "改 a".into(),
        doc_level: DocLevel::None,
        contract: None,
    }
}

pub(super) fn cfg<'a>(
    store: &'a StateStore,
    repo: &'a Path,
    wl: &[&str],
    criteria: &[&str],
) -> ChangeRun<'a> {
    // 引擎审计修复 #7：基线 sha 从 store() 落的文件读（真 git 仓）；读不到才回退占位。
    let baseline_sha = std::fs::read_to_string(repo.join(".git/head-sha"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "base123".to_string());
    let leaked: &'a str = Box::leak(baseline_sha.into_boxed_str());
    // 台账根在目标仓外（2026-10-25 反转 D003）：测试用仓的同级目录
    let change_dir: &'a Path = Box::leak(
        repo.parent()
            .unwrap_or(repo)
            .join("plantree-changes")
            .into_boxed_path(),
    );
    ChangeRun {
        store,
        repo,
        change_dir,
        change_id: "change-20261020-x",
        baseline_sha: leaked,
        criteria: criteria.iter().map(|s| s.to_string()).collect(),
        whitelist: wl.iter().map(|s| s.to_string()).collect(),
        usage: "1,000 tokens（in 900 / out 100）".into(),
        run_id: "run-1".into(),
        wall_secs: 12,
    }
}

pub(super) fn issues() -> Vec<String> {
    vec![ISSUE_WITH_LOCATION.to_string(), ISSUE_PLAIN.to_string()]
}

pub(super) fn context(round: usize) -> TestFailureContext {
    TestFailureContext {
        round,
        issues: issues(),
    }
}

// ---------------- 验收 1：新状态无上下文 + 无上下文时开工语逐字不变 ----------------

