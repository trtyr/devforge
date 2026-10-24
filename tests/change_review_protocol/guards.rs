//! （ARC-R6 自 change_review_protocol.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{protocol::*, support::*};

/// 验收 25 · 协议违例上限判 Quality、审查实例失败上限判 System（守卫不得空转）。
#[test]
pub(super) fn a25_source_guard_pins_quality_for_violations_and_system_for_instance_failures() {
    // 错实现形态：把违例上限写成 System（会被自动重启）/ 把实例失败写成 Quality → 本用例必须变红
    let gate = include_str!("../../src/sop/review_gate.rs");
    let q = gate
        .find("DevforgeError::Quality")
        .expect("共享门里应有 Quality 构造");
    let q_near = &gate[q..(q + 200).min(gate.len())];
    assert!(
        q_near.contains("输出协议违例"),
        "协议违例上限必须判 Quality（不自动重启）：{q_near}"
    );
    let s = gate
        .find("DevforgeError::System")
        .expect("共享门里应有 System 构造");
    let s_near = &gate[s..(s + 200).min(gate.len())];
    assert!(
        s_near.contains("判定为基础设施故障"),
        "审查实例失败上限必须判 System（可重启）：{s_near}"
    );
    let server = include_str!("../../src/server/tests/retry.rs");
    assert!(
        server.contains("review_gate.rs"),
        "源码守卫必须扩到新模块（否则绿但没守任何东西）"
    );

    // 行为面：同一个共享门里，违例 → Quality，实例失败 → System
    let mut factory = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(NO_VERDICT),
    ]);
    let mut budget = ReviewBudget::default();
    let err = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        |text| {
            let report = parse_review_report(text)?;
            Ok(ReviewOutcome {
                report: report.clone(),
                violation: report.protocol_violation.then(|| report.issues.join("；")),
            })
        },
    )
    .unwrap_err();
    assert!(
        err.is_quality_failure(),
        "违例上限 = 质量失败（不自动重启）：{err}"
    );

    let mut factory = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(""),
        ScriptedReply::text(""),
        ScriptedReply::text(""),
        ScriptedReply::text(""),
    ]);
    let mut budget = ReviewBudget::default();
    let err = review_with_gate(
        &mut factory,
        0,
        |_| "审".to_string(),
        &mut budget,
        |text| {
            let report = parse_review_report(text)?;
            Ok(ReviewOutcome {
                report: report.clone(),
                violation: report.protocol_violation.then(|| report.issues.join("；")),
            })
        },
    )
    .unwrap_err();
    assert!(
        !err.is_quality_failure(),
        "实例失败上限 = 基础设施故障（可重启）：{err}"
    );
    assert_eq!(
        MAX_PROTOCOL_VIOLATIONS, 3,
        "违例重审上限常量是单一事实源（3）"
    );
    assert_eq!(
        MAX_REVIEW_INSTANCE_FAILURES, 3,
        "实例失败重试上限常量是单一事实源（3）"
    );
}

// ---------------- 验收 26：审查轮次计入重审 ----------------

