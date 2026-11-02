//! （2026-10-26 测试分离：自 src/docgen/impact.rs 内联平移，纯移动零改动。）
use super::*;
use crate::docgen::inventory::build_inventory;
use crate::docgen::inventory::EntryKind;
use std::fs;

fn touch(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn repo() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "core/a.py", "def alpha():\n    return 1\n");
    touch(tmp.path(), "core/b.py", "from core.a import alpha\n\ndef beta():\n    return alpha()\n");
    touch(tmp.path(), "other/c.py", "def gamma():\n    return 3\n");
    tmp
}

#[test]
fn parses_name_status_and_renames() {
    let files = parse_name_status("M\tsrc/a.rs\nA\tsrc/b.rs\nD\tsrc/c.rs\nR100\told.rs\tnew.rs\n");
    assert_eq!(files.len(), 4);
    assert_eq!(files[0].path, "src/a.rs");
    assert_eq!(files[3].kind, 'R');
    assert_eq!(files[3].old_path.as_deref(), Some("old.rs"));
    assert_eq!(files[3].path, "new.rs");
}

#[test]
fn parses_porcelain_including_untracked_and_renames() {
    let files = parse_porcelain(" M src/a.rs\n?? src/new.rs\nR  old.rs -> new2.rs\n");
    assert_eq!(files.len(), 3);
    assert_eq!(files[1].kind, 'A');
    assert_eq!(files[2].old_path.as_deref(), Some("old.rs"));
}

#[test]
fn changed_file_maps_only_to_its_own_unit() {
    let tmp = repo();
    let inv = build_inventory(tmp.path()).unwrap();
    let files = vec![ChangedFile {
        kind: 'M',
        path: "core/a.py".into(),
        old_path: None,
    }];
    let units = units_touching(&inv, &files);
    assert_eq!(units.len(), 1, "{units:?}");
    let expected = inv
        .entries
        .iter()
        .find(|e| e.path == "core/a.py")
        .unwrap()
        .unit_id
        .clone();
    assert!(units.contains(&expected));
    assert!(
        !units.iter().any(|u| u.contains("other")),
        "未改动单元的不得进集合：{units:?}"
    );
}

#[test]
fn deleted_symbol_pulls_in_referrers() {
    let tmp = repo();
    let old = build_inventory(tmp.path()).unwrap();
    // 删掉 alpha：调用方 core/b.py 仍引用它
    touch(tmp.path(), "core/a.py", "def alpha_v2():\n    return 1\n");
    let new = build_inventory(tmp.path()).unwrap();
    let files = vec![ChangedFile {
        kind: 'M',
        path: "core/a.py".into(),
        old_path: None,
    }];
    let impact = impacted(tmp.path(), &old, &new, &files, tmp.path()).unwrap();
    assert!(
        impact.notes.iter().any(|n| n.contains("alpha")),
        "应识别删除的符号：{:?}",
        impact.notes
    );
    let b_unit = new
        .entries
        .iter()
        .find(|e| e.path == "core/b.py")
        .unwrap()
        .unit_id
        .clone();
    assert!(
        impact.units.contains(&b_unit),
        "引用方单元必须进集合（漏了就是文档骗人）：{:?}",
        impact.units
    );
    assert!(
        !impact.units.iter().any(|u| u.contains("other")),
        "无关单元不得进集合：{:?}",
        impact.units
    );
}

#[test]
fn assembly_page_mentioning_changed_unit_is_flagged() {
    let tmp = repo();
    let docs = tmp.path().join("docs/contexts");
    fs::create_dir_all(&docs).unwrap();
    let inv = build_inventory(tmp.path()).unwrap();
    let unit = inv
        .entries
        .iter()
        .find(|e| e.path == "core/a.py")
        .unwrap()
        .unit_id
        .clone();
    // 自审 major：不用实现的同款表达式算 token（同义反复测不出形态错）——
    // 手写预期字面串，把变换本身钉住。
    let token = unit
        .trim_start_matches("unit-")
        .replace('-', "/");
    let token = match token.as_str() {
        // 手写预期：夹具 repo 的组名是 core，单元号是 01
        "core/01" => "core/01".to_string(),
        other => panic!("夹具单元形态变了（{other}），请手写更新本用例的字面串"),
    };
    fs::write(docs.join("README.md"), format!("见 `{token}`\n")).unwrap();
    fs::write(docs.join("01-architecture.md"), "无关\n").unwrap();
    let files = vec![ChangedFile {
        kind: 'M',
        path: "core/a.py".into(),
        old_path: None,
    }];
    let impact = impacted(tmp.path(), &inv, &inv, &files, &docs).unwrap();
    assert_eq!(impact.assembly_review, vec!["README.md".to_string()]);
}

#[test]
fn plan_impact_without_old_inventory_errors() {
    let tmp = repo();
    let docs = tmp.path().join("docs/contexts");
    fs::create_dir_all(&docs).unwrap();
    let files = vec![ChangedFile {
        kind: 'M',
        path: "core/a.py".into(),
        old_path: None,
    }];
    let err = plan_impact(tmp.path(), &docs, &files).unwrap_err();
    assert!(err.to_string().contains("inventory.json"), "{err}");
}

