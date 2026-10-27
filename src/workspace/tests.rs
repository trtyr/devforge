//! （2026-10-26 测试分离：自 src/workspace/mod.rs 内联平移，纯移动零改动。）
use super::*;

/// git 仓夹具：init + 首个 commit；返回（目录，基线 sha）。
fn repo_with_base() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]).unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn a() {}\n").unwrap();
    git(dir.path(), &["add", "-A"]).unwrap();
    git(dir.path(), &[
        "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base",
    ])
    .unwrap();
    let base = git_out(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    (dir, base.trim().to_string())
}

fn spec(dir: &Path, base: &str) -> WorkspaceSpec {
    WorkspaceSpec {
        repo: dir.to_path_buf(),
        task_id: "af-batch-a".into(),
        base_rev: base.to_string(),
    }
}

#[test]
fn create_makes_worktree_on_task_branch_at_base() {
    let (dir, base) = repo_with_base();
    let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
    assert!(ws.path.exists(), "worktree 目录应存在");
    assert_eq!(ws.branch, "task/af-batch-a");
    // 分支与基点对齐：worktree HEAD == base
    let head = Workspace::freeze(&ws).unwrap();
    assert_eq!(head, base, "worktree 应 pin 在基点");
    // spawn 视角的 cwd 就在 worktree 里，不在 main
    assert_ne!(ws.path, dir.path());
}

#[test]
fn remove_drops_worktree_but_keeps_branch() {
    let (dir, base) = repo_with_base();
    let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
    let branch = ws.branch.clone();
    ws.remove(dir.path()).unwrap();
    assert!(!ws.path.exists(), "worktree 目录应被移除");
    // 分支保留（候选归因要它）
    let branches = git_out(dir.path(), &["branch", "--list", &branch]).unwrap();
    assert!(branches.contains(&branch), "分支应保留：{branches}");
    // 注册表摘除
    assert!(scan_orphans(dir.path()).unwrap().is_empty());
}

/// 自审 critical 回归：`remove()` 保留分支，同一 task 的第二次 run **必须能重建**
/// （旧实现无条件 `-b` → git 报 branch already exists，第二次 run 必输）。
#[test]
fn recreate_after_remove_succeeds_when_branch_still_at_base() {
    let (dir, base) = repo_with_base();
    let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
    ws.remove(dir.path()).unwrap();
    let again = Workspace::create(&spec(dir.path(), &base)).expect("分支仍在基点，应能复用重建");
    assert!(again.path.exists());
    assert_eq!(Workspace::freeze(&again).unwrap(), base);
}

/// 分支上已有候选提交（≠基点）时，重建必须**明确拒绝并指路**，
/// 不得静默切基点（否则本轮 diff 归因错位）。
#[test]
fn recreate_with_advanced_branch_is_rejected_with_actionable_message() {
    let (dir, base) = repo_with_base();
    let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
    std::fs::write(ws.path.join("candidate.txt"), "x\n").unwrap();
    git(&ws.path, &["add", "-A"]).unwrap();
    git(
        &ws.path,
        &[
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "candidate",
        ],
    )
    .unwrap();
    ws.remove(dir.path()).unwrap();
    let err = Workspace::create(&spec(dir.path(), &base)).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("已存在"), "要指明分支已存在：{msg}");
    assert!(msg.contains("git branch -D"), "要给出清理命令：{msg}");
}

#[test]
fn duplicate_create_is_rejected() {
    let (dir, base) = repo_with_base();
    Workspace::create(&spec(dir.path(), &base)).unwrap();
    let err = Workspace::create(&spec(dir.path(), &base)).unwrap_err();
    assert!(err.to_string().contains("已存在"), "{err}");
}

#[test]
fn missing_directory_is_detected_as_orphan() {
    let (dir, base) = repo_with_base();
    let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
    // 模拟手删：物理删目录（worktree 元数据还在 main 的 .git/worktrees 里）
    std::fs::remove_dir_all(&ws.path).unwrap();
    let orphans = scan_orphans(dir.path()).unwrap();
    assert_eq!(orphans.len(), 1, "{orphans:?}");
    assert_eq!(orphans[0].task_id, "af-batch-a");
    remove_orphan(dir.path(), &orphans[0]).unwrap();
    assert!(scan_orphans(dir.path()).unwrap().is_empty(), "清理后无孤儿");
}

#[test]
fn metadata_loss_is_detected_as_orphan() {
    let (dir, base) = repo_with_base();
    let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
    // 模拟 worktree 元数据被删（目录在但 git 不认了）
    git(dir.path(), &["worktree", "remove", "--force", &ws.path.to_string_lossy()])
        .unwrap();
    std::fs::create_dir_all(&ws.path).unwrap();
    std::fs::write(ws.path.join("residue.txt"), "残骸").unwrap();
    let orphans = scan_orphans(dir.path()).unwrap();
    assert_eq!(orphans.len(), 1, "{orphans:?}");
    remove_orphan(dir.path(), &orphans[0]).unwrap();
    assert!(!ws.path.join("residue.txt").exists(), "残骸应被物理清除");
}

