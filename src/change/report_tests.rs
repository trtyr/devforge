//! （2026-10-26 测试分离：自 src/change/report.rs 内联平移，纯移动零改动。）
use super::*;

fn sample(acceptance: AcceptanceSummary) -> ChangeReport {
    ChangeReport {
        change_id: "change-20261020-x".into(),
        run_id: "run-1".into(),
        baseline_sha: "abc123".into(),
        wall_secs: 42,
        diff: DiffStats {
            files: 3,
            insertions: 10,
            deletions: 2,
        },
        review_rounds: 4,
        roundtrips: 1,
        usage: "1,234 tokens（in 1000 / out 234），成本 $0.0100".into(),
        acceptance,
    }
}

fn all_ok() -> AcceptanceSummary {
    AcceptanceSummary {
        total: 2,
        done: 2,
        missing: vec![],
    }
}

#[test]
fn render_carries_every_required_field() {
    let text = sample(all_ok()).render();
    for anchor in [
        "change-20261020-x",
        "run-1",
        "abc123",
        "42s",
        "+10 / -2",
        "审查轮次: 4",
        "往返: 1",
        "tokens",
        "验收对账: 通过",
    ] {
        assert!(text.contains(anchor), "报告缺字段：{anchor}\n{text}");
    }
}

#[test]
fn failed_reconciliation_is_visible_in_report() {
    let text = sample(AcceptanceSummary {
        total: 2,
        done: 1,
        missing: vec!["空输入报错".into()],
    })
    .render();
    assert!(text.contains("未通过"), "{text}");
    assert!(text.contains("缺 1 条"), "{text}");
    assert!(text.contains("## 未对账的验收标准"), "{text}");
    assert!(text.contains("空输入报错"), "{text}");
}

#[test]
fn empty_criteria_is_reported_as_anomaly() {
    let text = sample(AcceptanceSummary {
        total: 0,
        done: 0,
        missing: vec![],
    })
    .render();
    assert!(text.contains("无验收标准"), "{text}");
}

#[test]
fn summarize_counts_missing() {
    let criteria = vec!["a".to_string(), "b".to_string()];
    let done = vec!["a".to_string()];
    let s = summarize_acceptance(&criteria, &done);
    assert_eq!(s.total, 2);
    assert_eq!(s.done, 1);
    assert_eq!(s.missing, vec!["b".to_string()]);
    assert!(!s.passed());
}

#[test]
fn diff_stats_reads_numstat() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    for args in [vec!["init", "-q"], vec!["add", "-A"]] {
        let _ = Command::new("git").args(&args).current_dir(repo).output();
    }
    // 基线提交
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "add", "-A"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", "base"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success());
    let sha = String::from_utf8_lossy(
        &Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_string();

    // 改动一个文件 + 新增一个（新增文件先入索引：coding 按步提交后 diff 才看得到它）
    std::fs::write(repo.join("a.txt"), "a\nb\nc\n").unwrap();
    std::fs::write(repo.join("b.txt"), "x\n").unwrap();
    let out = Command::new("git")
        .args(["add", "b.txt"])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success());

    let stats = diff_stats(repo, &sha).unwrap();
    assert_eq!(stats.files, 2, "{stats:?}");
    assert_eq!(stats.insertions, 3, "{stats:?}");
    assert_eq!(stats.deletions, 0, "{stats:?}");
}

#[test]
fn write_report_creates_directory_and_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_report(tmp.path(), "change-20261020-x", &sample(all_ok())).unwrap();
    assert!(path.is_file());
    assert!(path.ends_with("docs/plantree/changes/change-20261020-x/report.md"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("变更报告"));
}
