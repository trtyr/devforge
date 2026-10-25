//! （ARC-R6 自 lint.rs mod tests 平移，纯移动。）

    use super::*;
fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn clean_doc_passes() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "entry-add.md",
        "---\ntype: entry\n---\n\n# 新增笔记\n\n## 用途\n创建一条笔记。\n\n## 输入\n- title: 必填\n\n## 输出\n返回笔记对象。\n\n## 错误\n400 validation_failed。\n\n## 示例\n`curl -X POST /api/notes`\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.passed(), "{:?}", report.violations());
}

#[test]
fn missing_required_section_is_failure_with_line() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "entry-bad.md",
        "---\ntype: entry\n---\n\n# 新增笔记\n\n## 用途\n创建一条笔记。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    let v: Vec<_> = report.failures().collect();
    assert!(!v.is_empty());
    assert!(v.iter().any(|x| x.check == Check::RequiredSection && x.line == 1));
    let msg = &v[0].message;
    assert!(msg.contains("输入"), "{msg}");
}

#[test]
fn na_section_counts_as_present_only_when_heading_exists() {
    let tmp = tempfile::tempdir().unwrap();
    // 有节 + 写 N/A → 通过
    write(
        tmp.path(),
        "fn-a.md",
        "---\ntype: function\n---\n\n# foo\n\n## 摘要\n做一件事。\n\n## 行为语义\n幂等。\n\n## 参数\nN/A（无参数）\n\n## 返回\n整数。\n\n## 异常\n不抛异常。\n",
    );
    assert!(lint_docs(tmp.path()).unwrap().passed());
}

#[test]
fn dead_link_reports_line_and_target() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "index.md",
        "# 索引\n\n见 [架构](architecture.md)。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    let v: Vec<_> = report.failures().collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].line, 3);
    assert_eq!(v[0].check, Check::Link);
    assert!(v[0].message.contains("architecture.md"));
}

#[test]
fn anchor_must_exist_in_target_file() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "target.md", "# 数据流\n\n内容\n");
    write(tmp.path(), "index.md", "# 索引\n\n见 [数据流](target.md#数据流)。\n");
    assert!(lint_docs(tmp.path()).unwrap().passed());

    write(tmp.path(), "index2.md", "# 索引\n\n见 [不存在](target.md#不存在)。\n");
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report
        .failures()
        .any(|v| v.check == Check::Link && v.message.contains("不存在")));
}

#[test]
fn runbook_requires_seven_items() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "10-runbook.md",
        "# Runbook\n\n## 环境要求\nPython 3.11\n\n## 依赖安装\npip install -r req.txt\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    let v: Vec<_> = report.failures().collect();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].check, Check::Runbook);
    assert!(v[0].message.contains("配置项"), "{}", v[0].message);
}

#[test]
fn banned_markers_are_warnings_not_failures() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "note.md", "# 笔记\n\nTODO: 补充错误码\n");
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.passed(), "提示级不应判失败");
    assert!(report
        .violations()
        .iter()
        .any(|v| v.check == Check::Banned && v.line == 3));
}

#[test]
fn oversized_code_block_is_warned() {
    let tmp = tempfile::tempdir().unwrap();
    let mut body = String::from("# 文档\n\n```\n");
    for i in 0..50 {
        body.push_str(&format!("line {i}\n"));
    }
    body.push_str("```\n");
    write(tmp.path(), "big.md", &body);
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.passed());
    assert!(report
        .violations()
        .iter()
        .any(|v| v.message.contains("代码块 50 行")));
}

#[test]
fn pasted_log_block_is_warned() {
    let tmp = tempfile::tempdir().unwrap();
    let mut body = String::from("# 文档\n\n");
    for i in 0..12 {
        body.push_str(&format!("2026-10-19T12:00:{i:02} INFO started\n"));
    }
    write(tmp.path(), "log.md", &body);
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report
        .violations()
        .iter()
        .any(|v| v.check == Check::Banned && v.message.contains("疑似粘贴日志 12 行")));
}

#[test]
fn malformed_frontmatter_does_not_crash() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "weird.md", "---\ntype: ghost\ntitle: x\n");
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.passed());
}

#[test]
fn module_doc_requires_responsibility_sections() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "m.md",
        "---\ntype: module\n---\n\n# m\n\n## 职责\n负责 A。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(
        report
            .failures()
            .any(|v| v.check == Check::RequiredSection && v.message.contains("公开成员")),
        "{:?}",
        report.violations()
    );
}

