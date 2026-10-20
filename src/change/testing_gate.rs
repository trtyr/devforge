//! 测试阶段的机械门（P009 T011 / D017-D019）。
//!
//! 口径：
//! - **回归优先**：test agent 先跑既有全套测试并把结果落 `evidence/regression.log`；
//!   日志退出码非 0 却声称「回归没问题」= 说谎，FAIL。
//! - **验收对账**：计划的每条验收标准都必须对到测试 `path:line`；缺一条即零覆盖（FAIL）。
//! - **回注不带定位**：bug 打回 coding/plan 时只给失败摘要，**不给文件行号**——测试 agent 读的是行为，
//!   不是代码；给了定位就等于让它越界做本地化（D018/D019）。

use std::path::Path;

use crate::change::coding_gate::parse_baseline_log;
use crate::error::{DevforgeError, Result};

/// 回归证据（相对黑板）。
pub const REGRESSION_LOG: &str = "evidence/regression.log";

/// 一条验收标准到测试的映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceMatch {
    pub criterion: String,
    pub test_ref: String,
}

/// 回归结果判定：日志退出码与 agent 的自述必须一致。
pub fn check_regression_claim(log_text: &str, claims_green: bool) -> Result<i32> {
    let ev = parse_baseline_log(log_text).map_err(|e| {
        DevforgeError::Quality(format!(
            "回归日志不可解析（要写 `exit_code: N`）：{e}"
        ))
    })?;
    if ev.exit_code != 0 && claims_green {
        return Err(DevforgeError::Quality(format!(
            "回归日志退出码 {} 与「回归通过」的自述矛盾（命令：{}）",
            ev.exit_code,
            if ev.command.is_empty() { "未记录" } else { &ev.command }
        )));
    }
    Ok(ev.exit_code)
}

/// 读黑板上的回归日志并判定。
pub fn check_regression_log(blackboard: &Path, claims_green: bool) -> Result<i32> {
    let path = blackboard.join(REGRESSION_LOG);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "缺少回归证据 {}（测试阶段必须先跑既有全套）：{e}",
            path.display()
        ))
    })?;
    check_regression_claim(&text, claims_green)
}

/// 解析验收对账表：`- <验收标准> → <测试引用 path:line>`（按**最后一个** `→` 切，标准本身可能含箭头）。
pub fn parse_acceptance_table(text: &str) -> Vec<AcceptanceMatch> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim().trim_start_matches(['-', '*']).trim();
        let Some((left, right)) = t.rsplit_once('→') else {
            continue;
        };
        let criterion = left.trim().to_string();
        let test_ref = right.trim().trim_matches('`').to_string();
        if !criterion.is_empty() && !test_ref.is_empty() {
            out.push(AcceptanceMatch {
                criterion,
                test_ref,
            });
        }
    }
    out
}

/// 逐条对账：计划里每条验收标准都要在表里出现，且映射到具体测试引用。
pub fn reconcile_acceptance(criteria: &[String], table: &[AcceptanceMatch]) -> Result<usize> {
    if criteria.is_empty() {
        return Err(DevforgeError::Quality(
            "计划里没有验收标准，无从对账（计划阶段就该写出来）".into(),
        ));
    }
    let mut missing: Vec<String> = Vec::new();
    for c in criteria {
        let hit = table
            .iter()
            .any(|m| m.criterion == *c || m.criterion.contains(c.as_str()) || c.contains(m.criterion.as_str()));
        if !hit {
            missing.push(c.clone());
        }
    }
    if !missing.is_empty() {
        return Err(DevforgeError::Quality(format!(
            "验收对账缺 {} 条（缺一条即零覆盖）：{}",
            missing.len(),
            missing.join("；")
        )));
    }
    // 对到空引用（写了标准但没指测试）同样算没覆盖
    let empty_refs: Vec<String> = table
        .iter()
        .filter(|m| m.test_ref.trim().is_empty())
        .map(|m| m.criterion.clone())
        .collect();
    if !empty_refs.is_empty() {
        return Err(DevforgeError::Quality(format!(
            "验收项没有指到测试 path:line：{}",
            empty_refs.join("；")
        )));
    }
    Ok(criteria.len())
}

