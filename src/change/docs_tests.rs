//! （2026-10-26 测试分离：自 src/change/docs.rs 内联平移，纯移动零改动。）
use super::*;

fn repo_with_code() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("mod")).unwrap();
    std::fs::write(tmp.path().join("mod").join("core.py"), "def f():\n    return 1\n").unwrap();
    tmp
}

#[test]
fn none_level_touches_no_docs() {
    let tmp = repo_with_code();
    let action = apply(tmp.path(), DocLevel::None).unwrap();
    assert_eq!(action, DocAction::Skipped);
    assert!(
        !tmp.path().join("docs").exists(),
        "doc_level=none 时不得建 docs 目录"
    );
}

#[test]
fn essential_level_emits_inventory_then_lets_plan_proceed() {
    let tmp = repo_with_code();
    let action = apply(tmp.path(), DocLevel::Essential).unwrap();
    assert_eq!(action, DocAction::EssentialDone);
    let path = inventory_path(tmp.path());
    assert!(path.is_file(), "essential 必须产出 inventory.json");
    let inv: crate::docgen::inventory::Inventory =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        inv.entries
            .iter()
            .any(|e| e.kind == crate::docgen::inventory::EntryKind::Module),
        "清单应至少含一个模块条目"
    );
    assert!(essential_ready(tmp.path()));
    // 轻量档不写正文文档（正文归 doc-from-zero）
    let bodies: Vec<_> = walk(tmp.path().join("docs/contexts"))
        .into_iter()
        .filter(|p| p.ends_with(".md") && !p.to_string_lossy().contains("_meta"))
        .collect();
    assert!(bodies.is_empty(), "essential 不产正文：{bodies:?}");
}

#[test]
fn complete_level_defers_to_doc_sop_and_writes_nothing() {
    let tmp = repo_with_code();
    let action = apply(tmp.path(), DocLevel::Complete).unwrap();
    assert_eq!(
        action,
        DocAction::FullChainRequired(DocLevel::Complete),
        "完整文档链必须交给 doc-from-zero，不得由引擎假装完成"
    );
    assert!(!tmp.path().join("docs").exists());
}

// ── 2026-10-27 自审 docs 批回归 ──

/// 自审 minor（63）：detailed 必须保留在自己的归因里（旧口径与 complete 折进同一个变体，
/// 文案硬编码 complete，detailed 配置的用户被误导）。
#[test]
fn detailed_keeps_its_own_level_in_the_verdict() {
    let tmp = repo_with_code();
    let action = apply(tmp.path(), DocLevel::Detailed).unwrap();
    assert_eq!(action, DocAction::FullChainRequired(DocLevel::Detailed));
    assert_ne!(
        action,
        DocAction::FullChainRequired(DocLevel::Complete),
        "detailed 与 complete 不得相等（否则两者在归因里不可区分）"
    );
}

/// 自审 minor（64）：事实层就绪需**两份产物都在**——glossary 缺失时不得判「已就绪」，
/// 且 apply 必须补写（旧口径只看 inventory.json，半成品被当成已完成）。
#[test]
fn glossary_missing_means_not_ready_and_gets_rewritten() {
    let tmp = repo_with_code();
    assert_eq!(
        apply(tmp.path(), DocLevel::Essential).unwrap(),
        DocAction::EssentialDone
    );
    let glossary = glossary_path(tmp.path());
    assert!(glossary.is_file());
    // 抹掉 glossary（模拟上次写词表失败）
    std::fs::remove_file(&glossary).unwrap();
    assert!(
        !essential_ready(tmp.path()),
        "只剩 inventory 不得判就绪（半成品）"
    );
    assert_eq!(
        apply(tmp.path(), DocLevel::Essential).unwrap(),
        DocAction::EssentialDone
    );
    assert!(glossary.is_file(), "apply 必须补写缺失的词表");
}

/// 自审 minor（64）：已就绪时幂等复用（不重写产物）。
#[test]
fn ready_fact_layer_is_reused_not_rewritten() {
    let tmp = repo_with_code();
    apply(tmp.path(), DocLevel::Essential).unwrap();
    let inv = inventory_path(tmp.path());
    let before = std::fs::metadata(&inv).unwrap().len();
    assert!(essential_ready(tmp.path()));
    assert_eq!(
        apply(tmp.path(), DocLevel::Essential).unwrap(),
        DocAction::EssentialDone
    );
    assert_eq!(
        std::fs::metadata(&inv).unwrap().len(),
        before,
        "复用路径不得重写清单（内容长度不变）"
    );
}

fn walk(root: PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(p));
        } else {
            out.push(p);
        }
    }
    out
}
