//! workbook 单测：作业本与 coverage_for_unit 的同源性、幂等性、落盘与审查注入。

use super::*;
use crate::docgen::inventory::{Coverage, EntryKind, Inventory, InventoryEntry};

fn entry(unit: &str, kind: EntryKind, name: &str, path: &str, line: usize) -> InventoryEntry {
    InventoryEntry {
        id: format!("{unit}:{name}"),
        kind,
        path: path.into(),
        line,
        canonical_name: name.into(),
        aliases: vec![],
        status: Coverage::Uncovered,
        unit_id: unit.into(),
    }
}

fn fixture() -> Inventory {
    let mut inv = Inventory::default();
    inv.meta.root = "/repo".into();
    inv.entries = vec![
        entry("unit-1", EntryKind::Module, "src_config", "src/config/mod.rs", 1),
        entry("unit-1", EntryKind::Symbol, "knobs_init", "src/config/knobs.rs", 42),
        // unit-2 的条目：绝不能漏进 unit-1 的作业本
        entry("unit-2", EntryKind::Module, "src_server", "src/server/mod.rs", 1),
        // 未分组条目（unit_id 空）：不属于任何单元作业本
        entry("", EntryKind::Data, "orphan_row", "src/orphan.rs", 7),
    ];
    inv
}

#[test]
fn workbook_lists_only_its_own_units_entries_and_matches_requires() {
    let inv = fixture();
    let text = render_unit_workbook(&inv, DocLevel::Complete, "unit-1").expect("有条目");

    // 只含本单元条目
    assert!(text.contains("knobs_init"), "本单元条目要在: {text}");
    assert!(text.contains("src_config"), "模块条目要在: {text}");
    assert!(!text.contains("src_server"), "别的单元条目不能漏进来: {text}");
    assert!(!text.contains("orphan_row"), "未分组条目不属于任何单元: {text}");

    // 档位口径同源对拍：required 计数与每条「要求覆盖」判定 = level.requires
    let required = inv
        .entries
        .iter()
        .filter(|e| e.unit_id == "unit-1" && DocLevel::Complete.requires(e.kind))
        .count();
    assert!(
        text.contains(&format!("要求覆盖 **{required}** 条")),
        "覆盖分母与 requires 过滤同源: {text}"
    );
    for e in inv.entries.iter().filter(|e| e.unit_id == "unit-1") {
        let want = if DocLevel::Complete.requires(e.kind) {
            "要求覆盖"
        } else {
            "不要求（可写进相关篇章）"
        };
        assert!(text.contains(&format!("| {} |", want)), "判定列与 requires 同源: {text}");
    }
    assert!(text.contains("档位 `complete`"), "档位标注: {text}");
}

#[test]
fn essential_level_marks_non_entries_as_not_required() {
    let inv = fixture();
    let text = render_unit_workbook(&inv, DocLevel::Essential, "unit-1").expect("有条目");
    // Essential 只要求 Entry；Module/Symbol 条目要在场但标注不要求
    assert!(text.contains("档位 `essential`"));
    assert!(text.contains("knobs_init"));
    assert!(text.contains("不要求（可写进相关篇章）"));
}

#[test]
fn empty_unit_returns_none() {
    let inv = fixture();
    assert!(render_unit_workbook(&inv, DocLevel::Complete, "unit-404").is_none());
}

#[test]
fn write_is_idempotent_and_section_reads_the_same_file() {
    let inv = fixture();
    let dir = tempfile::tempdir().unwrap();
    let docs = dir.path().join("docs").join("contexts");

    let p1 = write_workbook(&docs, &inv, DocLevel::Complete, "unit-1")
        .unwrap()
        .expect("有条目应落盘");
    let t1 = std::fs::read_to_string(&p1).unwrap();
    // 幂等：重写内容逐字节一致
    let p2 = write_workbook(&docs, &inv, DocLevel::Complete, "unit-1")
        .unwrap()
        .expect("有条目应落盘");
    assert_eq!(p1, p2, "路径确定性");
    assert_eq!(t1, std::fs::read_to_string(&p2).unwrap(), "幂等重生成");

    // 审查侧读同一个文件（同源：读到的就是写手读到的）
    let section = workbook_section(&docs, "unit-1").expect("应读到");
    assert!(section.contains(t1.trim_start_matches("# 单元作业本 · unit-1\n").get(..40).unwrap_or("")));
    assert!(section.contains("评分标准，与写手同源"), "注入段要点名同源: {section}");

    // 读不到 = None（审查注入降级，不炸）
    assert!(workbook_section(&docs, "unit-404").is_none());
}

#[test]
fn stamp_changes_when_inventory_changes() {
    let mut inv = fixture();
    let a = inventory_stamp(&inv);
    inv.entries.push(entry("unit-9", EntryKind::Config, "c1", "src/c1.rs", 1));
    let b = inventory_stamp(&inv);
    assert_ne!(a, b, "清单变了版戳必须变");
    assert_eq!(a, inventory_stamp(&fixture()), "同清单版戳稳定");
}
