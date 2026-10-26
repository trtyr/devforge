//! （2026-10-26 测试分离：自 src/change/plan_audit.rs 内联平移，纯移动零改动。）
use super::*;

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(tmp.path().join("src/b.rs"), "// b\n").unwrap();
    tmp
}

fn good_plan() -> String {
    "# 变更计划\n\n\
## 1 需求复述\n把 a 改成 b。\n\n\
## 2 影响面\n- 直接命中：`src/a.rs:1`\n- 调用方：`src/b.rs:1`\n\n\
## 3 要改的\n| 文件 | 理由 |\n| --- | --- |\n| `src/a.rs` | 需求点 |\n\n\
## 4 白名单\n`src/a.rs`\n`tests/a_test.rs`\n\n\
## 5 验收标准\n- 给定 x=1 → 返回 2\n\n\
## 6 风险\n- 无\n"
        .to_string()
}

#[test]
fn plan_facts_ignores_table_rows_and_keeps_bullets() {
    // t26 v3 实证：plan 把验收标准写成 markdown 表格，旧行为连表头都抽成条款（23 条鬼条款）
    let plan = "## 5 验收标准\n\
        | # | 给定 | 期望 |\n\
        |---|---|---|\n\
        | AC1 | count_words(a b) | == 2 |\n\
        普通段落不算条款。\n\
        - 给定 x=1 → 返回 2\n\
        * 给定空串 → 返回 0\n";
    let (criteria, _) = plan_facts(plan);
    assert_eq!(
        criteria,
        vec![
            "给定 x=1 → 返回 2".to_string(),
            "给定空串 → 返回 0".to_string()
        ],
        "只有列表行算条款：{criteria:?}"
    );
}

#[test]
fn clean_plan_passes_with_no_issues() {
    let r = repo();
    // 引擎审计修复 #2：不再预先创建 tests/a_test.rs——白名单中的新增文件
    // （不在「要改的」里）在 plan 时必然不存在，不再报 issue。
    let a = audit(&good_plan(), r.path());
    assert!(a.is_clean(), "审计应无问题：{:?}", a.issues);
    assert!(a.path_refs >= 2);
    assert!(a.missing_blocks.is_empty());
}

/// 引擎审计修复 #2 回归：存在性只对「要改的」强制——修改目标不存在仍是 issue。
#[test]
fn modify_target_must_exist() {
    let r = repo();
    let plan = "# 变更计划\n\
## 1 需求复述\n把 a 改成 b。\n\n\
## 2 影响面\n- 直接命中：`src/ghost.rs:1`\n\n\
## 3 要改的\n| 文件 | 理由 |\n| --- | --- |\n| `src/ghost.rs` | 需求点 |\n\n\
## 4 白名单\n`src/ghost.rs`\n\n\
## 5 验收标准\n- 给定 x=1 → 返回 2\n\n\
## 6 风险\n- 无\n";
    let a = audit(plan, r.path());
    assert!(
        a.issues.iter().any(|i| i.contains("要改的文件不存在")),
        "修改目标不存在仍应报 issue：{:?}",
        a.issues
    );
}

#[test]
fn nonexistent_path_is_listed() {
    let r = repo();
    let plan = good_plan().replace("`src/b.rs:1`", "`src/ghost.rs:42`");
    let a = audit(&plan, r.path());
    assert!(
        a.issues.iter().any(|i| i.contains("src/ghost.rs:42")),
        "{:?}",
        a.issues
    );
}

#[test]
fn missing_block_is_reported() {
    let r = repo();
    let plan = good_plan().replace("## 6 风险", "## 六 其他");
    let a = audit(&plan, r.path());
    assert!(a.missing_blocks.contains(&"风险".to_string()));
    assert!(a.issues.iter().any(|i| i.contains("六块不全")));
}

#[test]
fn file_outside_whitelist_is_flagged() {
    let r = repo();
    let plan = good_plan().replace(
        "| `src/a.rs` | 需求点 |",
        "| `src/a.rs` | 需求点 |\n| `src/b.rs` | 顺手改 |",
    );
    let a = audit(&plan, r.path());
    assert!(
        a.issues.iter().any(|i| i.contains("src/b.rs") && i.contains("不在白名单")),
        "{:?}",
        a.issues
    );
}

