//! （2026-10-26 测试分离：自 src/docgen/audit.rs 内联平移，纯移动零改动。）
use super::*;
use crate::docgen::inventory::build_inventory;

fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/main.py"),
        "from core import create_note\ncreate_note()\n",
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("src/core.py"),
        "def create_note(title):\n    return title\n\ndef orphan_fn():\n    pass\n",
    )
    .unwrap();
    tmp
}

fn write_doc(repo: &Path, name: &str, body: &str) {
    let dir = repo.join("docs/contexts");
    let target = dir.join(name);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(target, body).unwrap();
}

#[test]
fn coverage_marks_missing_entries() {
    let tmp = fixture();
    write_doc(tmp.path(), "core.md", "# core\n\n`create_note` 写笔记。\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
    assert!(!rep.passed());
    assert!(rep.missing.iter().any(|m| m.canonical_name == "orphan_fn"));
    assert!(rep.render().contains("orphan_fn"));
}

/// 引擎审计修复 #1 回归：_meta/glossary.md（门 0 写入全部 Symbol 名）不得计入
/// 覆盖分母——错实现形态：词表把每个 Symbol 都 contains 一遍，单元正文零字
/// 覆盖就已 100% 假绿。
#[test]
fn meta_glossary_is_excluded_from_coverage() {
    let tmp = fixture();
    // 模拟门 0：_meta/glossary.md 含全部符号名，但没有任何单元正文
    write_doc(
        tmp.path(),
        "_meta/glossary.md",
        "# 词表\n\n`create_note` 与 `orphan_fn`。\n",
    );
    let inv = build_inventory(tmp.path()).unwrap();
    let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
    assert!(
        !rep.passed(),
        "词表不得充当覆盖证据（单元正文未写出）：{:?}",
        rep.missing
    );
    assert_eq!(
        rep.missing.len(),
        rep.required,
        "全部条目都应视为缺失"
    );
}

/// 引擎审计修复 #7 回归：档位解析默认 complete（DEVFORGE_DOC_LEVEL 未设/非法值
/// 都落 complete——wiring 读环境变量后经此归一，essential/detailed 显式给）。
#[test]
fn doc_level_parse_defaults_to_complete() {
    assert!(matches!(DocLevel::parse("nonsense"), DocLevel::Complete));
    assert!(matches!(DocLevel::parse(""), DocLevel::Complete));
    assert!(matches!(DocLevel::parse(" essential "), DocLevel::Essential));
    assert!(matches!(DocLevel::parse("DETAILED"), DocLevel::Detailed));
}

#[test]
fn full_coverage_requires_modules_and_symbols() {
    let tmp = fixture();
    write_doc(
        tmp.path(),
        "all.md",
        "---\ntype: module\n---\n\n# all\n\n`src/main.py` / `src/core.py`：`create_note` 与 `orphan_fn`。\n",
    );
    let inv = build_inventory(tmp.path()).unwrap();
    let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
    assert!(rep.passed(), "{:?}", rep.missing);
    assert!(rep.render().contains("覆盖 100%"));
}

#[test]
fn essential_level_only_requires_entry_points() {
    let tmp = fixture();
    write_doc(tmp.path(), "entry.md", "# entry\n\n`src/main.py`\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Essential).unwrap();
    assert!(rep.passed(), "{:?}", rep.missing);
}

#[test]
fn unit_scope_only_counts_own_entries() {
    let tmp = fixture();
    write_doc(tmp.path(), "core.md", "# core\n\n`create_note`\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let unit = inv
        .entries
        .iter()
        .find(|e| e.canonical_name == "create_note")
        .map(|e| e.unit_id.clone())
        .unwrap();
    let rep =
        coverage_for_unit(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete, &unit)
            .unwrap();
    assert!(rep.required > 0);
    assert!(rep.missing.iter().all(|m| m.unit_id == unit));
}

#[test]
fn uncovered_units_groups_misses() {
    let tmp = fixture();
    let inv = build_inventory(tmp.path()).unwrap();
    let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
    let units = uncovered_units(&rep);
    assert!(!units.is_empty());
    assert!(units.iter().all(|u| u.starts_with("unit-")));
}

#[test]
fn runbook_commands_extracted_from_bash_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    write_doc(
        tmp.path(),
        "10-runbook.md",
        "# Runbook\n\n```bash\npip install -r requirements.txt\npython -m app\n```\n\n```text\n不是命令\n```\n",
    );
    let cmds = runbook_commands(&tmp.path().join("docs/contexts")).unwrap();
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].contains("pip install"));
    assert!(cmds[0].contains("python -m app"));
}

