//! （ARC-R6 自 change_review_protocol.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{guards::*, protocol::*};

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

pub(super) struct NoUser;

impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

/// 审查实例的一条脚本回复。
pub(super) enum ScriptedReply {
    /// 正常文本。
    Text(String),
    /// 模型通道错误（审查调用返回 `Err`）。
    Fail(String),
}

impl ScriptedReply {
    pub(super) fn text(s: &str) -> Self {
        ScriptedReply::Text(s.to_string())
    }
}

/// 脚本化的 agent 会话：按队列吐出回复，并记录收到的每条消息。
#[derive(Clone)]
pub(super) struct ScriptedAgent {
    replies: Rc<RefCell<VecDeque<String>>>,
    seen: Rc<RefCell<Vec<String>>>,
}

impl ScriptedAgent {
    pub(super) fn new(replies: &[&str]) -> Self {
        ScriptedAgent {
            replies: Rc::new(RefCell::new(
                replies.iter().map(|s| s.to_string()).collect(),
            )),
            seen: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// 收到的消息（观测点：违例**不得**回注被审 agent）。
    pub(super) fn seen(&self) -> Vec<String> {
        self.seen.borrow().clone()
    }
}

impl PlanAgentSession for ScriptedAgent {
    fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        self.seen.borrow_mut().push(message.to_string());
        Ok(self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("Agent 脚本耗尽"))
    }
}

/// 脚本化的审查实例：每 `open()` 一次 = 一个**全新实例**。
pub(super) struct ScriptedReview {
    reply: Option<ScriptedReply>,
}

impl ReviewSession for ScriptedReview {
    fn review(&mut self, instruction: &str) -> Result<String> {
        let _ = instruction;
        match self.reply.take().expect("审查实例被用了两次") {
            ScriptedReply::Text(t) => Ok(t),
            ScriptedReply::Fail(m) => Err(DevforgeError::agent("change-review", m)),
        }
    }
}

/// 审查实例工厂：记录开过几个实例（= 审查轮次）。
#[derive(Clone)]
pub(super) struct ScriptedReviewFactory {
    replies: Rc<RefCell<VecDeque<ScriptedReply>>>,
    opened: Rc<Cell<usize>>,
}

impl ScriptedReviewFactory {
    pub(super) fn new(replies: Vec<ScriptedReply>) -> Self {
        ScriptedReviewFactory {
            replies: Rc::new(RefCell::new(replies.into())),
            opened: Rc::new(Cell::new(0)),
        }
    }

    /// 开过的实例数。
    pub(super) fn opened(&self) -> usize {
        self.opened.get()
    }
}

impl ReviewSessionFactory for ScriptedReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        self.opened.set(self.opened.get() + 1);
        let reply = self
            .replies
            .borrow_mut()
            .pop_front()
            .expect("Review 脚本耗尽");
        Ok(Box::new(ScriptedReview {
            reply: Some(reply),
        }))
    }
}

/// 计划文本（六块齐全 + 一条验收标准，与计划 §5 的对账口径一致）。
pub(super) fn plan_text() -> String {
    "# 变更计划\n\n\
## 1 需求复述\n改 a。\n\n\
### 1.1 歧义与本计划采用的默认\n\
- [自决] 命名细节由 agent 定\n\n\
## 2 影响面\n- `src/a.rs:1`\n\n\
## 3 要改的\n| `src/a.rs` | 需求 |\n\n\
## 4 白名单\n`src/a.rs`\n\n\
## 5 验收标准\n- 给定 x=1 → 返回 2\n\n\
## 6 风险\n- 无\n"
        .to_string()
}

/// 一次性夹具：目标仓（带 git 历史）+ 黑板（证据 / 步骤 / 对账表）+ 计划文件。
pub(super) struct Fixture {
    /// 持有 TempDir（落盘生命周期由它决定；字段本身不读）。
    pub(super) _tmp: tempfile::TempDir,
    pub(super) ctx: ChangeCtx,
    /// 目标仓的基线提交。
    pub(super) baseline: String,
}

