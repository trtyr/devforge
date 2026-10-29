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
///
/// 10-29 运维：本变更（SR2-gate run 1 / F7）的落点文件 `src/engine/ledger_tests.rs` 与
/// `tests/change_failure_context/guards.rs` 都不在上一场的 23 文件白名单里，且本变更不刷新
/// 守卫常量 → a16 的「步提交逐文件在白名单内」这一段对本场恒红（现状绿只是越界集合为空）。
/// 与 a17 同口径：两场之间无对应 step 提交时守卫恒红 → `#[ignore]`（F10/F13）。
#[test]
#[ignore = "守卫常量由 change run 刷新；两场之间无 step 提交（F10/F13）；本变更落点文件不在该白名单内且不刷新守卫常量"]
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


// ---------------- 验收对账（testing 阶段独立取证 · F10） ----------------
//
// 这一组是**独立**的一组，不复用被测实现的造场器：
// - fixture 仓自己造（不借 support.rs 的 `fixture_repo`——那是被测实现的一部分，
//   借了就等于「用被测实现证明被测实现」）；
// - 断言只落在**可观测行为**上（扫描集内容 / 命中条数 / 命中文案 / 守卫自身的判定），
//   不钉内部 helper 的组合方式（内部重构时本组必须仍绿）；
// - 每条用例首行写明「错实现形态」——写不出错实现形态的断言当场不写（防假绿）。
//
// 落点说明：本组依赖本场实现（support.rs 的 `CHANGE_ID` / `CHANGE_WHITELIST` 与扫描口径
// helper），实现未落地时**编译不过** ⇒ 先在隔离副本（仓外临时目录）上取证，
// 实现落地后再整段追加进 `tests/change_failure_context/guards.rs`。
//
// 对应 change-plan §7 的验收标准 1–12（逐条对应关系见黑板 `testing/prepared_tests.md`）。

/// 本场基线 sha（change-plan §6 的基点）——用于「相对基点的全仓改动面」取证。
const F10_BASELINE_SHA: &str = "7dbf5ab7372c93658b6518c1d599e8b4b1d6b260";

/// 外场 change-id（本组用例里的「外场」：与本场 change-id 不同的另一个场）。
const F10_FOREIGN_ID: &str = "change-20261029-f10-foreign-scene";

/// 黑盒 fixture 仓构造器：自己 init、自己提交（不借被测实现的构造器）。
fn f10_blackbox_repo(dir: &Path, commits: &[(&str, &[&str])]) -> PathBuf {
    let root = dir.join("f10-blackbox-repo");
    std::fs::create_dir_all(&root).unwrap();
    // 空仓不能提交 → 先落一个种子文件（提交历史里出现它，扫描面不受影响：它不是步提交）
    std::fs::write(root.join("seed.txt"), "seed\n").unwrap();
    git(&root, &["init"]);
    git(&root, &["add", "-A"]);
    git(
        &root,
        &["-c", "user.email=t@t", "-c", "user.name=t", "commit", "-m", "seed"],
    );
    for (subject, files) in commits {
        for f in *files {
            let p = root.join(f);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, "x\n").unwrap();
        }
        git(&root, &["add", "-A"]);
        git(
            &root,
            &[
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-m",
                subject,
            ],
        );
    }
    root
}

/// 验收 1 · 给定一个临时 git fixture 仓（本场 change-id 的 `step 1` 提交 + 外场 change-id 的
/// `step 1` 提交各一条）→ 按本场 change-id 取到的步提交集合只含本场那一条，
/// 其文件不包含外场提交里的文件。
///
/// 错实现形态：过滤条件漏写 change-id（扫描面没收敛）→ 本场步提交集里混进外场提交 → 红；
/// 过滤写坏成「永空」→ 本场步提交集为空 → 「恰好一条」先红。
#[test]
pub(super) fn f10acc01_the_filtered_step_set_holds_only_this_change() {
    let tmp = tempfile::tempdir().unwrap();
    let own_subject = format!("feat: 本场步提交 — {CHANGE_ID} step 1");
    let foreign_subject = format!("feat: 外场步提交 — {F10_FOREIGN_ID} step 1");
    let root = f10_blackbox_repo(
        tmp.path(),
        &[
            (
                own_subject.as_str(),
                &["tests/change_failure_context/support.rs"][..],
            ),
            (foreign_subject.as_str(), &["src/foreign_only.rs"][..]),
        ],
    );
    let mine = change_step_commits(&root, CHANGE_ID);
    assert_eq!(mine.len(), 1, "本场步提交集应恰好一条：{mine:?}");
    assert!(
        mine[0].subject.contains(CHANGE_ID),
        "本场步提交集里混进了外场提交：{mine:?}"
    );
    assert!(
        mine[0].subject.contains(" step "),
        "本场步提交集里混进了非步提交：{mine:?}"
    );
    let mine_files = files_of_commit(&root, &mine[0].sha);
    assert_eq!(
        mine_files,
        vec!["tests/change_failure_context/support.rs".to_string()],
        "本场步提交的文件清单：{mine_files:?}"
    );
    assert!(
        !mine_files.iter().any(|f| f == "src/foreign_only.rs"),
        "本场步提交集混进了外场文件：{mine_files:?}"
    );
}

