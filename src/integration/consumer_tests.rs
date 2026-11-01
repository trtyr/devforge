//! 自动消费单测（gap-E）：开关关零动作 / 绿门集成 / 红门修复 / System 重试两次 /
//! 已集成判定 / rearm。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::integration::settings::{IntegrationGates, IntegrationSettings};
use crate::integration::{git, git_out};

/// git 仓夹具：init（**master** 分支——生产目标 ref 是 `refs/heads/master`）+ 首个 commit。
fn repo_with_base() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "master"]).unwrap();
    std::fs::write(dir.path().join("base.txt"), "base\n").unwrap();
    git(dir.path(), &["add", "-A"]).unwrap();
    git(dir.path(), &[
        "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base",
    ])
    .unwrap();
    let sha = git_out(dir.path(), &["rev-parse", "HEAD"]).unwrap();
    (dir, sha.trim().to_string())
}

/// 造一个候选：在仓里改文件并 commit，返回（候选，commit sha）。
fn make_candidate(repo: &Path, base: &str, task_id: &str, file: &str) -> Candidate {
    make_candidate_with(repo, base, task_id, file, &format!("{file}\n"))
}

/// 造一个写指定内容的候选（两个候选写**不同**内容且改同一文件 → merge 必冲突）。
fn make_candidate_with(
    repo: &Path,
    base: &str,
    task_id: &str,
    file: &str,
    content: &str,
) -> Candidate {
    let spec = crate::workspace::WorkspaceSpec {
        repo: repo.to_path_buf(),
        task_id: task_id.into(),
        base_rev: base.into(),
    };
    let ws = crate::workspace::Workspace::create(&spec).unwrap();
    std::fs::write(ws.path.join(file), content).unwrap();
    git(&ws.path, &["add", "-A"]).unwrap();
    git(&ws.path, &[
        "-c", "user.name=t", "-c", "user.email=t=localhost", "commit", "-q", "-m", task_id,
    ])
    .unwrap();
    let frozen = ws.freeze().unwrap();
    ws.remove(repo).unwrap();
    Candidate {
        task_id: task_id.into(),
        frozen_sha: frozen,
        base_rev: base.into(),
        owned_paths: vec![file.to_string()],
        status: CandidateStatus::Pending,
        evidence_paths: vec![],
    }
}

/// 开关设置（总开关 + 可选门）。
fn settings(enabled: bool) -> IntegrationSettings {
    IntegrationSettings {
        auto_integrate_enabled: enabled,
        gates: IntegrationGates::default(),
    }
}

/// 任务 id → 仓库路径（server 层从 store 快照取的那张表）。
fn repos(repo: &Path, task_id: &str) -> HashMap<String, PathBuf> {
    let mut m = HashMap::new();
    m.insert(task_id.to_string(), repo.to_path_buf());
    m
}

/// 入队一个候选并落盘。
fn enqueue(plans: &Path, cand: Candidate) {
    let mut q = MergeQueue::load(plans, "p").unwrap();
    q.enqueue(cand);
    q.save().unwrap();
}

/// 读回队列里的候选状态。
fn candidate_status(plans: &Path, task_id: &str) -> CandidateStatus {
    MergeQueue::load(plans, "p")
        .unwrap()
        .candidates
        .into_iter()
        .find(|c| c.task_id == task_id)
        .expect("候选应在队列")
        .status
}

/// 目标 ref 当前值。
fn target_sha(repo: &Path) -> String {
    git_out(repo, &["rev-parse", "refs/heads/master"])
        .unwrap()
        .trim()
        .to_string()
}

/// 目标 ref 上的 commit 数（用于「无新 merge commit」判定）。
fn commit_count(repo: &Path) -> usize {
    git_out(repo, &["rev-list", "--count", "refs/heads/master"])
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// 开关缺省关 → 零动作：候选仍 Pending、目标 ref 不变、**不产生集成 worktree 目录**。
///
/// 错实现形态：开关关时仍去读队列/跑 merge → 候选状态变化或工作树目录出现，本用例红。
#[test]
fn switch_off_means_zero_action() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    enqueue(plans.path(), make_candidate(repo.path(), &base, "task-off", "off.rs"));
    let before = target_sha(repo.path());

    let report = drain_project(
        &settings(false),
        "p",
        &repos(repo.path(), "task-off"),
        plans.path(),
        |_| Ok(()),
    )
    .unwrap();

    assert!(!report.enabled, "开关关时报告必须标 disabled：{report:?}");
    assert!(report.consumed.is_none(), "开关关时不得消费：{report:?}");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-off"),
            CandidateStatus::Pending
        ),
        "开关关时候选必须仍 Pending"
    );
    assert_eq!(target_sha(repo.path()), before, "开关关时目标 ref 不得动");
    // 集成 worktree 目录（`_integration`）不得被创建
    let integration_dir = repo
        .path()
        .parent()
        .unwrap()
        .join(format!(
            "{}-worktrees",
            repo.path().file_name().unwrap().to_string_lossy()
        ))
        .join("_integration");
    assert!(
        !integration_dir.exists(),
        "开关关时不得产生集成 worktree 目录：{}",
        integration_dir.display()
    );
}

