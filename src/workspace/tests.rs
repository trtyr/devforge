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
///
/// 文案口径（2026-10-29 冻结候选收养变更）：拒绝时**不得**再教人删分支——
/// 分支上可能留着唯一一份候选提交，删了它候选就变孤儿。改为指向人工核验。
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
    assert!(msg.contains("非基点也非冻结候选"), "要指明分支既非基点也非冻结候选：{msg}");
    assert!(msg.contains("人工核验"), "要指向人工核验分支当前 sha：{msg}");
    assert!(
        !msg.contains("git branch -D"),
        "拒绝文案不得教人删分支（候选会变孤儿）：{msg}"
    );
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
    // WA-03 后语义：漂移检测是 sha 对 sha——「字符串不同但同 commit」不再误报，
    // 主仓前进（移动 ref 指向新 commit）才叫漂移。旧口径「值不同即漂移」
    // 恰是审计点名的缺陷：字符串相等时 HEAD 前进了也判不出漂移。
    let mut same_sha = spec(dir.path(), &base);
    same_sha.base_rev = "HEAD~0".into(); // 解析后与 base 同一 commit → 应复用不误报
    let ws2 = Workspace::ensure(&same_sha).unwrap();
    assert_eq!(ws.path, ws2.path);
    // 真漂移：主仓前进后 HEAD 指向新 commit → 拒绝
    std::fs::write(dir.path().join("new.rs"), "fn new() {}\n").unwrap();
    git(dir.path(), &["add", "-A"]).unwrap();
    git(
        dir.path(),
        &[
            "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "advance",
        ],
    )
    .unwrap();
    let mut drifted = spec(dir.path(), &base);
    drifted.base_rev = "HEAD".into();
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

// ── 2026-10-27 自审 WA 域 majors（01a1450b 会话认领）──

/// WA-03：移动引用冻结成 sha——主仓前进后再 ensure，同 spec（base_rev=HEAD）
/// 必须报基点漂移。旧口径记录的是字符串 "HEAD"，主仓前进后仍字符串相等 →
/// 「契约显示新基点、worktree 停在旧基点」的错位不可检。
#[test]
fn moving_ref_base_rev_detects_drift_after_repo_advances() {
    let (dir, base) = repo_with_base();
    let mut s = spec(dir.path(), &base);
    s.base_rev = "HEAD".into();
    let ws = Workspace::ensure(&s).unwrap();
    assert!(ws.base_rev.starts_with(|c: char| c.is_ascii_hexdigit()), "记录应已是 sha：{}", ws.base_rev);
    // 主仓前进
    std::fs::write(dir.path().join("n.rs"), "fn n() {}\n").unwrap();
    git(dir.path(), &["add", "-A"]).unwrap();
    git(
        dir.path(),
        &[
            "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "advance",
        ],
    )
    .unwrap();
    let err = Workspace::ensure(&s).unwrap_err();
    assert!(err.to_string().contains("漂移"), "HEAD 前进后必须报漂移：{err}");
}

/// WA-04：注册表写失败必须回滚 worktree 与分支——旧实现留下无记录残骸，
/// 孤儿扫描以注册表为唯一索引永远看不见它，同一 task 再 ensure 永久报错。
#[test]
fn registry_write_failure_rolls_back_worktree_and_branch() {
    let (dir, base) = repo_with_base();
    // 把注册表目录位占领成普通文件 → record_write 的 create_dir_all 必败
    let registry = dir.path().join(".git").join("devforge-workspaces");
    std::fs::write(&registry, b"not a dir").unwrap();
    let s = spec(dir.path(), &base);
    let err = Workspace::create(&s).unwrap_err();
    assert!(err.to_string().contains("回滚"), "{err}");
    // worktree 目录与分支都不应残留
    assert!(!Workspace::target_path(&s).exists(), "worktree 应已回滚");
    assert!(
        git_out(dir.path(), &["rev-parse", "--verify", "--quiet", "task/af-batch-a"]).is_err(),
        "新建分支应已回滚"
    );
}

/// ensure 入口同样过 task_id 校验（WA-01 只护了 create，复用路径裸着——
/// target_path 拿未校验 task_id 拼 exists() 探测路径、record_read 拼注册表读路径）。
#[test]
fn ensure_entry_validates_task_id_too() {
    let (dir, base) = repo_with_base();
    let mut s = spec(dir.path(), &base);
    s.task_id = "a/../b".into();
    let err = Workspace::ensure(&s).unwrap_err();
    assert!(err.to_string().contains("非法字符"), "got: {err}");
}