/// 验收 26 · change 侧发生过一次违例重审 → 报告里的「审查轮次」计入这次重审（不是恒为 1）。
#[test]
pub(super) fn a26_review_rounds_count_the_re_review() {
    // 错实现形态：把审查轮次硬编码成 1（重审被藏起来）→ 本用例必须变红
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(PASS),
    ]);
    let mut session = plan_only(f.ctx.clone(), plan_agent, plan_review);
    session.run_plan(&mut NoUser, 0).expect("违例后应通过");
    assert_eq!(
        session.plan_reviews(),
        2,
        "plan 审查轮次 = 实际开过的实例数（违例重审要计入）"
    );

    // 端到端：整条 run 的报告里，审查轮次 = plan(2) + coding(1) + test(1)
    let f = fixture();
    let plan_agent = ScriptedAgent::new(&["写完了\n<stage-complete>1</stage-complete>"]);
    let plan_review = ScriptedReviewFactory::new(vec![
        ScriptedReply::text(NO_VERDICT),
        ScriptedReply::text(PASS),
    ]);
    let coding_agent = ScriptedAgent::new(&[
        "基线取证：cargo test\nexit_code: 0\n<stage-complete>1</stage-complete>",
        "改完了\n<stage-complete>1</stage-complete>",
    ]);
    let test_agent = ScriptedAgent::new(&["测完了\n<stage-complete>1</stage-complete>"]);
    let mut session = prod_session(
        f.ctx.clone(),
        plan_agent,
        plan_review,
        coding_agent,
        ScriptedReviewFactory::new(vec![ScriptedReply::text(PASS)]),
        test_agent,
        ScriptedReviewFactory::new(vec![ScriptedReply::text(PASS)]),
    );
    let store = StateStore::new(f.ctx.blackboard.join("state"), StateKey::new("proj", "task", CHANGE_SOP));
    let run = ChangeRun {
        store: &store,
        repo: &f.ctx.repo,
        change_id: "change-1",
        baseline_sha: &f.baseline,
        criteria: Vec::new(),
        whitelist: Vec::new(),
        usage: "1,000 tokens".into(),
        run_id: "run-1".into(),
        wall_secs: 1,
    };
    let outcome = run_change(run, &mut session, &mut NoUser).expect("整条 run 应走通");
    assert_eq!(
        outcome.report.review_rounds, 4,
        "审查轮次 = 实际开过的实例数（plan 2 + coding 1 + test 1），不是恒为 1"
    );
}

// ---------------- 验收 27：决策台账 D026 ----------------

/// 验收 27 · 决策台账含 `## D026 ·` 一节（三裁决 + 三通道 + 尾部协议位 + 遗留），既有条目一字不动。
#[test]
pub(super) fn a27_decision_ledger_registers_d026_without_touching_the_old_entries() {
    // 错实现形态：把 D026 写成 D024/D025（撞号）/ 删掉既有条目 → 本用例必须变红
    let path = repo_root().join("docs/plantree/plans/009-change-sop/decisions/README.md");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("台账不可读：{}", path.display()));
    assert_eq!(text.matches("## D026 ·").count(), 1, "D026 只能登记一次");
    assert_eq!(text.matches("## D024 ·").count(), 1, "D024 仍在且只登记一次");
    assert_eq!(text.matches("## D025 ·").count(), 1, "D025 仍在且只登记一次");

    let d026 = text
        .split("## D026 ·")
        .nth(1)
        .expect("台账里有 D026 一节");
    for anchor in [
        "三通道",
        "判负",
        "协议违例",
        "实例失败",
        "空箱必拒收",
        "请补齐失败明细后重发",
        "尾部协议位",
        "遗留",
    ] {
        assert!(d026.contains(anchor), "D026 缺登记项：{anchor}");
    }
}

// ---------------- 验收 28：topic 文档口径 ----------------

/// 验收 28 · agents-change.md 改正旧口径 + vs-mvp.md 改正差异四。
#[test]
pub(super) fn a28_topic_docs_match_the_new_contract() {
    // 错实现形态：文档仍写「测试 review FAIL → `<route>coding</route>`」/「判基础设施故障」→ 本用例必须变红
    let agents = std::fs::read_to_string(
        repo_root().join("docs/plantree/plans/009-change-sop/topics/agents-change.md"),
    )
    .expect("agents-change.md 可读");
    assert!(
        !agents.contains("测试 review FAIL → `<route>coding</route>`"),
        "旧口径（与代码不符）必须改正"
    );
    for anchor in ["判负", "协议违例", "实例失败", "尾部协议位", "<issues>"] {
        assert!(agents.contains(anchor), "agents-change.md 缺新口径：{anchor}");
    }

    let vs = std::fs::read_to_string(
        repo_root().join("docs/plantree/plans/009-change-sop/topics/vs-mvp.md"),
    )
    .expect("vs-mvp.md 可读");
    assert!(
        !vs.contains("连续 3 次无标记 → 第 4 次判基础设施故障"),
        "差异四的旧口径（与代码不符）必须改正"
    );
    assert!(
        vs.contains("连续 3 次无标记 → 第 4 次判**质量失败**"),
        "差异四要写明「判质量失败」"
    );
    assert!(
        vs.contains("重审有界（≤3）"),
        "差异四要补「审查输出违例的重审有界（≤3）」"
    );
}

// ---------------- 验收 29/30/31：白名单常量与提交内容 ----------------