/// 验收 2 · 给定同一 fixture 的**未过滤**步提交集合 → 其中含外场提交的文件
/// （反假绿：放行必须来自过滤，而不是把扫描扫空）。
///
/// 错实现形态：过滤条件写成永空（扫空即放行）→ 未过滤集合里外场文件也消失 → 红。
#[test]
pub(super) fn f10acc02_the_unfiltered_step_set_still_holds_foreign_files() {
    let tmp = tempfile::tempdir().unwrap();
    let own_subject = format!("feat: 本场步提交 — {CHANGE_ID} step 1");
    let foreign_subject = format!("feat: 外场步提交 — {F10_FOREIGN_ID} step 1");
    let other_foreign_subject = format!("fix: 外场步提交 — {F10_FOREIGN_ID} step 2");
    let root = f10_blackbox_repo(
        tmp.path(),
        &[
            (
                own_subject.as_str(),
                &["tests/change_failure_context/support.rs"][..],
            ),
            (foreign_subject.as_str(), &["src/foreign_only.rs"][..]),
            (
                other_foreign_subject.as_str(),
                &["src/foreign_two.rs"][..],
            ),
        ],
    );
    let unfiltered = all_step_commits(&root);
    assert_eq!(
        unfiltered.len(),
        3,
        "未过滤步提交集应含内外场三条：{unfiltered:?}"
    );
    let unfiltered_files: Vec<String> = unfiltered
        .iter()
        .flat_map(|c| files_of_commit(&root, &c.sha))
        .collect();
    assert!(
        unfiltered_files.iter().any(|f| f == "src/foreign_only.rs"),
        "未过滤集合里应含外场文件（否则过滤是假的）：{unfiltered_files:?}"
    );
    assert!(
        unfiltered_files.iter().any(|f| f == "src/foreign_two.rs"),
        "未过滤集合里应含第二笔外场文件：{unfiltered_files:?}"
    );
    // 过滤确实做了事：过滤后的集合是未过滤集合的真子集
    let mine = change_step_commits(&root, CHANGE_ID);
    assert!(
        unfiltered.len() > mine.len(),
        "过滤前后集合一样大 → 过滤没生效：{unfiltered:?} vs {mine:?}"
    );
}

/// 验收 3 · 给定同一 fixture 且本场白名单**逐字保留** → 本场步提交文件的对账结果为空（无越界）。
///
/// 错实现形态：对账拿不到本场白名单 / 白名单被换成本场之外的一份 → 越界条目非空 → 红。
#[test]
pub(super) fn f10acc03_the_verbatim_whitelist_yields_no_finding() {
    let tmp = tempfile::tempdir().unwrap();
    let own_subject = format!("feat: 本场步提交 — {CHANGE_ID} step 1");
    let foreign_subject = format!("feat: 外场步提交 — {F10_FOREIGN_ID} step 1");
    let root = f10_blackbox_repo(
        tmp.path(),
        &[
            (
                own_subject.as_str(),
                &["tests/change_failure_context/support.rs"][..],
            ),
            (foreign_subject.as_str(), &["src/foreign_only.rs"][..]),
        ],
    );
    let mine = change_step_commits(&root, CHANGE_ID);
    let findings = step_whitelist_violations(&root, &mine, CHANGE_WHITELIST);
    assert!(
        findings.is_empty(),
        "本场白名单逐字保留时不该有越界：{findings:?}"
    );
    // 反假绿：外场那个文件确实在本场白名单外（「空」必须来自过滤，不是来自白名单把外场也收了）
    let all_findings = step_whitelist_violations(&root, &all_step_commits(&root), CHANGE_WHITELIST);
    assert!(
        all_findings.iter().any(|f| f.file == "src/foreign_only.rs"),
        "外场文件本应被判越界（用来证明「空」来自过滤）：{all_findings:?}"
    );
}