/// 自审 minor（193）：契约基点不得被当成 git 选项解析（`--output=` / `--git-dir=` 一类）。
#[test]
fn option_shaped_base_rev_is_refused_before_git_sees_it() {
    let (dir, _base) = repo_with_base();
    let err = Workspace::ensure(&WorkspaceSpec {
        repo: dir.path().to_path_buf(),
        task_id: "opt-probe".into(),
        base_rev: "--output=/tmp/pwned".into(),
    })
    .unwrap_err();
    assert!(
        matches!(err, DevforgeError::Config(_)),
        "应是契约错误（Config）：{err:?}"
    );
    assert!(err.to_string().contains("形状非法"), "{err}");
}

// ── 2026-10-29 冻结候选收养（change-20261029-resume-workspace-sr1-run-7-8-10-bug-work）──

/// 造「上一轮候选」的现场：任务分支上多一个提交，worktree 已清理（分支保留）。
/// 返回候选 sha。断点续跑重建工作区时就是这个状态。
fn branch_with_candidate(dir: &Path, base: &str) -> String {
    let ws = Workspace::create(&spec(dir, base)).unwrap();
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
    let candidate = Workspace::freeze(&ws).unwrap();
    ws.remove(dir).unwrap();
    candidate
}

/// 分支当前 tip（断言「收养不改写分支引用」用）。
fn branch_tip(dir: &Path, task_id: &str) -> String {
    git_out(
        dir,
        &["rev-parse", "--verify", &format!("task/{task_id}")],
    )
    .unwrap()
    .trim()
    .to_string()
}

/// 验收 1：分支 tip == 冻结候选（≠基点）时，带候选的 ensure 必须收养——
/// worktree 目录存在、HEAD 落在候选 tip、分支引用不被改写。
#[test]
fn ensure_adopts_the_frozen_candidate_branch() {
    let (dir, base) = repo_with_base();
    let candidate = branch_with_candidate(dir.path(), &base);
    let s = spec(dir.path(), &base);

    let ws = Workspace::ensure_with_candidate(&s, Some(&candidate)).unwrap();
    assert!(ws.path.exists(), "收养后 worktree 目录应存在");
    assert_eq!(ws.path, Workspace::target_path(&s));
    assert_eq!(
        Workspace::freeze(&ws).unwrap(),
        candidate,
        "worktree HEAD 必须落在候选 tip（收养不是重建）"
    );
    assert_eq!(branch_tip(dir.path(), "af-batch-a"), candidate, "收养不得改写分支 tip");
    assert_eq!(ws.base_rev, base, "注册记录记的仍是解析后的基点，不是候选 tip");
}

/// 验收 2：create 侧的带候选入口同样收养，且分支引用前后一致。
#[test]
fn create_with_candidate_adopts_without_rewriting_the_branch_ref() {
    let (dir, base) = repo_with_base();
    let candidate = branch_with_candidate(dir.path(), &base);
    let before = branch_tip(dir.path(), "af-batch-a");

    let ws = Workspace::create_with_candidate(&spec(dir.path(), &base), Some(&candidate)).unwrap();
    assert!(ws.path.exists());
    assert_eq!(Workspace::freeze(&ws).unwrap(), candidate);
    assert_eq!(branch_tip(dir.path(), "af-batch-a"), before, "分支引用不得被改写");
    assert_eq!(ws.base_rev, base);
}

/// 验收 3：worktree 已存在（收养建好的）、注册记录 base_rev == 基点、
/// 分支 tip == 冻结候选 → 再 ensure 复用同一路径，不新建分支、不改写 tip。
#[test]
fn ensure_reuses_the_adopted_worktree_and_keeps_the_branch_tip() {
    let (dir, base) = repo_with_base();
    let candidate = branch_with_candidate(dir.path(), &base);
    let s = spec(dir.path(), &base);
    let adopted = Workspace::ensure_with_candidate(&s, Some(&candidate)).unwrap();

    let again = Workspace::ensure_with_candidate(&s, Some(&candidate)).unwrap();
    assert_eq!(again.path, adopted.path, "必须复用同一路径");
    assert_eq!(Workspace::freeze(&again).unwrap(), candidate);
    assert_eq!(branch_tip(dir.path(), "af-batch-a"), candidate);
}

/// 验收 4：分支 tip 既非基点也非传入的冻结候选（外来指针）→ Store 错误，
/// 文案含「非基点也非冻结候选」与「人工核验」，且**不含** `git branch -D`。
#[test]
fn foreign_branch_tip_is_rejected_without_destructive_command_advice() {
    let (dir, base) = repo_with_base();
    branch_with_candidate(dir.path(), &base);
    let s = spec(dir.path(), &base);

    // 传入的候选 = 基点（合法形状，但不是分支 tip）→ 属于「既非基点也非冻结候选」
    let err = Workspace::create_with_candidate(&s, Some(&base)).unwrap_err();
    assert!(matches!(err, DevforgeError::Store(_)), "应是 Store 错误：{err:?}");
    let msg = err.to_string();
    assert!(msg.contains("非基点也非冻结候选"), "{msg}");
    assert!(msg.contains("人工核验"), "{msg}");
    assert!(!msg.contains("git branch -D"), "拒绝文案不得教人删分支：{msg}");
    assert!(
        !Workspace::target_path(&s).exists(),
        "拒绝时不得留下 worktree 目录"
    );
}