#[test]
fn internal_symbol_missing_sections_is_only_warning() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "internal.md",
        "---\ntype: internal\n---\n\n# helper\n\n## 摘要\n辅助函数。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.passed());
    assert!(report
        .violations()
        .iter()
        .any(|v| v.check == Check::RequiredSection && v.severity == Severity::Warn));
}

// ===== t4：实体存在性 / 同名异写 =====

fn fixture_repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(
        tmp.path().join("src/core.py"),
        "def create_note(title):\n    return title\n\nclass NoteStore:\n    pass\n",
    )
    .unwrap();
    tmp
}

#[test]
fn real_entities_pass_with_zero_false_positives() {
    let repo = fixture_repo();
    let docs = repo.path().join("docs/contexts");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("core.md"),
        "---\ntype: internal\n---\n\n# core\n\n入口 `create_note()` 写入 `NoteStore`，源码在 `src/core.py`，参见 `src/core.py:1`。\n",
    )
    .unwrap();
    let index = EntityIndex::build(repo.path(), None).unwrap();
    let report = lint_docs_with_entities(&docs, &index).unwrap();
    assert!(report.passed(), "不得有误报: {:?}", report.violations());
}

#[test]
fn ghost_symbol_is_reported_with_line() {
    let repo = fixture_repo();
    let docs = repo.path().join("docs/contexts");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("ghost.md"),
        "---\ntype: internal\n---\n\n# ghost\n\n下面这个函数不存在：`ghost_function_xyz`。\n",
    )
    .unwrap();
    let index = EntityIndex::build(repo.path(), None).unwrap();
    let report = lint_docs_with_entities(&docs, &index).unwrap();
    let hit = report
        .failures()
        .find(|v| v.message.contains("ghost_function_xyz"))
        .expect("必须报幽灵实体");
    assert_eq!(hit.check, Check::Entity);
    assert_eq!(hit.line, 7, "行号要指向引用处（frontmatter 占 4 行）");
}

#[test]
fn ghost_path_is_reported() {
    let repo = fixture_repo();
    let docs = repo.path().join("docs/contexts");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("p.md"), "---\ntype: internal\n---\n\n# p\n\n见 `src/nowhere.py`。\n").unwrap();
    let index = EntityIndex::build(repo.path(), None).unwrap();
    let report = lint_docs_with_entities(&docs, &index).unwrap();
    assert!(report
        .failures()
        .any(|v| v.check == Check::Entity && v.message.contains("幽灵路径")));
}

#[test]
fn out_of_range_line_reference_is_reported() {
    let repo = fixture_repo();
    let docs = repo.path().join("docs/contexts");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(docs.join("l.md"), "---\ntype: internal\n---\n\n# l\n\n见 `src/core.py:999`。\n").unwrap();
    let index = EntityIndex::build(repo.path(), None).unwrap();
    let report = lint_docs_with_entities(&docs, &index).unwrap();
    assert!(report
        .failures()
        .any(|v| v.message.contains("行号超出文件范围")));
}

#[test]
fn alias_usage_is_warning_not_failure() {
    let repo = fixture_repo();
    let docs = repo.path().join("docs/contexts");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("a.md"),
        "---\ntype: internal\n---\n\n# a\n\n用 `create_note`（别名 `addNote` 也出现过）。\n",
    )
    .unwrap();
    let mut inv = crate::docgen::inventory::build_inventory(repo.path()).unwrap();
    for e in &mut inv.entries {
        if e.canonical_name == "create_note" {
            e.aliases.push("addNote".into());
        }
    }
    let index = EntityIndex::build(repo.path(), Some(&inv)).unwrap();
    let report = lint_docs_with_entities(&docs, &index).unwrap();
    assert!(report.passed(), "别名只是提示: {:?}", report.violations());
    assert!(report
        .violations()
        .iter()
        .any(|v| v.check == Check::Entity && v.message.contains("同名异写")));
}

#[test]
fn fenced_code_is_not_entity_checked() {
    let repo = fixture_repo();
    let docs = repo.path().join("docs/contexts");
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::write(
        docs.join("code.md"),
        "# code\n\n```bash\ncurl -X POST `ghost_thing`\n```\n",
    )
    .unwrap();
    let index = EntityIndex::build(repo.path(), None).unwrap();
    let report = lint_docs_with_entities(&docs, &index).unwrap();
    assert!(report.passed(), "代码块内不查实体: {:?}", report.violations());
}