#[test]
fn duplicate_mentions_flags_repeated_symbol() {
    let tmp = fixture();
    write_doc(tmp.path(), "a.md", "# a\n\n`create_note` 在这里。\n");
    write_doc(tmp.path(), "b.md", "# b\n\n`create_note` 又写了一次。\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let dups = duplicate_mentions(&tmp.path().join("docs/contexts"), &inv).unwrap();
    assert!(dups.iter().any(|d| d.contains("create_note")), "{dups:?}");
}

#[test]
fn duplicate_mentions_ignores_meta_dir() {
    let tmp = fixture();
    write_doc(tmp.path(), "a.md", "# a\n\n`create_note`\n");
    write_doc(tmp.path(), "_meta/glossary.md", "# 词表\n\n`create_note`\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let dups = duplicate_mentions(&tmp.path().join("docs/contexts"), &inv).unwrap();
    assert!(dups.is_empty(), "元数据不应参与重复判定: {dups:?}");
}

#[test]
fn duplicated_content_flags_same_paragraph() {
    let tmp = fixture();
    let para = "这一段是跨模块重复的解释：写笔记的流程先校验标题非空，再落盘并返回生成的 id，最后把整张表回写给调用方。";
    write_doc(tmp.path(), "a.md", &format!("# a\n\n{para}\n"));
    write_doc(tmp.path(), "b.md", &format!("# b\n\n{para}\n"));
    let dups = duplicated_content(&tmp.path().join("docs/contexts")).unwrap();
    assert_eq!(dups.len(), 1, "{dups:?}");
    assert!(dups[0].contains("2 篇文档"), "{dups:?}");
}

#[test]
fn duplicated_content_ignores_short_lines_and_meta() {
    let tmp = fixture();
    write_doc(tmp.path(), "a.md", "# a\n\n`create_note`\n");
    write_doc(tmp.path(), "b.md", "# b\n\n`create_note`\n");
    let para = "这一段是跨模块重复的解释：写笔记的流程先校验标题非空，再落盘并返回生成的 id，最后把整张表回写给调用方。";
    write_doc(tmp.path(), "_meta/x.md", &format!("# x\n\n{para}\n"));
    write_doc(tmp.path(), "_meta/y.md", &format!("# y\n\n{para}\n"));
    let dups = duplicated_content(&tmp.path().join("docs/contexts")).unwrap();
    assert!(dups.is_empty(), "元数据与短行不应计入: {dups:?}");
}

#[test]
fn module_mentioned_only_in_prose_is_not_covered() {
    use crate::docgen::inventory::Coverage;
    let m = InventoryEntry {
        id: "mod:src/core.py".into(),
        kind: EntryKind::Module,
        path: "src/core.py".into(),
        line: 1,
        canonical_name: "core".into(),
        aliases: Vec::new(),
        status: Coverage::Uncovered,
        unit_id: "unit-src-01".into(),
    };
    // 只在一篇普通文档（如 Runbook）里被提一句：不算覆盖
    let prose = vec![DocFile {
        text: "启动：`python -m src.core`\n".into(),
        ty: None,
        heading: Some("Runbook".into()),
    }];
    assert!(!entry_covered(&prose, &m));
    // 有一篇标题就是它（无论 type 写没写）：算覆盖
    let module_doc = vec![DocFile {
        text: "# core\n\n`src/core.py` 的职责。\n".into(),
        ty: None,
        heading: Some("core".into()),
    }];
    assert!(entry_covered(&module_doc, &m));
    // 被链接指向：也算覆盖（总览/索引本身就是入口）
    let linked = vec![DocFile {
        text: "- [core](src/core.py)\n".into(),
        ty: None,
        heading: Some("索引".into()),
    }];
    assert!(entry_covered(&linked, &m));
}

#[test]
fn level_parse_defaults_to_complete() {        assert_eq!(DocLevel::parse("whatever"), DocLevel::Complete);
    assert_eq!(DocLevel::parse("essential"), DocLevel::Essential);
    assert_eq!(DocLevel::parse("detailed"), DocLevel::Detailed);
}

/// 自审 major 回归：词边界——`log` 不得被 `login`/`catalog` 命中；
/// 中文直接嵌入（「使用 log 模块」）照常算覆盖。
#[test]
fn name_matches_require_word_boundaries() {
let tmp = fixture();
// 符号 create_note 的邻居：只出现 login_note（包含 create_note? 否——
// 用「含目标名为子串的更长标识符」直接验证边界）
write_doc(
    tmp.path(),
    "boundary.md",
    "# 边界\n\n`recreate_notex` 不是 `create_note`。中文里直接写 create_note 算命中。\n",
);
let inv = build_inventory(tmp.path()).unwrap();
let rep = coverage(&inv, &tmp.path().join("docs/contexts"), DocLevel::Complete).unwrap();
let create_note_missing = rep
    .missing
    .iter()
    .any(|m| m.canonical_name == "create_note");
assert!(
    !create_note_missing,
    "词边界内的真命中要算覆盖；边界外的 recreate_notex 不得命中：{:?}",
    rep.missing
);
}

/// 模块覆盖规则回归：正文提到路径但既无 type: module 声明也无同名标题
/// → 不得判覆盖；补上 frontmatter 声明 → 覆盖。
#[test]
fn module_coverage_requires_self_declared_or_titled() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/auth.py"), "def login():\n    pass\n").unwrap();
    let docs = tmp.path().join("docs/contexts");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("guide.md"),
        "# 指南\n\n源码在 `src/auth.py`，使用时先初始化。\n",
    )
    .unwrap();
    let inv = build_inventory(tmp.path()).unwrap();
    let rep = coverage(&inv, &docs, DocLevel::Complete).unwrap();
    assert!(
        rep.missing
            .iter()
            .any(|m| m.kind == EntryKind::Module && m.path == "src/auth.py"),
        "正文提路径但无声明无同名标题，不得假覆盖：{:?}",
        rep.missing
    );

    std::fs::write(
        docs.join("guide.md"),
        "---\ntype: module\n---\n\n# 指南\n\n源码在 `src/auth.py`，使用时先初始化。\n",
    )
    .unwrap();
    let rep = coverage(&inv, &docs, DocLevel::Complete).unwrap();
    assert!(
        !rep
            .missing
            .iter()
            .any(|m| m.kind == EntryKind::Module && m.path == "src/auth.py"),
        "type: module 声明后要算覆盖：{:?}",
        rep.missing
    );
}