/// 验收 5：冻结候选内容不合法（短 sha / 空串 / 纯空白 / 40 位 sha 后跟别的行）
/// → 不收养，走与外来指针同一条拒绝路径，且 worktree 目录不得被创建。
#[test]
fn dirty_frozen_candidate_never_adopts() {
    let cases = ["cafebabe".to_string(), String::new(), "   ".to_string()];
    for dirty in cases {
        let (dir, base) = repo_with_base();
        branch_with_candidate(dir.path(), &base);
        let s = spec(dir.path(), &base);
        let err = Workspace::create_with_candidate(&s, Some(&dirty)).unwrap_err();
        assert!(
            matches!(err, DevforgeError::Store(_)),
            "脏候选 {dirty:?} 必须走拒绝路径：{err:?}"
        );
        assert!(
            err.to_string().contains("非基点也非冻结候选"),
            "脏候选 {dirty:?} 的拒绝文案：{err}"
        );
        assert!(
            !Workspace::target_path(&s).exists(),
            "脏候选 {dirty:?} 不得创建 worktree"
        );
    }
    // 40 位 sha 后跟别的非空行 → 同样不是候选
    let (dir, base) = repo_with_base();
    branch_with_candidate(dir.path(), &base);
    let s = spec(dir.path(), &base);
    let dirty = format!("{base}\ngarbage\n");
    let err = Workspace::create_with_candidate(&s, Some(&dirty)).unwrap_err();
    assert!(err.to_string().contains("非基点也非冻结候选"), "{err}");
    assert!(!Workspace::target_path(&s).exists());
}

/// 验收 6：合法 40 位 sha 但带首尾空白 / 尾随换行 → 收养成功
///（与 change_engine 的写盘格式 `<sha>\n` 自洽）。
#[test]
fn frozen_candidate_with_surrounding_whitespace_is_adopted() {
    let (dir, base) = repo_with_base();
    let candidate = branch_with_candidate(dir.path(), &base);
    let padded = format!("  {candidate}\n");

    let ws = Workspace::create_with_candidate(&spec(dir.path(), &base), Some(&padded)).unwrap();
    assert_eq!(Workspace::freeze(&ws).unwrap(), candidate, "带空白也必须收养");
    assert_eq!(branch_tip(dir.path(), "af-batch-a"), candidate);
}

/// 验收 7：收养路径上注册表写失败 → worktree 回滚，但**候选分支不得被删**
///（run 8 事故机制：回滚误删分支 → 候选变孤儿）。
#[test]
fn adopted_workspace_rollback_keeps_the_candidate_branch() {
    let (dir, base) = repo_with_base();
    let candidate = branch_with_candidate(dir.path(), &base);
    let s = spec(dir.path(), &base);
    // 把注册表目录位占领成普通文件 → record_write 必败
    // （上一轮已建过注册表目录，先拆掉才能占位）
    let registry = dir.path().join(".git").join("devforge-workspaces");
    if registry.is_dir() {
        std::fs::remove_dir_all(&registry).unwrap();
    }
    std::fs::write(&registry, b"not a dir").unwrap();

    let err = Workspace::create_with_candidate(&s, Some(&candidate)).unwrap_err();
    assert!(err.to_string().contains("回滚"), "{err}");
    assert!(!Workspace::target_path(&s).exists(), "worktree 应已回滚");
    assert_eq!(
        branch_tip(dir.path(), "af-batch-a"),
        candidate,
        "收养路径的回滚不得删候选分支（候选会变孤儿）"
    );
}

/// 验收 8：分支 tip == 基点时，传入任何冻结候选都不改变行为——照旧挂载复用。
#[test]
fn branch_at_base_is_reused_even_when_a_candidate_is_supplied() {
    let (dir, base) = repo_with_base();
    let s = spec(dir.path(), &base);
    let ws = Workspace::create(&s).unwrap();
    ws.remove(dir.path()).unwrap();

    let other = "a".repeat(40); // 合法形状，但不是分支 tip
    let reused = Workspace::create_with_candidate(&s, Some(&other)).unwrap();
    assert_eq!(Workspace::freeze(&reused).unwrap(), base, "分支停在基点时应复用");
    assert_eq!(reused.base_rev, base);
}