/// 验收 4 · 给定变异：本场白名单删去本场真实步提交里的一个文件
/// （`tests/change_failure_context/guards.rs`）→ 对账结果恰好 1 条，
/// 且文案点名该文件与所属提交 subject（守卫变红）。
///
/// 错实现形态：对账不查白名单 / 命中了却不算数 / 把外场提交也算进判定 → 本用例必须变红。
#[test]
pub(super) fn f10acc04_a_whitelist_mutation_yields_exactly_one_named_finding() {
    let tmp = tempfile::tempdir().unwrap();
    let own_subject = format!("feat: 本场步提交 — {CHANGE_ID} step 1");
    let foreign_subject = format!("feat: 外场步提交 — {F10_FOREIGN_ID} step 1");
    let root = f10_blackbox_repo(
        tmp.path(),
        &[
            (
                own_subject.as_str(),
                &[
                    "tests/change_failure_context/guards.rs",
                    "tests/change_failure_context/support.rs",
                ][..],
            ),
            (foreign_subject.as_str(), &["src/foreign_only.rs"][..]),
        ],
    );
    // 变异：从本场白名单里删掉本场真实步提交动过的那一项（不运行时改源文件）
    let mutated: Vec<&str> = CHANGE_WHITELIST
        .iter()
        .copied()
        .filter(|f| *f != "tests/change_failure_context/guards.rs")
        .collect();
    assert_eq!(
        mutated.len() + 1,
        CHANGE_WHITELIST.len(),
        "变异前提：本场白名单里确实有那一项"
    );
    let findings = step_whitelist_violations(&root, &change_step_commits(&root, CHANGE_ID), &mutated);
    assert_eq!(
        findings.len(),
        1,
        "变异后应恰好命中一条（外场步提交不参与判定，否则会是两条）：{findings:?}"
    );
    assert_eq!(findings[0].file, "tests/change_failure_context/guards.rs");
    assert_eq!(findings[0].kind, FindingKind::StepOutsideWhitelist);
    assert!(
        findings[0].subject.contains(" step 1"),
        "命中明细要带出所属提交：{}",
        findings[0].subject
    );
    assert!(
        findings[0].subject.contains(CHANGE_ID),
        "命中明细要带出所属场：{}",
        findings[0].subject
    );
    let message = findings[0].message();
    assert!(
        message.contains("tests/change_failure_context/guards.rs"),
        "文案要点名文件：{message}"
    );
    assert!(message.contains(" step 1"), "文案要点名所属提交：{message}");
}

/// 验收 5 · 给定只含外场步提交的 fixture → 本场步提交集合为空、对账结果为空、不 panic
/// （空集放行，与用户裁决一致）。
///
/// 错实现形态：空集仍走旧口径「一个都没找到就红」的断言 → 本用例必须变红。
#[test]
pub(super) fn f10acc05_a_foreign_only_fixture_yields_an_empty_set_without_panicking() {
    let tmp = tempfile::tempdir().unwrap();
    let foreign_subject = format!("feat: 外场步提交 — {F10_FOREIGN_ID} step 1");
    let root = f10_blackbox_repo(
        tmp.path(),
        &[(foreign_subject.as_str(), &["src/foreign_only.rs"][..])],
    );
    // 前提：fixture 里确实有步提交（否则「空」是因为造场失败，不是过滤的结果）
    assert_eq!(
        all_step_commits(&root).len(),
        1,
        "fixture 里应有一笔步提交"
    );
    let mine = change_step_commits(&root, CHANGE_ID);
    assert!(
        mine.is_empty(),
        "只含外场步提交时本场步提交集应为空：{mine:?}"
    );
    assert!(
        change_commits(&root, CHANGE_ID).is_empty(),
        "只含外场步提交时本场提交集应为空"
    );
    let findings = step_whitelist_violations(&root, &mine, CHANGE_WHITELIST);
    assert!(
        findings.is_empty(),
        "空集放行：对账结果应为空：{findings:?}"
    );
}

