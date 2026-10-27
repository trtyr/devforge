//! （2026-10-26 测试分离：自 src/change/coding_gate.rs 内联平移，纯移动零改动。）
use super::*;

/// （自生产文件迁入：cfg(test) 专用帮手，消费方全在测试。）
#[cfg(test)]
pub fn check_baseline_log(blackboard: &Path) -> Result<BaselineEvidence> {
    check_evidence_fresh(
        blackboard.join(BASELINE_LOG),
        "缺少基线证据 {}（动手前必须先跑一遍既有测试）",
        "基线不是绿的",
        "不许在红基线上动代码",
        parse_baseline_log,
        std::time::SystemTime::UNIX_EPOCH,
    )
}

#[cfg(test)]
pub fn check_self_check(blackboard: &Path) -> Result<BaselineEvidence> {    let path = blackboard.join(SELF_CHECK_LOG);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "缺少自证证据 {}（提交前必须留下命令原文 + 退出码 + 摘要）：{e}",
            path.display()
        ))
    })?;
    let ev = parse_evidence_log(&text, SELF_CHECK_LOG, "自证")?;
    if ev.exit_code != 0 {
        return Err(DevforgeError::Quality(format!(
            "自证不是绿的（exit_code={}，命令：{}）——先证后报：证据红着不许提交",
            ev.exit_code,
            if ev.command.is_empty() {
                "未记录".to_string()
            } else {
                ev.command
            }
        )));
    }
    Ok(ev)
}
fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?} 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_rev(repo: &Path, what: &str) -> String {
    let out = Command::new("git")
        .args(["rev-parse", what])
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(out.status.success(), "git rev-parse {what} 失败");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(repo: &Path, msg: &str) {
    git(repo, &["add", "-A"]);
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            msg,
        ])
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn repo_with_baseline() -> (tempfile::TempDir, String) {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();
    commit(tmp.path(), "devforge: baseline");
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (tmp, sha)
}

/// 复验 #2 回归：白名单外的未跟踪文件必须进入 changed_files（否则门禁三处盲区）。
#[test]
fn untracked_files_are_included_in_changed_files() {
    let (tmp, baseline) = repo_with_baseline();
    std::fs::write(tmp.path().join("outside.txt"), "secret\n").unwrap();
    let files = changed_files(tmp.path(), &baseline).unwrap();
    assert!(
        files.iter().any(|f| f == "outside.txt"),
        "未跟踪文件必须出现在变更集：{files:?}"
    );
}

/// 引擎审计修复 #3 回归：已有 git 仓 + doc_level=essential 的 untracked 文档
/// （docs/contexts/_meta/*）是 SOP 自产产物，不得进变更集触发范围膨胀 FAIL——
/// 错实现形态：过滤清单只有 docs/plantree/ 与 evidence/，每次带文档档位的
/// change 必踩 OutOfBoundary。
#[test]
fn essential_doc_artifacts_are_excluded_from_changed_files() {
    let (tmp, baseline) = repo_with_baseline();
    std::fs::create_dir_all(tmp.path().join("docs/contexts/_meta")).unwrap();
    std::fs::write(
        tmp.path().join("docs/contexts/_meta/inventory.json"),
        "{\"meta\":{}}",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("docs/contexts/glossary.md"),
        "# 词汇表\n",
    )
    .unwrap();
    let files = changed_files(tmp.path(), &baseline).unwrap();
    assert!(
        !files.iter().any(|f| f.starts_with("docs/contexts/")),
        "SOP 自产文档域不得进变更集：{files:?}"
    );
}

/// 复验 N4 回归：非 ASCII 文件名不被 quotePath 转义，白名单能精确匹配。
#[test]
fn non_ascii_paths_are_not_escaped() {
    let (tmp, baseline) = repo_with_baseline();
    std::fs::write(tmp.path().join("caf\u{e9}.txt"), "x\n").unwrap();
    let files = changed_files(tmp.path(), &baseline).unwrap();
    assert!(
        files.iter().any(|f| f == "caf\u{e9}.txt"),
        "非 ASCII 路径不得带 C 引号转义：{files:?}"
    );
}

