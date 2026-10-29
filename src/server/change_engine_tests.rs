//! （2026-10-26 测试分离：自 src/server/change_engine.rs 内联平移，纯移动零改动。）
use super::*;
use crate::change::report::{AcceptanceSummary, ChangeReport, DiffStats};
use crate::change::state::{ChangePhase, ChangeState};
use crate::change::orchestrator::ChangeOutcome;

fn outcome() -> ChangeOutcome {
    ChangeOutcome {
        state: ChangeState::new("change-1", "base"),
        report: ChangeReport {
            change_id: "change-1".into(),
            run_id: "run-1".into(),
            baseline_sha: "base".into(),
            wall_secs: 0,
            diff: DiffStats {
                files: 1,
                insertions: 2,
                deletions: 0,
            },
            review_rounds: 2,
            roundtrips: 1,
            usage: String::new(),
            acceptance: AcceptanceSummary {
                total: 1,
                done: 1,
                missing: vec![],
            },
        },
    }
}

#[test]
fn ctx_paths_point_at_repo_plan_dir_and_sop_blackboard() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/a.rs"), "// a\n").unwrap();
    let prep = crate::change::prepare::prepare(
        &tmp.path().join("plans"),
        &repo,
        "p",
        "t",
        "add x",
        "none",
        (2026, 10, 20),
    )
    .unwrap();
    let ctx = build_ctx(&prep, &repo, "add x", None);
    assert_eq!(
        ctx.plan_path(),
        crate::change::prepare::plantree_root(&tmp.path().join("plans"), "p")
            .join("changes")
            .join(&prep.change_id)
            .join("change-plan.md"),
        "计划路径必须在台账根（2026-10-25 反转 D003：台账不进仓）"
    );
    assert!(!ctx.plan_path().starts_with(&repo), "计划不得落进目标仓");
    assert!(
        ctx.blackboard.ends_with("p/t/change"),
        "证据根必须是 SOP 级黑板：{}",
        ctx.blackboard.display()
    );
    assert!(ctx.blackboard.join("evidence").is_dir(), "门 0 骨架要在会话用的黑板上");
    assert!(ctx.blackboard.join("testing/reports").is_dir());
    assert_eq!(ctx.baseline, prep.baseline.sha);
}

#[test]
fn finalize_writes_report_with_usage_and_wall_clock() {
    let tmp = tempfile::tempdir().unwrap();
    let mut out = outcome();
    let f = finalize(
        &mut out,
        &tmp.path().join("sessions"),
        "run-t",
        &tmp.path().join("ledger").join("change-20261020-x"),
        std::time::Instant::now(),
    )
    .unwrap();
    assert!(f.report_path.is_file());
    let text = std::fs::read_to_string(&f.report_path).unwrap();
    assert!(text.contains("用量:"), "{text}");
    assert!(text.contains("验收对账: 通过"), "{text}");
    assert!(text.contains("审查轮次: 2"), "{text}");
    assert!(text.contains("往返: 1"), "{text}");
    assert_eq!(out.state.phase, ChangePhase::Plan, "收尾不得改阶段");
}

#[test]
fn session_dirs_cover_all_six_profiles() {
    let dirs = session_dirs(std::path::Path::new("/s"), "run-x");
    let names: Vec<String> = dirs
        .iter()
        .map(|d| d.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), CHANGE_PROFILES.len());
    for n in [
        "change-plan",
        "change-plan-reviews",
        "change-coding",
        "change-coding-reviews",
        "change-testing",
        "change-test-reviews",
    ] {
        assert!(names.contains(&n.to_string()), "缺会话目录 {n}");
    }
    // 引擎批次 #4：会话必须先落在 sessions/<run-id>/ 下
    assert!(
        dirs.iter().all(|d| d.parent().unwrap().file_name().unwrap() == "run-x"),
        "会话目录必须按 run-id 隔离：{dirs:?}"
    );
}

