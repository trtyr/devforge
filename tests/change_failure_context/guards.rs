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
                // coding 步提交：文件必须逐个在白名单内
                assert!(
                    WHITELIST.contains(&f.as_str()),
                    "coding 步提交越界（白名单外）：{f}（提交 {subject}）"
                );
                coding_files.push(f);
            } else {
                // 测试阶段自己的提交：只许是测试文件
                assert!(
                    f.starts_with("tests/"),
                    "白名单外的非测试改动：{f}（提交 {subject}）"
                );
            }
        }
    }
    // 白名单里的 23 个文件都要真的被本场改到（不是「少改了」也算过）
    for f in WHITELIST {
        assert!(
            coding_files.iter().any(|x| x == f),
            "白名单文件 {f} 没被本场任何 coding 步提交改到"
        );
    }
    // 未跟踪产物一律不得被 git add 进本变更
    for artifact in [
        ".pi/goals/",
        "tests/e2e/results/",
        "docs/plantree/changes/",
    ] {
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