/// 复验 #1 回归：非法基线（git diff 失败）必须 Err，不得静默空数组。
#[test]
fn invalid_baseline_is_error_not_empty() {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();
    let err = changed_files(tmp.path(), "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
        .unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[test]
fn green_baseline_log_passes() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(
        tmp.path().join(BASELINE_LOG),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    let ev = check_baseline_log(tmp.path()).unwrap();
    assert_eq!(ev.exit_code, 0);
    assert_eq!(ev.command, "cargo test");
}

#[test]
fn red_baseline_blocks_coding() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(
        tmp.path().join(BASELINE_LOG),
        "command: pytest -q\nexit_code: 2\n",
    )
    .unwrap();
    let err = check_baseline_log(tmp.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("基线不是绿的"), "{msg}");
    assert!(msg.contains("pytest -q"), "错误里要带自己跑的命令：{msg}");
}

#[test]
fn missing_log_is_quality_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let err = check_baseline_log(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("缺少基线证据"), "{err}");
    assert!(crate::error::DevforgeError::Quality(String::new()).is_quality_failure());
    assert!(err.is_quality_failure());
}

#[test]
fn log_without_exit_code_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(tmp.path().join(BASELINE_LOG), "跑了一遍，看起来还行\n").unwrap();
    let err = check_baseline_log(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("缺退出码"), "{err}");
}

#[test]
fn baseline_log_error_text_still_points_at_the_baseline_file() {
    // 回归路径的第二个调用方（testing_gate）依赖这套文案，本门不得改它
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(tmp.path().join(BASELINE_LOG), "command: cargo test\n").unwrap();
    let err = check_baseline_log(tmp.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("基线证据缺退出码"), "{msg}");
    assert!(msg.contains(BASELINE_LOG), "{msg}");
}

#[test]
fn missing_self_check_log_is_quality_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let err = check_self_check(tmp.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains(SELF_CHECK_LOG), "错误要指向自证证据文件：{msg}");
    assert!(
        !msg.contains(BASELINE_LOG),
        "自证门的错误不得指向基线文件：{msg}"
    );
}

#[test]
fn self_check_without_exit_code_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(
        tmp.path().join(SELF_CHECK_LOG),
        "command: cargo test\n跑完了，看着没问题\n",
    )
    .unwrap();
    let err = check_self_check(tmp.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("退出码"), "{msg}");
    assert!(msg.contains(SELF_CHECK_LOG), "{msg}");
    assert!(
        !msg.contains(BASELINE_LOG),
        "自证门的错误不得指向基线文件：{msg}"
    );
}

#[test]
fn red_self_check_is_rejected_with_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(
        tmp.path().join(SELF_CHECK_LOG),
        "command: cargo test\nexit_code: 1\n",
    )
    .unwrap();
    let err = check_self_check(tmp.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("exit_code=1"), "{msg}");
    assert!(
        !msg.contains(BASELINE_LOG),
        "自证门的错误不得指向基线文件：{msg}"
    );
}

#[test]
fn green_self_check_passes() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(
        tmp.path().join(SELF_CHECK_LOG),
        "command: cargo test\nexit_code: 0\n455 passed\n",
    )
    .unwrap();
    let ev = check_self_check(tmp.path()).unwrap();
    assert_eq!(ev.exit_code, 0);
    assert_eq!(ev.command, "cargo test");
}

#[test]
fn parse_baseline_log_keeps_its_single_argument_signature() {
    // 第二调用方（testing_gate）按这个签名调：签名一变就得连带改它（不在白名单内）
    let _f: fn(&str) -> Result<BaselineEvidence> = parse_baseline_log;
}

#[test]
fn parse_accepts_markdown_list_item_evidence() {
    // F16：agent 用 markdown 列表项写证据（`- exit_code: 0`）语义与裸格式相同，
    // 列表符号不参与语义（10-22 c7b 场：列表项被判「缺退出码」→ 整场 Quality）
    let ev = parse_baseline_log("- command: `env -u X cargo test`\n- exit_code: 0\n").unwrap();
    assert_eq!(ev.exit_code, 0);
    assert_eq!(ev.command, "`env -u X cargo test`");
}

#[test]
fn parse_still_rejects_list_item_without_exit_code() {
    // 容差只认列表符号，不放松语义：真缺退出码依旧拒
    let err = parse_baseline_log("- command: cargo test\n- 摘要：567 passed\n").unwrap_err();
    let crate::error::DevforgeError::Quality(msg) = &err else {
        panic!("应是质量失败：{err}");
    };
    assert!(msg.contains("缺退出码"), "{msg}");
}

#[test]
fn parse_accepts_bold_list_item_exit_code() {
    // F16b：`- exit_code: **0**`（列表项+粗体）与 `exit_code: 0` 同义
    // （10-23 c7b 场：测试 agent 以列表项+粗体写回归日志，粗体未剥被判「缺退出码」）
    let ev = parse_baseline_log(
        "- command: `env -u X cargo test --offline`\n- exit_code: **0**\n- 摘要：578 passed\n",
    )
    .unwrap();
    assert_eq!(ev.exit_code, 0);
}