// ── 2026-10-27 自审 change_engine 批回归 ──

fn test_git(repo: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 自审 minor（102/198）：worktree 隔离链路的冻结锚点语义必须可测——
/// HEAD 未前进时 `freeze()` == 基点（此时不得写盘/当候选入队），有提交后才与基点不同。
#[test]
fn freeze_equals_baseline_until_the_agent_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    test_git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
    test_git(&repo, &["add", "-A"]);
    test_git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
    let base = String::from_utf8(
        std::process::Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    let ws = crate::workspace::Workspace::ensure(&crate::workspace::WorkspaceSpec {
        repo: repo.clone(),
        task_id: "freeze-probe".into(),
        base_rev: base.clone(),
    })
    .unwrap();
    assert_eq!(
        ws.freeze().unwrap(),
        base,
        "未提交时应冻结在基点（这正是「空候选不得入队」的判据）"
    );

    // agent 提交之后锚点才前进
    std::fs::write(ws.path.join("a.rs"), "fn a() { let _x = 1; }\n").unwrap();
    test_git(&ws.path, &["add", "-A"]);
    test_git(
        &ws.path,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "impl",
        ],
    );
    let frozen = ws.freeze().unwrap();
    assert_ne!(frozen, base, "有提交后冻结锚点必须前进");
    assert_eq!(frozen.len(), 40, "冻结锚点必须是完整 commit sha：{frozen}");
    ws.remove(&repo).unwrap();
}

/// 外部审计 #2（2026-10-28）+ 自审 critical（102 前案）：报告必须落在**台账根的
/// change_dir**——写 worktree 会被 remove --force 连目录删（102 前案：run 报
/// Completed 但 report_path 指向不存在的文件）；写主仓则把台账产物塞进目标仓，
/// 违反「台账不进仓」（2026-10-25 反转 D003）。
#[test]
fn finalize_writes_report_into_the_ledger_change_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let worktree = tmp.path().join("repo-worktrees").join("t1");
    std::fs::create_dir_all(&worktree).unwrap();
    let change_dir = tmp
        .path()
        .join("ledger")
        .join("proj")
        .join("change-20261020-x");
    let sessions = worktree.join("sessions"); // 会话目录在 worktree 侧
    let mut out = outcome();
    let f = finalize(
        &mut out,
        &sessions,
        "run-w",
        &change_dir,
        std::time::Instant::now(),
    )
    .unwrap();
    assert!(
        f.report_path.starts_with(&change_dir),
        "报告必须落在台账 change_dir：{}",
        f.report_path.display()
    );
    assert!(
        !f.report_path.starts_with(&worktree),
        "报告不得落在会被 remove --force 删掉的 worktree 里"
    );
    assert!(
        !f.report_path.starts_with(&repo),
        "报告也不得写进目标主仓（台账不进仓）：{}",
        f.report_path.display()
    );
    assert!(f.report_path.is_file());
}

// ── 2026-10-29 冻结候选读盘（change-20261029-resume-workspace-sr1-run-7-8-10-bug-work）──

/// 验收 9：任务目录没有冻结候选文件 → 传给 workspace 层的是 None
/// （不收养；非 worktree / 未冻结任务的旧口径完全不变）。
#[test]
fn frozen_candidate_is_absent_for_a_task_without_the_file() {
    let tmp = tempfile::tempdir().unwrap();
    let task_root = tmp.path().join("plans").join("p").join("t");
    std::fs::create_dir_all(&task_root).unwrap();
    assert!(
        read_frozen_candidate(&task_root).is_none(),
        "没有冻结候选文件时必须传 None（不收养）"
    );
}