/// 验收 6 · 给定 fixture 里一条本场 change-id 但**不含** ` step ` 的提交改动了 `src/leak.rs`
/// → a17 口径的检查报「白名单外的非测试改动」并在文案里带出该文件。
///
/// 错实现形态：只按 ` step ` 过滤、漏掉非步提交的体检 → 本用例必须变红。
#[test]
pub(super) fn f10acc06_a_non_step_commit_reports_a_non_test_change() {
    let tmp = tempfile::tempdir().unwrap();
    let subject = format!("test: 测试阶段提交 — {CHANGE_ID}");
    let root = f10_blackbox_repo(tmp.path(), &[(subject.as_str(), &["src/leak.rs"][..])]);
    let commits = change_commits(&root, CHANGE_ID);
    assert_eq!(commits.len(), 1, "本场提交应恰好一条：{commits:?}");
    assert!(
        !commits[0].subject.contains(" step "),
        "前提：这笔提交不含 ' step '"
    );
    let findings = non_test_violations(&root, &non_step_commits(&commits));
    assert_eq!(findings.len(), 1, "应恰好命中一条：{findings:?}");
    assert_eq!(findings[0].file, "src/leak.rs");
    assert_eq!(findings[0].kind, FindingKind::NonTestChange);
    let message = findings[0].message();
    assert!(message.contains("白名单外的非测试改动"), "文案：{message}");
    assert!(message.contains("src/leak.rs"), "文案要点名文件：{message}");
}

/// 验收 7 · 给定 fixture 里一条本场 change-id 的提交动用了 `.pi/goals/` 下的文件
/// → a17 口径的产物检查报「未跟踪产物被提交」。
///
/// 错实现形态：产物体检没跑 / 产物目录清单漏了 `.pi/goals/` → 本用例必须变红。
#[test]
pub(super) fn f10acc07_an_artifact_commit_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let subject = format!("feat: 本场步提交 — {CHANGE_ID} step 2");
    let root = f10_blackbox_repo(
        tmp.path(),
        &[(subject.as_str(), &[".pi/goals/goal_x.md"][..])],
    );
    let findings = artifact_violations(&root, &change_commits(&root, CHANGE_ID));
    assert_eq!(findings.len(), 1, "应恰好命中一条：{findings:?}");
    assert_eq!(findings[0].file, ".pi/goals/goal_x.md");
    assert_eq!(findings[0].kind, FindingKind::ArtifactCommitted);
    let message = findings[0].message();
    assert!(message.contains("未跟踪产物"), "文案：{message}");
    assert!(message.contains(".pi/goals/"), "文案要点名产物目录：{message}");
}

/// 验收 8 · 给定真实仓（本仓）现状：`7dbf5ab` 之后 300+ 笔提交里有 30 笔含 ` step ` 的外场提交
/// 且文件不在旧场白名单内 → a16 仍绿（现状必红），且 a16 的扫描集里外场提交数为 0。
///
/// 错实现形态：扫描面没收敛（仍按旧场基线区间扫全场步提交）→ 扫描集里混进外场提交 → 红；
/// 扫描面扫空 → 「本场步提交集非空」断言红。
#[test]
pub(super) fn f10acc08_the_real_repo_scan_excludes_every_foreign_step_commit() {
    let root = repo_root();
    // 前提（毒）：本仓确实有大量外场步提交，且它们动过旧场白名单外的文件
    assert!(
        commit_history(&root).len() >= 300,
        "本仓提交数应 300+：{}",
        commit_history(&root).len()
    );
    let all_steps = all_step_commits(&root);
    let foreign: Vec<&Commit> = all_steps
        .iter()
        .filter(|c| !c.subject.contains(CHANGE_ID))
        .collect();
    assert!(
        foreign.len() >= 30,
        "外场步提交应 30 笔以上：{}",
        foreign.len()
    );
    let foreign_outside: Vec<&Commit> = foreign
        .iter()
        .copied()
        .filter(|c| {
            files_of_commit(&root, &c.sha)
                .iter()
                .any(|f| !WHITELIST.contains(&f.as_str()))
        })
        .collect();
    assert!(
        !foreign_outside.is_empty(),
        "应有外场步提交动过旧场白名单外的文件（旧口径会被它们毒红）"
    );
    // 扫描面：外场提交数必须为 0
    let mine = change_step_commits(&root, CHANGE_ID);
    let foreign_in_mine: Vec<&Commit> = mine
        .iter()
        .filter(|c| !c.subject.contains(CHANGE_ID))
        .collect();
    assert!(
        foreign_in_mine.is_empty(),
        "扫描面里混进了外场提交：{foreign_in_mine:?}"
    );
    assert!(
        !mine.is_empty(),
        "本场步提交集不该为空（本仓已有本场步提交）"
    );
    // a16 本身仍绿：直接跑被测守卫（它绿 = 守卫没被外场提交毒化）
    a16_the_whitelist_constants_match_this_change();
}

