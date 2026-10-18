//! Testing SOP 布局与发现记录协议（P007 · 第三个内置 SOP）。
//!
//! 布局常量 = plans/007-testing-stage decisions/README.md 的代码化（D009 findings 台账；
//! 单一事实源在文档，这里只固化约定）。解析类协议（stage-complete 复用 / reroute）见同模块
//! 后续小节，保守缺省哲学与 planning/coding 同源（P8-ISSUE-10 家族）。

use std::path::Path;

use crate::error::Result;

// ===== 黑板布局常量 =====

/// testing 阶段过程资产子树根（任务目录下）。
pub const TESTING_DIR: &str = "testing";
/// 测试阶段划分与状态（Test Agent 写，Review 审计）。
pub const TESTING_STAGES_FILE: &str = "testing/stages.md";
/// 当前阶段 todo list（随阶段滚动）。
pub const TESTING_TODO_FILE: &str = "testing/todo.md";
/// findings 台账目录（D009：一条发现一个文件，blockers 同款形制）。
pub const FINDINGS_DIR: &str = "testing/findings";
/// 追溯矩阵（Q007：文档章节 | 需求点 | 对应测试 | 状态）。
pub const COVERAGE_MATRIX_FILE: &str = "testing/coverage-matrix.md";
/// Review Report 目录（review-N.md / doc-review-N.md）。
pub const TESTING_REPORTS_DIR: &str = "testing/reports";
/// 偏差留痕目录（沿用 coding 偏差协议形制，decisions/005 家族）。
pub const TESTING_BLOCKERS_DIR: &str = "testing/blockers";

/// 骨架目录集合（引擎在 run_testing 前创建；stages/todo/coverage-matrix 由 Agent 自写）。
pub const TESTING_SKELETON_DIRS: &[&str] = &[FINDINGS_DIR, TESTING_REPORTS_DIR, TESTING_BLOCKERS_DIR];

/// 创建 testing 黑板骨架（幂等：已存在不报错）。
pub fn create_testing_skeleton(blackboard: &Path) -> Result<()> {
    for dir in TESTING_SKELETON_DIRS {
        std::fs::create_dir_all(blackboard.join(dir))?;
    }
    Ok(())
}

// ===== findings 记录协议（D009） =====

/// 发现处置状态机（D009：open→triaged→fixed→verified）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FindingStatus {
    /// 新记录，未处置。
    #[default]
    Open,
    /// 已裁决（改码/改档/接受）。
    Triaged,
    /// 处置动作已完成。
    Fixed,
    /// 回归验证关闭。
    Verified,
}

impl FindingStatus {
    /// 从状态行文本解析；未知/空 → `Open`（保守缺省：未明确标记的一律按未处置）。
    pub fn parse(text: &str) -> Self {
        match text.trim() {
            "triaged" | "Triaged" | "已裁决" => Self::Triaged,
            "fixed" | "Fixed" | "已修复" => Self::Fixed,
            "verified" | "Verified" | "已验证" => Self::Verified,
            _ => Self::Open,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Triaged => "triaged",
            Self::Fixed => "fixed",
            Self::Verified => "verified",
        }
    }
}

/// 一条发现的解析结果（字段缺失 → 空串；解析永不报错——Agent 写的文件只信结构不信内容）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindingRecord {
    /// 文档出处（如 `coding/docs/02-todo-api.md §3.2`）。
    pub doc_source: String,
    /// 文档说（预期行为）。
    pub doc_says: String,
    /// 实测（系统实际行为）。
    pub observed: String,
    /// 复现步骤。
    pub repro: String,
    /// 严重度（high/medium/low，自由文本不约束）。
    pub severity: String,
    /// 处置状态。
    pub status: FindingStatus,
    /// 处置摘要（doc-fix 回写：改了哪份文档哪一节）。
    pub disposition: String,
}