#[test]
fn vague_acceptance_is_flagged() {
    let r = repo();
    let plan = good_plan().replace("- 给定 x=1 → 返回 2", "- 功能正常");
    let a = audit(&plan, r.path());
    assert!(
        a.issues.iter().any(|i| i.contains("不可观测")),
        "{:?}",
        a.issues
    );
}

#[test]
fn empty_whitelist_is_flagged() {
    let r = repo();
    let plan = good_plan().replace("## 4 白名单\n`src/a.rs`\n`tests/a_test.rs`", "## 4 白名单\n");
    let a = audit(&plan, r.path());
    assert!(a.issues.iter().any(|i| i.contains("白名单为空")));
}

#[test]
fn whitelist_outside_contract_is_flagged_with_contract_awareness() {
    // P012 挂账①：契约存在时，白名单超出契约 owned_paths = 提案越界（plan 阶段即打回）
    let r = repo();
    let plan = good_plan();
    // 无契约：不产生越界 issue（向后兼容）
    let a = audit(&plan, r.path());
    assert!(!a.issues.iter().any(|i| i.contains("超出契约")));
    // 契约只覆盖 docs/**：白名单的 src/a.rs 越界
    let contract = crate::domain::contract::WorkItemContract {
        owned_paths: vec!["docs/**".into()],
        forbidden_paths: vec![],
        base_rev: None,
        isolation: crate::domain::contract::Isolation::None,
    };
    let a = audit_with_contract(&plan, r.path(), Some(&contract));
    assert!(
        a.issues.iter().any(|i| i.contains("src/a.rs") && i.contains("超出契约")),
        "提案越界应被打回：{:?}",
        a.issues
    );
    // 契约覆盖 src/** + tests/**：全白名单合规 → 无越界 issue
    let contract = crate::domain::contract::WorkItemContract {
        owned_paths: vec!["src/**".into(), "tests/**".into()],
        forbidden_paths: vec![],
        base_rev: None,
        isolation: crate::domain::contract::Isolation::None,
    };
    let a = audit_with_contract(&plan, r.path(), Some(&contract));
    assert!(!a.issues.iter().any(|i| i.contains("超出契约")));
}

#[test]
fn review_instruction_carries_audit_plan_text_and_forbids_rerun() {
    let r = repo();
    let plan = good_plan();
    let a = audit(&plan, r.path());
    let plan_path = r.path().join("docs/changes/x/change-plan.md");
    let text = review_instruction("change-20261020-x", &plan_path, &plan, &a);
    for anchor in [
        "不看 diff",
        "不跑验收命令",
        "勿质疑、勿重算",
        "影响面完整性",
        "验收标准可观测",
        "<stage-verdict>PASS|FAIL</stage-verdict>",
    ] {
        assert!(text.contains(anchor), "审查指令缺锚点：{anchor}");
    }
    // 审查子进程 cwd 是黑板：不给路径与全文，审查官读不到计划
    assert!(
        text.contains(&plan_path.display().to_string()),
        "审查指令必须给计划路径：{text}"
    );
    assert!(text.contains("## 计划全文"), "{text}");
    assert!(
        text.contains("给定 x=1 → 返回 2"),
        "审查指令必须附计划全文：{text}"
    );
}

// ---------------- 裁决门（问询环的结构与留痕） ----------------

/// 造一份带「歧义」小节的计划（其余五块齐全，不影响本组用例）。
fn plan_with_ambiguity(items: &str) -> String {
    format!(
        "# 变更计划\n\n\
## 1 需求复述\n改 a。\n\n\
### 1.1 歧义与本计划采用的默认\n\
{items}\n\n\
## 2 影响面\n- `src/a.rs:1`\n\n\
## 3 要改的\n| `src/a.rs` | 需求 |\n\n\
## 4 白名单\n`src/a.rs`\n\n\
## 5 验收标准\n- 给定 x=1 → 返回 2\n\n\
## 6 风险\n- 无\n"
    )
}

