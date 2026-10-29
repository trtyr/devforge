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

// ── 2026-10-27 自审 testing_gate 批回归 ──

/// 自审 minor（30）：快照/门的 **fail-closed** Err 分支此前没有用例钉住——把它改回
/// fail-open（Ok 空集）整套测试仍绿。这里钉三条来源不同的失败。
#[test]
fn snapshot_failures_are_closed_not_open() {
    let tmp = tempfile::tempdir().unwrap();
    // ① 形状非法（Config：确定性错误，重启不会变好——90）
    let err = snapshot_tracked_changes(tmp.path(), "not-a-sha").unwrap_err();
    assert!(
        matches!(err, DevforgeError::Config(_)),
        "形状非法应归 Config（旧口径归 System 白耗重试预算）：{err}"
    );
    // ② 仓不存在/不是 git 仓（System：基础设施）
    let err = snapshot_tracked_changes(tmp.path(), "abcdef1").unwrap_err();
    assert!(
        matches!(err, DevforgeError::System(_)),
        "非 git 仓应归 System：{err}"
    );
}

/// 自审 minor（92）：只有**列表项**才是验收对账行——散文里的箭头不算
/// （旧口径把任何含 → 的行当验收项，说明文字会触发不可恢复的硬失败）。
#[test]
fn acceptance_table_ignores_prose_arrows() {
    let text = "验收标准如下（箭头 → 表示映射）：\n\
                - x=1 → tests/a.rs:12\n\
                说明：上面的 → 只是举例\n\
                * y=2 → tests/b.rs:34\n";
    let rows = parse_acceptance_table(text);
    assert_eq!(rows.len(), 2, "只认列表项：{rows:?}");
    assert_eq!(rows[0].criterion, "x=1");
    assert_eq!(rows[1].test_ref, "tests/b.rs:34");
}

/// 自审 minor（88）：回归门的解析错误必须指向回归日志本身。
#[test]
fn regression_parse_error_points_at_the_regression_log() {
    let err = check_regression_claim("command: cargo test\n", true).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("evidence/regression.log"),
        "要指向回归日志：{msg}"
    );
    assert!(
        !msg.contains("baseline-test.log"),
        "不得指向基线证据文件：{msg}"
    );
}

// ── 围栏失明修复（F11）：回归证据两段式扫描 ──
//
// 回归日志是 agent 手写的 markdown，把命令与退出码包进代码围栏是标准写法。
// 旧口径按围栏跳过（防把文档示例当成真证据），于是围栏内成对写着 command +
// exit_code 的真实证据被判「缺退出码」→ 整轮陪葬（F10 run 1 实证）。

/// F11 主回归：围栏里成对写着 command + exit_code → 必须取到退出码。
///
/// 错实现形态：解析器只扫围栏外（旧口径，无 stage b）→ 本用例必须变红（判「缺退出码」）。
#[test]
fn a_fenced_command_and_exit_code_pair_counts_as_evidence() {
    let text = "回归结果：\n```\ncommand: env -u DEVFORGE_MANAGED cargo test\nexit_code: 0\n```\n";
    let code = check_regression_claim(text, true).unwrap();
    assert_eq!(code, 0, "围栏内同块成对必须被认作证据：{text}");
}

/// F11 防御回归：围栏里只有孤立的 `exit_code: 0`（没有 command 配对）→ 仍判缺。
///
/// 错实现形态：回退时只认 exit_code 不认 command 配对 → 本用例必须变红。
#[test]
fn an_isolated_exit_code_inside_a_fence_is_not_evidence() {
    let text = "```\nexit_code: 0\n```\n";
    let err = check_regression_claim(text, true).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("缺退出码"), "孤立的 exit_code 不作数：{msg}");
    assert!(
        msg.contains("evidence/regression.log"),
        "要指向回归日志：{msg}"
    );
    assert!(err.is_quality_failure(), "示例防御不放松：{msg}");
}

/// F11 边界回归：`command:` 在围栏外、`exit_code:` 在围栏内 → 跨边界不成对，仍判缺。
///
/// 错实现形态：回退时跨块拼 command / exit_code → 本用例必须变红。
#[test]
fn an_exit_code_in_a_fence_does_not_pair_with_a_command_outside() {
    let text = "command: cargo test\n```\nexit_code: 0\n```\n";
    let err = check_regression_claim(text, true).unwrap_err();
    assert!(err.to_string().contains("缺退出码"), "{err}");
}

/// F11 优先级回归：围栏外 `exit_code: 1` 与围栏内成对并存 → 取围栏外的 1。
///
/// 错实现形态：围栏优先 / 两段结果混用 → 本用例必须变红。
#[test]
fn the_exit_code_outside_the_fence_wins_over_a_fenced_pair() {
    let text = "command: cargo test\nexit_code: 1\n```\ncommand: cargo test\nexit_code: 0\n```\n";
    // 诚实自述（claims_green=false）→ 取围栏外的 1
    assert_eq!(
        check_regression_claim(text, false).unwrap(),
        1,
        "围栏外优先：{text}"
    );
    // 声称通过 → 与围栏外的 1 矛盾 → Quality
    let err = check_regression_claim(text, true).unwrap_err();
    assert!(err.to_string().contains("退出码 1"), "{err}");
}

/// F11 多块回归：第 1 块不成对（只有 `exit_code: 9`）、第 2 块成对（0）→ 取第 2 块。
///
/// 第 3 块也成对（7）但排在后面 → 不取（判别项：让「取首个成对块」与「取最后一个块」
/// 两种实现给出不同结果，「不成对块被跳过」也一并可观测）。
///
/// 错实现形态：取最后一个块 / 不跳过不成对块 / 跨块拼 command → 本用例必须变红。
#[test]
fn the_first_qualifying_fenced_block_wins() {
    let text = "```\nexit_code: 9\n```\n```\ncommand: cargo test\nexit_code: 0\n```\n```\ncommand: cargo test\nexit_code: 7\n```\n";
    let code = check_regression_claim(text, true).unwrap();
    assert_eq!(code, 0, "不成对块被跳过，取首个成对块：{text}");
}

/// F11 防退化锚钉：同一段「围栏内同块成对」的文本喂基线门 → 仍判缺退出码。
///
/// 回退只服务回归门；一旦被下沉进共用解析器（基线门 / 自证门），本用例必须变红。
#[test]
fn the_baseline_gate_does_not_accept_a_fenced_pair() {
    let text = "```\ncommand: cargo test\nexit_code: 0\n```\n";
    let err = crate::change::coding_gate::parse_baseline_log(text).unwrap_err();
    assert!(err.to_string().contains("缺退出码"), "{err}");
}