/// 从 finding 文件正文解析记录。固定小节格式（`- 键：值` 行），缺失字段留空，
/// `状态` 行经保守解析映射到 [`FindingStatus`]。
pub fn parse_finding_record(text: &str) -> FindingRecord {
    let mut rec = FindingRecord { status: FindingStatus::Open, ..Default::default() };
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('：') else {
            continue;
        };
        let value = value.trim();
        match key.trim_start_matches('-').trim() {
            "文档出处" => rec.doc_source = value.to_string(),
            "文档说" => rec.doc_says = value.to_string(),
            "实测" => rec.observed = value.to_string(),
            "复现步骤" => rec.repro = value.to_string(),
            "严重度" => rec.severity = value.to_string(),
            "状态" => rec.status = FindingStatus::parse(value),
            "处置摘要" => rec.disposition = value.to_string(),
            _ => {}
        }
    }
    rec
}

// ===== Review 报告协议（P007 T003：D003 reroute 工具侧路由） =====

use crate::sop::coding_protocol::{extract_block, extract_lines, NextAction, StageVerdict};

/// 阶段完成标记解析复用：testing 与 coding 同一标记家族，单一实现不重复。
pub use crate::sop::coding_protocol::parse_stage_complete;

const STAGE_VERDICT_TAG: &str = "stage-verdict";
const NEXT_TAG: &str = "next";
const NOTES_TAG: &str = "notes";
const ISSUES_TAG: &str = "issues";
const REROUTE_TAG: &str = "reroute";

/// reroute 路由标注（D003：test-side 退回测试 Agent；doc-side 转 doc-fix 子 SOP）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reroute {
    TestSide,
    DocSide,
}

impl Reroute {
    /// 保守解析：缺失/未知值 → `TestSide`（语义安全：FAIL 自然先修测试，
    /// doc-side 是升级动作，宁退回不滥升级）。
    pub fn parse(text: Option<&str>) -> Self {
        match text.map(str::trim) {
            Some("doc-side") => Self::DocSide,
            _ => Self::TestSide,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::TestSide => "test-side",
            Self::DocSide => "doc-side",
        }
    }
}

/// Test Review 解析后的结构化报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestReviewReport {
    pub verdict: StageVerdict,
    pub next: NextAction,
    /// 仅 FAIL 时有意义；PASS 时恒为 TestSide（占位，路由器不读）。
    pub reroute: Reroute,
    /// FAIL 时的问题清单（按 reroute 路由回注）。
    pub issues: Vec<String>,
    /// PASS 时可附的非阻塞建议。
    pub notes: Vec<String>,
    /// 协议违例标记（C19-R1）：审查输出本身不合规，不是对测试的判定——编排层重跑审查。
    pub protocol_violation: bool,
}

/// 解析 Test Review 文本。保守缺省与 coding parse_review_report 同哲学，另加：
/// FAIL 的 `<reroute>` 缺失/未知值 → 缺省 `TestSide`（不滥升级，D003）。
pub fn parse_test_review_report(text: &str) -> TestReviewReport {
    let Some(verdict_raw) = extract_block(text, STAGE_VERDICT_TAG) else {
        return conservative_test("Test Review 输出缺 <stage-verdict> 协议块");
    };
    let verdict = match verdict_raw {
        "PASS" => StageVerdict::Pass,
        "FAIL" => StageVerdict::Fail,
        other => {
            return conservative_test(&format!(
                "未知 stage-verdict `{other}`（只认 PASS/FAIL），保守按 FAIL 重审"
            ));
        }
    };

    let Some(next_raw) = extract_block(text, NEXT_TAG) else {
        // FAIL 缺 next 缺省 Fix；PASS 缺 next 是协议违例（PASS 必须携带去向）
        return match verdict {
            StageVerdict::Fail => {
                let mut report = TestReviewReport {
                    verdict,
                    next: NextAction::Fix,
                    reroute: Reroute::TestSide,
                    issues: extract_lines(text, ISSUES_TAG),
                    notes: Vec::new(),
                    protocol_violation: false,
                };
                report.reroute = Reroute::parse(extract_block(text, REROUTE_TAG));
                report
            }
            StageVerdict::Pass => conservative_test(
                "stage-verdict=PASS 但缺 <next> 协议块（PASS 必须携带 fix|advance|finish 去向）",
            ),
        };
    };
    let next = match next_raw {
        "fix" => NextAction::Fix,
        "advance" => NextAction::Advance,
        "finish" => NextAction::Finish,
        other => {
            return conservative_test(&format!(
                "未知 next `{other}`（只认 fix/advance/finish），保守按 FAIL 重审"
            ));
        }
    };

    // 语义一致性：PASS+fix、FAIL+advance/finish 都是自我矛盾（与 coding 同源）
    let contradictory = matches!(
        (verdict.clone(), next),
        (StageVerdict::Pass, NextAction::Fix)
            | (StageVerdict::Fail, NextAction::Advance)
            | (StageVerdict::Fail, NextAction::Finish)
    );
    if contradictory {
        return conservative_test(&format!(
            "协议矛盾：stage-verdict={verdict_raw} 与 next={next_raw} 不可能同时成立，保守按 FAIL 重审"
        ));
    }

    let mut report = TestReviewReport {
        issues: extract_lines(text, ISSUES_TAG),
        notes: extract_lines(text, NOTES_TAG),
        verdict,
        next,
        reroute: Reroute::TestSide,
        protocol_violation: false,
    };
    if report.verdict == StageVerdict::Fail {
        report.reroute = Reroute::parse(extract_block(text, REROUTE_TAG));
    }
    report
}

