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

/// 测试阶段入口快照：基线以来 tracked 文件的改动集 + 未跟踪的新产品文件。
///
/// 自审 major：旧口径只看 `git diff baseline`——测试 agent 新建且未 git add 的
/// 产品源文件完全逃过「不改产品代码」门（与 coding 侧复验 #2 同款盲区）。
/// 现在并入 untracked 扫描：带源码扩展名、不在 tests/ docs/ evidence/ 豁免前缀、
/// 不在常见构建产物目录里的新文件也算改动集（源码扩展名单与 inventory 同源）。
pub fn snapshot_tracked_changes(
    repo: &Path,
    baseline: &str,
) -> Result<std::collections::BTreeSet<String>> {
    // 自审 major：baseline 先过形状门（hex）再进 git 参数——防参数注入/垃圾锚点。
    let well_formed = baseline.len() >= 7
        && baseline.len() <= 64
        && baseline.chars().all(|c| c.is_ascii_hexdigit());
    if !well_formed {
        return Err(DevforgeError::System(format!(
            "基线 sha 形状非法（{baseline:?}）——拒绝当作 git 参数"
        )));
    }
    let mut set = std::collections::BTreeSet::new();
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--name-only",
            baseline,
            "--",
        ])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git diff 失败：{e}")))?;
    if !out.status.success() {
        // fail-closed（change 批次 #2 收口）：快照失败 = 本阶段机械门未生效，
        // 必须 Err 让本轮测试作废，绝不把「查不了」当「查过、干净」。
        return Err(DevforgeError::System(format!(
            "测试阶段产品代码快照失败（git 基线 {baseline}）：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let t = line.trim();
        if !t.is_empty() {
            set.insert(t.to_string());
        }
    }
    // untracked 产品源文件（未 add 的新文件不进 diff）
    let out = std::process::Command::new("git")
        .args(["-c", "core.quotePath=false", "status", "--porcelain", "-uall"])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git status 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "测试阶段产品代码快照失败（git status）：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some(rest) = line.strip_prefix("?? ") else {
            continue;
        };
        let path = rest.trim();
        if is_exempt_or_junk(path) {
            continue;
        }
        set.insert(path.to_string());
    }
    Ok(set)
}

/// 豁免前缀（tests/ docs/ evidence/）+ 常见构建/缓存产物目录。
fn is_exempt_or_junk(path: &str) -> bool {
    const EXEMPT: [&str; 3] = ["tests/", "docs/", "evidence/"];
    const JUNK_DIRS: [&str; 6] = [
        "node_modules/",
        "target/",
        "dist/",
        "__pycache__/",
        ".pytest_cache/",
        ".venv/",
    ];
    if EXEMPT.iter().any(|p| path.starts_with(p)) {
        return true;
    }
    if JUNK_DIRS
        .iter()
        .any(|p| path.contains(&format!("{p}")))
    {
        return true;
    }
    // 带源码扩展名 = 产品代码 → 不豁免（进快照）；无源码扩展名（report.html、
    // .log 之类的运行产物）→ 当噪音跳过。
    // 注意：SOURCE_EXTENSIONS 存裸扩展名（无点），必须补点比对。
    !crate::docgen::inventory::SOURCE_EXTENSIONS
        .iter()
        .any(|(e, _)| path.ends_with(&format!(".{e}")))
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
    // 自审 major：还原检测——入口快照里有、出口没了的产品路径 = 测试阶段把
    // coding 的成果 checkout 回基线/删掉。旧口径只看「新增项」，回退完全隐形。
    let vanished: Vec<&String> = before
        .difference(after)
        .filter(|f| {
            !f.starts_with("tests/")
                && !f.starts_with("docs/")
                && !f.starts_with("evidence/")
        })
        .collect();
    if !vanished.is_empty() {
        return Err(DevforgeError::Quality(format!(
            "测试阶段删除/还原了产品代码（coding 的改动不得被测试阶段抹掉）：{}——发现 bug 回注，不自己修",
            vanished
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

    /// 自审 major 回归：未跟踪的新产品源文件必须进快照（旧口径只看 tracked diff，
    /// 测试 agent 不 git add 就能逃门）。
    #[test]
    fn untracked_product_source_file_enters_the_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/a.rs"), "a\n").unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@l", "commit", "--allow-empty", "-q", "-m", "b"]);
        let baseline = String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();
        // 测试 agent 新建产品源文件，不 git add
        std::fs::write(repo.join("src/hack.py"), "x\n").unwrap();
        std::fs::create_dir_all(repo.join("tests")).unwrap();
        std::fs::write(repo.join("tests/new_test.py"), "x\n").unwrap();
        let snap = snapshot_tracked_changes(&repo, &baseline).unwrap();
        assert!(
            snap.contains("src/hack.py"),
            "未 add 的产品源文件必须进快照：{snap:?}"
        );
        assert!(
            !snap.contains("tests/new_test.py"),
            "tests/ 豁免：{snap:?}"
        );
    }

    /// 自审 major 回归：垃圾 baseline（非 hex）拒绝进 git 参数。
    #[test]
    fn garbage_baseline_sha_is_rejected_before_git() {
        let tmp = tempfile::tempdir().unwrap();
        let err = snapshot_tracked_changes(tmp.path(), "--output=/tmp/evil").unwrap_err();
        assert!(err.to_string().contains("形状非法"), "{err}");
    }

    /// 自审 major 回归：入口有、出口没了的产品路径 = 还原/删除，必须 Quality——
    /// 旧口径只看新增项，把 coding 成果 checkout 回基线完全隐形。
    #[test]
    fn reverted_product_code_is_quality_failure() {
        let before = std::collections::BTreeSet::from([
            "src/feature.rs".to_string(),
            "tests/t.py".to_string(),
        ]);
        let after = std::collections::BTreeSet::from(["tests/t.py".to_string()]);
        let err = check_no_product_code_edits(&before, &after).unwrap_err();
        assert!(err.to_string().contains("删除/还原"), "{err}");
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