/// 生成回注给 coding/plan 的消息：**只给失败摘要，不带文件行号定位**。
///
/// `target` 只影响措辞（`coding` / `plan`），不改变「不给定位」这条硬约束。
pub fn route_back_message(target: &str, failures: &[String]) -> String {
    let mut out = format!(
        "测试阶段未通过，按 <route>{target}</route> 回去处理。请自己定位原因（本消息不提供文件位置）。\n失败摘要：\n"
    );
    for f in failures {
        out.push_str(&format!("- {}\n", strip_locations(f)));
    }
    out
}

/// 去掉文本里的 `path:line` 定位（防止上游把定位顺手带进来）。
fn strip_locations(text: &str) -> String {
    let mut out = String::new();
    for tok in text.split_whitespace() {
        let is_loc = tok
            .rsplit_once(':')
            .is_some_and(|(p, l)| p.contains('.') && l.chars().all(|c| c.is_ascii_digit()) && !l.is_empty());
        if is_loc {
            out.push_str("<位置略>");
        } else {
            out.push_str(tok);
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn red_regression_contradicting_the_claim_fails() {
        let err = check_regression_claim("command: cargo test\nexit_code: 1\n", true).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("矛盾"), "{msg}");
        assert!(msg.contains("cargo test"), "要带自己跑的命令：{msg}");
        assert!(err.is_quality_failure());
    }

    #[test]
    fn red_regression_with_honest_claim_is_fine() {
        // agent 老实说「回归红了」→ 不是矛盾，由编排层决定回注
        let code = check_regression_claim("command: pytest\nexit_code: 2\n", false).unwrap();
        assert_eq!(code, 2);
    }

    #[test]
    fn green_regression_passes() {
        let code = check_regression_claim("command: go test ./...\nexit_code: 0\n", true).unwrap();
        assert_eq!(code, 0);
    }

    #[test]
    fn missing_regression_log_is_quality_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let err = check_regression_log(tmp.path(), true).unwrap_err();
        assert!(err.to_string().contains("缺少回归证据"), "{err}");
    }

    #[test]
    fn acceptance_table_parses_arrows() {
        let table = parse_acceptance_table(
            "- 给定 x=1 返回 2 → tests/test_a.py:12\n- 空输入报错 → `tests/test_b.py:7`\n",
        );
        assert_eq!(table.len(), 2);
        assert_eq!(table[0].test_ref, "tests/test_a.py:12");
        assert_eq!(table[1].criterion, "空输入报错");
    }

    #[test]
    fn one_missing_criterion_is_zero_coverage() {
        let criteria = vec!["给定 x=1 返回 2".to_string(), "空输入报错".to_string()];
        let table = parse_acceptance_table("- 给定 x=1 返回 2 → tests/test_a.py:12\n");
        let err = reconcile_acceptance(&criteria, &table).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("缺 1 条"), "{msg}");
        assert!(msg.contains("空输入报错"), "{msg}");
    }

    #[test]
    fn full_reconciliation_passes() {
        let criteria = vec!["给定 x=1 返回 2".to_string()];
        let table = parse_acceptance_table("- 给定 x=1 返回 2 → tests/test_a.py:12\n");
        assert_eq!(reconcile_acceptance(&criteria, &table).unwrap(), 1);
    }

    #[test]
    fn empty_criteria_is_rejected() {
        let err = reconcile_acceptance(&[], &[]).unwrap_err();
        assert!(err.to_string().contains("没有验收标准"), "{err}");
    }

    #[test]
    fn route_back_message_carries_no_location() {
        let msg = route_back_message(
            "coding",
            &["tests/test_a.py:12 断言失败".to_string(), "接口返回 500".to_string()],
        );
        assert!(msg.contains("<route>coding</route>"));
        assert!(!msg.contains("test_a.py:12"), "回注不得带定位：{msg}");
        assert!(msg.contains("<位置略>"), "{msg}");
        // 用计划的 path_refs 解析器复核：消息里不得再出现可解析的 path:line
        assert!(
            crate::change::plan_audit::path_refs(&msg).is_empty(),
            "回注消息里出现了 path:line：{msg}"
        );
    }

    #[test]
    fn route_to_plan_is_also_possible() {
        let msg = route_back_message("plan", &["验收标准与实现不符".to_string()]);
        assert!(msg.contains("<route>plan</route>"), "{msg}");
    }
}
