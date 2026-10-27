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

