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

/// R9-P81：跨 agent 格式漂移不再等价「这条没验」——粗体围栏/反引号/多空格
/// 在归一化后比对（逐字节比较曾把纯格式差异判成缺条 Quality 判死）。
#[test]
fn summarize_tolerates_formatting_drift() {
    let criteria = vec!["**给定 x=1** → 返回 2".to_string()];
    // 手抄侧：无粗体 + 多一空格
    let done = vec!["给定 x=1  → 返回 2".to_string()];
    let s = summarize_acceptance(&criteria, &done);
    assert!(s.passed(), "格式差异不应判缺条：{:?}", s);
}

/// R9-P81：反引号包裹的变量名同属格式标记，两侧同剥。
#[test]
fn summarize_tolerates_backtick_variants() {
    let criteria = vec!["`count_words(a b)` 等于 2".to_string()];
    let done = vec!["count_words(a b) 等于 2".to_string()];
    let s = summarize_acceptance(&criteria, &done);
    assert!(s.passed(), "反引号差异不应判缺条：{:?}", s);
}

/// R9-P81：内容真不同仍然拦（归一化不给假绿）。
#[test]
fn summarize_real_content_diff_still_missing() {
    let criteria = vec!["给定 x=1 → 返回 2".to_string()];
    let done = vec!["给定 x=1 → 返回 3".to_string()];
    let s = summarize_acceptance(&criteria, &done);
    assert!(!s.passed(), "内容不同应仍判缺条：{:?}", s);
    assert_eq!(s.missing, vec!["给定 x=1 → 返回 2".to_string()]);
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

/// R9-P85：统计豁免与 testing 门 is_exempt_path 同源——段级 tests/docs/evidence，
/// 源码形态不豁免（R4-P49）。旧纯根前缀丢弃把 docs/ 下源码从统计里抹掉，
/// src/evidence/ 下的数据文件反而进统计（同词不同答）。
#[test]
fn diff_stats_untracked_exemption_matches_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    for args in [vec!["init", "-q"], vec!["add", "-A"]] {
        let _ = Command::new("git").args(&args).current_dir(repo).output();
    }
    std::fs::write(repo.join("base.txt"), "base\n").unwrap();
    let _ = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "add", "-A"])
        .current_dir(repo)
        .output();
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

    // 四象限：docs/ 下源码进统计（旧口径消失）；段级豁免纯文档不进；
    // src/evidence/ 下源码进（源码形态不豁免）；evidence/ 下证据不进
    std::fs::create_dir_all(repo.join("docs")).unwrap();
    std::fs::create_dir_all(repo.join("evidence")).unwrap();
    std::fs::create_dir_all(repo.join("src/evidence")).unwrap();
    std::fs::write(repo.join("docs/app.rb"), "puts 1\n").unwrap();
    std::fs::write(repo.join("evidence/run.log"), "ok\n").unwrap();
    std::fs::write(repo.join("src/evidence/store.rs"), "fn x() {}\n").unwrap();

    let stats = diff_stats(repo, &sha).unwrap();
    assert_eq!(stats.files, 2, "docs/ 源码与 src/evidence/ 源码进统计：{stats:?}");
    // insertions = docs/app.rb(1) + src/evidence/store.rs(1) = 2
    assert_eq!(stats.insertions, 2, "{stats:?}");
}

#[test]
fn write_report_creates_directory_and_file() {
    let tmp = tempfile::tempdir().unwrap();
    let change_dir = tmp.path().join("ledger").join("change-20261020-x");
    let path = write_report(&change_dir, &sample(all_ok())).unwrap();
    assert!(path.is_file());
    assert!(path.ends_with("change-20261020-x/report.md"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("变更报告"));
}