/// 验收 29 · 守卫文件的两个常量 = 本场基线 + coding 白名单 23 个文件，提交扫描通过。
#[test]
#[ignore = "守卫常量由 change run 刷新；两场之间无 step 提交（F10/F13）"]
pub(super) fn a29_guard_constants_point_at_this_change() {
    // 错实现形态：常量没刷新（还指着上一场的基线 / 上一场的白名单）→ 本用例必须变红
    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("守卫文件可读");
    assert!(
        guard.contains(&format!("const BASELINE: &str = \"{BASELINE}\";")),
        "守卫的 BASELINE 必须等于本场基线 sha {BASELINE}"
    );
    let listed = parse_whitelist(&guard);
    assert_eq!(
        listed.len(),
        CODING_WHITELIST.len(),
        "守卫的 WHITELIST 必须逐个列出 coding 白名单的 {} 个文件（实际 {listed:?}）",
        CODING_WHITELIST.len()
    );
    for f in CODING_WHITELIST {
        assert!(listed.iter().any(|x| x == f), "守卫的 WHITELIST 缺 {f}");
    }
    assert!(
        !listed.iter().any(|x| x == "tests/f6_observability/main.rs"),
        "测试阶段文件不得进 coding 步白名单常量"
    );

    // 提交扫描：coding 步提交的文件都在白名单内；其他提交只许是 tests/ 下的文件
    let root = repo_root();
    let log = git(&root, &["log", "--format=%H%x09%s", &format!("{BASELINE}..HEAD")]);
    assert!(!log.trim().is_empty(), "基线之后应当有本变更的提交");
    let mut coding_files: Vec<String> = Vec::new();
    for line in log.lines() {
        let (sha, subject) = line.split_once('\t').expect("提交行格式");
        for f in git(&root, &["show", "--name-only", "--format=", sha])
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let f = f.trim().to_string();
            if subject.contains(" step ") {
                assert!(
                    CODING_WHITELIST.contains(&f.as_str()),
                    "coding 步提交越界（白名单外）：{f}（提交 {subject}）"
                );
                coding_files.push(f);
            } else {
                assert!(
                    f.starts_with("tests/"),
                    "白名单外的非测试改动：{f}（提交 {subject}）"
                );
            }
        }
    }
    for f in CODING_WHITELIST {
        assert!(
            coding_files.iter().any(|x| x == f),
            "白名单文件 {f} 没被本场任何 coding 步提交改到"
        );
    }
}

/// 验收 30 · 上一场验收测试文件的两个常量与守卫文件逐字一致。
#[test]
pub(super) fn a30_acceptance_file_constants_match_the_guard() {
    // 错实现形态：两份常量不同步（a16 比对失败）→ 本用例必须变红
    let guard = std::fs::read_to_string(repo_root().join("tests/change_ask_ring.rs"))
        .expect("守卫文件可读");
    let acc = std::fs::read_to_string(repo_root().join("tests/change_failure_context/support.rs"))
        .expect("验收测试文件可读");
    assert!(
        acc.contains(&format!("const BASELINE: &str = \"{BASELINE}\";")),
        "验收测试文件的 BASELINE 必须与守卫一致"
    );
    assert_eq!(
        parse_whitelist(&acc),
        parse_whitelist(&guard),
        "两份 WHITELIST 常量必须逐字一致"
    );
}

/// 验收 31 · 本变更的提交内容 ⊆ 计划 §4 白名单（24 个文件），未跟踪产物不入库。
#[test]
#[ignore = "守卫常量由 change run 刷新；两场之间无 step 提交（F10/F13）"]
pub(super) fn a31_the_change_commits_stay_inside_the_plan_whitelist() {
    // 错实现形态：把白名单外的文件带进本变更的提交 / 顺手 `git add -A` 把未跟踪产物卷进来 → 本用例必须变红
    let root = repo_root();
    let log = git(&root, &["log", "--format=%H%x09%s", &format!("{BASELINE}..HEAD")]);
    assert!(!log.trim().is_empty(), "基线之后应当有本变更的提交");
    for line in log.lines() {
        let (sha, subject) = line.split_once('\t').expect("提交行格式");
        for f in git(&root, &["show", "--name-only", "--format=", sha])
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let f = f.trim();
            assert!(
                PLAN_WHITELIST.contains(&f),
                "提交内容越界（计划 §4 白名单外）：{f}（提交 {subject}）"
            );
        }
    }
    for artifact in [".pi/goals/", "tests/e2e/results/", "docs/plantree/changes/"] {
        let touched = git(
            &root,
            &[
                "log",
                "--format=%H",
                "--name-only",
                &format!("{BASELINE}..HEAD"),
                "--",
                artifact,
            ],
        );
        assert!(
            touched.trim().is_empty(),
            "未跟踪产物 {artifact} 被提交进来了：{touched}"
        );
    }
}