#[test]
fn must_ask_entry_without_answer_field_is_a_hard_issue() {
    // 错实现形态：把「留痕检查」删掉（只数条目不看字段）→ 本用例必须变红
    let plan = plan_with_ambiguity("- [必须裁决] 复合词算一个词还是两个词\n");
    let a = audit_verdicts(&plan);
    assert!(
        a.issues.len() >= 1,
        "标了必须裁决却没留痕必须判失败：{:?}",
        a.facts
    );
    let issue = &a.issues[0];
    assert!(issue.contains("必须裁决"), "问题要点名标记：{issue}");
    assert!(
        issue.contains("ask-user-question"),
        "问题要告诉 agent 下一步该干什么：{issue}"
    );
}

#[test]
fn must_ask_entry_with_answer_field_passes() {
    // 错实现形态：把「续行里的字段」当没留痕（只认首行）→ 本用例必须变红
    let plan = plan_with_ambiguity(
        "- [必须裁决] 复合词算一个词还是两个词\n  用户裁决：算一个词\n",
    );
    let a = audit_verdicts(&plan);
    assert!(a.is_clean(), "留痕齐全不该有问题：{:?}", a.issues);
    // 同一条目同行写也认
    let inline = plan_with_ambiguity("- [必须裁决] 口径未定（用户裁决：算一个词）\n");
    assert!(
        audit_verdicts(&inline).is_clean(),
        "同行留痕也该通过：{:?}",
        audit_verdicts(&inline).issues
    );
}

#[test]
fn must_ask_entry_with_empty_answer_field_is_a_hard_issue() {
    // 错实现形态：把「字段在但答复为空」当已留痕 → 本用例必须变红
    let plan = plan_with_ambiguity("- [必须裁决] 口径未定\n  用户裁决：\n");
    let a = audit_verdicts(&plan);
    assert!(
        a.issues.len() >= 1,
        "字段在但答复为空 = 没问过：{:?}",
        a.facts
    );
}

#[test]
fn all_self_decided_entries_pass() {
    // 错实现形态：把「[自决] 也要求留痕」→ 本用例必须变红（自决项不触发问询要求）
    let plan = plan_with_ambiguity("- [自决] 内部实现细节由 agent 定\n");
    let a = audit_verdicts(&plan);
    assert!(a.is_clean(), "全是自决项不该有问题：{:?}", a.issues);
}

#[test]
fn unclassified_entry_is_a_fact_not_a_failure() {
    // 错实现形态：把「未分类条目」当硬失败 → 本用例必须变红（分类恰当性交审查官）
    let plan = plan_with_ambiguity("- 复合词算一个词还是两个词\n");
    let a = audit_verdicts(&plan);
    assert!(a.is_clean(), "未分类条目不判硬失败：{:?}", a.issues);
    assert!(
        a.facts.iter().any(|f| f.contains("未分类") && f.contains("复合词")),
        "未分类条目要进确定性事实：{:?}",
        a.facts
    );
}

#[test]
fn prose_declaration_is_a_fact_not_a_failure() {
    // 错实现形态：把「散文里的必须裁决声明」当硬失败 → 本用例必须变红
    let plan = plan_with_ambiguity("这条口径必须用户裁决。\n- [自决] 别的点\n");
    let a = audit_verdicts(&plan);
    assert!(a.is_clean(), "散文声明不判硬失败：{:?}", a.issues);
    assert!(
        a.facts.iter().any(|f| f.contains("散文") && f.contains("必须用户裁决")),
        "散文声明要进确定性事实：{:?}",
        a.facts
    );
}

#[test]
fn missing_ambiguity_block_is_a_fact_not_a_failure() {
    // 错实现形态：把「歧义块缺失」当硬失败 → 本用例必须变红（结构缺失归审查官）
    let plan = "# 变更计划\n\n## 1 需求复述\n改 a。\n";
    let a = audit_verdicts(&plan);
    assert!(a.is_clean(), "歧义块缺失不判硬失败：{:?}", a.issues);
    assert!(
        a.facts.iter().any(|f| f.contains("歧义块缺失")),
        "歧义块缺失要进确定性事实：{:?}",
        a.facts
    );
}

