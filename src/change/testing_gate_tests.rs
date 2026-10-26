//! （2026-10-26 测试分离：自 src/change/testing_gate.rs 内联平移，纯移动零改动。）
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
