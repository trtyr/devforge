//! change 编排器（P009 T013）：阶段推进 + coding→plan 反向回退 + 往返上限 3。
//!
//! 与 MVP 编排的区别（不是「单向 vs 带回路」——MVP 每段本来就有 agent⇄review）：
//! - 多一个**反向回退**：coding 发现计划漏了文件 → 申报 → 回 plan 补边界 → 重进 coding；
//! - 多一个**基线锚点**：diff、白名单、回归都对着基线；
//! - 断点由引擎写（这里复用 plan 010 的 `StateStore`，跨会话不丢往返计数）。
//!
//! 本模块只管**编排**：它不懂 prompt，也不碰子进程；每一阶段的真实执行由 [`ChangeSession`]
//! 注入（生产是 pi 子进程，单测是脚本化的假 session）。

use std::path::Path;

use crate::change::boundary::{self, BoundaryVerdict, ImpactGap};
use crate::change::report::{self, AcceptanceSummary, ChangeReport, summarize_acceptance};
use crate::change::state::{ChangePhase, ChangeState, load, save};
use crate::error::{DevforgeError, Result};
use crate::runstate::StateStore;

/// 测试阶段失败后回注 coding 的防御性上限（决策只给了 plan↔coding 的 3 次；这里防止死循环）。
pub const MAX_TEST_REROUTES: usize = 3;

/// 计划阶段结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanTurn {
    Pass,
    Fail { issues: Vec<String> },
}

/// coding 阶段结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingTurn {
    /// agent 的完整回复（含可能的 `<route>plan</route>` + `<impact-gap>`）。
    pub reply: String,
    /// 相对基线的改动文件（`git diff --name-only <baseline>`）。
    pub changed: Vec<String>,
}

/// 测试阶段结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestingTurn {
    Pass {
        /// 已对账通过的验收项。
        acceptance_done: Vec<String>,
    },
    /// 回归/测试失败 → 回注 coding（不给定位）。
    RouteCoding { failures: Vec<String> },
    /// 发现计划层面的问题 → 回 plan。
    RoutePlan { failures: Vec<String> },
}

/// 各阶段的执行器（生产= pi 子进程；单测=脚本化假实现）。
pub trait ChangeSession {
    /// 跑一次 plan（含审查）；`roundtrip` 用于让实现知道这是第几次往返。
    fn run_plan(&mut self, roundtrip: usize) -> Result<PlanTurn>;
    /// 带缺口打回 plan：只补边界并重审。
    fn amend_plan(&mut self, gaps: &[ImpactGap], roundtrip: usize) -> Result<PlanTurn>;
    /// 跑 coding（含按步提交）。
    fn run_coding(&mut self, roundtrip: usize) -> Result<CodingTurn>;
    /// 跑 testing（回归优先 + 验收对账）。
    fn run_testing(&mut self) -> Result<TestingTurn>;
}

/// 编排输入。
pub struct ChangeRun<'a> {
    pub store: &'a StateStore,
    /// 目标仓（diff、报告都落这里）。
    pub repo: &'a Path,
    pub change_id: &'a str,
    pub baseline_sha: &'a str,
    /// 计划的验收标准（对账用）。
    pub criteria: Vec<String>,
    /// 计划的白名单（初始）。
    pub whitelist: Vec<String>,
    /// 用量行（会话账汇总；空串则写「未记录」）。
    pub usage: String,
    pub run_id: String,
    /// 墙钟（调用方计时；单测给固定值）。
    pub wall_secs: u64,
}

/// 编排结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeOutcome {
    pub state: ChangeState,
    pub report: ChangeReport,
}