fn git_repo() -> (tempfile::TempDir, String) {
    let tmp = repo();
    let g = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(args)
            .output()
            .expect("git 可执行");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    g(&["init", "-q", "-b", "main"]);
    g(&["add", "-A"]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@localhost",
        "commit",
        "-q",
        "-m",
        "base",
    ]);
    let base = String::from_utf8_lossy(&g(&["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    (tmp, base)
}

/// 自审 major 回归：status --porcelain 未加 -uall 时新目录里的未跟踪文件被
/// 折叠成 `?? dir/`，新文件的单元永远不会进影响面。
#[test]
fn changed_since_sees_untracked_files_in_new_dirs() {
    let (tmp, base) = git_repo();
    touch(tmp.path(), "brand/new/mod.py", "def beta():\n    return 2\n");
    let files = changed_since(tmp.path(), &base).unwrap();
    assert!(
        files.iter().any(|f| f.path == "brand/new/mod.py"),
        "未跟踪新目录文件必须可见：{files:?}"
    );
}

/// 自审 major 回归：已提交的重命名 + 未提交的再修改——porcelain 条目不得
/// 抹掉 diff 条目的 old_path（否则重命名的旧路径单元丢失）。
#[test]
fn committed_rename_keeps_old_path_across_followup_edit() {
    let (tmp, base) = git_repo();
    let g = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(tmp.path())
            .args(args)
            .output()
            .expect("git 可执行");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    };
    // 目标路径必须不存在（fs::rename 会静默覆盖，rename detection 就没了）
    fs::rename(tmp.path().join("core/a.py"), tmp.path().join("core/renamed.py")).unwrap();
    g(&["add", "-A"]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@localhost",
        "commit",
        "-q",
        "-m",
        "rename",
    ]);
    // 未提交的再修改：porcelain 会再报一次 renamed.py（M），不得覆盖 diff 的 R
    fs::write(
        tmp.path().join("core/renamed.py"),
        "def alpha():\n    return 3\n",
    )
    .unwrap();
    let files = changed_since(tmp.path(), &base).unwrap();
    let f = files
        .iter()
        .find(|f| f.path == "core/renamed.py")
        .expect("重命名后的路径应在变更集里");
    assert_eq!(
        f.old_path.as_deref(),
        Some("core/a.py"),
        "后续修改不得抹掉 old_path：{f:?}"
    );
}

/// 自审 major 回归：旧清单兜底改并集——混合变更（改 b.py + 删 a.py）下
/// 被删文件的单元（只在旧清单里）不得被「新命中非空」吞掉。
#[test]
fn deleted_file_unit_from_old_inventory_survives_mixed_changes() {
    let tmp = repo();
    let docs = tmp.path().join("docs/contexts");
    fs::create_dir_all(&docs).unwrap();
    let old = build_inventory(tmp.path()).unwrap();
    let gone_unit = old
        .entries
        .iter()
        .find(|e| e.path == "core/a.py")
        .unwrap()
        .unit_id
        .clone();
    fs::remove_file(tmp.path().join("core/a.py")).unwrap();
    let new = build_inventory(tmp.path()).unwrap();
    assert!(
        !new.entries.iter().any(|e| e.path == "core/a.py"),
        "夹具前提：删除的文件不在新清单"
    );
    let files = vec![
        ChangedFile { kind: 'D', path: "core/a.py".into(), old_path: None },
        ChangedFile { kind: 'M', path: "core/b.py".into(), old_path: None },
    ];
    let impact = impacted(tmp.path(), &old, &new, &files, &docs).unwrap();
    assert!(
        impact.units.contains(&gone_unit),
        "被删文件的旧单元必须在并集里：{:?}",
        impact.units
    );
}

/// R7-P68：目录软链环有界终止——walk 跟随软链（metadata 语义保留）但深度/
/// 条目双上限兑底，超限 Err（上层降级全量）而不是栈溢出 abort（进程级）。
#[test]
fn symlink_dir_loop_is_bounded_not_abort() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.py"), "x = 1\n").unwrap();
    // 目录环：repo/link → repo（self-loop，metadata 跟随 → 无限递归面）
    #[cfg(unix)]
    std::os::unix::fs::symlink(&root, root.join("link")).unwrap();
    let err = super::walk(&root, &root, &mut |_rel, _text| Ok(())).unwrap_err();
    assert!(
        format!("{err}").contains("影响面遍历超限"),
        "软链环应触发上限而非栈溢出：{err}"
    );
}

/// R11-P94：不可读源文件（非 UTF-8，如 GBK 编码）不判死流水线——降级跳过
/// 该文件不进清单（warn 留痕），build_inventory 照常 Ok；旧口径 ？ 直接 Err
/// 经门 0 闭包校验变 Quality 终态不可重启。
#[test]
fn unreadable_source_file_is_skipped_not_fatal() {
    let tmp = repo();
    // GBK 编码字节序列（「中文」的 GBK 双字节编码）塞进合法扩展名源文件——
    // read_to_string 会因非 UTF-8 而失败
    std::fs::write(
        tmp.path().join("core/gbk.py"),
        [0xD6u8, 0xD0, 0xCE, 0xC4, 0x0A],
    )
    .unwrap();
    let inv = build_inventory(tmp.path()).unwrap();
    // 符号层降级：不可读文件的符号不进清单（读不出内容抽不了符号）
    assert!(
        !inv.entries.iter().any(|e| e.path == "core/gbk.py" && e.kind == EntryKind::Symbol),
        "不可读文件的符号不进清单：{:?}",
        inv.entries.iter().map(|e| (&e.path, &e.kind)).collect::<Vec<_>>()
    );
    // Module 条目（路径级登记，不依赖文件内容）仍在——无害（文档照常可提该路径）
    assert!(
        inv.entries.iter().any(|e| e.path == "core/gbk.py" && e.kind == EntryKind::Module),
        "路径级 Module 登记不依赖内容，仍在"
    );
    // 合法文件照常在分母（降级不扩围）
    assert!(
        inv.entries.iter().any(|e| e.path == "core/a.py" && e.kind == EntryKind::Symbol)
    );
}