#[test]
fn freeze_tracks_new_commits() {
    let (dir, base) = repo_with_base();
    let ws = Workspace::create(&spec(dir.path(), &base)).unwrap();
    std::fs::write(ws.path.join("changed.rs"), "fn changed() {}\n").unwrap();
    git(&ws.path, &["add", "-A"]).unwrap();
    git(&ws.path, &[
        "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "step 1",
    ])
    .unwrap();
    let frozen = Workspace::freeze(&ws).unwrap();
    assert_ne!(frozen, base, "冻结 commit 应随新提交前进");
    // base..frozen 的 diff 归因：改动只有 changed.rs
    let diff = git_out(&ws.path, &["diff", "--name-only", &format!("{base}..{frozen}")]);
    assert!(diff.unwrap().contains("changed.rs"));
}

#[test]
fn ensure_reuses_existing_worktree_and_rejects_base_drift() {
    // T1.4 resume 对齐：worktree 在 → 复用；base 漂移 → 拒绝
    let (dir, base) = repo_with_base();
    let s = spec(dir.path(), &base);
    let ws = Workspace::ensure(&s).unwrap();
    // resume：同 spec 再 ensure → 同一 worktree 复用
    let ws2 = Workspace::ensure(&s).unwrap();
    assert_eq!(ws.path, ws2.path);
    // base 漂移：换 base_rev 再 ensure → 拒绝
    let mut drifted = spec(dir.path(), &base);
    drifted.base_rev = "HEAD~0".into(); // 值不同即漂移（同 sha 也会被字符串不等拦住）
    let err = Workspace::ensure(&drifted).unwrap_err();
    assert!(err.to_string().contains("漂移"), "{err}");
}

#[test]
fn two_worktrees_run_concurrently_without_stepping_on_each_other() {
    // M1 验收：两 worktree 并发各跑一任务不互踩——同仓不同 task，各改各的文件
    let (dir, base) = repo_with_base();
    let mut spec_a = spec(dir.path(), &base);
    spec_a.task_id = "task-a".into();
    let mut spec_b = spec(dir.path(), &base);
    spec_b.task_id = "task-b".into();
    let wa = Workspace::create(&spec_a).unwrap();
    let wb = Workspace::create(&spec_b).unwrap();
    assert_ne!(wa.path, wb.path);
    // 各自改动各自的文件并提交
    std::fs::write(wa.path.join("from-a.rs"), "// a\n").unwrap();
    std::fs::write(wb.path.join("from-b.rs"), "// b\n").unwrap();
    for (w, msg) in [(&wa, "task-a step"), (&wb, "task-b step")] {
        git(&w.path, &["add", "-A"]).unwrap();
        git(&w.path, &[
            "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", msg,
        ])
        .unwrap();
    }
    // 互不踩踏：a 看不到 b 的文件，反之亦然
    assert!(!wa.path.join("from-b.rs").exists());
    assert!(!wb.path.join("from-a.rs").exists());
    assert!(wa.path.join("from-a.rs").exists());
    assert!(wb.path.join("from-b.rs").exists());
    // main 工作树不被直改
    assert!(!dir.path().join("from-a.rs").exists());
    assert!(!dir.path().join("from-b.rs").exists());
}

// ── P012-T004 · WA-01：task_id 路径安全校验 ──

#[test]
fn task_id_with_dotdot_is_rejected() {
    let (dir, base) = repo_with_base();
    let mut s = spec(dir.path(), &base);
    s.task_id = "a/../b".into();
    let err = Workspace::create(&s).unwrap_err();
    assert!(err.to_string().contains("非法字符"), "got: {err}");
}

#[test]
fn task_id_with_separator_is_rejected() {
    let (dir, base) = repo_with_base();
    let mut s = spec(dir.path(), &base);
    s.task_id = "batch/a".into();
    let err = Workspace::create(&s).unwrap_err();
    assert!(err.to_string().contains("非法字符"), "got: {err}");
}

#[test]
fn task_id_empty_and_dot_are_rejected() {
    let (dir, base) = repo_with_base();
    for bad in ["", ".", ".."] {
        let mut s = spec(dir.path(), &base);
        s.task_id = bad.to_string();
        assert!(Workspace::create(&s).is_err(), "task_id {bad:?} 应被拒");
    }
}

#[test]
fn legal_task_id_still_creates_worktree() {
    let (dir, base) = repo_with_base();
    let mut s = spec(dir.path(), &base);
    s.task_id = "af-batch-a".into();
    let w = Workspace::create(&s).unwrap();
    assert!(w.path.exists());
}