/// 跑完整条 change 流程（从断点继续）。
pub fn run<S: ChangeSession>(run: ChangeRun<'_>, session: &mut S) -> Result<ChangeOutcome> {
    let mut state = load(run.store, run.change_id, run.baseline_sha);
    if state.baseline_sha.is_empty() {
        state.baseline_sha = run.baseline_sha.to_string();
    }
    let mut whitelist = run.whitelist.clone();

    // ---- plan ----
    if matches!(state.phase, ChangePhase::Plan) {
        match session.run_plan(state.roundtrips)? {
            PlanTurn::Pass => {
                state.phase = ChangePhase::Coding;
                save(run.store, &state)?;
            }
            PlanTurn::Fail { issues } => {
                return Err(DevforgeError::Quality(format!(
                    "计划审查未通过：{}",
                    issues.join("；")
                )));
            }
        }
    }

    // ---- coding ⇄ plan ----
    if matches!(state.phase, ChangePhase::Coding) {
        loop {
            let turn = session.run_coding(state.roundtrips)?;
            match boundary::classify(&turn.changed, &whitelist, &turn.reply) {
                BoundaryVerdict::InBoundary => {
                    state.phase = ChangePhase::Testing;
                    save(run.store, &state)?;
                    break;
                }
                BoundaryVerdict::OutOfBoundary { extra } => {
                    return Err(DevforgeError::Quality(format!(
                        "越界改动未申报（范围膨胀）：{}",
                        extra.join("、")
                    )));
                }
                BoundaryVerdict::RoutePlan { gaps } => {
                    let next = state.roundtrips + 1;
                    boundary::check_roundtrip(next)?;
                    state.bump_roundtrip();
                    boundary::merge_gaps_into_whitelist(&mut whitelist, &gaps);
                    match session.amend_plan(&gaps, state.roundtrips)? {
                        PlanTurn::Pass => {
                            save(run.store, &state)?;
                            continue;
                        }
                        PlanTurn::Fail { issues } => {
                            return Err(DevforgeError::Quality(format!(
                                "补边界后计划审查仍未过：{}",
                                issues.join("；")
                            )));
                        }
                    }
                }
            }
        }
    }

    // ---- testing ----
    // 失败会以 Quality 结束本次调用，并把阶段回推到 coding/plan；下次 start 从断点继续
    // （回注次数记在断点里，跳会话累计，不会被重启清零）。
    let mut acceptance_done: Vec<String> = state.acceptance_done.clone();
    if matches!(state.phase, ChangePhase::Testing) {
        match session.run_testing()? {
            TestingTurn::Pass {
                acceptance_done: done,
            } => {
                for c in done {
                    state.mark_acceptance_done(&c);
                }
                acceptance_done = state.acceptance_done.clone();
                let summary = summarize_acceptance(&run.criteria, &acceptance_done);
                if !summary.passed() {
                    return Err(DevforgeError::Quality(format!(
                        "验收对账缺 {} 条：{}",
                        summary.missing.len(),
                        summary.missing.join("；")
                    )));
                }
                state.phase = ChangePhase::Done;
                save(run.store, &state)?;
            }
            TestingTurn::RouteCoding { failures } => {
                state.bump_test_reroute();
                if state.test_reroutes > MAX_TEST_REROUTES {
                    save(run.store, &state)?;
                    return Err(DevforgeError::Quality(format!(
                        "测试→coding 已回注 {} 次（上限 {MAX_TEST_REROUTES}），停止：{}",
                        state.test_reroutes,
                        failures.join("；")
                    )));
                }
                // 回注只带摘要（不给 path:line 定位）——测试 agent 读行为不读代码
                let msg = crate::change::testing_gate::route_back_message("coding", &failures);
                tracing::warn!(component = "change-orchestrator", %msg, "测试失败回注 coding");
                state.phase = ChangePhase::Coding;
                save(run.store, &state)?;
                return Err(DevforgeError::Quality(format!(
                    "测试未通过，已回注 coding。{msg}"
                )));
            }
            TestingTurn::RoutePlan { failures } => {
                let msg = crate::change::testing_gate::route_back_message("plan", &failures);
                state.phase = ChangePhase::Plan;
                save(run.store, &state)?;
                return Err(DevforgeError::Quality(format!(
                    "测试发现计划问题，已回注 plan。{msg}"
                )));
            }
        }
    }

    // ---- 报告（引擎写）----
    let diff = report::diff_stats(run.repo, run.baseline_sha).unwrap_or_default();
    let acceptance: AcceptanceSummary = summarize_acceptance(&run.criteria, &acceptance_done);
    let timeline = ChangeReport {
        change_id: run.change_id.to_string(),
        run_id: run.run_id.clone(),
        baseline_sha: state.baseline_sha.clone(),
        wall_secs: run.wall_secs,
        diff,
        review_rounds: state.review_rounds,
        roundtrips: state.roundtrips,
        usage: if run.usage.trim().is_empty() {
            "未记录".to_string()
        } else {
            run.usage.clone()
        },
        acceptance,
    };
    Ok(ChangeOutcome {
        state,
        report: timeline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runstate::StateKey;
    use std::collections::VecDeque;

    fn store(root: &Path) -> StateStore {
        StateStore::new(
            root,
            StateKey::new("proj", "task", crate::change::state::CHANGE_SOP),
        )
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// 脚本化 session：按队列吐出各阶段结果，并记录调用序列。
    struct Scripted {
        plans: VecDeque<PlanTurn>,
        amends: VecDeque<PlanTurn>,
        codings: VecDeque<CodingTurn>,
        testings: VecDeque<TestingTurn>,
        pub log: Vec<String>,
    }

    impl Scripted {
        fn new() -> Self {
            Self {
                plans: VecDeque::new(),
                amends: VecDeque::new(),
                codings: VecDeque::new(),
                testings: VecDeque::new(),
                log: Vec::new(),
            }
        }
    }

    impl ChangeSession for Scripted {
        fn run_plan(&mut self, rt: usize) -> Result<PlanTurn> {
            self.log.push(format!("plan(rt={rt})"));
            Ok(self.plans.pop_front().expect("plan 脚本耗尽"))
        }
        fn amend_plan(&mut self, gaps: &[ImpactGap], rt: usize) -> Result<PlanTurn> {
            self.log.push(format!("amend(rt={rt},gaps={})", gaps.len()));
            Ok(self.amends.pop_front().expect("amend 脚本耗尽"))
        }
        fn run_coding(&mut self, rt: usize) -> Result<CodingTurn> {
            self.log.push(format!("coding(rt={rt})"));
            Ok(self.codings.pop_front().expect("coding 脚本耗尽"))
        }
        fn run_testing(&mut self) -> Result<TestingTurn> {
            self.log.push("testing".into());
            Ok(self.testings.pop_front().expect("testing 脚本耗尽"))
        }
    }

    fn in_boundary() -> CodingTurn {
        CodingTurn {
            reply: "干完了\n<stage-complete>1</stage-complete>".into(),
            changed: s(&["src/a.rs"]),
        }
    }

    fn declared_gap() -> CodingTurn {
        declared_gap_file("src/extra.rs")
    }

    fn declared_gap_file(file: &str) -> CodingTurn {
        CodingTurn {
            reply: format!(
                "计划漏了\n<route>plan</route>\n<impact-gap>\n- 文件：{file}\n- 为什么必须改：接口在这\n- 证据：{file}:9\n</impact-gap>"
            ),
            changed: s(&["src/a.rs", file]),
        }
    }

    fn pass_testing() -> TestingTurn {
        TestingTurn::Pass {
            acceptance_done: s(&["x=1 → 2"]),
        }
    }

    fn cfg<'a>(store: &'a StateStore, repo: &'a Path, wl: &[&str], criteria: &[&str]) -> ChangeRun<'a> {
        ChangeRun {
            store,
            repo,
            change_id: "change-20261020-x",
            baseline_sha: "base123",
            criteria: s(criteria),
            whitelist: s(wl),
            usage: "1,000 tokens（in 900 / out 100）".into(),
            run_id: "run-1".into(),
            wall_secs: 12,
        }
    }

    #[test]
    fn happy_path_plan_coding_testing_report() {
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut session = Scripted::new();
        session.plans.push_back(PlanTurn::Pass);
        session.codings.push_back(in_boundary());
        session.testings.push_back(pass_testing());

        let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session).unwrap();
        assert_eq!(out.state.phase, ChangePhase::Done);
        assert_eq!(out.report.roundtrips, 0);
        assert_eq!(
            session.log,
            vec!["plan(rt=0)", "coding(rt=0)", "testing"],
            "顺序必须是 plan → coding → testing"
        );
        assert!(out.report.render().contains("验收对账: 通过"));
    }

    #[test]
    fn coding_routes_back_to_plan_then_reenters_coding() {
        // 审计要求：单测重现「coding → route plan → 改边界 → 重进 coding」全回路
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut session = Scripted::new();
        session.plans.push_back(PlanTurn::Pass);
        session.codings.push_back(declared_gap());
        session.amends.push_back(PlanTurn::Pass);
        // 补完边界后重进 coding：这次改动全在白名单里
        session.codings.push_back(CodingTurn {
            reply: "补完了".into(),
            changed: s(&["src/a.rs", "src/extra.rs"]),
        });
        session.testings.push_back(pass_testing());

        let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session).unwrap();
        assert_eq!(out.state.phase, ChangePhase::Done);
        assert_eq!(out.state.roundtrips, 1, "往返计数必须记下来");
        assert_eq!(
            session.log,
            vec![
                "plan(rt=0)",
                "coding(rt=0)",
                "amend(rt=1,gaps=1)",
                "coding(rt=1)",
                "testing"
            ],
            "补完边界必须重进 coding"
        );
        // 断点里往返计数已落盘
        let saved = load(&st, "change-20261020-x", "base123");
        assert_eq!(saved.roundtrips, 1);
    }

    #[test]
    fn out_of_boundary_without_declaration_fails_and_stays_coding() {
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut session = Scripted::new();
        session.plans.push_back(PlanTurn::Pass);
        session.codings.push_back(CodingTurn {
            reply: "顺手改的".into(),
            changed: s(&["src/a.rs", "src/rogue.rs"]),
        });

        let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session).unwrap_err();
        assert!(err.to_string().contains("范围膨胀"), "{err}");
        assert!(err.is_quality_failure());
        let saved = load(&st, "change-20261020-x", "base123");
        assert_eq!(saved.phase, ChangePhase::Coding, "不能把失败当推进");
        assert_eq!(saved.roundtrips, 0);
    }

    #[test]
    fn fourth_roundtrip_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut session = Scripted::new();
        session.plans.push_back(PlanTurn::Pass);
        // 连续四次「越界+申报」→ 第 4 次（roundtrips 3→4）必须报错；
        // 每次都报不同的文件，否则上一轮补进白名单后就不再越界了
        for i in 0..4 {
            session
                .codings
                .push_back(declared_gap_file(&format!("src/extra{i}.rs")));
        }
        for _ in 0..3 {
            session.amends.push_back(PlanTurn::Pass);
        }

        let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session).unwrap_err();
        assert!(err.to_string().contains("上限 3"), "{err}");
        assert!(err.is_quality_failure(), "往返超限不该自动重启");
        let saved = load(&st, "change-20261020-x", "base123");
        assert_eq!(saved.roundtrips, 3, "停在第四次之前");
    }

    #[test]
    fn resume_from_checkpoint_skips_plan_and_keeps_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        // 预置断点：plan 已完成、coding 阶段、已往返 2 次
        let mut seeded = ChangeState::new("change-20261020-x", "base123");
        seeded.phase = ChangePhase::Coding;
        seeded.bump_roundtrip();
        seeded.bump_roundtrip();
        save(&st, &seeded).unwrap();

        let mut session = Scripted::new();
        session.codings.push_back(in_boundary());
        session.testings.push_back(pass_testing());

        let out = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session).unwrap();
        assert_eq!(out.state.roundtrips, 2, "续跑不得清零往返计数");
        assert_eq!(out.report.roundtrips, 2);
        assert!(
            !session.log.iter().any(|l| l.starts_with("plan")),
            "plan 已完成，续跑不该重跑：{:?}",
            session.log
        );
    }

    #[test]
    fn testing_failure_routes_back_and_is_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut session = Scripted::new();
        session.plans.push_back(PlanTurn::Pass);
        session.codings.push_back(in_boundary());
        session.testings.push_back(TestingTurn::RouteCoding {
            failures: s(&["tests/test_a.py:12 断言失败"]),
        });

        let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("回注 coding"), "{msg}");
        assert!(!msg.contains("test_a.py:12"), "回注不得带定位：{msg}");
        let saved = load(&st, "change-20261020-x", "base123");
        assert_eq!(saved.phase, ChangePhase::Coding, "应回到 coding");
    }

    #[test]
    fn test_reroutes_are_bounded_across_sessions() {
        // 回注次数记在断点里：跳会话累计，不会被重启清零
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut seeded = ChangeState::new("change-20261020-x", "base123");
        seeded.phase = ChangePhase::Testing;
        seeded.test_reroutes = MAX_TEST_REROUTES;
        save(&st, &seeded).unwrap();

        let mut session = Scripted::new();
        session.testings.push_back(TestingTurn::RouteCoding {
            failures: s(&["回归红了"]),
        });

        let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2"]), &mut session).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("上限 3"), "{msg}");
        let saved = load(&st, "change-20261020-x", "base123");
        assert_eq!(saved.test_reroutes, MAX_TEST_REROUTES + 1);
    }

    #[test]
    fn acceptance_missing_one_item_fails_run() {
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut session = Scripted::new();
        session.plans.push_back(PlanTurn::Pass);
        session.codings.push_back(in_boundary());
        session.testings.push_back(TestingTurn::Pass {
            acceptance_done: s(&["x=1 → 2"]),
        });

        let err = run(
            cfg(&st, tmp.path(), &["src/a.rs"], &["x=1 → 2", "空输入报错"]),
            &mut session,
        )
        .unwrap_err();
        assert!(err.to_string().contains("缺 1 条"), "{err}");
        assert!(err.to_string().contains("空输入报错"), "{err}");
    }

    #[test]
    fn plan_rejection_is_quality_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let st = store(tmp.path());
        let mut session = Scripted::new();
        session.plans.push_back(PlanTurn::Fail {
            issues: vec!["影响面缺调用方".into()],
        });
        let err = run(cfg(&st, tmp.path(), &["src/a.rs"], &["x"]), &mut session).unwrap_err();
        assert!(err.to_string().contains("计划审查未通过"), "{err}");
        assert!(err.is_quality_failure());
    }
}