#[test]
fn parse_bold_exit_code_with_prose_tail_takes_leading_int() {
    // 粗体后带散文：取前导整数
    let ev = parse_baseline_log("- exit_code: **0**（44 passed / 0 failed）\n").unwrap();
    assert_eq!(ev.exit_code, 0);
}

#[test]
fn parse_first_exit_code_wins_over_later_reverse_validation() {
    // 首中即停：主回归在最前；后面的 exit_code 行是反向验证「能失败」的
    // 故意 101，不得覆盖主结果（否则永不绿）
    let ev = parse_baseline_log(
        "command: cargo test\nexit_code: 0\n\n## 反向验证\n- exit_code: **101**（故意破坏后必红）\n",
    )
    .unwrap();
    assert_eq!(ev.exit_code, 0);
}

#[test]
fn regression_claim_text_is_unchanged() {
    // 回归路径的文案不得被本次改造带偏（质量失败载荷仍以「回归日志不可解析」开头）
    let err = crate::change::testing_gate::check_regression_claim("command: cargo test\n", true)
        .unwrap_err();
    let crate::error::DevforgeError::Quality(msg) = &err else {
        panic!("应是质量失败：{err}");
    };
    // 自审 minor（88）：文案必须指向**回归日志本身**（旧口径借用基线门的措辞，
    // 错误里出现 evidence/baseline-test.log，运维会找错文件）。
    assert!(msg.starts_with("回归日志 evidence/regression.log 不可解析"), "{msg}");
    assert!(
        !msg.contains("baseline-test.log"),
        "回归门的错误不得指向基线证据文件：{msg}"
    );
}

#[test]
#[ignore = "依赖本机台账（~/.devforge/plantree），换机器/CI 必红——需要时手动跑"]
fn decision_ledger_registers_d023() {
    // 决策台账同步（D023）：纪律供给口径 + change 路径的先证后报机械门 + 门只落 change 路径。
    // 放在本文件：白名单内不得新增测试文件，而本变更的门与该决策同源。
    let path = crate::change::prepare::default_plantree_root("devforge")
        .join("plans/009-change-sop/decisions/README.md");
    // 2026-10-27 自审 minor（59）：台账已移出仓（2026-10-25），无该台账的机器/CI 上
    // 不报红也不 panic——直接跳过并说明（之前是 panic，忽略标记只是绕开）。
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("跳过：决策台账不在本机（{}），需人工核对 D023", path.display());
        return;
    };
    assert!(text.contains("D023"), "台账未登记 D023");
    for anchor in ["coding-discipline", "evidence/self-check.log", "只落 change 路径"] {
        assert!(text.contains(anchor), "D023 缺登记点：{anchor}");
    }
}

#[test]
fn self_check_and_baseline_logs_are_independent_files() {
    // 两道门各读各的文件：基线绿不代表自证绿，自证绿也不代表基线绿
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(
        tmp.path().join(BASELINE_LOG),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(SELF_CHECK_LOG),
        "command: cargo test\nexit_code: 1\n",
    )
    .unwrap();
    assert!(check_baseline_log(tmp.path()).is_ok());
    assert!(check_self_check(tmp.path()).is_err());
}