/// 验收 10：文件存在 → **原文**下传（含尾随换行）——形状判定在 workspace 层做，
/// 这里不做 trim（两处各判一次迟早判定漂移）。
#[test]
fn frozen_candidate_is_passed_through_verbatim() {
    let tmp = tempfile::tempdir().unwrap();
    let task_root = tmp.path().join("plans").join("p").join("t");
    std::fs::create_dir_all(&task_root).unwrap();
    let sha = "a".repeat(40);
    std::fs::write(task_root.join(FROZEN_COMMIT_FILE), format!("{sha}\n")).unwrap();
    let raw = read_frozen_candidate(&task_root).expect("冻结候选文件应被读到");
    assert_eq!(
        raw,
        format!("{sha}\n"),
        "原文下传（含尾随换行），不得在这里 trim"
    );
}

/// 验收 12：文件读失败（路径被占成目录 → IO 错误，非 NotFound）→ 降级为 None
/// 且不 panic；run 不能被一个读不出的文件中断。
#[test]
fn unreadable_frozen_candidate_degrades_to_none_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let task_root = tmp.path().join("plans").join("p").join("t");
    std::fs::create_dir_all(task_root.join(FROZEN_COMMIT_FILE)).unwrap();
    assert!(
        read_frozen_candidate(&task_root).is_none(),
        "读失败必须降级为 None（不得 panic、不得中断 run）"
    );
}

/// 端到端行为（验收 9+10 的装配闭环）：台账里的冻结候选 → 真的收养上一轮的候选分支
/// （worktree HEAD 落在候选 tip）。这条钉住「读盘 → 传参 → 收养」整条链，
/// 任何一环断掉都会红。
#[test]
fn frozen_candidate_file_drives_worktree_adoption() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    test_git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
    test_git(&repo, &["add", "-A"]);
    test_git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "base",
        ],
    );
    let base = String::from_utf8(
        std::process::Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    // 上一轮：建 worktree → 提交候选 → 清理 worktree（分支保留）
    let spec = crate::workspace::WorkspaceSpec {
        repo: repo.clone(),
        task_id: "cand-probe".into(),
        base_rev: base.clone(),
    };
    let ws = crate::workspace::Workspace::create(&spec).unwrap();
    std::fs::write(ws.path.join("impl.rs"), "fn impl() {}\n").unwrap();
    test_git(&ws.path, &["add", "-A"]);
    test_git(
        &ws.path,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "impl",
        ],
    );
    let candidate = ws.freeze().unwrap();
    ws.remove(&repo).unwrap();

    // 台账里落冻结候选（change_engine 的写盘格式：<sha>\n）
    let task_root = tmp.path().join("plans").join("p").join("t");
    std::fs::create_dir_all(&task_root).unwrap();
    std::fs::write(
        task_root.join(FROZEN_COMMIT_FILE),
        format!("{candidate}\n"),
    )
    .unwrap();

    // 读出来交给 workspace 层 → 收养上一轮的候选分支
    let raw = read_frozen_candidate(&task_root).expect("冻结候选应被读到");
    let adopted = crate::workspace::Workspace::ensure_with_candidate(&spec, Some(&raw)).unwrap();
    assert_eq!(
        adopted.freeze().unwrap(),
        candidate,
        "冻结候选必须让 worktree 落到候选 tip（收养）"
    );
}

// ── 验收对账用例（测试阶段独立补写）────────────────────────────────────
// change-20261029-resume-workspace-sr1-run-7-8-10-bug-work
//
// 立场：黑盒。engine 侧的验收点全在「读盘 → 下传 → workspace 层怎么用」这条链上，
// 因此这里既盯读盘函数自身的行为（None / 原文 / WARN 痕迹），也盯它与 workspace 层
// 合起来的行为（脏内容不收养、读失败不中断 run）。
mod acceptance {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    /// 任务黑板目录（台账根 `plans/<project>/<task>`）。
    fn task_root(tmp: &Path) -> PathBuf {
        tmp.join("plans").join("p").join("t")
    }