/// 开关开 + 绿门 → 一轮 drain 后候选 `Integrated`，且 master 历史含候选改动。
///
/// 错实现形态：消费后不置 Integrated（或不推进目标 ref）→ 本用例红。
#[test]
fn green_gate_integrates_candidate_and_advances_master() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let cand = make_candidate(repo.path(), &base, "task-ok", "ok.rs");
    let frozen = cand.frozen_sha.clone();
    enqueue(plans.path(), cand);

    let report = drain_project(
        &settings(true),
        "p",
        &repos(repo.path(), "task-ok"),
        plans.path(),
        |_| Ok(()),
    )
    .unwrap();

    let Some(Consumed::Integrated { task_id, .. }) = report.consumed else {
        panic!("绿门应集成成功：{report:?}");
    };
    assert_eq!(task_id, "task-ok");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-ok"),
            CandidateStatus::Integrated { .. }
        ),
        "候选应落 Integrated"
    );
    // master 历史含候选改动
    let log = git_out(repo.path(), &["log", "--oneline", "refs/heads/master"]).unwrap();
    assert!(log.contains("task-ok"), "master 历史应含候选改动：{log}");
    let merged = git_out(repo.path(), &["rev-parse", "refs/heads/master"]).unwrap();
    assert_ne!(merged.trim(), base, "目标 ref 必须前进");
    // 候选改动确实在 master 的树里
    let file = git_out(repo.path(), &["show", "refs/heads/master:ok.rs"]).unwrap();
    assert!(
        file.contains("ok.rs"),
        "master 的树里应含候选文件：{file}"
    );
    assert!(!frozen.is_empty());
}

/// 红门（Quality）→ 候选 `NeedsRepair` + 产物引用 + **目标 ref 逐字不动**（零回滚语义）。
///
/// 错实现形态：红门仍推进目标 ref（或不落 NeedsRepair）→ 本用例红。
#[test]
fn red_gate_marks_needs_repair_and_leaves_master_untouched() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    enqueue(plans.path(), make_candidate(repo.path(), &base, "task-red", "red.rs"));
    let before = target_sha(repo.path());

    let report = drain_project(
        &settings(true),
        "p",
        &repos(repo.path(), "task-red"),
        plans.path(),
        |_| Err(DevforgeError::Quality("test boom_red failed".into())),
    )
    .unwrap();

    let Some(Consumed::NeedsRepair { reason, artifact, .. }) = report.consumed else {
        panic!("红门应转修复队列：{report:?}");
    };
    assert!(reason.contains("boom_red"), "修复理由要带失败摘要：{reason}");
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&artifact).unwrap()).unwrap();
    assert_eq!(body["kind"], "validation_failed", "{body}");
    assert_eq!(body["attribution"], "quality", "{body}");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-red"),
            CandidateStatus::NeedsRepair { .. }
        ),
        "候选应落 NeedsRepair"
    );
    // 产物路径必须进候选的 evidence_paths（需求 4：修复候选要能引用到失败产物）。
    // 错实现形态：产物写了但不进 evidence_paths → 本用例红。
    let queue = MergeQueue::load(plans.path(), "p").unwrap();
    let cand = queue
        .candidates
        .iter()
        .find(|c| c.task_id == "task-red")
        .expect("候选应在队列");
    assert_eq!(
        cand.evidence_paths,
        vec![artifact.to_string_lossy().to_string()],
        "产物路径必须进 evidence_paths：{:?}",
        cand.evidence_paths
    );
    assert_eq!(target_sha(repo.path()), before, "红门时目标 ref 不得前进（零回滚）");
}

