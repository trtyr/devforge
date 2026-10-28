use super::{MarkerNudge, MarkerStep, MAX_MARKER_NUDGES};

/// 上限内一直要求补，第 limit+1 次才判超限——边界不能差一。
#[test]
fn bounded_allows_exactly_limit_nudges() {
    let mut gate = MarkerNudge::bounded(3);
    for expected in 1..=3 {
        assert_eq!(gate.observe(false), MarkerStep::Nudge(expected));
        assert_eq!(gate.nudges(), expected);
    }
    assert_eq!(gate.observe(false), MarkerStep::Exhausted(4));
    assert_eq!(gate.nudges(), 4);
}

/// 交了就打回原点：下一条链路的第一次失败必须又从 1 开始数。
#[test]
fn success_resets_the_streak() {
    let mut gate = MarkerNudge::bounded(3);
    gate.observe(false);
    gate.observe(false);
    assert_eq!(gate.observe(true), MarkerStep::Done);
    assert_eq!(gate.nudges(), 0);
    assert_eq!(gate.observe(false), MarkerStep::Nudge(1));
}

/// 错号等「不是缺标记」的失败也走同一个计数——否则模型反复答错就能无限重问。
#[test]
fn bump_counts_without_a_presence_check() {
    let mut gate = MarkerNudge::bounded(1);
    assert_eq!(gate.bump(), MarkerStep::Nudge(1));
    assert_eq!(gate.bump(), MarkerStep::Exhausted(2));
}

/// 零上限入口永不耗尽（decisions/003：MVP 规划阶段不设硬限制）。
#[test]
fn unlimited_never_exhausts() {
    let mut gate = MarkerNudge::unlimited();
    for _ in 0..50 {
        assert!(matches!(gate.observe(false), MarkerStep::Nudge(_)));
    }
    assert_eq!(gate.nudges(), 50);
    assert_eq!(gate.observe(true), MarkerStep::Done);
}

/// 上限字段要能被超限文案直接引用（「已追补 N 次」），不能是私有魔数。
#[test]
fn limit_is_reported_for_the_error_message() {
    let gate = MarkerNudge::bounded(MAX_MARKER_NUDGES);
    assert_eq!(gate.limit(), 3);
    assert_eq!(MAX_MARKER_NUDGES, 3);
}
