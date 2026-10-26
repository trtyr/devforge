//! （2026-10-26 测试分离：自 src/change/prepare.rs 内联平移，纯移动零改动。）
use super::*;

fn repo_with_code() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/a.rs"), "// a\n").unwrap();
    tmp
}

#[test]
fn bare_repo_gets_baseline_and_blackboard_skeleton() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_code();
    let plans = tmp.path().join("plans");

    let prep = prepare(
        &plans,
        repo.path(),
        "proj",
        "task1",
        "add book api",
        "none",
        (2026, 10, 20),
    )
    .unwrap();

    assert_eq!(prep.change_id, "change-20261020-add-book-api");
    // 计划目录必须落在台账根（2026-10-25 反转 D003：台账不进仓）
    assert_eq!(
        prep.change_dir,
        plantree_root(&plans, "proj")
            .join("changes")
            .join(&prep.change_id),
        "change_dir 指的是台账根下的计划目录，已不在目标仓内"
    );
    assert!(
        !prep.change_dir.starts_with(repo.path()),
        "计划目录不得落在目标仓（台账不进仓）"
    );
    assert!(prep.change_dir.is_dir());
    assert!(
        !prep.change_dir.starts_with(&prep.blackboard),
        "计划目录与黑板是两处，不能混"
    );
    let baseline_txt = prep.change_dir.join("baseline.txt");
    assert!(baseline_txt.is_file(), "baseline.txt 要落在计划目录");
    assert_eq!(
        std::fs::read_to_string(&baseline_txt).unwrap().trim(),
        prep.baseline.sha
    );
    assert!(prep.baseline.created_repo, "无 .git 时应新建仓库");
    for sub in ["evidence", "testing/reports", "coding/reports"] {
        assert!(
            prep.blackboard.join(sub).is_dir(),
            "黑板缺子目录 {sub}：{}",
            prep.blackboard.display()
        );
    }
    assert!(
        prep.blackboard.ends_with("proj/task1/change"),
        "黑板应是 SOP 级目录 plans/<p>/<t>/change：{}",
        prep.blackboard.display()
    );
}

#[test]
fn none_level_does_not_touch_docs() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_code();
    prepare(
        &tmp.path().join("plans"),
        repo.path(),
        "p",
        "t",
        "x",
        "none",
        (2026, 10, 20),
    )
    .unwrap();
    assert!(!repo.path().join("docs/contexts").exists());
}

#[test]
fn essential_level_emits_inventory_then_proceeds() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_code();
    let prep = prepare(
        &tmp.path().join("plans"),
        repo.path(),
        "p",
        "t",
        "x",
        "essential",
        (2026, 10, 20),
    )
    .unwrap();
    assert_eq!(prep.doc_action, DocAction::EssentialDone);
    assert!(repo.path().join("docs/contexts/_meta/inventory.json").is_file());
}

#[test]
fn complete_level_is_refused_with_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_code();
    let err = prepare(
        &tmp.path().join("plans"),
        repo.path(),
        "p",
        "t",
        "x",
        "complete",
        (2026, 10, 20),
    )
    .unwrap_err();
    assert!(err.to_string().contains("doc-from-zero"), "{err}");
}

#[test]
fn repeated_prepare_reuses_change_id() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_code();
    let plans = tmp.path().join("plans");
    let first = prepare(&plans, repo.path(), "p", "t", "add book", "none", (2026, 10, 20)).unwrap();
    let second =
        prepare(&plans, repo.path(), "p", "t", "totally different words", "none", (2026, 10, 21))
            .unwrap();
    assert_eq!(first.change_id, second.change_id, "同一任务必须复用 change-id");
    assert_eq!(first.change_dir, second.change_dir);
    // 已有基线不重建仓库
    assert!(!second.baseline.created_repo);
    assert_eq!(first.baseline.sha, second.baseline.sha);
}

#[test]
fn missing_repo_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let err = prepare(
        tmp.path(),
        &tmp.path().join("nope"),
        "p",
        "t",
        "x",
        "none",
        (2026, 10, 20),
    )
    .unwrap_err();
    assert!(err.to_string().contains("不存在"), "{err}");
}

#[test]
fn bad_doc_level_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = repo_with_code();
    let err = prepare(
        tmp.path(),
        repo.path(),
        "p",
        "t",
        "x",
        "full",
        (2026, 10, 20),
    )
    .unwrap_err();
    assert!(err.to_string().contains("doc_level"), "{err}");
}