/// 基础设施失败第一次 → 候选仍 `Pending` + 侧车计数 = 1 + 目标 ref 不变。
///
/// 错实现形态：第一次就落 NeedsRepair（基础设施抖动污染质量记录）→ 本用例红。
#[test]
fn first_infra_failure_keeps_pending_and_counts_retry() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    enqueue(plans.path(), make_candidate(repo.path(), &base, "task-infra", "infra.rs"));
    let before = target_sha(repo.path());

    let report = drain_project(
        &settings(true),
        "p",
        &repos(repo.path(), "task-infra"),
        plans.path(),
        |_| Err(DevforgeError::System("cargo 起不来".into())),
    )
    .unwrap();

    let Some(Consumed::InfraRetry { infra_failures, .. }) = report.consumed else {
        panic!("第一次基础设施失败应保留 Pending 并记重试：{report:?}");
    };
    assert_eq!(infra_failures, 1, "第一次失败计数应为 1");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-infra"),
            CandidateStatus::Pending
        ),
        "第一次基础设施失败候选必须仍 Pending"
    );
    assert_eq!(target_sha(repo.path()), before, "基础设施失败时目标 ref 不得动");
    // 不落产物（基础设施失败不是质量失败——写一份假质量记录会污染候选质量画像）
    assert!(
        !plans.path().join("integrations/p/artifacts").exists(),
        "基础设施失败不得写产物"
    );
    // 侧车计数落盘
    let retries_path = plans
        .path()
        .join("integrations")
        .join("p")
        .join("retries.json");
    let body = std::fs::read_to_string(&retries_path).expect("侧车计数文件应落盘");
    assert!(body.contains("infra_failures"), "{body}");
    assert!(body.contains("task-infra"), "{body}");
}

/// 同一候选连续两次基础设施失败 → 第二次落 `NeedsRepair` + `validation_infra` 产物 + 计数键清除。
///
/// 错实现形态：第二次仍只记重试（永不落修复队列）→ 本用例红。
#[test]
fn second_infra_failure_marks_needs_repair_and_clears_retry_key() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let cand = make_candidate(repo.path(), &base, "task-infra2", "infra2.rs");
    let frozen = cand.frozen_sha.clone();
    enqueue(plans.path(), cand);
    let repos = repos(repo.path(), "task-infra2");

    let first = drain_project(&settings(true), "p", &repos, plans.path(), |_| {
        Err(DevforgeError::System("cargo 起不来".into()))
    })
    .unwrap();
    assert!(matches!(
        first.consumed,
        Some(Consumed::InfraRetry { infra_failures: 1, .. })
    ));

    let second = drain_project(&settings(true), "p", &repos, plans.path(), |_| {
        Err(DevforgeError::System("cargo 起不来".into()))
    })
    .unwrap();

    let Some(Consumed::NeedsRepair { artifact, .. }) = second.consumed else {
        panic!("第二次基础设施失败应落修复队列：{second:?}");
    };
    let body = std::fs::read_to_string(&artifact).unwrap();
    assert!(body.contains("validation_infra"), "{body}");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-infra2"),
            CandidateStatus::NeedsRepair { .. }
        ),
        "第二次基础设施失败候选应落 NeedsRepair"
    );
    // 计数键清除（该候选不再占重试额度）
    let retries_path = plans
        .path()
        .join("integrations")
        .join("p")
        .join("retries.json");
    let body = std::fs::read_to_string(&retries_path).unwrap();
    assert!(
        !body.contains(&frozen),
        "重试耗尽后该候选的计数键必须清除：{body}"
    );
}

/// 候选已是目标 ref 祖先（CAS 成功但队列未落盘）→ 直接 `Integrated`，
/// 目标 ref 不变、**无新 commit**。
///
/// 错实现形态：重复 merge（多出 merge commit）→ 本用例红。
#[test]
fn already_integrated_candidate_is_not_merged_again() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let cand = make_candidate(repo.path(), &base, "task-done", "done.rs");
    let frozen = cand.frozen_sha.clone();
    // 模拟「CAS 已推进目标 ref」：把候选 commit 合进 master
    git(repo.path(), &["merge", "--no-ff", "--no-edit", &frozen]).unwrap();
    let before_tip = target_sha(repo.path());
    let before_count = commit_count(repo.path());
    enqueue(plans.path(), cand);

    let report = drain_project(
        &settings(true),
        "p",
        &repos(repo.path(), "task-done"),
        plans.path(),
        |_| Ok(()),
    )
    .unwrap();

    let Some(Consumed::AlreadyIntegrated { integrated_sha, .. }) = report.consumed else {
        panic!("已在目标历史里的候选应直接置 Integrated：{report:?}");
    };
    assert_eq!(integrated_sha, frozen, "integrated_sha 应是候选 commit");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-done"),
            CandidateStatus::Integrated { integrated_sha } if integrated_sha == frozen
        ),
        "候选应落 Integrated 且 sha 为候选 commit"
    );
    assert_eq!(target_sha(repo.path()), before_tip, "目标 ref 不得动");
    assert_eq!(
        commit_count(repo.path()),
        before_count,
        "不得产生新 merge commit"
    );
}