#[test]
fn steps_are_parsed_from_headings() {
    let md = "# 实施步骤\n## 步骤 1 改接口\n## 步骤 2：补测试\n### Step 3 收尾\n";
    let steps = parse_steps(md);
    assert_eq!(
        steps.iter().map(|s| s.number).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(steps[1].title, "补测试");
}

/// 2026-10-27 自审 minor（15）：围栏代码块里的 `# 注释` 不是步骤标题——
/// 旧口径只看行首 `#`，脚本示例会把步骤表弄脏（多出幽灵步骤 → 对账必红）。
#[test]
fn fenced_code_headings_are_not_steps() {
    let md = "## 步骤 1 改接口\n\n```sh\n# 步骤 9 本地跑一下\nrun_ci.sh\n```\n\n~~~\n# 步骤 8 也不是\n~~~\n\n## 步骤 2 补测试\n";
    let steps = parse_steps(md);
    assert_eq!(
        steps.iter().map(|s| s.number).collect::<Vec<_>>(),
        vec![1, 2],
        "围栏内的 # 注释不得当成步骤：{steps:?}"
    );
}

#[test]
fn step_count_matches_commits() {
    let (tmp, sha) = repo_with_baseline();
    std::fs::write(tmp.path().join("a.txt"), "a2\n").unwrap();
    commit(tmp.path(), "change: change-20261020-x step 1");
    std::fs::write(tmp.path().join("b.txt"), "b\n").unwrap();
    commit(tmp.path(), "change: change-20261020-x step 2");

    let steps = vec![
        Step { number: 1, title: "one".into() },
        Step { number: 2, title: "two".into() },
    ];
    let n = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap();
    assert_eq!(n, 2);
}

/// 自审 major 回归：一步一提交是文档承诺的不变量——单个提交声明两个步号
/// 必须硬门拒绝（旧实现只比对集合，声明多号照样过）。
#[test]
fn one_commit_declaring_two_steps_is_rejected() {
    let (tmp, sha) = repo_with_baseline();
    std::fs::write(tmp.path().join("a.txt"), "a2\n").unwrap();
    commit(tmp.path(), "change: change-20261020-x step 1 step 2");
    let steps = vec![
        Step { number: 1, title: "one".into() },
        Step { number: 2, title: "two".into() },
    ];
    let err = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap_err();
    assert!(err.to_string().contains("一步一提交"), "{err}");
}

/// 自审 major 回归：git 故障分类对齐——基线 SHA 失效（git log 非零退出）
/// 是基础设施态，必须 System 可重启，不得 Quality 判死。
#[test]
fn broken_baseline_sha_is_system_not_quality() {
    let (tmp, _sha) = repo_with_baseline();
    std::fs::write(tmp.path().join("a.txt"), "a2\n").unwrap();
    commit(tmp.path(), "change: change-20261020-x step 1");
    let steps = vec![Step { number: 1, title: "one".into() }];
    let err =
        check_step_commits(tmp.path(), "deadbeef0000000", "change-20261020-x", &steps)
            .unwrap_err();
    assert!(
        !err.is_quality_failure(),
        "基线 SHA 失效应是 System 可重启：{err}"
    );
}

/// 自审 major 回归：新鲜度硬门此前零覆盖——上轮残留证据（mtime 早于本轮开始）
/// 必须被 _fresh 门拒收，弱化门传 UNIX_EPOCH 恒放行的形态不得在生产面出现。
#[test]
fn fresh_gates_reject_stale_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
    std::fs::write(
        tmp.path().join(BASELINE_LOG),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(SELF_CHECK_LOG),
        "command: cargo test\nexit_code: 0\n",
    )
    .unwrap();
    // not_before = 写入之后 60s → 现有文件必然是「上一轮残留」
    // （2026-10-27：新鲜度门有 2s 粒度容差（粗粒度文件系统），测试阈值必须远大于容差）
    let not_before = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
    let err = check_baseline_log_fresh(tmp.path(), not_before).unwrap_err();
    assert!(err.to_string().contains("上一轮残留"), "{err}");
    let err = check_self_check_fresh(tmp.path(), not_before).unwrap_err();
    assert!(err.to_string().contains("上一轮残留"), "{err}");
}

#[test]
fn missing_step_commit_fails() {
    let (tmp, sha) = repo_with_baseline();
    std::fs::write(tmp.path().join("a.txt"), "a3\n").unwrap();
    commit(tmp.path(), "change: change-20261020-x step 1");

    let steps = vec![
        Step { number: 1, title: "one".into() },
        Step { number: 2, title: "two".into() },
    ];
    let err = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap_err();
    assert!(err.to_string().contains("对不上"), "{err}");
}

#[test]
fn duplicate_step_or_missing_prefix_fails() {
    let (tmp, sha) = repo_with_baseline();
    commit(tmp.path(), "change: change-20261020-x step 1");
    commit(tmp.path(), "随手改一下");
    let steps = vec![Step { number: 1, title: "one".into() }];
    let err = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap_err();
    assert!(err.to_string().contains("无法机械归属"), "{err}");
}

#[test]
fn extra_commit_without_step_number_fails() {
    let (tmp, sha) = repo_with_baseline();
    commit(tmp.path(), "change: change-20261020-x step 1");
    commit(tmp.path(), "change: change-20261020-x 顺手改");
    let steps = vec![Step { number: 1, title: "one".into() }];
    let err = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap_err();
    assert!(err.to_string().contains("没写步骤号"), "{err}");
}

#[test]
fn empty_impl_steps_is_quality_failure() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(IMPL_STEPS), "# 实施步骤\n随便写写\n").unwrap();
    let err = read_steps(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("没有任何"), "{err}");
    assert!(err.is_quality_failure());
}