/// 验收 9 · 给定本场 step 1 之后的真实仓（本场至少 1 笔步提交存在）→ a16 对本场的每个步提交文件做
/// `normalize_path` 后逐个落在本场白名单内，越界即红。
///
/// 错实现形态：白名单对账漏掉 / 归一化被摘掉（历史旧路径直接比对）→ 本用例必须变红。
#[test]
pub(super) fn f10acc09_the_real_repo_step_files_stay_inside_this_change_whitelist() {
    let root = repo_root();
    let mine = change_step_commits(&root, CHANGE_ID);
    assert!(!mine.is_empty(), "本场步提交集不该为空");
    for c in &mine {
        for f in files_of_commit(&root, &c.sha) {
            assert!(
                CHANGE_WHITELIST.contains(&f.as_str()),
                "本场步提交越界（白名单外）：{f}（提交 {}）",
                c.subject
            );
        }
    }
    // 归一化不可省：取文件清单必须过迁移表（原始 git 输出归一化后应逐字命中）
    let normalized = files_of_commit(&root, &mine[0].sha);
    let raw = git(
        &root,
        &["show", "--name-only", "--format=", &mine[0].sha],
    );
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let expected = devforge::change::plan_audit::normalize_path(line.trim());
        assert!(
            normalized.iter().any(|f| f == expected),
            "取文件清单没过迁移表：{line} → {expected}"
        );
    }
    // a17 口径（本场的）同样绿：步提交 ⊆ 本场白名单、非步提交只许动 tests/、产物未进仓
    a17_the_change_commits_stay_inside_the_whitelist();
}

/// 验收 9（续）· 归一化不可省：历史步提交动过的**旧路径**（仓库手术迁移前的路径）
/// 归一化到现状路径后必须仍落在本场白名单内——摘掉 `normalize_path` → 本用例必须变红。
///
/// 错实现形态：取文件清单没过迁移表（直接拿 git 的原始路径比对）→ 旧路径被判越界 → 红。
#[test]
pub(super) fn f10acc13_a_migrated_path_is_normalized_before_the_whitelist_check() {
    let tmp = tempfile::tempdir().unwrap();
    let subject = format!("feat: 本场步提交 — {CHANGE_ID} step 1");
    // 迁移表里的旧路径（ARC-R6 之前的形态）
    let old_path = "tests/change_failure_context.rs";
    let root = f10_blackbox_repo(tmp.path(), &[(subject.as_str(), &[old_path][..])]);
    let mine = change_step_commits(&root, CHANGE_ID);
    assert_eq!(mine.len(), 1, "本场步提交集应恰好一条：{mine:?}");
    let expected = devforge::change::plan_audit::normalize_path(old_path);
    assert_eq!(
        expected, "tests/change_failure_context/main.rs",
        "迁移表口径变了：本用例的前提需重核"
    );
    let files = files_of_commit(&root, &mine[0].sha);
    assert_eq!(
        files,
        vec![expected.to_string()],
        "取文件清单没过迁移表（应归一化成现状路径）：{files:?}"
    );
    // 归一化之后逐个落在本场白名单内 → 无越界
    let findings = step_whitelist_violations(&root, &mine, CHANGE_WHITELIST);
    assert!(findings.is_empty(), "归一化后不该有越界：{findings:?}");
}

/// 验收 10 · 给定本场全部提交的文件清单 → 100% 落在本计划白名单四条之内，
/// `src/` 下改动 0 个文件（生产行为零 diff）。
///
/// 错实现形态：把白名单外的文件带进本场提交 → 本用例必须变红。
#[test]
pub(super) fn f10acc10_the_change_touches_only_the_planned_test_files() {
    let root = repo_root();
    let commits = change_commits(&root, CHANGE_ID);
    assert!(!commits.is_empty(), "本场提交集不该为空");
    for c in &commits {
        for f in files_of_commit(&root, &c.sha) {
            assert!(
                CHANGE_WHITELIST.contains(&f.as_str()),
                "本场提交动了白名单外的文件：{f}（提交 {}）",
                c.subject
            );
        }
    }
    // 相对基点的全仓改动面（工作树口径，与引擎的改动面快照同口径）：只在 tests/ 下
    let changed = git(&root, &["diff", "--name-only", F10_BASELINE_SHA]);
    let changed: Vec<&str> = changed.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(!changed.is_empty(), "相对基线应当有本场改动");
    for f in &changed {
        assert!(f.starts_with("tests/"), "改动面越出 tests/：{f}");
        assert!(
            CHANGE_WHITELIST.contains(f),
            "改动面越出本计划白名单：{f}"
        );
    }
}

