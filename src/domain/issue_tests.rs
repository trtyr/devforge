//! Issue 实体单测：状态转移表（含非法拒收）/ timeline / key 形态。

use super::*;

#[test]
fn human_transition_table_accepts_legal_paths() {
    let mk = |s: IssueStatus| Issue {
        status: s,
        ..Issue::new(
            "DEVFORGE-1".into(),
            "devforge",
            "t",
            "b",
            IssueSource::Operator,
            IssueSeverity::Minor,
            IssueCategory::Bug,
            vec![],
            1,
        )
    };
    // 主链
    assert!(mk(IssueStatus::Open).can_transition(IssueStatus::Triaged));
    assert!(mk(IssueStatus::Triaged).can_transition(IssueStatus::Queued));
    // 旁路终态
    assert!(mk(IssueStatus::Open).can_transition(IssueStatus::Rejected));
    assert!(mk(IssueStatus::Triaged).can_transition(IssueStatus::Wontfix));
    assert!(mk(IssueStatus::Queued).can_transition(IssueStatus::Rejected));
    // 回炉/翻案
    assert!(mk(IssueStatus::Queued).can_transition(IssueStatus::Open));
    assert!(mk(IssueStatus::Resolved).can_transition(IssueStatus::Open));
    assert!(mk(IssueStatus::Rejected).can_transition(IssueStatus::Open));
    assert!(mk(IssueStatus::Wontfix).can_transition(IssueStatus::Open));
}

#[test]
fn human_transition_table_rejects_illegal_paths() {
    let mk = |s: IssueStatus| Issue {
        status: s,
        ..Issue::new(
            "DEVFORGE-1".into(),
            "devforge",
            "t",
            "b",
            IssueSource::Operator,
            IssueSeverity::Minor,
            IssueCategory::Bug,
            vec![],
            1,
        )
    };
    // 跳级：Open 不得直转 Queued/Resolved
    assert!(!mk(IssueStatus::Open).can_transition(IssueStatus::Queued));
    assert!(!mk(IssueStatus::Open).can_transition(IssueStatus::Resolved));
    // Queued → Resolved 只能由引擎联动（auto_resolve），人工不得手标
    assert!(!mk(IssueStatus::Queued).can_transition(IssueStatus::Resolved));
    // 终态之间不横跳
    assert!(!mk(IssueStatus::Rejected).can_transition(IssueStatus::Wontfix));
    assert!(!mk(IssueStatus::Resolved).can_transition(IssueStatus::Triaged));
    // 终态只能翻案回 Open
    assert!(!mk(IssueStatus::Resolved).can_transition(IssueStatus::Rejected));
}

#[test]
fn note_appends_timeline_and_bumps_updated_at() {
    let mut i = Issue::new(
        "DEVFORGE-1".into(),
        "devforge",
        "t",
        "b",
        IssueSource::Agent,
        IssueSeverity::Major,
        IssueCategory::Flow,
        vec![],
        100,
    );
    assert_eq!(i.timeline.len(), 1, "创建即有 created 条目");
    assert_eq!(i.timeline[0].action, "created");
    assert_eq!(i.timeline[0].actor, "agent");
    i.note("system", "transitioned", "Open → Triaged", 200);
    assert_eq!(i.timeline.len(), 2);
    assert_eq!(i.updated_at_ms, 200);
    assert_eq!(i.timeline[1].detail, "Open → Triaged");
}

#[test]
fn key_shape_follows_project_prefix_ruling() {
    // 拍板：<PROJECT>-<n> 项目前缀序（大写化），key 不可变
    let mut i = Issue::new(
        "DEVFORGE-3".into(),
        "devforge",
        "t",
        "b",
        IssueSource::Operator,
        IssueSeverity::Minor,
        IssueCategory::Bug,
        vec![],
        1,
    );
    assert_eq!(i.key, "DEVFORGE-3");
    assert_eq!(i.project, "devforge");
    i.key = "DEVFORGE-4".into(); // 编译期无不可变保证，纪律断言：key 仅供展示/引用
    assert_eq!(i.key, "DEVFORGE-4");
}