/// 验收 9：分支不存在时，冻结候选不参与新建路径——照旧在基点新建分支。
#[test]
fn missing_branch_is_created_at_base_even_when_a_candidate_is_supplied() {
    let (dir, base) = repo_with_base();
    let s = spec(dir.path(), &base);
    let other = "b".repeat(40);

    let ws = Workspace::create_with_candidate(&s, Some(&other)).unwrap();
    assert_eq!(Workspace::freeze(&ws).unwrap(), base, "新建分支必须落在基点");
    assert_eq!(ws.base_rev, base);
    assert_eq!(branch_tip(dir.path(), "af-batch-a"), base);
}

// ── 验收对账用例（测试阶段独立补写）────────────────────────────────────
// change-20261029-resume-workspace-sr1-run-7-8-10-bug-work
//
// 立场：黑盒。夹具一律用**裸 git** 现场造「上一轮留下候选分支、worktree 已清理」的
// 状态，**不用**被测的 `Workspace::create` 造前置（否则用例的期望会依赖被测实现自己的
// 前置行为 → 假绿）。每条用例的注释写明「换成哪种错实现会红」——不会红的测试是装饰品。
mod acceptance {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    // ---------- 夹具 ----------

    /// 裸 git（不经被测层的 git 包装）：非零退出即 panic（带 stderr）。
    fn raw_git(repo: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
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

    /// 建仓 + 首个提交，返回基点 sha。
    fn init_repo_with_base(repo: &Path) -> String {
        raw_git(repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.rs"), "fn a() {}\n").unwrap();
        raw_git(repo, &["add", "-A"]);
        raw_git(
            repo,
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
        rev_parse(repo, "HEAD")
    }

    /// 再造一个提交，返回它的 sha。
    fn commit_file(repo: &Path, name: &str, body: &str, msg: &str) -> String {
        std::fs::write(repo.join(name), body).unwrap();
        raw_git(repo, &["add", "-A"]);
        raw_git(
            repo,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@localhost",
                "commit",
                "-q",
                "-m",
                msg,
            ],
        );
        rev_parse(repo, "HEAD")
    }

    fn spec_of(repo: &Path, task_id: &str, base: &str) -> WorkspaceSpec {
        WorkspaceSpec {
            repo: repo.to_path_buf(),
            task_id: task_id.to_string(),
            base_rev: base.to_string(),
        }
    }

    /// 「重建工作区」的现场：主仓停在基点，任务分支按需停在基点 / 候选 / 外来提交，
    /// worktree 目录不存在（重建场景）。
    struct AdoptionRepo {
        _dir: tempfile::TempDir,
        spec: WorkspaceSpec,
        base: String,
        /// 分支 tip（分支不存在时为空串）
        tip: String,
    }

    impl AdoptionRepo {
        /// 任务分支停在候选提交（≠基点）——断点续跑重建工作区时的状态。
        fn with_candidate_branch(task_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().to_path_buf();
            let base = init_repo_with_base(&repo);
            let candidate = commit_file(&repo, "candidate.rs", "fn candidate() {}\n", "candidate");
            raw_git(&repo, &["branch", &format!("task/{task_id}"), &candidate]);
            raw_git(&repo, &["reset", "-q", "--hard", &base]);
            Self {
                spec: spec_of(&repo, task_id, &base),
                base,
                tip: candidate,
                _dir: dir,
            }
        }

        /// 任务分支停在基点（上一轮没产生提交）。
        fn with_branch_at_base(task_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().to_path_buf();
            let base = init_repo_with_base(&repo);
            raw_git(&repo, &["branch", &format!("task/{task_id}"), &base]);
            Self {
                spec: spec_of(&repo, task_id, &base),
                base: base.clone(),
                tip: base,
                _dir: dir,
            }
        }

        /// 任务分支不存在（首跑）。
        fn without_branch(task_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().to_path_buf();
            let base = init_repo_with_base(&repo);
            Self {
                spec: spec_of(&repo, task_id, &base),
                base,
                tip: String::new(),
                _dir: dir,
            }
        }

        /// 任务分支停在外来提交上（既非基点、也不是传入的候选）。
        fn with_foreign_branch(task_id: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = dir.path().to_path_buf();
            let base = init_repo_with_base(&repo);
            let foreign = commit_file(&repo, "foreign.rs", "fn foreign() {}\n", "foreign");
            raw_git(&repo, &["branch", &format!("task/{task_id}"), &foreign]);
            raw_git(&repo, &["reset", "-q", "--hard", &base]);
            Self {
                spec: spec_of(&repo, task_id, &base),
                base,
                tip: foreign,
                _dir: dir,
            }
        }

        fn branch_tip(&self) -> String {
            rev_parse(&self.spec.repo, &format!("task/{}", self.spec.task_id))
        }
    }

    impl Drop for AdoptionRepo {
        fn drop(&mut self) {
            // worktree 建在仓外兄弟目录（tempdir 同级）：用例结束整目录清掉，不留残骸
            let target = Workspace::target_path(&self.spec);
            if let Some(dir) = target.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    /// 当前测试二进制的用例清单（`--list`）：证明用例**真的被编译进被执行的二进制**。
    fn test_binary_listing() -> String {
        let exe = std::env::current_exe().expect("当前测试二进制路径");
        let out = Command::new(&exe)
            .arg("--list")
            .output()
            .expect("列举测试二进制内的用例（--list）");
        assert!(
            out.status.success(),
            "--list 退出码 {:?}（stderr: {}）",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    fn crate_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    /// 递归列出仓内所有 .rs 文件。
    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                rust_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }

    // ---------- 验收 1：ensure 侧收养 ----------

    /// 验收 1：任务分支 tip == 冻结候选（≠基点）且 worktree 目录不存在 →
    /// 带冻结候选的 ensure 返回 Ok，worktree 目录存在，worktree HEAD 等于该 sha。
    /// 错实现 A（候选永不匹配 / 仍按老口径一律拒绝）→ 这条红。
    #[test]
    fn ac01_ensure_with_candidate_adopts_the_frozen_candidate_branch() {
        let fx = AdoptionRepo::with_candidate_branch("ac01-adopt");
        let before = fx.branch_tip();

        let ws = Workspace::ensure_with_candidate(&fx.spec, Some(&fx.tip)).unwrap();
        assert!(ws.path.exists(), "收养后 worktree 目录应存在");
        assert_eq!(ws.path, Workspace::target_path(&fx.spec));
        assert_eq!(
            Workspace::freeze(&ws).unwrap(),
            fx.tip,
            "worktree HEAD 必须落在候选 tip（收养不是重建）"
        );
        assert_eq!(fx.branch_tip(), before, "收养不得改写分支 tip");
        assert_eq!(ws.base_rev, fx.base, "注册记录记的仍是解析后的基点");
    }

    // ---------- 验收 2：create 侧收养 ----------

    /// 验收 2：调用 create 侧的带冻结候选入口 → 返回 Ok，worktree HEAD 等于候选 sha，
    /// 且分支引用未被改写（tip 前后一致）。
    /// 错实现 B（create 侧不认候选 / 重建分支）→ 这条红。
    #[test]
    fn ac02_create_with_candidate_adopts_and_keeps_the_branch_ref() {
        let fx = AdoptionRepo::with_candidate_branch("ac02-adopt");
        let before = fx.branch_tip();

        let ws = Workspace::create_with_candidate(&fx.spec, Some(&fx.tip)).unwrap();
        assert!(ws.path.exists());
        assert_eq!(Workspace::freeze(&ws).unwrap(), fx.tip, "HEAD 必须是候选 tip");
        assert_eq!(fx.branch_tip(), before, "分支引用前后一致（不得被改写）");
        assert_eq!(ws.base_rev, fx.base);
    }

    // ---------- 验收 3：复用已收养的工作区 ----------

    /// 验收 3：worktree 目录已存在、注册记录 base_rev == 解析后的基点、分支 tip ==
    /// 冻结候选 → 带冻结候选的 ensure 复用同一路径返回 Ok，不新建分支、不改写分支 tip。
    /// 错实现 C（复用时重建分支 / 记成候选 tip 导致下次误判漂移）→ 这条红。
    #[test]
    fn ac03_ensure_reuses_the_adopted_worktree_and_keeps_the_branch_tip() {
        let fx = AdoptionRepo::with_candidate_branch("ac03-adopt");
        let adopted = Workspace::ensure_with_candidate(&fx.spec, Some(&fx.tip)).unwrap();

        let again = Workspace::ensure_with_candidate(&fx.spec, Some(&fx.tip)).unwrap();
        assert_eq!(again.path, adopted.path, "必须复用同一路径");
        assert_eq!(Workspace::freeze(&again).unwrap(), fx.tip);
        assert_eq!(fx.branch_tip(), fx.tip, "复用不得改写分支 tip");

        // 注册记录里的 base_rev 仍是基点（不是候选 tip）——否则下一次 ensure 会误判漂移
        let record = fx
            .spec
            .repo
            .join(".git")
            .join("devforge-workspaces")
            .join(format!("{}.json", fx.spec.task_id));
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
        assert_eq!(
            parsed["base_rev"].as_str().unwrap(),
            fx.base,
            "注册记录必须记解析后的基点"
        );

        // 分支没有被重建：仓里只有一条 task/<id> 分支
        let listed = raw_git(&fx.spec.repo, &["branch", "--list", "task/ac03-adopt"]);
        assert_eq!(
            listed
                .lines()
                .filter(|l| l.contains("task/ac03-adopt"))
                .count(),
            1,
            "不得新建出第二条同名分支：{listed}"
        );
    }

    // ---------- 验收 4：外来指针 → 拒绝 + 文案 ----------

    /// 验收 4：分支 tip 既非基点也非传入的冻结候选（外来指针）→ 返回 Store 错误，
    /// 文案含「非基点也非冻结候选」与「人工核验」，且**不含**子串 `git branch -D`。
    /// 错实现 D（文案教人删分支 / 静默切基点）→ 这条红。
    #[test]
    fn ac04_foreign_branch_tip_is_rejected_with_a_non_destructive_message() {
        let fx = AdoptionRepo::with_foreign_branch("ac04-foreign");
        // 形状合法的候选，但它既不是分支 tip（也不是基点）→ 属于「外来指针」
        let candidate = "f".repeat(40);

        let err = Workspace::create_with_candidate(&fx.spec, Some(&candidate)).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, DevforgeError::Store(_)), "应是 Store 错误：{msg}");
        assert!(msg.contains("非基点也非冻结候选"), "文案要点明既非基点也非候选：{msg}");
        assert!(msg.contains("人工核验"), "文案要指向人工核验：{msg}");
        assert!(
            !msg.contains("git branch -D"),
            "拒绝文案不得教人删分支（删了候选就变孤儿）：{msg}"
        );
        assert!(
            !Workspace::target_path(&fx.spec).exists(),
            "拒绝时不得留下 worktree 目录"
        );
    }

    // ---------- 验收 5：脏冻结内容 → 不收养 ----------

    /// 验收 5：冻结候选内容为 `cafebabe` / 空串 / 纯空白 / 合法 40 位 sha 后跟其它
    /// 非空行 → 不收养：与「外来指针」同一条拒绝路径，且 worktree 目标目录未被创建。
    /// 错实现 E（形状门放宽 / 脏内容被当候选）→ 这条红。
    #[test]
    fn ac05_dirty_frozen_candidate_never_adopts() {
        let dirties = ["cafebabe".to_string(), String::new(), "   ".to_string()];
        for (i, dirty) in dirties.iter().enumerate() {
            let task_id = format!("ac05-dirty-{i}");
            let fx = AdoptionRepo::with_candidate_branch(&task_id);
            let err = Workspace::create_with_candidate(&fx.spec, Some(dirty)).unwrap_err();
            let msg = err.to_string();
            assert!(
                matches!(err, DevforgeError::Store(_)),
                "脏候选 {dirty:?} 必须走拒绝路径：{msg}"
            );
            assert!(
                msg.contains("非基点也非冻结候选"),
                "脏候选 {dirty:?} 的拒绝文案：{msg}"
            );
            assert!(
                !Workspace::target_path(&fx.spec).exists(),
                "脏候选 {dirty:?} 不得创建 worktree"
            );
        }
        // 合法 40 位 sha 后跟别的非空行 → 同样不是候选
        let fx = AdoptionRepo::with_candidate_branch("ac05-dirty-tail");
        let dirty = format!("{}\ngarbage\n", fx.base);
        let err = Workspace::create_with_candidate(&fx.spec, Some(&dirty)).unwrap_err();
        assert!(err.to_string().contains("非基点也非冻结候选"), "{err}");
        assert!(!Workspace::target_path(&fx.spec).exists());
    }

    // ---------- 验收 6：带空白的合法候选 → 收养 ----------

    /// 验收 6：冻结候选内容为合法 40 位 sha 但带首尾空白 / 尾随换行，且等于分支 tip
    /// → 收养成功，worktree HEAD 等于该 sha（与 change_engine 写盘格式自洽）。
    /// 错实现 F（不 trim / trim 后多算字符）→ 这条红。
    #[test]
    fn ac06_padded_frozen_candidate_is_adopted() {
        let fx = AdoptionRepo::with_candidate_branch("ac06-padded");
        let padded = format!("  {}\n", fx.tip);

        let ws = Workspace::create_with_candidate(&fx.spec, Some(&padded)).unwrap();
        assert_eq!(
            Workspace::freeze(&ws).unwrap(),
            fx.tip,
            "带首尾空白/尾随换行的合法候选也必须收养"
        );
        assert_eq!(fx.branch_tip(), fx.tip);

        // 大小写无关（计划 §1.3 判定口径）
        let fx2 = AdoptionRepo::with_candidate_branch("ac06-upper");
        let upper = fx2.tip.to_ascii_uppercase();
        let ws2 = Workspace::create_with_candidate(&fx2.spec, Some(&upper)).unwrap();
        assert_eq!(Workspace::freeze(&ws2).unwrap(), fx2.tip);
    }

    // ---------- 验收 7：收养路径回滚不得删候选分支 ----------

    /// 验收 7：收养路径上注册表目录被占成普通文件（写记录必失败）→ 返回 Store 错误
    /// 且 worktree 目录已回滚，同时分支 `task/<id>` 仍存在且 tip 仍等于候选 sha。
    /// 错实现 G（收养路径误置 created_branch=true → 回滚删掉候选分支 = run 8 事故）→ 这条红。
    #[test]
    fn ac07_adoption_rollback_keeps_the_candidate_branch() {
        let fx = AdoptionRepo::with_candidate_branch("ac07-rollback");
        let registry = fx.spec.repo.join(".git").join("devforge-workspaces");
        std::fs::write(&registry, b"not a dir").unwrap();

        let err = Workspace::create_with_candidate(&fx.spec, Some(&fx.tip)).unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, DevforgeError::Store(_)), "应是 Store 错误：{msg}");
        assert!(msg.contains("回滚"), "文案要点明已回滚：{msg}");
        assert!(
            !Workspace::target_path(&fx.spec).exists(),
            "worktree 应已回滚"
        );
        assert_eq!(
            fx.branch_tip(),
            fx.tip,
            "收养路径的回滚不得删候选分支（候选会变孤儿）"
        );
    }

    // ---------- 验收 8：分支停在基点 → 行为不变 ----------

    /// 验收 8：分支 tip 等于基点（无候选）且传入任意冻结候选 → 行为与现状一致：
    /// 直接挂载复用，worktree HEAD 等于基点。
    /// 错实现 H（候选优先于基点判定 / 重建分支）→ 这条红。
    #[test]
    fn ac08_branch_at_base_is_reused_even_when_a_candidate_is_supplied() {
        let fx = AdoptionRepo::with_branch_at_base("ac08-base");
        let other = "b".repeat(40); // 形状合法，但不是分支 tip

        let ws = Workspace::create_with_candidate(&fx.spec, Some(&other)).unwrap();
        assert_eq!(
            Workspace::freeze(&ws).unwrap(),
            fx.base,
            "分支停在基点时应挂载复用，HEAD 落在基点"
        );
        assert_eq!(fx.branch_tip(), fx.base, "分支 tip 不得被改写");
        assert_eq!(ws.base_rev, fx.base);
    }

    // ---------- 验收 9：分支不存在 → 行为不变 ----------

    /// 验收 9：分支不存在且传入任意冻结候选 → 行为与现状一致：新建分支在基点，
    /// worktree HEAD 等于基点（冻结候选不参与新建路径）。
    /// 错实现 I（拿候选当新建起点 → 分支不在基点）→ 这条红。
    #[test]
    fn ac09_missing_branch_is_created_at_base_even_with_a_candidate() {
        let fx = AdoptionRepo::without_branch("ac09-new");
        let other = "c".repeat(40);

        let ws = Workspace::create_with_candidate(&fx.spec, Some(&other)).unwrap();
        assert_eq!(
            Workspace::freeze(&ws).unwrap(),
            fx.base,
            "新建分支必须落在基点"
        );
        assert_eq!(fx.branch_tip(), fx.base);
        assert_eq!(ws.base_rev, fx.base);
    }

    // ---------- 验收 13：workspace 域基线用例仍在册 ----------

    /// 验收 13：现有 workspace 域 18 条用例 → 一条未被删改到消失（仍在本测试二进制
    /// 的清单里）。（「全部通过」由 evidence/regression.log 里 workspace 域 27 条
    /// 全绿的命令与退出码给出；「语义不退化」由这 18 条既有用例本身继续钉着。）
    /// 错实现 J（删掉/改名既有基线用例）→ 这条红。
    #[test]
    fn ac10_workspace_domain_baseline_cases_are_still_registered() {
        let listing = test_binary_listing();
        for name in [
            "create_makes_worktree_on_task_branch_at_base",
            "remove_drops_worktree_but_keeps_branch",
            "recreate_after_remove_succeeds_when_branch_still_at_base",
            "recreate_with_advanced_branch_is_rejected_with_actionable_message",
            "duplicate_create_is_rejected",
            "missing_directory_is_detected_as_orphan",
            "metadata_loss_is_detected_as_orphan",
            "freeze_tracks_new_commits",
            "ensure_reuses_existing_worktree_and_rejects_base_drift",
            "two_worktrees_run_concurrently_without_stepping_on_each_other",
            "task_id_with_dotdot_is_rejected",
            "task_id_with_separator_is_rejected",
            "task_id_empty_and_dot_are_rejected",
            "legal_task_id_still_creates_worktree",
            "moving_ref_base_rev_detects_drift_after_repo_advances",
            "registry_write_failure_rolls_back_worktree_and_branch",
            "ensure_entry_validates_task_id_too",
            "option_shaped_base_rev_is_refused_before_git_sees_it",
        ] {
            assert!(
                listing.contains(&format!("workspace::tests::{name}")),
                "workspace 域基线用例 {name} 必须仍在测试二进制清单里（语义不退化的前提）"
            );
        }
    }

    // ---------- 验收 14：未新增豁免 + 新用例确实在跑 ----------

    /// 验收 14：本次变更**未新增 allow 豁免**（四个改动文件里一个都没有），且本轮新增的
    /// 验收用例真的被编译进测试二进制（清单里逐条在册）。
    /// （「测试 0 失败 / clippy 0 警告」由 evidence/regression.log 的命令与退出码给出。）
    /// 错实现 K（加豁免压警告 / 新用例没进二进制）→ 这条红。
    #[test]
    fn ac11_no_allow_exemption_and_new_cases_are_registered() {
        // needle 运行时拼：写字面量会把本用例自己也匹配上
        let needle = format!("#[{k}", k = "allow(");
        for rel in [
            "src/workspace/mod.rs",
            "src/workspace/tests.rs",
            "src/server/change_engine.rs",
            "src/server/change_engine_tests.rs",
        ] {
            let src = std::fs::read_to_string(crate_root().join(rel)).unwrap();
            assert!(
                !src.contains(&needle),
                "{rel} 出现了 allow 豁免（本变更不得新增豁免）"
            );
        }

        let listing = test_binary_listing();
        for name in [
            "workspace::tests::acceptance::ac01_ensure_with_candidate_adopts_the_frozen_candidate_branch",
            "workspace::tests::acceptance::ac02_create_with_candidate_adopts_and_keeps_the_branch_ref",
            "workspace::tests::acceptance::ac03_ensure_reuses_the_adopted_worktree_and_keeps_the_branch_tip",
            "workspace::tests::acceptance::ac04_foreign_branch_tip_is_rejected_with_a_non_destructive_message",
            "workspace::tests::acceptance::ac05_dirty_frozen_candidate_never_adopts",
            "workspace::tests::acceptance::ac06_padded_frozen_candidate_is_adopted",
            "workspace::tests::acceptance::ac07_adoption_rollback_keeps_the_candidate_branch",
            "workspace::tests::acceptance::ac08_branch_at_base_is_reused_even_when_a_candidate_is_supplied",
            "workspace::tests::acceptance::ac09_missing_branch_is_created_at_base_even_with_a_candidate",
            "workspace::tests::acceptance::ac10_workspace_domain_baseline_cases_are_still_registered",
            "workspace::tests::acceptance::ac11_no_allow_exemption_and_new_cases_are_registered",
            "workspace::tests::acceptance::ac12_the_adoption_api_stays_inside_the_whitelisted_files",
            "server::change_engine::tests::acceptance::ac13_absent_frozen_file_yields_none_and_a_warn",
            "server::change_engine::tests::acceptance::ac14_frozen_file_is_passed_through_verbatim",
            "server::change_engine::tests::acceptance::ac15_dirty_frozen_content_is_read_but_never_adopted",
            "server::change_engine::tests::acceptance::ac16_unreadable_frozen_file_degrades_to_none_with_a_warn",
        ] {
            assert!(
                listing.contains(name),
                "新增用例 {name} 必须在测试二进制清单里（写了不跑等于没写）"
            );
        }
    }

    // ---------- 验收 15：改动面没有第五个文件 ----------

    /// 验收 15（重划范围后的可执行形态）：本变更新增的**收养 API**不得泄漏到白名单
    /// 四文件之外——第五个文件一旦被碰（引用新入口 / 新读盘函数 / 新事件键）这条就红。
    /// （「本次变更的文件清单恰好四文件」的命令级证据在 evidence/regression.log §4。）
    /// 错实现 L（顺手在第五个文件里改用新入口）→ 这条红。
    #[test]
    fn ac12_the_adoption_api_stays_inside_the_whitelisted_files() {
        let whitelist = [
            "src/workspace/mod.rs",
            "src/workspace/tests.rs",
            "src/server/change_engine.rs",
            "src/server/change_engine_tests.rs",
        ];
        let markers = [
            "ensure_with_candidate",
            "create_with_candidate",
            "read_frozen_candidate",
            "workspace.adopted",
        ];
        let root = crate_root();
        let mut files = Vec::new();
        rust_files(&root.join("src"), &mut files);
        assert!(!files.is_empty(), "源码目录扫不到 .rs 文件");

        let mut offenders = Vec::new();
        for file in files {
            let rel = file
                .strip_prefix(&root)
                .unwrap_or(&file)
                .to_string_lossy()
                .to_string();
            let text = std::fs::read_to_string(&file).unwrap();
            if markers.iter().any(|m| text.contains(m)) && !whitelist.contains(&rel.as_str()) {
                offenders.push(rel);
            }
        }
        assert!(
            offenders.is_empty(),
            "收养 API 泄漏到白名单四文件之外（第五个文件被触碰）：{offenders:?}"
        );
    }
}