/// `NeedsRebase` 候选 → 本轮先 rearm 成 `Pending` 并被实际消费（状态离开 NeedsRebase）。
///
/// 错实现形态：rearm 只在 tick 侧做（消费者漏掉）→ 候选停在 NeedsRebase，本用例红。
#[test]
fn needs_rebase_candidate_is_rearmed_and_consumed() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let mut cand = make_candidate(repo.path(), &base, "task-reb", "reb.rs");
    cand.status = CandidateStatus::NeedsRebase {
        target_moved_to: "beefcafe".into(),
    };
    enqueue(plans.path(), cand);

    let report = drain_project(
        &settings(true),
        "p",
        &repos(repo.path(), "task-reb"),
        plans.path(),
        |_| Ok(()),
    )
    .unwrap();

    assert_eq!(report.rearmed, 1, "本轮应 rearm 1 个候选：{report:?}");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-reb"),
            CandidateStatus::Integrated { .. }
        ),
        "rearm 后应被实际消费：{:?}",
        candidate_status(plans.path(), "task-reb")
    );
}

/// 合并冲突 → 候选 `NeedsRepair` + 产物 `kind == "merge_conflict"` 且 `attribution == "quality"`
/// + 产物路径进候选的 `evidence_paths`（需求 4：冲突产物也要可被修复候选引用）。
///
/// 错实现形态：冲突产物不写 attribution，或路径不进 evidence_paths → 本用例红。
#[test]
fn merge_conflict_artifact_is_referenced_by_the_candidate() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    // 两个候选改同一个文件且内容不同 → 后者 merge 必冲突
    let first = make_candidate(repo.path(), &base, "task-c1", "shared.rs");
    let second = make_candidate_with(
        repo.path(),
        &base,
        "task-c2",
        "shared.rs",
        "task-c2 的不同改动\n",
    );
    let mut q = MergeQueue::load(plans.path(), "p").unwrap();
    q.enqueue(first);
    q.enqueue(second);
    q.save().unwrap();
    let mut repos_map = repos(repo.path(), "task-c1");
    repos_map.insert("task-c2".to_string(), repo.path().to_path_buf());

    // 第一轮：先合入 task-c1（目标 ref 前进）
    let r1 = drain_project(&settings(true), "p", &repos_map, plans.path(), |_| Ok(())).unwrap();
    assert!(
        matches!(r1.consumed, Some(Consumed::Integrated { .. })),
        "第一轮应集成首个候选：{r1:?}"
    );

    // 第二轮：task-c2 与已合入的 task-c1 冲突 → 修复队列 + 冲突产物
    let r2 = drain_project(&settings(true), "p", &repos_map, plans.path(), |_| Ok(())).unwrap();
    let Some(Consumed::NeedsRepair { artifact, .. }) = r2.consumed else {
        panic!("冲突应转修复队列：{r2:?}");
    };
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&artifact).unwrap()).unwrap();
    assert_eq!(body["kind"], "merge_conflict", "{body}");
    assert_eq!(body["attribution"], "quality", "{body}");
    let queue = MergeQueue::load(plans.path(), "p").unwrap();
    let conflicted = queue
        .candidates
        .iter()
        .find(|c| c.task_id == "task-c2")
        .expect("冲突候选应在队列");
    assert_eq!(
        conflicted.evidence_paths,
        vec![artifact.to_string_lossy().to_string()],
        "冲突产物路径必须进 evidence_paths：{:?}",
        conflicted.evidence_paths
    );
    assert!(
        matches!(conflicted.status, CandidateStatus::NeedsRepair { .. }),
        "冲突候选应落 NeedsRepair：{:?}",
        conflicted.status
    );
}

