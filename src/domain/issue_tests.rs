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

#[test]
fn agent_report_parse_args_validates() {
    // P020-k5：工具 args → 载荷的校验面
    let ok = super::AgentIssueReport::parse_args(
        &serde_json::json!({"title": " t ", "body": "b", "severity": "critical", "category": "flow"}),
        "devforge",
        "t1",
        "run-1",
    )
    .expect("合法参数应解析");
    assert_eq!(ok.title, "t", "title 要 trim");
    assert_eq!(ok.severity, super::IssueSeverity::Critical);
    assert_eq!(ok.category, super::IssueCategory::Flow);
    assert_eq!(ok.project, "devforge");
    assert_eq!(ok.task_id, "t1");
    assert_eq!(ok.run_id, "run-1");

    assert!(
        super::AgentIssueReport::parse_args(
            &serde_json::json!({"body": "x", "severity": "info", "category": "bug"}),
            "p",
            "t",
            "r"
        )
        .is_none(),
        "缺 title 拒收"
    );
    assert!(
        super::AgentIssueReport::parse_args(
            &serde_json::json!({"title": "t", "severity": "urgent", "category": "bug"}),
            "p",
            "t",
            "r"
        )
        .is_none(),
        "severity 枚举越界拒收"
    );
    assert!(
        super::AgentIssueReport::parse_args(
            &serde_json::json!({"title": "   ", "body": "", "severity": "info", "category": "bug"}),
            "p",
            "t",
            "r"
        )
        .is_none(),
        "纯空白 title 拒收"
    );
}