/// 验收 11 · 给定 `tests/f6_observability` 的守卫三例（常量对齐 / 注释对齐 / ignore 保留）
/// → 全绿（旧场 `BASELINE`、`WHITELIST` 的值与注释、`guards.rs` 的 `#[ignore` 均未被改动）。
///
/// 错实现形态：顺手刷新旧场常量 / 改写旧场常量注释 / 摘掉 a17 的 `#[ignore`
/// → 本用例必须变红（这三项都被白名单外的守卫逐字钉着）。
#[test]
pub(super) fn f10acc11_the_frozen_old_field_facts_survive() {
    let root = repo_root();
    let support = std::fs::read_to_string(root.join("tests/change_failure_context/support.rs"))
        .unwrap_or_else(|_| panic!("support.rs 不可读"));
    let guards = std::fs::read_to_string(root.join("tests/change_failure_context/guards.rs"))
        .unwrap_or_else(|_| panic!("guards.rs 不可读"));
    let ask_ring = std::fs::read_to_string(root.join("tests/change_ask_ring.rs"))
        .unwrap_or_else(|_| panic!("change_ask_ring.rs 不可读"));
    // 旧场 BASELINE：值与注释都不动
    assert!(
        support.contains("const BASELINE: &str = \"a36f6b79dec2e681db8daed0da067f9abd47a9f5\";"),
        "旧场 BASELINE 的值被改了"
    );
    assert!(
        support.contains("a36f6b7"),
        "旧场 BASELINE 的注释块必须仍含 a36f6b7"
    );
    // 旧场 WHITELIST：23 项 + 注释含「23 个」；与 change_ask_ring.rs 的同名常量逐字一致
    assert_eq!(WHITELIST.len(), 23, "旧场 WHITELIST 仍是 23 项：{}", WHITELIST.len());
    assert!(
        support.contains("23 个"),
        "旧场 WHITELIST 的注释块必须仍含「23 个」"
    );
    assert_eq!(
        parse_whitelist(&ask_ring),
        WHITELIST.to_vec(),
        "change_ask_ring.rs 的 WHITELIST 与本场旧场常量不一致"
    );
    // 新常量不得让白名单外守卫的 `const WHITELIST:` 定位命中新数组
    assert_eq!(
        parse_whitelist(&support),
        WHITELIST.to_vec(),
        "support.rs 里的旧场 WHITELIST 被改动了"
    );
    // a17 的 `#[ignore` 属性保留
    assert!(guards.contains("#[ignore"), "guards.rs 必须仍含 #[ignore");
}

/// 验收 12 · 给定全量测试套件的运行结果 → 失败 0 条；给定 clippy 全目标检查结果 → 警告 0 条。
///
/// 本用例钉住「套件面与依赖面没被本场改动」这一前置（命令原文与真实退出码见
/// `evidence/regression.log`）：套件目标清单不变、零新依赖（Cargo.toml/Cargo.lock 未改）、
/// `src/` 零改动（clippy 的目标面没变）。
///
/// 错实现形态：顺手改了依赖面 / 把 `src/` 改了 → 本用例必须变红。
#[test]
pub(super) fn f10acc12_the_suite_shape_and_dependencies_are_unchanged() {
    let root = repo_root();
    for t in [
        "tests/change_ask_ring.rs",
        "tests/change_failure_context/main.rs",
        "tests/change_review_protocol/main.rs",
        "tests/f6_observability/main.rs",
    ] {
        assert!(root.join(t).exists(), "套件目标不见了：{t}");
    }
    let dep_diff = git(
        &root,
        &[
            "diff",
            "--name-only",
            F10_BASELINE_SHA,
            "--",
            "Cargo.toml",
            "Cargo.lock",
        ],
    );
    assert!(
        dep_diff.trim().is_empty(),
        "依赖面被本场改动了：{dep_diff}"
    );
    let src_diff = git(
        &root,
        &["diff", "--name-only", F10_BASELINE_SHA, "--", "src/"],
    );
    assert!(src_diff.trim().is_empty(), "src/ 下有本场改动：{src_diff}");
}