/// 保守 FAIL 报告（Test Review 形制）：reason 为审查输出违例原因，
/// 标记 protocol_violation 让编排层重跑审查而非回注被测 Agent（C19-R1）。
fn conservative_test(reason: &str) -> TestReviewReport {
    TestReviewReport {
        verdict: StageVerdict::Fail,
        next: NextAction::Fix,
        reroute: Reroute::TestSide,
        issues: vec![format!(
            "{reason}。请原样重新提交完整协议块（此为审查输出协议问题，非测试问题）"
        )],
        notes: Vec::new(),
        protocol_violation: true,
    }
}

/// Doc Review 解析后的结构化报告（D004：无 next/reroute，只有裁决+清单）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocReviewReport {
    pub verdict: StageVerdict,
    pub issues: Vec<String>,
    pub notes: Vec<String>,
    /// 协议违例标记（C19-R1）：输出不合规 → 编排层重跑审查，不回注 doc-fix  Agent。
    pub protocol_violation: bool,
}

/// 解析 Doc Review 文本：协议块缺失/未知值 → 保守 FAIL（打回 doc-fix 重来）。
pub fn parse_doc_review_report(text: &str) -> DocReviewReport {
    let Some(verdict_raw) = extract_block(text, STAGE_VERDICT_TAG) else {
        return DocReviewReport {
            verdict: StageVerdict::Fail,
            issues: vec!["Doc Review 输出缺 <stage-verdict> 协议块，保守按 FAIL 打回".into()],
            notes: Vec::new(),
            protocol_violation: true,
        };
    };
    match verdict_raw {
        "PASS" => DocReviewReport {
            verdict: StageVerdict::Pass,
            issues: Vec::new(),
            notes: extract_lines(text, NOTES_TAG),
            protocol_violation: false,
        },
        "FAIL" => DocReviewReport {
            verdict: StageVerdict::Fail,
            issues: extract_lines(text, ISSUES_TAG),
            notes: Vec::new(),
            protocol_violation: false,
        },
        other => DocReviewReport {
            verdict: StageVerdict::Fail,
            issues: vec![format!(
                "未知 stage-verdict `{other}`（只认 PASS/FAIL），保守按 FAIL 打回"
            )],
            notes: Vec::new(),
            protocol_violation: true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skeleton_creates_all_dirs_and_is_idempotent() {
        let board = tempfile::tempdir().unwrap();
        create_testing_skeleton(board.path()).unwrap();
        for dir in TESTING_SKELETON_DIRS {
            assert!(board.path().join(dir).is_dir(), "{dir} 应存在");
        }
        // 幂等：重复创建不报错
        create_testing_skeleton(board.path()).unwrap();
    }

    #[test]
    fn finding_record_parses_full_form() {
        let text = "\n# Finding F-001\n\n- 文档出处：coding/docs/02-api.md §3.2\n\
- 文档说：空标题创建返回 400\n\
- 实测：系统返回 500\n\
- 复现步骤：POST /todos {\"title\":\"\"}\n\
- 严重度：high\n\
- 状态：open\n";
        let rec = parse_finding_record(text);
        assert_eq!(rec.doc_source, "coding/docs/02-api.md §3.2");
        assert_eq!(rec.doc_says, "空标题创建返回 400");
        assert_eq!(rec.observed, "系统返回 500");
        assert_eq!(rec.repro, "POST /todos {\"title\":\"\"}");
        assert_eq!(rec.severity, "high");
        assert_eq!(rec.status, FindingStatus::Open);
        assert!(rec.disposition.is_empty());
    }

    #[test]
    fn finding_status_is_conservative() {
        assert_eq!(FindingStatus::parse("open"), FindingStatus::Open);
        assert_eq!(FindingStatus::parse("triaged"), FindingStatus::Triaged);
        assert_eq!(FindingStatus::parse("fixed"), FindingStatus::Fixed);
        assert_eq!(FindingStatus::parse("verified"), FindingStatus::Verified);
        // 未知/空/胡写 → Open（保守：未明确标记的一律按未处置）
        assert_eq!(FindingStatus::parse(""), FindingStatus::Open);
        assert_eq!(FindingStatus::parse("done??"), FindingStatus::Open);
        assert_eq!(FindingStatus::parse("closed"), FindingStatus::Open);
    }

    #[test]
    fn finding_record_missing_fields_default_empty_and_open() {
        let rec = parse_finding_record("# 空 finding\n\n随便写点没有小节格式的东西\n");
        assert_eq!(rec, FindingRecord::default());
        assert_eq!(rec.status, FindingStatus::Open);
    }

    #[test]
    fn finding_record_disposition_and_verified_status() {
        let text = "- 文档出处：coding/docs/01.md §1\n- 状态：verified\n- 处置摘要：已补 02-api.md §3.2 错误码表\n";
        let rec = parse_finding_record(text);
        assert_eq!(rec.status, FindingStatus::Verified);
        assert_eq!(rec.disposition, "已补 02-api.md §3.2 错误码表");
    }

    // ===== Test Review 解析（T003：保守路径全覆盖） =====

    #[test]
    fn test_review_fail_with_doc_side_reroute() {
        let text = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n\
                    <reroute>doc-side</reroute>\n\
                    <issues>- 缺分页查询场景\n- 错误码覆盖不足</issues>\n";
        let r = parse_test_review_report(text);
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert_eq!(r.next, NextAction::Fix);
        assert_eq!(r.reroute, Reroute::DocSide);
        assert_eq!(r.issues.len(), 2);
        assert!(r.issues[0].contains("分页"));
    }

    #[test]
    fn test_review_fail_missing_reroute_defaults_test_side() {
        let text = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n<issues>- 断言与文档矛盾</issues>\n";
        let r = parse_test_review_report(text);
        assert_eq!(r.reroute, Reroute::TestSide, "缺失 → 保守 test-side（D003）");
    }

    #[test]
    fn test_review_fail_invalid_reroute_defaults_test_side() {
        let text = "<stage-verdict>FAIL</stage-verdict>\n<next>fix</next>\n\
                    <reroute>user-side</reroute>\n<issues>- 场景不合理</issues>\n";
        let r = parse_test_review_report(text);
        assert_eq!(r.reroute, Reroute::TestSide, "未知值 → 保守 test-side（不滥升级）");
    }

    #[test]
    fn test_review_pass_advance_with_notes_reroute_placeholder() {
        let text = "<stage-verdict>PASS</stage-verdict>\n<next>advance</next>\n\
                    <reroute>doc-side</reroute>\n<notes>- 建议补超时用例</notes>\n";
        let r = parse_test_review_report(text);
        assert_eq!(r.verdict, StageVerdict::Pass);
        assert_eq!(r.next, NextAction::Advance);
        assert_eq!(r.reroute, Reroute::TestSide, "PASS 的 reroute 无意义，恒归位");
        assert_eq!(r.notes, vec!["建议补超时用例"]);
    }

    #[test]
    fn test_review_pass_finish() {
        let text = "<stage-verdict>PASS</stage-verdict>\n<next>finish</next>\n";
        let r = parse_test_review_report(text);
        assert_eq!(r.next, NextAction::Finish);
        assert!(r.issues.is_empty() && r.notes.is_empty());
    }

    #[test]
    fn test_review_conservative_missing_verdict() {
        let r = parse_test_review_report("没有协议块的回复");
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert_eq!(r.next, NextAction::Fix);
        assert_eq!(r.reroute, Reroute::TestSide);
        assert!(r.issues[0].contains("缺 <stage-verdict>"), "违例原因作为 issue");
    }

    #[test]
    fn test_review_conservative_unknown_verdict_and_next() {
        let r = parse_test_review_report("<stage-verdict>OK</stage-verdict>\n<next>fix</next>\n");
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert!(r.issues[0].contains("未知 stage-verdict"));
        let r = parse_test_review_report("<stage-verdict>FAIL</stage-verdict>\n<next>skip</next>\n");
        assert_eq!(r.next, NextAction::Fix);
        assert!(r.issues[0].contains("未知 next"));
    }

    #[test]
    fn test_review_conservative_pass_missing_next() {
        let r = parse_test_review_report("<stage-verdict>PASS</stage-verdict>\n");
        assert_eq!(r.verdict, StageVerdict::Fail, "PASS 缺去向 → 保守 FAIL");
        assert!(r.issues[0].contains("缺 <next>"));
    }

    #[test]
    fn test_review_conservative_fail_missing_next_defaults_fix() {
        let text = "<stage-verdict>FAIL</stage-verdict>\n<reroute>doc-side</reroute>\n<issues>- 文档薄</issues>\n";
        let r = parse_test_review_report(text);
        assert_eq!(r.next, NextAction::Fix, "FAIL 缺 next 缺省 Fix");
        assert_eq!(r.reroute, Reroute::DocSide);
    }

    #[test]
    fn test_review_conservative_contradictions() {
        let r = parse_test_review_report("<stage-verdict>PASS</stage-verdict>\n<next>fix</next>\n");
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert!(r.issues[0].contains("协议矛盾"));
        let r = parse_test_review_report("<stage-verdict>FAIL</stage-verdict>\n<next>finish</next>\n");
        assert_eq!(r.verdict, StageVerdict::Fail);
        assert!(r.issues[0].contains("协议矛盾"));
    }

    #[test]
    fn stage_complete_reuse_across_sops() {
        // testing 与 coding 同一标记家族：单一实现（re-export 不复制逻辑）
        assert_eq!(parse_stage_complete("<stage-complete>3</stage-complete>"), Some(3));
        assert_eq!(parse_stage_complete("没有标记"), None);
    }

    // ===== Doc Review 解析（D004） =====

    #[test]
    fn doc_review_pass_fail_and_conservative() {
        let ok = parse_doc_review_report("<stage-verdict>PASS</stage-verdict>\n<notes>- 建议 Runbook 加端口说明</notes>");
        assert_eq!(ok.verdict, StageVerdict::Pass);
        assert_eq!(ok.notes.len(), 1);

        let bad = parse_doc_review_report("<stage-verdict>FAIL</stage-verdict>\n<issues>- 缺种子数据节</issues>");
        assert_eq!(bad.verdict, StageVerdict::Fail);
        assert_eq!(bad.issues, vec!["缺种子数据节"]);

        let missing = parse_doc_review_report("啥都没有");
        assert_eq!(missing.verdict, StageVerdict::Fail, "协议块缺失 → 保守 FAIL");

        let junk = parse_doc_review_report("<stage-verdict>MAYBE</stage-verdict>");
        assert_eq!(junk.verdict, StageVerdict::Fail, "未知值 → 保守 FAIL");
    }

    #[test]
    fn reroute_parse_direct() {
        assert_eq!(Reroute::parse(Some("doc-side")), Reroute::DocSide);
        assert_eq!(Reroute::parse(Some("test-side")), Reroute::TestSide);
        assert_eq!(Reroute::parse(Some("  doc-side ")), Reroute::DocSide);
        assert_eq!(Reroute::parse(None), Reroute::TestSide);
        assert_eq!(Reroute::parse(Some("junk")), Reroute::TestSide);
    }
}
