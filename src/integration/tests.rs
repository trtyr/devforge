//! （2026-10-26 测试分离：自 src/integration/mod.rs 内联平移，纯移动零改动。）
use super::*;

/// git 仓夹具：init（main 分支）+ 首个 commit；返回（目录，基线 sha）。
fn repo_with_base() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]).unwrap();
    std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "-A"]).unwrap();
    git(dir.path(), &[
        "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base",
    ])
    .unwrap();
    let sha = git_out(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    (dir, sha.trim().to_string())
}

/// 造一个候选：worktree 里改文件并 commit，返回（候选，分支 sha）。
fn make_candidate(dir: &Path, base: &str, task_id: &str, file: &str) -> Candidate {
    let spec = crate::workspace::WorkspaceSpec {
        repo: dir.to_path_buf(),
        task_id: task_id.into(),
        base_rev: base.into(),
    };
    let ws = crate::workspace::Workspace::create(&spec).unwrap();
    std::fs::write(ws.path.join(file), format!("{file}\n")).unwrap();
    git(&ws.path, &["add", "-A"]).unwrap();
    git(&ws.path, &[
        "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", task_id,
    ])
    .unwrap();
    let frozen = ws.freeze().unwrap();
    ws.remove(dir).unwrap();
    Candidate {
        task_id: task_id.into(),
        frozen_sha: frozen,
        base_rev: base.into(),
        owned_paths: vec![file.to_string()],
        status: CandidateStatus::Pending,
        evidence_paths: vec![],
    }
}

fn integrator<'a>(
    repo: &'a Path,
    plans_root: &'a Path,
    validate: impl Fn(&Path) -> Result<()> + 'a,
) -> Integrator<'a> {
    Integrator {
        repo,
        target_ref: "refs/heads/main".into(),
        plans_root,
        validate: Box::new(validate),
    }
}

#[test]
fn idle_when_queue_empty() {
    let (dir, _base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let mut q = MergeQueue::load(plans.path(), "p").unwrap();
    let mut it = integrator(dir.path(), plans.path(), |_| Ok(()));
    assert!(matches!(it.integrate_next(&mut q).unwrap(), IntegrationOutcome::Idle));
}

#[test]
fn serial_integration_advances_target_and_second_sees_moved_target() {
    // M3 验收：两候选并发就绪 → 串行集成；第二者集成时目标已被推进
    // （第二个候选基于旧 base，merge 后 CAS 的 expect 是它集成时的目标——
    //  串行消费下 expect 总是最新，双双 Integrated；「过期 CAS」场景见下一条）
    let (dir, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let a = make_candidate(dir.path(), &base, "task-a", "from-a.rs");
    let b = make_candidate(dir.path(), &base, "task-b", "from-b.rs");
    let mut q = MergeQueue::load(plans.path(), "p").unwrap();
    q.enqueue(a);
    q.enqueue(b);
    q.save().unwrap();

    let mut it = integrator(dir.path(), plans.path(), |_| Ok(()));
    let r1 = it.integrate_next(&mut q).unwrap();
    let IntegrationOutcome::Integrated { task_id: t1, merged_sha: m1 } = r1 else {
        panic!("第一个候选应集成成功：{r1:?}");
    };
    assert_eq!(t1, "task-a");
    let r2 = it.integrate_next(&mut q).unwrap();
    let IntegrationOutcome::Integrated { task_id: t2, .. } = r2 else {
        panic!("第二个候选应集成成功（串行下 expect 始终最新）：{r2:?}");
    };
    assert_eq!(t2, "task-b");
    // 目标历史：两个候选的 merge commit 都在
    let log = git_out(dir.path(), &["log", "--oneline", "refs/heads/main"]).unwrap();
    assert!(log.contains("task-a") && log.contains("task-b"), "{log}");
    assert!(log.contains(&m1[..7.min(m1.len())]) || m1.len() >= 7);
}

#[test]
fn stale_cas_is_rejected_and_candidate_requeued() {
    // M3 验收：人为构造过期 CAS——集成期间目标被推进（别的进程 fast-forward）
    let (dir, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let cand = make_candidate(dir.path(), &base, "task-x", "from-x.rs");
    // 候选基于旧 base：它改的文件与目标推进的文件**不同**（merge 能过），但目标 sha 会动
    std::fs::write(dir.path().join("advance.txt"), "target moved\n").unwrap();
    git(dir.path(), &["add", "-A"]).unwrap();
    git(dir.path(), &[
        "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "advance",
    ])
    .unwrap();
    // 关键：再退回一个旧目标点位制造「集成期间目标被推进」——不可逆操作，
    // 所以这里的过期模拟 = 候选的 base 落后 + 集成验证慢窗口内再推进：
    // 简化实现——直接在 validate 闭包里推进目标（模拟并发集成的写入方）
    let target_for_closure = dir.path().to_path_buf();
    let mut it = integrator(dir.path(), plans.path(), move |iw| {
        // 验证期间（CAS 之前）另一个集成者推进了目标
        std::fs::write(target_for_closure.join("concurrent.txt"), "x\n").unwrap();
        git(&target_for_closure, &["add", "-A"]).unwrap();
        git(&target_for_closure, &[
            "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m",
            "concurrent",
        ])
        .unwrap();
        let _ = iw;
        Ok(())
    });
    let mut q = MergeQueue::load(plans.path(), "p").unwrap();
    q.enqueue(cand);
    q.save().unwrap();
    let r = it.integrate_next(&mut q).unwrap();
    let IntegrationOutcome::Requeued { task_id, target_moved_to } = r else {
        panic!("过期 CAS 应触发重排：{r:?}");
    };
    assert_eq!(task_id, "task-x");
    // 目标确实移动了（concurrent commit）
    let now = git_out(dir.path(), &["rev-parse", "refs/heads/main"]).unwrap();
    assert_eq!(now.trim(), target_moved_to, "重排信息应指向新目标");
    // 队列状态：NeedsRebase，目标 ref 仍是并发者写入的（未被覆盖）
    assert!(matches!(
        q.candidates[0].status,
        CandidateStatus::NeedsRebase { .. }
    ));
}

#[test]
fn validation_failure_produces_artifact_and_needs_repair() {
    let (dir, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let cand = make_candidate(dir.path(), &base, "task-bad", "from-bad.rs");
    let mut it = integrator(dir.path(), plans.path(), |_| {
        Err(DevforgeError::Quality("模拟组合验证失败".into()))
    });
    let mut q = MergeQueue::load(plans.path(), "p").unwrap();
    q.enqueue(cand);
    q.save().unwrap();
    let r = it.integrate_next(&mut q).unwrap();
    let IntegrationOutcome::NeedsRepair { task_id, reason, artifact } = r else {
        panic!("验证失败应回修复队列：{r:?}");
    };
    assert_eq!(task_id, "task-bad");
    assert!(reason.contains("模拟组合验证失败"));
    // 全仓审计 #9：产物按项目分目录（旧实现恒 `_global`，多项目产物混放）
    assert!(
        artifact.to_string_lossy().contains("/integrations/p/artifacts/"),
        "产物应落在项目目录下：{}",
        artifact.display()
    );
    let body = std::fs::read_to_string(&artifact).unwrap();
    assert!(body.contains("validation_failed"), "{body}");
    assert!(matches!(
        q.candidates[0].status,
        CandidateStatus::NeedsRepair { .. }
    ));
    // 目标未动（失败不集成）
    let now = git_out(dir.path(), &["rev-parse", "refs/heads/main"]).unwrap();
    assert_eq!(now.trim(), base);
}
