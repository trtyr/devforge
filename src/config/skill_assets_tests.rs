//! （2026-10-26 测试分离：自 src/config/skill_assets.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn plan_tree_skill_has_frontmatter_and_body() {
    let files = builtin_skill_files("plan-tree").expect("plan-tree 应有内置资产");
    assert!(files.iter().any(|(name, _)| *name == "SKILL.md"));
    let (_, body) = files.iter().find(|(name, _)| *name == "SKILL.md").unwrap();
    assert!(body.starts_with("---"), "应有 frontmatter");
    assert!(body.contains("# plan-tree"));
    assert!(body.contains("目录约定"), "应有结构约定正文");
}

#[test]
fn doc_contract_skill_carries_frontmatter_templates_and_rules() {
    // P008 T000a：契约必须自带四样东西，少一样就是四处漂移的起点
    let files = builtin_skill_files("doc-contract").expect("doc-contract 应有内置资产");
    let (_, body) = files.iter().find(|(name, _)| *name == "SKILL.md").unwrap();
    assert!(body.starts_with("---"), "需带 frontmatter 供 skill 加载器识别");
    assert!(body.contains("name: doc-contract"));
    assert!(body.contains("必填"), "需含必填节规则");
    assert!(body.contains("N/A"), "需含 N/A 声明规则（省略不等于不适用）");
    assert!(body.contains("反模式"), "需含反模式表");
    assert!(body.contains("path:line"), "需含真实性回指要求");
    assert!(body.contains("Runbook 七件套"), "需含 Runbook 门禁条目");
}

#[test]
fn unknown_skill_returns_none() {
    assert!(builtin_skill_files("ghost").is_none());
}

#[test]
fn coding_discipline_skill_is_registered_with_pi_loadable_frontmatter() {
    // 供给纪律：内置资产必须真落内容（无 skills_source 时 profile 也要拿到真东西）
    let files = builtin_skill_files("coding-discipline").expect("coding-discipline 应有内置资产");
    assert!(files.iter().any(|(name, _)| *name == "SKILL.md"));
    let (_, body) = files.iter().find(|(name, _)| *name == "SKILL.md").unwrap();
    // pi skill 加载契约：frontmatter 缺 description → skill 根本不加载（等于没装）
    assert!(body.starts_with("---"), "需带 frontmatter 供 skill 加载器识别");
    assert!(body.contains("name: coding-discipline"));
    let desc = body
        .lines()
        .find(|l| l.trim_start().starts_with("description:"))
        .expect("frontmatter 需有 description");
    let desc_value = desc.trim().trim_start_matches("description:").trim();
    assert!(!desc_value.is_empty(), "description 不得为空（空则 pi 不加载该 skill）");
    // disable-model-invocation 为真 → 该 skill 被排除出系统提示的技能清单（agent 看不见）
    assert!(
        !body.contains("disable-model-invocation: true"),
        "不得设 disable-model-invocation: true——coding agent 没有按名路由的层"
    );
}

#[test]
fn coding_discipline_skill_carries_twelve_entries_with_counterexamples_and_evidence() {
    let files = builtin_skill_files("coding-discipline").unwrap();
    let (_, body) = files.iter().find(|(name, _)| *name == "SKILL.md").unwrap();
    // 12 条纪律条目，每条都要有「反例」与「证据形态」两节
    for anchor in [
        "行为断言",
        "先证后报",
        "减法优先",
        "最小 diff",
        "根因",
        "边界收口",
        "类型纪律",
        "读者负担",
        "上下文守护",
        "不阻塞人类",
        "可验证单元",
        "幂等",
    ] {
        assert!(body.contains(anchor), "纪律正文缺条目：{anchor}");
    }
    assert!(
        body.matches("- **反例**：").count() >= 12,
        "每条纪律都要有反例节，实得 {}",
        body.matches("- **反例**：").count()
    );
    assert!(
        body.matches("- **证据形态**：").count() >= 12,
        "每条纪律都要有证据形态节，实得 {}",
        body.matches("- **证据形态**：").count()
    );
    // 来源与许可登记（蒸馏改写，保留 MIT 归属）
    for s in ["cursor/plugins/pstack", "MIT", "Lauren Tan"] {
        assert!(body.contains(s), "缺来源/许可登记：{s}");
    }
}

#[test]
fn language_skills_vendor_coverage() {
    // P005 T003：四语言 vendor 完整性（来源登记见 P005 evidence/skills-sources.md）
    let rust = builtin_language_files("rust-best-practices").unwrap();
    assert_eq!(rust.len(), 1);
    assert!(rust[0].1.contains("Rust Strict Standard"));
    let go = builtin_language_files("go-best-practices").unwrap();
    assert!(go.iter().any(|(n, b)| *n == "SKILL.md" && b.contains("Go Strict Standard")));
    let ts = builtin_language_files("ts-js-best-practices").unwrap();
    assert_eq!(ts.len(), 2, "SKILL.md(TS) + javascript-strict.md");
    let py = builtin_language_files("python-best-practices").unwrap();
    // 自审 major：抽查改全量对账——内嵌清单是手写常量，删一项测试曾照样全绿
    //（资产静默漏发无守卫）。三处硬数字 + 名字唯一性。
    assert_eq!(rust.len(), 1, "rust 应恰好 1 个文件");
    assert_eq!(go.len(), 1, "go 应恰好 1 个文件");
    assert_eq!(ts.len(), 2, "ts 应恰好 2 个文件");
    assert_eq!(py.len(), 36, "python 应恰好 36 个文件（对账 assets 目录）");
    let mut names: Vec<_> = py.iter().map(|(n, _)| *n).collect();
    names.sort_unstable();
    let uniq = names.len();
    names.dedup();
    assert_eq!(names.len(), uniq, "文件名不得重复：{names:?}");
    assert!(names.contains(&"SKILL.md"));
    assert!(names.contains(&"references/standard.md"));
    assert!(names.iter().any(|n| n.starts_with("scripts/")), "scaffold/验收脚本随 skill 分发");
    // 原样入库：frontmatter 保留（pi 按 description 触发）
    assert!(rust[0].1.starts_with("---"));
}
