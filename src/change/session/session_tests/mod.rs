//! ProdChangeSession 测试共享夹具（ARC-R6 自 session.rs mod tests 平移，纯移动）。
//! 子 mod（plan/runtime）经 `use super::*` 继承本 mod 全部名字（含 glob 链的生产项）。

use super::*;
use crate::sop::planning::{ReviewSession, ReviewSessionFactory};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::process::Command;
use std::rc::Rc;

struct NoUser;
impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _options: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("单测不应问用户：{q}")))
    }
}

struct Agent {
    replies: VecDeque<String>,
    seen: Rc<RefCell<Vec<String>>>,
}
impl Agent {
    fn new(replies: &[&str]) -> Self {
        Self {
            replies: replies.iter().map(|s| s.to_string()).collect(),
            seen: Rc::new(RefCell::new(Vec::new())),
        }
    }
}
impl PlanAgentSession for Agent {
    fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        self.seen.borrow_mut().push(message.to_string());
        Ok(self.replies.pop_front().expect("Agent 脚本耗尽"))
    }
}

struct OneReview {
    queue: Rc<RefCell<VecDeque<String>>>,
    seen: Rc<RefCell<Vec<String>>>,
}
impl ReviewSession for OneReview {
    fn review(&mut self, instruction: &str) -> Result<String> {
        self.seen.borrow_mut().push(instruction.to_string());
        Ok(self.queue.borrow_mut().pop_front().expect("Review 脚本耗尽"))
    }
}
struct ReviewFactory {
    queue: Rc<RefCell<VecDeque<String>>>,
    seen: Rc<RefCell<Vec<String>>>,
}
impl ReviewFactory {
    fn new(replies: &[&str]) -> Self {
        Self {
            queue: Rc::new(RefCell::new(replies.iter().map(|s| s.to_string()).collect())),
            seen: Rc::new(RefCell::new(Vec::new())),
        }
    }
}
impl ReviewSessionFactory for ReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        Ok(Box::new(OneReview {
            queue: self.queue.clone(),
            seen: self.seen.clone(),
        }))
    }
}

const PLAN_PASS: &str = "<stage-verdict>PASS</stage-verdict>\n<next>advance</next>";
const PLAN_FAIL: &str = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>\n- 影响面漏了调用方\n</issues>";

fn plan_text() -> String {
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

struct Fixture {
    tmp: tempfile::TempDir,
    ctx: ChangeCtx,
}

fn fixture(with_git: bool) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// a\n").unwrap();
    // P012-T007（CG-09）：验收对账引用的测试文件必须真实存在且行号在范围内——
    // fixture 预置 tests/a_test.rs（≥10 行），对账表示例 `tests/a_test.rs:10` 才是真实引用
    std::fs::create_dir_all(repo.join("tests")).unwrap();
    std::fs::write(
        repo.join("tests/a_test.rs"),
        (1..=12).map(|i| format!("// line {i}\n")).collect::<String>(),
    )
    .unwrap();
    let blackboard = tmp.path().join("bb");
    std::fs::create_dir_all(&blackboard).unwrap();
    let change_dir = repo.join("docs/plantree/changes/change-1");
    std::fs::create_dir_all(&change_dir).unwrap();
    std::fs::write(change_dir.join("change-plan.md"), plan_text()).unwrap();
    let mut baseline = String::new();
    if with_git {
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "-A"]);
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@localhost",
                "commit",
                "-q",
                "-m",
                "baseline",
            ])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(out.status.success());
        baseline = String::from_utf8_lossy(
            &Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();
    }
    Fixture {
        ctx: ChangeCtx {
            repo,
            blackboard,
            change_dir,
            change_id: "change-1".into(),
            baseline,
            requirement: "把 a 改成 b".into(),
            doc_level: DocLevel::None,
            contract: None,
        },
        tmp,
    }
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}


fn coding_ready(f: &Fixture) -> usize {
    // 基线绿 + 一步实施 + 一次提交（与 impl-steps 对得上）
    std::fs::create_dir_all(f.ctx.blackboard.join("evidence")).unwrap();
    std::fs::write(
        f.ctx.blackboard.join("evidence/baseline-test.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    std::fs::write(
        f.ctx.blackboard.join("evidence/self-check.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    // 证据 mtime 刷到未来 2s：模拟「agent 本轮实时写入」（证据门拒绝早于本轮开始的残留）
    for name in ["baseline-test.log", "self-check.log"] {
        let p = f.ctx.blackboard.join("evidence").join(name);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
    }
    std::fs::write(
        f.ctx.blackboard.join("impl-steps.md"),
        "# 实施步骤\n## 步骤 1 改 a\n",
    )
    .unwrap();
    std::fs::write(f.ctx.repo.join("src/a.rs"), "// a changed\n").unwrap();
    git(&f.ctx.repo, &["add", "-A"]);
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "change: change-1 step 1",
        ])
        .current_dir(&f.ctx.repo)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    1
}

mod plan;
mod runtime;
