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
}