#[test]
fn step_numbers_in_subject_handles_chinese_and_repeats() {
    assert_eq!(step_numbers_in("change: c1 step 3"), vec![3]);
    assert_eq!(step_numbers_in("change: c1 步骤 2"), vec![2]);
    assert_eq!(step_numbers_in("change: c1 step 2 step 2"), vec![2]);
}

#[test]
fn step_numbers_ignore_digits_in_prose_after_a_marker() {
    // F15：正文数字不是步号——标记后必须紧跟数字（允许空白/冒号分隔），
    // 否则跳过该次出现，继续找下一个标记（不远处吞到别的数字）
    assert_eq!(
        step_numbers_in(
            "change: c1 step 8 —— 把「正在做什么」写进心跳的步骤字段…下一次 start 撞 409"
        ),
        vec![8]
    );
    assert_eq!(step_numbers_in("change: c1 步骤字段里提到 409"), Vec::<u32>::new());
}

#[test]
fn eleven_steps_with_prose_digits_reconcile_via_check_step_commits() {
    // 2026-10-27 自审 minor（60）：旧用例自己复刻了对账逻辑（只测 step_numbers_in + 自己
    // sort/比较）——真实 check_step_commits 坏了它照样绿（空洞）。现改成真跑一次 git 仓 +
    // 真实 impl-steps，走 check_step_commits 本尊。
    // 11 步提交，其中第 8 步正文含「409」；旧 F15 缺陷会吞出幻影步号。
    let (tmp, _sha) = repo_with_baseline();
    for n in 1..=11u32 {
        std::fs::write(tmp.path().join(format!("f{n}.txt")), format!("v{n}\n")).unwrap();
        let extra = if n == 8 { "：下一次 start 撞 409" } else { "" };
        commit(
            tmp.path(),
            &format!("change: change-20261020-x step {n} 第 {n} 步{extra}"),
        );
    }
    let baseline = git_rev(tmp.path(), "HEAD~11");
    let steps: Vec<Step> = (1..=11).map(|n| Step { number: n, title: String::new() }).collect();
    let commits = check_step_commits(tmp.path(), &baseline, "change-20261020-x", &steps)
        .expect("11 步一一对应应当对账通过");
    assert_eq!(commits, 11, "返回的是真实提交数");
}

#[test]
fn empty_subject_commit_is_not_silently_dropped() {
    // 自审 minor（55）回归：`git commit --allow-empty-message` 造的提交主题为空——
    // 旧口径在 commits_since 里把空串过滤掉，「每个提交都必须声明步号」对它不生效，
    // 提交数也少算。现在必须报质量失败并说清是什么提交。
    let (tmp, baseline) = repo_with_baseline();
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "--allow-empty",
            "--allow-empty-message",
            "-m",
            "",
        ])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "空主题提交应能造出来：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let steps = vec![Step {
        number: 1,
        title: String::new(),
    }];
    let err = check_step_commits(tmp.path(), &baseline, "change-20261020-x", &steps)
        .expect_err("空主题提交不得被静默放过");
    assert!(
        err.to_string().contains("空主题"),
        "错误文案要点名空主题提交：{err}"
    );
}

// ── P012-T007 · CG-02：格式示例不再遮蔽真实结果 ──
#[test]
fn format_example_with_concrete_number_does_not_mask_real_red() {
    // 报告探针用例 C：示例具体数字 0 在前，真实 101 在后 → 必须取 101（旧实现取 0 判绿）
    let text = "## 1 基线测试\n\n\
        证据格式应为：\n\
        exit_code: 0\n\n\
        实际执行：\n\
        command: cargo test\n\
        exit_code: 101\n";
    let e = parse_evidence_log(text, "baseline.log", "基线").unwrap();
    assert_eq!(e.exit_code, 101);
}

#[test]
fn fenced_example_does_not_mask_real_green() {
    let text = "说明：\n\
        ```text\n\
        exit_code: 0\n\
        ```\n\
        command: cargo test\n\
        exit_code: 0\n";
    let e = parse_evidence_log(text, "baseline.log", "基线").unwrap();
    assert_eq!(e.exit_code, 0);
}

#[test]
fn reverse_verification_after_real_result_still_loses() {
    // 原设计意图回归：真结果（绿 0）在前，反向验证故意 101 在后 → 仍取 0
    let text = "command: cargo test\nexit_code: 0\n\n## 反向验证\nexit_code: 101\n";
    let e = parse_evidence_log(text, "baseline.log", "基线").unwrap();
    assert_eq!(e.exit_code, 0);
}
