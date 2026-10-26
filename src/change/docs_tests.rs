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
        DocAction::FullChainRequired,
        "完整文档链必须交给 doc-from-zero，不得由引擎假装完成"
    );
    assert!(!tmp.path().join("docs").exists());
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