// ===== t8：置信度诚实性 / 缺口登记 =====

#[test]
fn green_mark_with_path_line_passes() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "a.md",
        "---\ntype: internal\n---\n\n# a\n\n🟢 入口在 `src/core.py:3`。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.passed(), "{:?}", report.violations());
}

#[test]
fn green_mark_with_replayable_command_passes() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "a.md",
        "---\ntype: internal\n---\n\n# a\n\n🟢 可重放：`python -m pytest -q`。\n",
    );
    assert!(lint_docs(tmp.path()).unwrap().passed());
}

#[test]
fn green_mark_without_evidence_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "a.md",
        "---\ntype: internal\n---\n\n# a\n\n🟢 这里肯定是对的。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    let v: Vec<_> = report.failures().collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].check, Check::Confidence);
    assert_eq!(v[0].line, 7);
}

#[test]
fn red_gap_without_gaps_file_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "a.md",
        "---\ntype: internal\n---\n\n# a\n\n🔴 设计意图看不出来。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report
        .failures()
        .any(|v| v.check == Check::Confidence && v.message.contains("gaps.md")));
}

#[test]
fn red_gap_with_gaps_file_passes() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "a.md",
        "---\ntype: internal\n---\n\n# a\n\n🔴 设计意图看不出来。\n",
    );
    write(tmp.path(), "_meta/gaps.md", "# 缺口\n\n- 设计意图（a.md）\n");
    assert!(lint_docs(tmp.path()).unwrap().passed());
}

/// 全仓审计 #8 回归：代码围栏里的 🔴 是示例/数据，不是真缺口——
/// 错实现形态：只看本行是否以 ``` 开头，围栏内每一行都被当成缺口，
/// 没写 gaps.md 就误报 FAIL。
#[test]
fn red_gap_inside_code_fence_is_not_a_gap() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "a.md",
        "---\ntype: internal\n---\n\n# a\n\n示例输出：\n\n```text\n🔴 这是日志里的红点，不是缺口\n```\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(
        !report
            .failures()
            .any(|v| v.check == Check::Confidence && v.message.contains("gaps.md")),
        "围栏内的 🔴 不得计入缺口：{:?}",
        report.failures().collect::<Vec<_>>()
    );
    // 围栏外仍然是真缺口（不能把围栏开关做成一刀切放过）
    write(
        tmp.path(),
        "b.md",
        "---\ntype: internal\n---\n\n# b\n\n🔴 这里确实没写清。\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(
        report
            .failures()
            .any(|v| v.check == Check::Confidence && v.message.contains("gaps.md")),
        "围栏外的 🔴 仍要报缺口"
    );
}

// ===== t9：汇编 / Mermaid / 空节 =====

#[test]
fn mermaid_block_must_start_with_diagram_kind() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "arch.md",
        "# 架构\n\n```mermaid\nflowchart TD\n  A --> B\n```\n",
    );
    assert!(lint_docs(tmp.path()).unwrap().passed());

    write(
        tmp.path(),
        "bad.md",
        "# 架构\n\n```mermaid\n这不是图源码\n```\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report
        .failures()
        .any(|v| v.check == Check::Mermaid && v.line == 3));
}

#[test]
fn empty_mermaid_block_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "e.md", "# e\n\n```mermaid\n```\n");
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.failures().any(|v| v.check == Check::Mermaid));
}

#[test]
fn required_section_with_empty_body_fails() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "e.md",
        "---\ntype: function\n---\n\n# f\n\n## 摘要\n做一件事。\n\n## 行为语义\n\n## 参数\nN/A（无参数）\n\n## 返回\nint\n\n## 异常\n不抛异常\n",
    );
    let report = lint_docs(tmp.path()).unwrap();
    let v: Vec<_> = report.failures().collect();
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("为空"), "{}", v[0].message);
    assert_eq!(v[0].line, 10);
}

#[test]
fn summary_reports_counts() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "a.md", "# 无类型文档\n\nTODO: x\n");
    let report = lint_docs(tmp.path()).unwrap();
    assert!(report.summary().contains("0 个失败"));
}
