//! （2026-10-26 测试分离：自 src/domain/contract.rs 内联平移，纯移动零改动。）
use super::*;

fn contract(paths: &[&str]) -> WorkItemContract {
    WorkItemContract {
        owned_paths: paths.iter().map(|s| s.to_string()).collect(),
        forbidden_paths: vec![],
        base_rev: None,
        isolation: Isolation::None,
    }
}

#[test]
fn exact_path_matches_only_itself() {
    let c = contract(&["src/a.rs"]);
    assert!(c.covers("src/a.rs"));
    assert!(!c.covers("src/a.rs.bak"));
    assert!(!c.covers("src/b.rs"));
}

#[test]
fn dir_prefix_matches_subtree() {
    let c = contract(&["src/config/"]);
    assert!(c.covers("src/config/render.rs"));
    assert!(!c.covers("src/configure/a.rs"));
    assert!(!c.covers("src/a.rs"));
}

#[test]
fn globstar_matches_prefix_tree() {
    let c = contract(&["docs/**"]);
    assert!(c.covers("docs/a.md"));
    assert!(c.covers("docs/x/y.md"));
    assert!(!c.covers("docsx/a.md"));
    assert!(!c.covers("src/a.rs"));
}

#[test]
fn any_entry_grants_coverage() {
    let c = contract(&["src/a.rs", "src/config/"]);
    assert!(c.covers("src/config/render.rs"));
    assert!(c.covers("src/a.rs"));
    assert!(!c.covers("src/b.rs"));
}
// ---------- 双端样例集同判（P012 topic 08：与 path-guard 插件 match.ts 共用） ----------

/// 样例集 = assets/extensions/path-guard/test-cases.json（插件 match.ts 的 node 测试
/// 用同一份）。修改 glob 语义必须让这份样例集在两端全绿。
#[test]
fn sample_cases_agree_with_plugin_match_semantics() {
    let raw = include_str!("../../assets/extensions/path-guard/test-cases.json");
    let cases: Vec<(String, String, bool)> =
        serde_json::from_str(raw).expect("样例集 JSON 合法");
    assert!(cases.len() >= 10, "样例集不应缩水：{}", cases.len());
    for (path, pattern, expected) in cases {
        // 单条目判定：path_covered_by 是私有 fn，经 covers 单条目化验证
        let single = contract(&[&pattern]);
        assert_eq!(
            single.covers(&path),
            expected,
            "样例 ({path:?}, {pattern:?}) 双端语义不一致"
        );
    }
}