/// 目标 ref 不可读（已集成判定失败 = System）→ 与验证失败**同一条重试路径**：
/// 第一次保留 `Pending` + 记一次重试，第二次才落 `NeedsRepair` + `validation_infra` 产物
/// + 清计数键。
///
/// 错实现形态：判定失败直接冒出 `drain_project`（绕过侧车计数）→ 候选永停 `Pending`、
/// 永不转修复队列 → 本用例红。
#[test]
fn unreadable_target_ref_falls_into_the_same_infra_retry_path() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let cand = make_candidate(repo.path(), &base, "task-noref", "noref.rs");
    let frozen = cand.frozen_sha.clone();
    enqueue(plans.path(), cand);
    // 目标 ref 不可读：把 master 删掉（已集成判定会拿到非 0/1 的退出码 → System）
    git(repo.path(), &["update-ref", "-d", "refs/heads/master"]).unwrap();
    let repos_map = repos(repo.path(), "task-noref");

    // 第一次：保留 Pending + 记一次重试
    let first = drain_project(&settings(true), "p", &repos_map, plans.path(), |_| Ok(())).unwrap();
    let Some(Consumed::InfraRetry { infra_failures, .. }) = first.consumed else {
        panic!("目标 ref 不可读应走基础设施重试路径：{first:?}");
    };
    assert_eq!(infra_failures, 1, "第一次失败计数应为 1");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-noref"),
            CandidateStatus::Pending
        ),
        "第一次基础设施失败候选必须仍 Pending"
    );

    // 第二次：仍 System → 落修复队列 + 产物 + 清计数键
    let second = drain_project(&settings(true), "p", &repos_map, plans.path(), |_| Ok(())).unwrap();
    let Some(Consumed::NeedsRepair { artifact, .. }) = second.consumed else {
        panic!("第二次基础设施失败应落修复队列：{second:?}");
    };
    let body: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&artifact).unwrap()).unwrap();
    assert_eq!(body["kind"], "validation_infra", "{body}");
    assert_eq!(body["attribution"], "system", "{body}");
    assert_eq!(body["stage"], "merge_base", "产物应点名失败环节：{body}");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-noref"),
            CandidateStatus::NeedsRepair { .. }
        ),
        "第二次基础设施失败候选应落 NeedsRepair"
    );
    // 计数键清除（该候选不再占重试额度）
    let retries_path = plans
        .path()
        .join("integrations")
        .join("p")
        .join("retries.json");
    let body = std::fs::read_to_string(&retries_path).unwrap();
    assert!(
        !body.contains(&frozen),
        "重试耗尽后该候选的计数键必须清除：{body}"
    );
}

/// 候选缺目标仓路径 → 跳过并留痕，不静默卡死（其他候选仍可被消费）。
///
/// 错实现形态：直接 panic 或整个队列停摆 → 本用例红。
#[test]
fn candidate_without_repo_is_skipped_and_logged() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    let mut orphan = make_candidate(repo.path(), &base, "task-orphan", "orphan.rs");
    orphan.status = CandidateStatus::Pending;
    enqueue(plans.path(), orphan);
    // repos 里只有另一个任务 → 孤儿候选没有仓库路径
    let mut r = HashMap::new();
    r.insert("other".to_string(), repo.path().to_path_buf());

    let report = drain_project(&settings(true), "p", &r, plans.path(), |_| Ok(())).unwrap();
    assert!(report.consumed.is_none(), "无可消费候选：{report:?}");
    assert!(
        matches!(
            candidate_status(plans.path(), "task-orphan"),
            CandidateStatus::Pending
        ),
        "缺仓库路径的候选应保持 Pending（等仓库可知）"
    );
}

/// 开关运行中从关切到开 → 下一轮 drain 即消费（热生效，不靠进程内状态重置）。
///
/// 错实现形态：开关态在进程内缓存（首次读到的值粘住）→ 第二轮仍不消费，本用例红。
#[test]
fn switch_toggled_at_runtime_takes_effect_on_next_drain() {
    let (repo, base) = repo_with_base();
    let plans = tempfile::tempdir().unwrap();
    enqueue(plans.path(), make_candidate(repo.path(), &base, "task-hot", "hot.rs"));
    let r = repos(repo.path(), "task-hot");

    // 第一轮：开关关 → 不消费
    let first = drain_project(&settings(false), "p", &r, plans.path(), |_| Ok(())).unwrap();
    assert!(first.consumed.is_none(), "开关关时不得消费：{first:?}");

    // 第二轮：开关开 → 立刻消费（每轮重读，无进程内状态残留）
    let second = drain_project(&settings(true), "p", &r, plans.path(), |_| Ok(())).unwrap();
    assert!(
        matches!(second.consumed, Some(Consumed::Integrated { .. })),
        "开关切开后下一轮必须消费：{second:?}"
    );
}
