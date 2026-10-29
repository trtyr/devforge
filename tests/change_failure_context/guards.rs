//! （ARC-R6 自 change_failure_context.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{context::*, support::*};

/// 取某节正文（从标题行到下一个 `## ` 标题之前）。
pub(super) fn section_body(text: &str, title_prefix: &str) -> String {
    let start = text
        .find(title_prefix)
        .unwrap_or_else(|| panic!("台账里没有 {title_prefix} 这一节"));
    let rest = &text[start..];
    let end = rest[1..]
        .find("\n## ")
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

// ---------------- 验收 15：三处旧口径改写 ----------------

/// 验收 15 · `agents-change.md` 的 :172 / :211 与 `vs-mvp.md` 的 :24 不再写旧口径。
#[test]
pub(super) fn a15_the_three_old_wording_places_are_rewritten() {
    // 错实现形态：只改了两处 / 改写时漏了「不定位」这半句 → 本用例必须变红
    // 台账已移出仓（2026-10-25）：从默认台账根读
    let plantree = devforge::change::prepare::default_plantree_root("devforge");
    let agents = plantree.join("plans/009-change-sop/topics/agents-change.md");
    let agents_text = std::fs::read_to_string(&agents)
        .unwrap_or_else(|_| panic!("文档不可读：{}", agents.display()));
    let vs_mvp = plantree.join("plans/009-change-sop/topics/vs-mvp.md");
    let vs_mvp_text = std::fs::read_to_string(&vs_mvp)
        .unwrap_or_else(|_| panic!("文档不可读：{}", vs_mvp.display()));

    for (name, text, line_no) in [
        ("agents-change.md", &agents_text, 172usize),
        ("agents-change.md", &agents_text, 213),
        ("vs-mvp.md", &vs_mvp_text, 24),
    ] {
        let line = text
            .lines()
            .nth(line_no - 1)
            .unwrap_or_else(|| panic!("{name} 没有第 {line_no} 行"));
        assert!(
            line.contains("失败明细逐条原文"),
            "{name}:{line_no} 要写「失败明细逐条原文」：{line}"
        );
        for old in ["不做定位", "不定位、不分析原因"] {
            assert!(
                !line.contains(old),
                "{name}:{line_no} 还在写旧口径「{old}」：{line}"
            );
        }
    }
    // 全文不得残留旧口径的措辞
    for (name, text) in [
        ("agents-change.md", &agents_text),
        ("vs-mvp.md", &vs_mvp_text),
    ] {
        for old in ["不做定位", "不定位、不分析原因"] {
            assert!(
                !text.contains(old),
                "{name} 全文还残留旧口径「{old}」"
            );
        }
        assert!(
            text.contains("失败明细逐条原文"),
            "{name} 要写明失败明细逐条原文送达"
        );
    }
}

// ---------------- 验收 16：白名单守卫的两个常量 ----------------

/// 验收 16 · `tests/change_ask_ring.rs` 的 `BASELINE` / `WHITELIST` 与本场一致，且扫描通过。
#[test]
pub(super) fn a16_the_whitelist_constants_match_this_change() {
    // 错实现形态：常量没刷新（还指着上一场的基线 / 上一场的白名单）→ 守卫会把本场提交判成越界
    // → 本用例必须变红
    let path = repo_root().join("tests/change_ask_ring.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("测试文件不可读：{}", path.display()));
    assert!(
        text.contains(&format!("const BASELINE: &str = \"{BASELINE}\";")),
        "守卫的 BASELINE 必须等于本场基线 sha {BASELINE}"
    );
    let listed = parse_whitelist(&text);
    assert_eq!(
        listed.len(),
        WHITELIST.len(),
        "守卫的 WHITELIST 必须逐个列出本场白名单的 {} 个文件（实际 {listed:?}）",
        WHITELIST.len()
    );
    for f in WHITELIST {
        assert!(
            listed.iter().any(|x| x == f),
            "守卫的 WHITELIST 缺 {f}（实际 {listed:?}）"
        );
    }
    // 扫描必须通过：本场 coding 步提交的每个文件都在本场白名单内
    // F10：扫描面从「旧场基线区间 + 全场步提交」收敛到「本场 change-id 的步提交」——
    // 外场步提交不参与判定；本场步提交仍逐个对账本场白名单（守卫语义不退化）。
    let root = repo_root();
    let commits = change_step_commits(&root, CHANGE_ID);
    if commits.is_empty() {
        // 空集放行（用户裁决）：本场尚无 step 提交 → 没有可对账的对象。
        // 旧口径在这里「一个都没找到就红」——两场之间恒红，正是 F10 的病。
        println!("本场尚无 step 提交，跳过对账");
        return;
    }
    let findings = step_whitelist_violations(&root, &commits, CHANGE_WHITELIST);
    assert!(
        findings.is_empty(),
        "本场 coding 步提交越界（白名单外）：\n{}",
        findings
            .iter()
            .map(|f| f.message())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 从 `tests/change_ask_ring.rs` 的源码里解析白名单数组。
pub(super) fn parse_whitelist(src: &str) -> Vec<String> {
    let start = src
        .find("const WHITELIST: &[&str] = &[")
        .expect("守卫文件里应有 WHITELIST 常量");
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

// ---------------- 验收 17：提交内容不越界 ----------------

/// 验收 17 · 本变更的提交内容白名单外无改动；未跟踪产物一律没被 `git add`。
/// 10-22 运维：两场之间无 step 提交，此守卫恒红 → `#[ignore]`（F10/F13：
/// 守卫常量随场增殖，两场之间全体转红，真修 = gate 时物化）。
#[test]
#[ignore = "守卫常量由 change run 刷新；两场之间无 step 提交（F10/F13）"]
pub(super) fn a17_the_change_commits_stay_inside_the_whitelist() {
    // 错实现形态：把白名单外的文件带进本变更的提交 / 顺手 `git add -A` 把未跟踪产物卷进来
    // → 本用例必须变红
    // F10：扫描面与 a16 同一条（本场 change-id 的步提交）+ 本场白名单对账。
    // 「旧场 23 个白名单文件都要被本场改到」这条按场过滤后本场恒不成立 → 删（用户裁决）；
    // 「基线之后应当有本变更的提交」这条空集断言同理（两场之间恒红，与 F10 同病）。
    let root = repo_root();
    let commits = change_commits(&root, CHANGE_ID);
    // coding 步提交：文件必须逐个在**本场**白名单内
    let steps = step_commits(&commits);
    let findings = step_whitelist_violations(&root, &steps, CHANGE_WHITELIST);
    assert!(
        findings.is_empty(),
        "本场 coding 步提交越界（白名单外）：\n{}",
        findings
            .iter()
            .map(|f| f.message())
            .collect::<Vec<_>>()
            .join("\n")
    );
    // 测试阶段自己的提交：只许是测试文件
    let findings = non_test_violations(&root, &non_step_commits(&commits));
    assert!(
        findings.is_empty(),
        "本场非步提交动了白名单外的非测试文件：\n{}",
        findings
            .iter()
            .map(|f| f.message())
            .collect::<Vec<_>>()
            .join("\n")
    );
    // 未跟踪产物一律不得被 git add 进本变更
    let findings = artifact_violations(&root, &commits);
    assert!(
        findings.is_empty(),
        "本场提交卷进了未跟踪产物：\n{}",
        findings
            .iter()
            .map(|f| f.message())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ---------------- F10 回归：按 change-id 收敛扫描面（git fixture 造场） ----------------
//
// 真实仓的现状恒定不了「外场步提交与本场步提交并存」这个形状（a16 在本基线上是绿的，
// 靠的是历史运气：那 30 笔外场步提交恰好都动了旧场白名单内的文件）——靠它分不开
// 「过滤生效」与「扫描扫空」两种实现，回归必须在可控的 fixture 仓上造场。

/// F10 回归 · 扫描面只取本场 change-id 的步提交；外场步提交不参与判定。
///
/// 错实现形态：过滤条件写错（漏了 change-id，或过滤成永空）→ 未过滤集合里外场文件
/// 消失 / 本场步提交集里混进外场 → 本用例必须变红。
#[test]
pub(super) fn f10_the_scan_takes_only_this_changes_step_commits() {
    let tmp = tempfile::tempdir().unwrap();
    let foreign = "change-20261029-some-other-change-id";
    let mine_subject = format!("feat: 本场步提交 — {CHANGE_ID} step 1");
    let foreign_subject = format!("feat: 外场步提交 — {foreign} step 1");
    let root = fixture_repo(
        tmp.path(),
        &[
            (
                mine_subject.as_str(),
                &["tests/change_failure_context/guards.rs"][..],
            ),
            (foreign_subject.as_str(), &["src/leak.rs"][..]),
        ],
    );
    // 本场步提交集：恰好一条，且不含外场提交的文件
    let mine = change_step_commits(&root, CHANGE_ID);
    assert_eq!(mine.len(), 1, "本场步提交集应恰好一条：{mine:?}");
    let mine_files = files_of_commit(&root, &mine[0].sha);
    assert!(
        !mine_files.iter().any(|f| f == "src/leak.rs"),
        "本场步提交集混进了外场文件：{mine_files:?}"
    );
    // 反假绿：未过滤的步提交集合里外场文件确实在（放行必须来自过滤，不是扫描扫空）
    let unfiltered = all_step_commits(&root);
    assert_eq!(
        unfiltered.len(),
        2,
        "未过滤步提交集应含内外场两条：{unfiltered:?}"
    );
    let unfiltered_files: Vec<String> = unfiltered
        .iter()
        .flat_map(|c| files_of_commit(&root, &c.sha))
        .collect();
    assert!(
        unfiltered_files.iter().any(|f| f == "src/leak.rs"),
        "未过滤集合里应含外场文件（否则过滤是假的）：{unfiltered_files:?}"
    );
    // 本场白名单逐字保留 → 本场步提交的对账结果为空
    let findings = step_whitelist_violations(&root, &mine, CHANGE_WHITELIST);
    assert!(findings.is_empty(), "本场步提交应无越界：{findings:?}");
}

/// F10 回归 · 只含外场步提交时：本场步提交集为空、对账结果为空、不 panic（空集放行）。
///
/// 错实现形态：空集仍走旧口径「一个都没找到就红」的断言 → 本用例必须变红
/// （用户裁决：空集判绿放行）。
#[test]
pub(super) fn f10_the_empty_step_set_passes() {
    let tmp = tempfile::tempdir().unwrap();
    let foreign = "change-20261029-some-other-change-id";
    let foreign_subject = format!("feat: 外场步提交 — {foreign} step 1");
    let root = fixture_repo(
        tmp.path(),
        &[(foreign_subject.as_str(), &["src/leak.rs"][..])],
    );
    let mine = change_step_commits(&root, CHANGE_ID);
    assert!(
        mine.is_empty(),
        "只含外场步提交时本场步提交集应为空：{mine:?}"
    );
    let findings = step_whitelist_violations(&root, &mine, CHANGE_WHITELIST);
    assert!(
        findings.is_empty(),
        "空集放行：对账结果应为空：{findings:?}"
    );
}

/// F10 回归 · 白名单变异（本场真实步提交动过的文件被删一项）→ 对账恰好 1 条，
/// 且文案点名该文件与所属提交 subject。
///
/// 错实现形态：对账不查白名单 / 命中了却不算数 → 本用例必须变红。
#[test]
pub(super) fn f10_the_whitelist_mutation_is_caught() {
    let tmp = tempfile::tempdir().unwrap();
    let foreign = "change-20261029-some-other-change-id";
    let subject = format!("feat: 本场步提交 — {CHANGE_ID} step 1");
    let foreign_subject = format!("feat: 外场步提交 — {foreign} step 1");
    let root = fixture_repo(
        tmp.path(),
        &[
            (
                subject.as_str(),
                &["tests/change_failure_context/guards.rs"][..],
            ),
            (foreign_subject.as_str(), &["src/leak.rs"][..]),
        ],
    );
    // 变异：从本场白名单里删掉本场真实步提交动过的那个文件（不在运行时改源文件）
    let mutated: Vec<&str> = CHANGE_WHITELIST
        .iter()
        .copied()
        .filter(|f| *f != "tests/change_failure_context/guards.rs")
        .collect();
    let findings =
        step_whitelist_violations(&root, &change_step_commits(&root, CHANGE_ID), &mutated);
    // 恰好 1 条：外场步提交不参与判定（否则这里会是 2 条）
    assert_eq!(findings.len(), 1, "变异后应恰好命中一条：{findings:?}");
    assert_eq!(findings[0].file, "tests/change_failure_context/guards.rs");
    assert!(
        findings[0].subject.contains(" step 1"),
        "命中明细要带出所属提交：{}",
        findings[0].subject
    );
    let message = findings[0].message();
    assert!(
        message.contains("tests/change_failure_context/guards.rs"),
        "文案要点名文件：{message}"
    );
    assert!(message.contains(" step 1"), "文案要点名所属提交：{message}");
}

/// F10 回归 · a17 口径：本场 change-id 但**不含** ` step ` 的提交动了非测试文件
/// → 报「白名单外的非测试改动」并在文案里带出该文件。
///
/// 错实现形态：只按 ` step ` 过滤、漏掉非步提交的体检 → 本用例必须变红。
#[test]
pub(super) fn f10_a17_reports_non_test_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let subject = format!("test: 测试阶段提交 — {CHANGE_ID}");
    let root = fixture_repo(tmp.path(), &[(subject.as_str(), &["src/leak.rs"][..])]);
    let commits = change_commits(&root, CHANGE_ID);
    assert_eq!(commits.len(), 1, "本场提交应恰好一条：{commits:?}");
    let findings = non_test_violations(&root, &non_step_commits(&commits));
    assert_eq!(findings.len(), 1, "应恰好命中一条：{findings:?}");
    assert_eq!(findings[0].file, "src/leak.rs");
    let message = findings[0].message();
    assert!(message.contains("白名单外的非测试改动"), "文案：{message}");
    assert!(message.contains("src/leak.rs"), "文案要点名文件：{message}");
}

/// F10 回归 · a17 口径：本场提交动了未跟踪产物目录 → 报「未跟踪产物被提交」。
///
/// 错实现形态：产物体检没跑 / 产物目录清单漏了 → 本用例必须变红。
#[test]
pub(super) fn f10_a17_reports_committed_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let subject = format!("test: 测试阶段提交 — {CHANGE_ID}");
    let root = fixture_repo(tmp.path(), &[(subject.as_str(), &[".pi/goals/x.md"][..])]);
    let findings = artifact_violations(&root, &change_commits(&root, CHANGE_ID));
    assert_eq!(findings.len(), 1, "应恰好命中一条：{findings:?}");
    let message = findings[0].message();
    assert!(message.contains("未跟踪产物"), "文案：{message}");
    assert!(message.contains(".pi/goals/"), "文案要点名产物目录：{message}");
}

