//! 测试阶段的机械门（P009 T011 / D017-D019）。
//!
//! 口径：
//! - **回归优先**：test agent 先跑既有全套测试并把结果落 `evidence/regression.log`；
//!   日志退出码非 0 却声称「回归没问题」= 说谎，FAIL。
//! - **验收对账**：计划的每条验收标准都必须对到测试 `path:line`；缺一条即零覆盖（FAIL）。
//! - **回注带失败明细原文**：bug 打回 coding/plan 时把失败明细**逐条原文**（含 `path:line`）
//!   带给实现方——回注是 coding agent 的**输入面**，不给定位等于让它自己猜
//!   （D025 改写 D018/D019 的「不做定位」口径：两个路由目标同口径，交付面与日志面同源）。

use std::path::Path;

use crate::change::coding_gate::parse_baseline_log;
use crate::change::state::TestFailureContext;
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
/// 引擎审计修复 #4：not_before 校验 mtime 不早于本轮开始（与 baseline/self-check 的
/// freshness 同口径）——上一轮残留的绿 log 不得给本轮提供通行证。
pub fn check_regression_log(
    blackboard: &Path,
    claims_green: bool,
    not_before: std::time::SystemTime,
) -> Result<i32> {
    let path = blackboard.join(REGRESSION_LOG);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "缺少回归证据 {}（测试阶段必须先跑既有全套）：{e}",
            path.display()
        ))
    })?;
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map_err(|e| {
            DevforgeError::Quality(format!(
                "回归证据 {} 的修改时间不可读：{e}",
                path.display()
            ))
        })?;
    if mtime < not_before {
        return Err(DevforgeError::Quality(format!(
            "回归证据 {} 是上一轮残留（mtime 早于本轮开始）——本轮必须重跑既有全套",
            path.display()
        )));
    }
    check_regression_claim(&text, claims_green)
}