    /// 捕获本线程内的 tracing 输出（断言只关心 event 字段的值）。
    fn capture_tracing<F: FnOnce()>(f: F) -> String {
        #[derive(Clone)]
        struct SharedBuf(Arc<Mutex<Vec<u8>>>);
        impl Write for SharedBuf {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("日志缓冲锁").extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Arc::new(Mutex::new(Vec::<u8>::new()));
        let writer = SharedBuf(sink.clone());
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        let bytes = sink.lock().expect("日志缓冲锁").clone();
        String::from_utf8_lossy(&bytes).to_string()
    }

    /// 裸 git：非零退出即 panic（带 stderr）。
    fn raw_git(repo: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("git 可执行");
        assert!(
            out.status.success(),
            "git {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn rev_parse(repo: &Path, rev: &str) -> String {
        raw_git(repo, &["rev-parse", "--verify", rev]).trim().to_string()
    }

    /// 「重建工作区」的现场：主仓停在基点，任务分支停在候选 / 不存在，worktree 目录不存在。
    struct BranchFixture {
        _dir: tempfile::TempDir,
        spec: crate::workspace::WorkspaceSpec,
        base: String,
        tip: String,
    }

    impl BranchFixture {
        /// 任务分支停在候选提交（≠基点）。
        fn with_candidate_branch(task_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().to_path_buf();
            raw_git(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
            raw_git(&repo, &["add", "-A"]);
            raw_git(
                &repo,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@localhost",
                    "commit",
                    "-q",
                    "-m",
                    "base",
                ],
            );
            let base = rev_parse(&repo, "HEAD");
            std::fs::write(repo.join("impl.rs"), "fn impl() {}\n").unwrap();
            raw_git(&repo, &["add", "-A"]);
            raw_git(
                &repo,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@localhost",
                    "commit",
                    "-q",
                    "-m",
                    "impl",
                ],
            );
            let candidate = rev_parse(&repo, "HEAD");
            raw_git(&repo, &["branch", &format!("task/{task_id}"), &candidate]);
            raw_git(&repo, &["reset", "-q", "--hard", &base]);
            Self {
                spec: crate::workspace::WorkspaceSpec {
                    repo: repo.clone(),
                    task_id: task_id.to_string(),
                    base_rev: base.clone(),
                },
                base,
                tip: candidate,
                _dir: dir,
            }
        }

        /// 任务分支不存在（首跑 / 旧口径新建路径）。
        fn without_branch(task_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().to_path_buf();
            raw_git(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
            raw_git(&repo, &["add", "-A"]);
            raw_git(
                &repo,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@localhost",
                    "commit",
                    "-q",
                    "-m",
                    "base",
                ],
            );
            let base = rev_parse(&repo, "HEAD");
            Self {
                spec: crate::workspace::WorkspaceSpec {
                    repo: repo.clone(),
                    task_id: task_id.to_string(),
                    base_rev: base.clone(),
                },
                base,
                tip: String::new(),
                _dir: dir,
            }
        }

        /// 任务分支停在基点（上一轮没产生提交）。
        fn with_branch_at_base(task_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().to_path_buf();
            raw_git(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
            raw_git(&repo, &["add", "-A"]);
            raw_git(
                &repo,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@localhost",
                    "commit",
                    "-q",
                    "-m",
                    "base",
                ],
            );
            let base = rev_parse(&repo, "HEAD");
            raw_git(&repo, &["branch", &format!("task/{task_id}"), &base]);
            Self {
                spec: crate::workspace::WorkspaceSpec {
                    repo: repo.clone(),
                    task_id: task_id.to_string(),
                    base_rev: base.clone(),
                },
                base: base.clone(),
                tip: base,
                _dir: dir,
            }
        }
    }

    impl Drop for BranchFixture {
        fn drop(&mut self) {
            // worktree 建在仓外兄弟目录（tempdir 同级）：用例结束整目录清掉，不留残骸
            let target = crate::workspace::Workspace::target_path(&self.spec);
            if let Some(dir) = target.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    // ---------- 验收 10：没有冻结候选文件 → 传 None ----------

    /// 验收 10：change_engine 侧任务目录下无 `frozen-commit.txt` → 传给 workspace 层的
    /// 是 None（不收养；非 worktree 或未冻结任务的旧口径完全不变），且留 WARN 痕迹。
    /// 错实现 A（缺文件时 panic / 造一个假候选）→ 这条红。
    #[test]
    fn ac13_absent_frozen_file_yields_none_and_a_warn() {
        let tmp = tempfile::tempdir().unwrap();
        let root = task_root(tmp.path());
        std::fs::create_dir_all(&root).unwrap();

        let logs = capture_tracing(|| {
            let got = read_frozen_candidate(&root);
            assert!(
                got.is_none(),
                "没有冻结候选文件时必须传 None（不收养），实际：{got:?}"
            );
        });
        assert!(
            logs.contains("change.frozen_candidate_absent"),
            "缺文件要留 WARN 痕迹（change.frozen_candidate_absent）：{logs}"
        );
    }

    // ---------- 验收 10/11：文件存在 → 原文下传 ----------

    /// 验收 10：change_engine 侧 `frozen-commit.txt` 存在 → **原文**下传（含尾随换行，
    /// 不在这里 trim）；形状判定归 workspace 层。
    /// 错实现 B（engine 擅自 trim / 改写原文）→ 这条红。
    #[test]
    fn ac14_frozen_file_is_passed_through_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let root = task_root(tmp.path());
        std::fs::create_dir_all(&root).unwrap();
        let sha = "a".repeat(40);
        std::fs::write(root.join(FROZEN_COMMIT_FILE), format!("{sha}\n")).unwrap();

        let raw = read_frozen_candidate(&root).expect("冻结候选文件应被读到");
        assert_eq!(
            raw,
            format!("{sha}\n"),
            "原文下传（含尾随换行），不得在这一层 trim"
        );
    }

    // ---------- 验收 11：脏内容 → 读得到但不收养 ----------

    /// 验收 11：change_engine 侧 `frozen-commit.txt` 内容为脏内容（非 40 位 hex）→
    /// 读取成功（读盘不报错，run 不因该文件中断）但**不收养**（形状门在 workspace 层落实）。
    /// 错实现 C（脏内容被当候选收养 / 脏内容导致 run 报错中断）→ 这条红。
    #[test]
    fn ac15_dirty_frozen_content_is_read_but_never_adopted() {
        // (a) 读得到：读盘成功 = 该文件本身不构成错误
        let tmp = tempfile::tempdir().unwrap();
        let root = task_root(tmp.path());
        std::fs::create_dir_all(&root).unwrap();
        let dirty = "cafebabe";
        std::fs::write(root.join(FROZEN_COMMIT_FILE), dirty).unwrap();
        let raw = read_frozen_candidate(&root).expect("脏内容也应被读到（读成功）");
        assert_eq!(raw, dirty, "原文下传");

        // (b) 不收养：分支 tip 是合法候选，但传入的是脏内容 → 走拒绝路径（不是收养）
        let fx = BranchFixture::with_candidate_branch("ac15-dirty");
        let err = crate::workspace::Workspace::create_with_candidate(&fx.spec, Some(&raw))
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("非基点也非冻结候选"),
            "脏内容不得被当候选收养：{msg}"
        );
        assert!(
            !crate::workspace::Workspace::target_path(&fx.spec).exists(),
            "脏内容不得创建 worktree"
        );
        assert_eq!(
            rev_parse(&fx.spec.repo, &format!("task/{}", fx.spec.task_id)),
            fx.tip,
            "拒绝路径不得改写分支 tip"
        );

        // (c) 分支停在基点时：脏内容不影响 run（worktree 照常建在基点，不报错中断）
        let fx2 = BranchFixture::without_branch("ac15-continue");
        let ws = crate::workspace::Workspace::create_with_candidate(&fx2.spec, Some(&raw)).unwrap();
        assert_eq!(
            ws.freeze().unwrap(),
            fx2.base,
            "脏内容不得让 run 报错中断（分支在基点 → 照常建工作区）"
        );
    }

    // ---------- 验收 12：读失败 → None + WARN ----------

    /// 验收 12：change_engine 侧 `frozen-commit.txt` 读取失败（IO 错误）→ 传 None 并留
    /// WARN 痕迹，run 继续按旧口径新建 worktree（不 panic、不中断）。
    /// 错实现 D（读失败时 panic / 读失败也报错中断 run）→ 这条红。
    #[test]
    fn ac16_unreadable_frozen_file_degrades_to_none_with_a_warn() {
        let tmp = tempfile::tempdir().unwrap();
        let root = task_root(tmp.path());
        // 路径被占成目录 → read_to_string 报 IO 错误（不是 NotFound）
        std::fs::create_dir_all(root.join(FROZEN_COMMIT_FILE)).unwrap();

        let logs = capture_tracing(|| {
            let got = read_frozen_candidate(&root);
            assert!(
                got.is_none(),
                "读失败必须降级为 None（不得 panic、不得中断 run），实际：{got:?}"
            );
        });
        assert!(
            logs.contains("change.frozen_candidate_unreadable"),
            "读失败要留 WARN 痕迹（change.frozen_candidate_unreadable）：{logs}"
        );

        // run 继续按旧口径新建 worktree（分支不存在 → 在基点新建）
        let fx = BranchFixture::without_branch("ac16-continue");
        let ws = crate::workspace::Workspace::create_with_candidate(&fx.spec, None).unwrap();
        assert_eq!(ws.freeze().unwrap(), fx.base, "读失败后 run 照常起工作区");
        assert_eq!(ws.base_rev, fx.base);
    }

    // ---------- 验收 6：首次语义（attempt = 0 / 1）仍然拒绝 ----------

    /// 验收 6：change_engine 侧 attempt = 1 或 attempt = 0 且同一脏分支现场（无候选）
    /// → 返回 Store 错误，文案含「非基点也非冻结候选」与「人工核验」；分支 tip 不变、
    /// worktree 未创建、**无** `workspace.retry_rebuilt_unfrozen` 留痕
    /// （首次语义完全不变）。
    /// 错实现 A（分派把 attempt <= 1 也当重试 → 首次语义被改写）→ 这条红。
    #[test]
    fn ac17_first_attempt_still_rejects_the_unfrozen_branch() {
        for attempt in [0u32, 1u32] {
            let fx = BranchFixture::with_candidate_branch(&format!("ac17-first-{attempt}"));
            let before = fx.tip.clone();

            let logs = capture_tracing(|| {
                let err = ensure_workspace(&fx.spec, None, attempt)
                    .expect_err("首次语义必须仍拒绝脏分支");
                assert!(matches!(err, DevforgeError::Store(_)), "应是 Store 错误：{err:?}");
                let msg = err.to_string();
                assert!(msg.contains("非基点也非冻结候选"), "文案要点明既非基点也非候选：{msg}");
                assert!(msg.contains("人工核验"), "文案要指向人工核验：{msg}");
                assert!(
                    !crate::workspace::Workspace::target_path(&fx.spec).exists(),
                    "拒绝时不得创建 worktree"
                );
            });
            assert_eq!(
                rev_parse(&fx.spec.repo, &format!("task/{}", fx.spec.task_id)),
                before,
                "拒绝时不得改写分支 tip（attempt={attempt}）"
            );
            assert!(
                !logs.contains("workspace.retry_rebuilt_unfrozen"),
                "首次语义不得触发重建留痕（attempt={attempt}）：{logs}"
            );
        }
    }

    // ---------- 验收 7：重试语义（attempt = 2 / 3）重建未冻结遗留分支 ----------

    /// 验收 7：change_engine 侧 attempt = 2 或 attempt = 3 且同一脏分支现场（无候选）
    /// → 重建成功、worktree HEAD == 基点、分支 tip 回到基点、留痕含完整 tip sha。
    /// 错实现 B（分派不认 attempt >= 2 / 重试入口没接上）→ 这条红。
    #[test]
    fn ac18_retry_attempt_rebuilds_the_unfrozen_branch() {
        for attempt in [2u32, 3u32] {
            let fx = BranchFixture::with_candidate_branch(&format!("ac18-retry-{attempt}"));
            let dirty_tip = fx.tip.clone();

            let logs = capture_tracing(|| {
                let ws = ensure_workspace(&fx.spec, None, attempt)
                    .expect("重试语义应重建未冻结遗留分支");
                assert!(ws.path.exists(), "重建后 worktree 目录应存在");
                assert_eq!(
                    ws.freeze().unwrap(),
                    fx.base,
                    "重建后的 worktree HEAD 必须落在基点"
                );
            });
            assert_eq!(
                rev_parse(&fx.spec.repo, &format!("task/{}", fx.spec.task_id)),
                fx.base,
                "未冻结遗留分支应被删掉并在基点重建（attempt={attempt}）"
            );
            assert!(
                logs.contains("workspace.retry_rebuilt_unfrozen"),
                "重建必须留痕（attempt={attempt}）：{logs}"
            );
            assert!(
                logs.contains(&dirty_tip),
                "留痕必须带完整 tip sha（attempt={attempt}，{dirty_tip}）：{logs}"
            );
        }
    }

    // ---------- 验收 7：重试语义 + 有可用冻结候选 → 收养 ----------

    /// 验收 7 后半：change_engine 侧 attempt = 2 且有合法冻结候选（等于分支 tip）
    /// → 收养（worktree HEAD == 候选 tip）、分支 tip 不变、**无**重建留痕
    /// （F2 行为未被本刀改写）。
    /// 错实现 C（重试入口见候选非 None 也删分支 / 重建抢在收养之前）→ 这条红。
    #[test]
    fn ac19_retry_attempt_with_a_valid_candidate_adopts_instead_of_rebuilding() {
        let fx = BranchFixture::with_candidate_branch("ac19-adopt");
        let before = fx.tip.clone();

        let logs = capture_tracing(|| {
            let ws = ensure_workspace(&fx.spec, Some(&before), 2)
                .expect("有可用冻结候选时重试语义应走收养");
            assert_eq!(
                ws.freeze().unwrap(),
                before,
                "收养：worktree HEAD 落在候选 tip"
            );
        });
        assert_eq!(
            rev_parse(&fx.spec.repo, &format!("task/{}", fx.spec.task_id)),
            before,
            "收养不得改写分支 tip"
        );
        assert!(
            !logs.contains("workspace.retry_rebuilt_unfrozen"),
            "有可用候选时不得走重建路径：{logs}"
        );
    }

    // ---------- 验收 9：重试上下文 + 分支不在基点 → 行为不变 ----------

    /// 验收 9：change_engine 侧 attempt = 2 且「分支不存在」/「分支 tip == 基点」
    /// → 行为与现状一致（worktree 在基点建/挂载），**无**重建留痕。
    /// 错实现 D（重试入口见分支就删 / 见非基点就重建）→ 这条红。
    #[test]
    fn ac20_retry_context_without_a_branch_or_at_base_does_not_rebuild() {
        // (a) 分支不存在
        let fx = BranchFixture::without_branch("ac20-absent");
        let logs = capture_tracing(|| {
            let ws = ensure_workspace(&fx.spec, None, 2).expect("分支不存在时照常在基点新建");
            assert_eq!(ws.freeze().unwrap(), fx.base);
        });
        assert!(
            !logs.contains("workspace.retry_rebuilt_unfrozen"),
            "分支不存在时不得触发重建：{logs}"
        );

        // (b) 分支停在基点
        let fx2 = BranchFixture::with_branch_at_base("ac20-at-base");
        let logs = capture_tracing(|| {
            let ws = ensure_workspace(&fx2.spec, None, 2).expect("分支停在基点时照常挂载复用");
            assert_eq!(ws.freeze().unwrap(), fx2.base);
        });
        assert!(
            !logs.contains("workspace.retry_rebuilt_unfrozen"),
            "分支停在基点时不得触发重建：{logs}"
        );
    }

    // ---------- 验收 8：run_with_retry 合成接线（线程本地 attempt 真的到位） ----------

    /// 验收 8：**真过** `crate::engine::policy::run_with_retry`（`RetryPolicy::fast()`、
    /// 未取消）的合成接线用例——第 1 次尝试造出「tip≠base、无候选」的脏分支后返回
    /// 基础设施故障（`DevforgeError::System` = 可重启），第 2 次尝试调分派函数时
    /// **线程本地 attempt 已经到 2**（重试层每次尝试开始时写入）→ 走重试入口：
    /// 重建成功、worktree HEAD == 基点、留痕含 tip sha。
    ///
    /// 整条用例不 spawn 任何子进程。写死 `set_attempt(2)` 的错实现会退化成纯注入、
    /// 不再证明 F6 现场——所以这里必须让 attempt 由重试层写、由本用例读。
    /// 错实现 E（分派读错 attempt 源 / 重试入口没接上）→ 这条红。
    #[test]
    fn ac21_run_with_retry_reaches_the_retry_entry_on_the_second_attempt() {
        let fx = BranchFixture::with_candidate_branch("ac21-wiring");
        let dirty_tip = fx.tip.clone();
        let policy = crate::engine::policy::RetryPolicy::fast();
        let control = crate::domain::runner::RunControl::new();

        let logs = capture_tracing(|| {
            let (result, attempts) = crate::engine::policy::run_with_retry(
                &policy,
                &control,
                |attempt| {
                    if attempt == 1 {
                        // 第 1 次尝试：制造基础设施故障（System = 可重启）→ 重试层进第 2 次。
                        // 现场已经是「tip≠base、无候选」的脏分支。
                        return Err(DevforgeError::System("模拟基础设施故障".into()));
                    }
                    // 第 2 次尝试：attempt 只能从线程本地读（重试层每次尝试开始时写入）。
                    // 这里读的就是生产调用点用的同一个来源（current_attempt）。
                    let attempt_now = current_attempt();
                    assert_eq!(
                        attempt_now, attempt,
                        "线程本地 attempt 必须等于本次尝试号（否则重试判定读错了源）"
                    );
                    let ws = ensure_workspace(&fx.spec, None, attempt_now)?;
                    assert_eq!(
                        ws.freeze().unwrap(),
                        fx.base,
                        "重试入口必须在基点重建"
                    );
                    Ok(crate::domain::runner::RunOutcome::Completed)
                },
            );
            assert_eq!(attempts, 2, "应当失败一次后重试一次");
            assert!(result.is_ok(), "第 2 次尝试应当成功：{result:?}");
        });

        assert!(
            logs.contains("workspace.retry_rebuilt_unfrozen"),
            "第 2 次尝试应留下重建留痕：{logs}"
        );
        assert!(
            logs.contains(&dirty_tip),
            "留痕必须带完整 tip sha（{dirty_tip}）：{logs}"
        );
        assert_eq!(
            logs.matches("workspace.retry_rebuilt_unfrozen").count(),
            1,
            "只有第 2 次尝试可以重建（第 1 次尝试的日志里不得出现重建留痕）：{logs}"
        );
        assert_eq!(
            rev_parse(&fx.spec.repo, &format!("task/{}", fx.spec.task_id)),
            fx.base,
            "未冻结遗留分支应被删掉并在基点重建"
        );

        // 线程本地 attempt 用完归零（不污染同线程后续用例；--test-threads=1 时尤其重要）
        crate::observability::set_attempt(0);
    }
}
