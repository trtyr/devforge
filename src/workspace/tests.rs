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