/// 验收 31（续）· 计划 §4 白名单 = 24 个文件（coding 23 + 测试阶段 1；coding_gate.rs 为操作员补录扩容）。
#[test]
pub(super) fn a32_the_plan_whitelist_has_twenty_three_files() {
    // 错实现形态：白名单数目写错（22/25）→ 本用例必须变红（函数名保留历史称呼）
    assert_eq!(PLAN_WHITELIST.len(), 24, "计划 §4 白名单 = 24 个文件");
    assert_eq!(CODING_WHITELIST.len(), 23, "coding 步白名单 = 23 个文件");
    assert!(
        PLAN_WHITELIST.contains(&"tests/f6_observability/main.rs"),
        "计划 §4 白名单要含唯一的测试阶段文件"
    );
    assert!(
        !CODING_WHITELIST.contains(&"tests/f6_observability/main.rs"),
        "测试阶段文件不得进 coding 步白名单"
    );
}

// ---------------- 验收 32：结构体字段与构造点 ----------------

/// 验收 32 · `ProdChangeSession` 新增 `plan_review_instances`，全仓字面量构造点都补齐。
#[test]
pub(super) fn a33_every_prod_change_session_literal_sets_the_new_field() {
    // 错实现形态：某个字面量构造点漏补 `plan_review_instances` → 编译不过（这里逐点扫出来）→ 本用例必须变红
    let root = repo_root();
    let mut checked = 0usize;
    for rel in [
        "src/change/session.rs",
        "src/server/change_engine.rs",
        "tests/change_failure_context/context.rs",
        "tests/change_ask_ring.rs",
    ] {
        let text = std::fs::read_to_string(root.join(rel)).unwrap_or_else(|_| panic!("不可读：{rel}"));
        let mut rest = text.as_str();
        while let Some(pos) = rest.find("ProdChangeSession {") {
            let head = &rest[..pos];
            // `pub struct ProdChangeSession {` 是定义、`impl ProdChangeSession {` /
            // `impl ChangeSession for ProdChangeSession {` 是方法块，都不是构造点
            let is_decl = ["struct ", "impl ", "for ", "enum ", "trait "]
                .iter()
                .any(|k| head.ends_with(k));
            let body = brace_body(rest, pos + "ProdChangeSession {".len() - 1);
            if !is_decl {
                assert!(
                    body.contains("plan_review_instances"),
                    "{rel} 的 ProdChangeSession 字面量漏补 plan_review_instances：\n{body}"
                );
                checked += 1;
            }
            rest = &rest[pos + "ProdChangeSession {".len()..];
        }
    }
    assert!(checked >= 4, "至少要扫到 4 个构造点（实际 {checked}）");
}

/// 取 `{` 起的配对花括号内容（结构体字面量的正文）。按**字节**扫描：花括号是 ASCII，
/// 多字节 UTF-8 序列里不会出现 ASCII 字节，按字节扫安全。
pub(super) fn brace_body(src: &str, open_at: usize) -> String {
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut end = bytes.len();
    for (i, c) in bytes.iter().enumerate().skip(open_at) {
        match c {
            b'{' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    String::from_utf8_lossy(&bytes[open_at..end]).to_string()
}

// ---------------- 工具 ----------------

/// 从守卫 / 验收测试文件的源码里解析白名单数组。
pub(super) fn parse_whitelist(src: &str) -> Vec<String> {
    let start = src
        .find("const WHITELIST: &[&str] = &[")
        .expect("文件里应有 WHITELIST 常量");
    let rest = &src[start..];
    let end = rest.find("];").expect("WHITELIST 数组应有结尾");
    rest[..end]
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            l.strip_prefix('"')?.strip_suffix("\",")
        })
        .map(|s| s.to_string())
        .collect()
}