pub(super) fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// a\n").unwrap();
    // P012-T007（CG-09）：验收对账表引用的 `tests/change_review_protocol.rs:1`
    // 必须在目标仓里真实存在——且必须在**基线提交之前**预置（让它进基线、
    // 不进「相对基线改动集」——否则 CG-10 修复后的 untracked 可见性会让
    // 边界门判它「越界改动未申报」）
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    std::fs::write(repo.join("tests/change_review_protocol.rs"), "// fixture marker\n").unwrap();

    // git：基线提交 + 一步一提交（编码门要核对「步骤 ↔ 提交」）
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "baseline",
        ],
    );
    let baseline = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
    std::fs::write(repo.join("src/a.rs"), "// a changed\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "change: change-1 step 1 —— 改 a",
        ],
    );

    // 黑板：基线证据 / 自证证据 / 回归证据 / 实施步骤 / 验收对账表
    let blackboard = tmp.path().join("bb");
    std::fs::create_dir_all(blackboard.join("evidence")).unwrap();
    std::fs::create_dir_all(blackboard.join("testing")).unwrap();
    std::fs::write(
        blackboard.join("evidence/baseline-test.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    std::fs::write(
        blackboard.join("evidence/self-check.log"),
        "command: cargo test --lib\nexit_code: 0\n",
    )
    .unwrap();
    // 证据 mtime 刷到未来 2s：模拟「agent 本轮实时写入」（证据门拒绝早于本轮开始的残留）
    for name in ["baseline-test.log", "self-check.log"] {
        let p = blackboard.join("evidence").join(name);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
    }
    std::fs::write(
        blackboard.join("evidence/regression.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    // 引擎审计修复 #4：regression.log 也在 freshness 门槛内——同样刷未来 mtime
    std::fs::File::options()
        .write(true)
        .open(blackboard.join("evidence/regression.log"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
        .unwrap();
    std::fs::write(blackboard.join("impl-steps.md"), "## 步骤 1 · 改 a\n").unwrap();
    std::fs::write(
        blackboard.join("testing/acceptance.md"),
        "- 给定 x=1 → 返回 2 → tests/change_review_protocol.rs:1\n",
    )
    .unwrap();

    // P012-T007（CG-09）：验收对账表引用的 `tests/change_review_protocol.rs:1`
    // 必须在目标仓里真实存在——引用检查现在会验存在性与行号范围
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    std::fs::write(repo.join("tests/change_review_protocol.rs"), "// fixture marker\n").unwrap();

    // 计划文件（放在目标仓的 changes 目录里，审查轮要读它）
    let change_dir = repo.join("docs/plantree/changes/change-1");
    std::fs::create_dir_all(&change_dir).unwrap();
    std::fs::write(change_dir.join("change-plan.md"), plan_text()).unwrap();

    // P012-T007（CG-09）：验收对账表引用的 `tests/change_review_protocol.rs:1`
    // 必须在目标仓里真实存在——引用检查现在会验存在性与行号范围
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    std::fs::write(repo.join("tests/change_review_protocol.rs"), "// fixture marker\n").unwrap();

    Fixture {
        _tmp: tmp,
        ctx: ChangeCtx {
            repo,
            blackboard,
            change_dir,
            change_id: "change-1".into(),
            baseline: baseline.clone(),
            requirement: "把 a 改成 b".into(),
            doc_level: DocLevel::None,
            contract: None,
        },
        baseline,
    }
}

/// 造一个 `ProdChangeSession`（六个会话槽 + 上下文）。
#[allow(clippy::too_many_arguments)]
pub(super) fn prod_session(
    ctx: ChangeCtx,
    plan_agent: ScriptedAgent,
    plan_review: ScriptedReviewFactory,
    coding_agent: ScriptedAgent,
    coding_review: ScriptedReviewFactory,
    test_agent: ScriptedAgent,
    test_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    ProdChangeSession {
        plan_agent: Box::new(plan_agent),
        plan_review: Box::new(plan_review),
        coding_agent: Box::new(coding_agent),
        coding_review: Box::new(coding_review),
        test_agent: Box::new(test_agent),
        test_review: Box::new(test_review),
        ctx,
        plan_review_instances: 0,
    }
}

/// 只给 plan 轮用的会话（其余槽给空脚本）。
pub(super) fn plan_only(
    ctx: ChangeCtx,
    plan_agent: ScriptedAgent,
    plan_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    prod_session(
        ctx,
        plan_agent,
        plan_review,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
    )
}

/// 只给 coding 轮用的会话。
pub(super) fn coding_only(
    ctx: ChangeCtx,
    coding_agent: ScriptedAgent,
    coding_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    prod_session(
        ctx,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        coding_agent,
        coding_review,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
    )
}

/// 只给 testing 轮用的会话。
pub(super) fn testing_only(
    ctx: ChangeCtx,
    test_agent: ScriptedAgent,
    test_review: ScriptedReviewFactory,
) -> ProdChangeSession {
    prod_session(
        ctx,
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        ScriptedAgent::new(&[]),
        ScriptedReviewFactory::new(Vec::new()),
        test_agent,
        test_review,
    )
}

// ---------------- 验收 1：plan 审查违例 → 换新实例重审 → Pass ----------------