/// 测试阶段入口快照：基线以来 tracked 文件的改动集（引擎审计修复 #6）。
/// 只看 tracked（git diff --name-only baseline）——untracked 的依赖环境/临时产物
/// 是黑盒测试的合法噪音，不纳入。
pub fn snapshot_tracked_changes(
    repo: &Path,
    baseline: &str,
) -> Result<std::collections::BTreeSet<String>> {
    let out = std::process::Command::new("git")
        .args(["-c", "core.quotePath=false", "diff", "--name-only", baseline])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git diff 失败：{e}")))?;
    if !out.status.success() {
        // 降级为空集 + WARN：git 不可用/无效基线时上游基线门已 Err，此处不重复报错
        // （否则会掩盖测试阶段真正的错误明细，如空箱拒收的 Quality 文案）。
        tracing::warn!(
            component = "change-engine",
            event = "gate.product_snapshot",
            baseline = %baseline,
            stderr = %String::from_utf8_lossy(&out.stderr).trim(),
            "产品代码快照不可用（降级为空集，#6 机械门本阶段失效）"
        );
        return Ok(Default::default());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// 测试阶段出口校验（引擎审计修复 #6）：测试期间新增的产品代码改动 = Quality。
/// 「不改产品代码」不再只是 prompt 约束——tests/ 与流水线产物（docs/evidence）之外
/// 的新增 tracked 改动一律拒收。
pub fn check_no_product_code_edits(
    before: &std::collections::BTreeSet<String>,
    after: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let new_edits: Vec<&String> = after
        .difference(before)
        .filter(|f| {
            !f.starts_with("tests/")
                && !f.starts_with("docs/")
                && !f.starts_with("evidence/")
        })
        .collect();
    if !new_edits.is_empty() {
        return Err(DevforgeError::Quality(format!(
            "测试阶段修改了产品代码（测试 Agent 只准写测试）：{}——发现 bug 回注，不自己修",
            new_edits
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("、")
        )));
    }
    Ok(())
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

/// 生成回注给 coding/plan 的消息：失败明细**逐条原文**（含 `path:line`）。
///
/// `target` 只影响措辞（`coding` / `plan`），两个目标同口径：明细同源
/// （`failures` 是 test agent 给的原文，不剥定位、不改写、不截断）。
pub fn route_back_message(target: &str, failures: &[String]) -> String {
    format!(
        "测试阶段未通过，按 <route>{target}</route> 回去处理。\n失败明细：\n{}\n",
        failure_details(failures)
    )
}

/// 失败明细渲染：issues 逐条原文，一行一条。
///
/// 回注消息与 coding 送达块共用这一份渲染 → 同一失败在三个出口同源（不漂移）。
pub fn failure_details(failures: &[String]) -> String {
    failures
        .iter()
        .map(|f| format!("- {f}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// coding 送达块的标题行（逐字可查：验收按这句话对账）。
pub const FAILURE_BLOCK_TITLE: &str = "上一轮测试失败明细如下（逐条原文）";

/// coding 送达块：上一轮测试失败的明细（逐条原文）。
///
/// 这是回注内容在 **coding agent 输入面**上的形态；明细与日志面/失败原因文本同源。
pub fn coding_failure_block(failure: &TestFailureContext) -> String {
    format!(
        "{}：\n{}",
        FAILURE_BLOCK_TITLE,
        failure_details(&failure.issues)
    )
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
        let err =
            check_regression_log(tmp.path(), true, std::time::SystemTime::UNIX_EPOCH).unwrap_err();
        assert!(err.to_string().contains("缺少回归证据"), "{err}");
    }

    /// 引擎审计修复 #4 回归：上一轮残留的绿 log（mtime 早于本轮开始）必须拒收——
    /// 错实现形态：只读文件+解析退出码，残留绿证据给本轮提供假通行证，
    /// 测试打回重入时可以跳过真重跑。
    #[test]
    fn stale_regression_log_is_rejected_by_freshness() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(REGRESSION_LOG);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "command: cargo test\nexit_code: 0\n").unwrap();
        // not_before 取未来 → 刚写的文件也算残留
        let future = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let err = check_regression_log(tmp.path(), true, future).unwrap_err();
        assert!(
            err.to_string().contains("上一轮残留"),
            "残留绿 log 应被 freshness 拒收：{err}"
        );
    }

    /// 引擎审计修复 #6 回归：测试期间新增的产品代码改动（tests/ 与流水线产物之外）
    /// = Quality；新增 tests/ 合法。
    #[test]
    fn product_code_edits_during_testing_are_quality_failure() {
        let before = std::collections::BTreeSet::from([
            "src/a.rs".to_string(),
            "tests/test_x.py".to_string(),
        ]);
        // 场景 1：测试期间改了 src/main.rs → Quality
        let after_bad = before
            .union(&std::collections::BTreeSet::from([
                "src/main.rs".to_string(),
                "tests/test_new.py".to_string(),
            ]))
            .cloned()
            .collect();
        let err = check_no_product_code_edits(&before, &after_bad).unwrap_err();
        assert!(err.to_string().contains("修改了产品代码"), "{err}");
        // 场景 2：只新增 tests/ → 合法
        let after_ok = before
            .union(&std::collections::BTreeSet::from([
                "tests/test_new.py".to_string(),
                "evidence/trace.log".to_string(),
            ]))
            .cloned()
            .collect();
        check_no_product_code_edits(&before, &after_ok).unwrap();
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
    fn route_back_message_carries_the_failure_details_verbatim() {
        // 错实现形态：回注把 path:line 剥成 <位置略>（旧口径）→ 本用例必须变红
        let msg = route_back_message(
            "coding",
            &[
                "tests/test_a.py:12 断言失败".to_string(),
                "接口返回 500".to_string(),
            ],
        );
        assert!(msg.contains("<route>coding</route>"));
        assert!(msg.contains("tests/test_a.py:12"), "回注必须带定位：{msg}");
        assert!(msg.contains("接口返回 500"), "回注必须带全部明细：{msg}");
        assert!(!msg.contains("<位置略>"), "不许再剥定位：{msg}");
        assert!(!msg.contains("请自己定位原因"), "不许写零信息话术：{msg}");
        assert!(!msg.contains("本消息不提供文件位置"), "不许写零信息话术：{msg}");
        // 用计划的 path_refs 解析器复核：明细里的 path:line 必须仍可解析
        assert!(
            crate::change::plan_audit::path_refs(&msg).contains(&("tests/test_a.py".to_string(), 12)),
            "回注消息里的 path:line 必须可解析：{msg}"
        );
    }

    #[test]
    fn route_to_plan_carries_the_same_details() {
        // 错实现形态：plan 目标仍剥定位（两个路由目标不同口径）→ 本用例必须变红
        let msg = route_back_message("plan", &["tests/test_a.py:12 断言失败".to_string()]);
        assert!(msg.contains("<route>plan</route>"), "{msg}");
        assert!(msg.contains("tests/test_a.py:12"), "两个路由目标同口径：{msg}");
        assert!(!msg.contains("<位置略>"), "{msg}");
        assert!(!msg.contains("本消息不提供文件位置"), "{msg}");
    }

    #[test]
    fn failure_details_render_one_issue_per_line_verbatim() {
        // 错实现形态：渲染时改写/截断明细 → 本用例必须变红
        let details = failure_details(&[
            "a.rs:1 失败".to_string(),
            "b.rs:2 失败".to_string(),
        ]);
        assert_eq!(details, "- a.rs:1 失败\n- b.rs:2 失败");
    }

    #[test]
    fn coding_failure_block_carries_the_title_and_verbatim_issues() {
        // 错实现形态：送达块漏标题行 / 漏一条 issue / 夹带零信息话术 → 本用例必须变红
        let ctx = TestFailureContext {
            round: 1,
            issues: vec![
                "tests/change_ask_ring.rs:557 断言失败".to_string(),
                "接口返回 500".to_string(),
            ],
        };
        let block = coding_failure_block(&ctx);
        assert!(block.contains(FAILURE_BLOCK_TITLE), "送达块要有标题行：{block}");
        assert!(
            block.contains("tests/change_ask_ring.rs:557"),
            "送达块要逐字带失败明细：{block}"
        );
        assert!(block.contains("接口返回 500"), "送达块要带全部明细：{block}");
        assert!(!block.contains("请自己定位原因"), "{block}");
        assert!(!block.contains("本消息不提供文件位置"), "{block}");
        assert!(!block.contains("<位置略>"), "{block}");
    }
}