#[test]
fn last_ambiguity_heading_wins() {
    // 错实现形态：把「同名标题取第一个」→ 本用例必须变红
    // （计划里「需求复述与歧义」与「歧义与本计划采用的默认」两处同名，只判后者）
    let plan = "# 变更计划\n\n\
## 1 需求复述与歧义\n\
- [必须裁决] 这条在旧标题下（不该被看到）\n\n\
### 1.3 歧义与本计划采用的默认\n\
- [自决] 这条在后面的标题下\n";
    let a = audit_verdicts(plan);
    assert!(a.is_clean(), "后出现的同名标题覆盖先前的：{:?}", a.issues);
}

#[test]
fn review_instruction_carries_verdict_facts_and_new_criteria() {        // 错实现形态：把「裁决门事实 / 新审点」从审查指令里删掉 → 本用例必须变红
    let r = repo();
    let plan = plan_with_ambiguity("- 复合词算一个词还是两个词\n");
    let a = audit(&plan, r.path());
    let plan_path = r.path().join("docs/changes/x/change-plan.md");
    let text = review_instruction("change-20261021-x", &plan_path, &plan, &a);
    assert!(text.contains("裁决门事实"), "审查指令要带裁决门事实段：{text}");
    assert!(
        text.contains("未分类") && text.contains("复合词"),
        "未分类条目要出现在审查指令里：{text}"
    );
    for anchor in ["留痕核对", "分类恰当性", "结构缺失"] {
        assert!(text.contains(anchor), "审查指令缺新审点：{anchor}");
    }
}

/// D024 登记守卫：台账里 D024 登记一次，D009 标题带「供给部分由 D024 增补」的旁注。
///
/// 错实现形态：把 D024 删了 / 把 D009 的旁注删了 → 本用例必须变红。
#[test]
fn decision_ledger_registers_d024_and_annotates_d009() {
    let path = crate::change::prepare::default_plantree_root("devforge")
        .join("plans/009-change-sop/decisions/README.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("决策台账不可读：{}（台账已移出仓，2026-10-25）", path.display()));
    assert_eq!(
        text.matches("## D024 ·").count(),
        1,
        "D024 在台账里必须只登记一次"
    );
    let d009 = text
        .lines()
        .find(|l| l.starts_with("## D009 ·"))
        .expect("D009 标题在位");
    assert!(
        d009.contains("D024"),
        "D009 标题要带「供给部分由 D024 增补」的旁注（两边都标）：{d009}"
    );
}

/// 文档口径守卫：agents-change.md 的答复载体 / 供给 / 审点三处与代码一致。
///
/// 错实现形态：把文档里的载体口径或审点删了 → 本用例必须变红。
#[test]
fn agents_change_doc_matches_the_implemented_contract() {
    let path = crate::change::prepare::default_plantree_root("devforge")
        .join("plans/009-change-sop/topics/agents-change.md");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("文档不可读：{}（台账已移出仓，2026-10-25）", path.display()));
    // ① 答复载体：标记 + 字段 + 禁默认放行
    for anchor in [VERDICT_MUST_ASK, VERDICT_SELF, VERDICT_ANSWER_FIELD] {
        assert!(text.contains(anchor), "文档缺答复载体口径：{anchor}");
    }
    assert!(
        text.contains("禁止") || text.contains("不许"),
        "文档缺「禁止带默认值自决放行」的口径"
    );
    // ② 供给：ask 包
    assert!(
        text.contains("ask-user-question"),
        "文档缺 ask 包供给口径（问询环的根因行）"
    );
    // ③ 审点：留痕核对 + 分类恰当性
    assert!(text.contains("留痕核对"), "文档缺留痕核对审点");
    assert!(text.contains("分类恰当性"), "文档缺分类恰当性审点");
}
