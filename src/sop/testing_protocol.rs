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
}
