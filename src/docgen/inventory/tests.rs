//! （ARC-R6 自 inventory.rs mod tests 平移，纯移动。）

    use super::*;
use super::scanner::MAX_UNIT_MODULES;
fn touch(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

#[test]
fn build_inventory_collects_modules_symbols_and_entries() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "src/main.py", "def main():\n    pass\n");
    touch(tmp.path(), "src/core.py", "def helper():\n    pass\n\nclass Store:\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    assert!(inv.entries.iter().any(|e| e.kind == EntryKind::Entry && e.path == "src/main.py"));
    assert!(inv
        .entries
        .iter()
        .any(|e| e.kind == EntryKind::Symbol && e.canonical_name == "Store"));
    // 私有符号（_开头）不入清单
    assert!(!inv.entries.iter().any(|e| e.canonical_name == "_secret"));
}

#[test]
fn private_symbols_are_excluded() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a.py", "def _secret():\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    assert!(inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Symbol)
        .all(|e| !e.canonical_name.starts_with('_')));
}

#[test]
fn other_languages_are_recorded_as_unsupported_gap() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "app.java", "public class App {}\n");
    let inv = build_inventory(tmp.path()).unwrap();
    assert!(inv.meta.unsupported_languages.contains(&"java".to_string()));
    assert!(inv.entries.iter().any(|e| e.path == "app.java"));
}

#[test]
fn unit_numbering_is_stable_across_runs() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "core/a.py", "def a():\n    pass\n");
    touch(tmp.path(), "web/b.py", "def b():\n    pass\n");
    let mut inv = build_inventory(tmp.path()).unwrap();
    let first: Vec<(String, String)> = inv
        .entries
        .iter()
        .map(|e| (e.id.clone(), e.unit_id.clone()))
        .collect();
    assign_units(&mut inv);
    let second: Vec<(String, String)> = inv
        .entries
        .iter()
        .map(|e| (e.id.clone(), e.unit_id.clone()))
        .collect();
    assert_eq!(first, second, "重复分配不得漂移");
    // 符号继承所属文件单元
    let sym = inv.entries.iter().find(|e| e.kind == EntryKind::Symbol).unwrap();
    let module = inv
        .entries
        .iter()
        .find(|e| e.kind == EntryKind::Module && e.path == sym.path)
        .unwrap();
    assert_eq!(sym.unit_id, module.unit_id);
}

#[test]
fn file_coverage_closure_catches_missing_module() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a.py", "def a():\n    pass\n");
    touch(tmp.path(), "b.py", "def b():\n    pass\n");
    let mut inv = build_inventory(tmp.path()).unwrap();
    // 反向验证：删掉 b.py 的 module 条目
    inv.entries.retain(|e| !(e.kind == EntryKind::Module && e.path == "b.py"));
    let findings = check_closures(tmp.path(), &inv).unwrap();
    assert!(findings
        .iter()
        .any(|f| f.closure == Closure::FileCoverage && f.path == "b.py"));
}

#[test]
fn symbol_coverage_closure_catches_missing_symbol() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a.py", "def a():\n    pass\n\ndef zzz():\n    pass\n");
    let mut inv = build_inventory(tmp.path()).unwrap();
    inv.entries
        .retain(|e| !(e.kind == EntryKind::Symbol && e.canonical_name == "zzz"));
    let findings = check_closures(tmp.path(), &inv).unwrap();
    assert!(findings
        .iter()
        .any(|f| f.closure == Closure::SymbolCoverage && f.message.contains("zzz")));
}

#[test]
fn entry_reachability_follows_python_imports() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "main.py", "from core import a\n\na()\n");
    touch(tmp.path(), "core.py", "def a():\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let findings = check_closures(tmp.path(), &inv).unwrap();
    assert!(
        findings.iter().all(|f| f.closure != Closure::EntryReachability),
        "{findings:?}"
    );
}

#[test]
fn unreachable_file_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "main.py", "print('hi')\n");
    touch(tmp.path(), "orphan.py", "def lonely():\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let findings = check_closures(tmp.path(), &inv).unwrap();
    assert!(findings
        .iter()
        .any(|f| f.closure == Closure::EntryReachability && f.path == "orphan.py"));
}

#[test]
fn missing_entry_points_reports_every_file() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "lib.py", "def a():\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let findings = check_closures(tmp.path(), &inv).unwrap();
    assert!(findings
        .iter()
        .any(|f| f.closure == Closure::EntryReachability && f.message.contains("没有入口点")));
}

#[test]
fn glossary_is_derived_from_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "core/a.py", "def alpha():\n    pass\n\nclass Beta:\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let g = export_glossary(&inv);
    assert!(g.contains("自动导出"));
    assert!(g.contains("`alpha`") && g.contains("`Beta`"));
    assert!(g.contains("core/a.py:"), "必须带证据指针: {g}");
}

#[test]
fn inventory_roundtrip_json() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "a.py", "def a():\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let path = tmp.path().join("_meta/inventory.json");
    inv.save(&path).unwrap();
    let back = Inventory::load(&path).unwrap();
    assert_eq!(back.entries.len(), inv.entries.len());
    assert_eq!(back.meta.unit_seq, inv.meta.unit_seq);
}

#[test]
fn skips_vendor_and_hidden_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "node_modules/x.js", "export const x = 1;\n");
    touch(tmp.path(), ".hidden/y.py", "def y():\n    pass\n");
    touch(tmp.path(), "src/z.py", "def z():\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let paths: Vec<&str> = inv.entries.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"src/z.py"));
    assert!(!paths.iter().any(|p| p.contains("node_modules")));
    assert!(!paths.iter().any(|p| p.starts_with(".hidden")));
}

#[test]
fn ts_export_symbols_detected() {
    let tmp = tempfile::tempdir().unwrap();
    touch(
        tmp.path(),
        "index.ts",
        "export function boot() {}\nexport class App {}\nconst internal = 1;\n",
    );
    let inv = build_inventory(tmp.path()).unwrap();
    let names: Vec<&str> = inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Symbol)
        .map(|e| e.canonical_name.as_str())
        .collect();
    assert!(names.contains(&"boot") && names.contains(&"App"));
    assert!(!names.contains(&"internal"));
}

#[test]
fn rust_pub_symbols_detected() {
    let tmp = tempfile::tempdir().unwrap();
    touch(
        tmp.path(),
        "src/lib.rs",
        "pub fn run() {}\npub struct Cfg;\nfn private() {}\n",
    );
    let inv = build_inventory(tmp.path()).unwrap();
    let names: Vec<&str> = inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Symbol)
        .map(|e| e.canonical_name.as_str())
        .collect();
    assert!(names.contains(&"run") && names.contains(&"Cfg"));
    assert!(!names.contains(&"private"));
}

#[test]
fn go_exported_symbols_detected() {
    let tmp = tempfile::tempdir().unwrap();
    touch(
        tmp.path(),
        "main.go",
        "package main\n\nfunc Run() {}\nfunc hidden() {}\nfunc (s *Srv) Serve() {}\n",
    );
    let inv = build_inventory(tmp.path()).unwrap();
    let names: Vec<&str> = inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Symbol)
        .map(|e| e.canonical_name.as_str())
        .collect();
    assert!(names.contains(&"Run") && names.contains(&"Serve"));
    assert!(!names.contains(&"hidden"));
}

// ---- T002 约定面：配置键 / 数据层 / 构建启动命令 ----

#[test]
fn config_keys_cover_four_languages() {
    let py = config_keys(
        Lang::Python,
        "import os\nkey = os.environ.get(\"PY_KEY\")\nother = os.getenv('PY_ALT')\n",
    );
    assert!(py.iter().any(|(_, k)| k == "PY_KEY"));
    assert!(py.iter().any(|(_, k)| k == "PY_ALT"));
    let ts = config_keys(Lang::TsJs, "const a = process.env.TS_KEY;\nconst b = process.env['TS_ALT'];\n");
    assert!(ts.iter().any(|(_, k)| k == "TS_KEY"));
    assert!(ts.iter().any(|(_, k)| k == "TS_ALT"));
    let rs = config_keys(Lang::Rust, "let a = std::env::var(\"RUST_KEY\");\n");
    assert!(rs.iter().any(|(_, k)| k == "RUST_KEY"));
    let go = config_keys(Lang::Go, "v := os.Getenv(\"GO_KEY\")\n");
    assert!(go.iter().any(|(_, k)| k == "GO_KEY"));
}

#[test]
fn env_example_keys_ignore_comments_and_blank_lines() {
    let keys = env_file_keys("# 注释\n\nDB_URL=postgres://x\nexport API_TOKEN=\"t\"\nBROKEN LINE\n");
    let names: Vec<&str> = keys.iter().map(|(_, k)| k.as_str()).collect();
    assert_eq!(names, vec!["DB_URL", "API_TOKEN"]);
    assert_eq!(keys[0].0, 3, "行号应是原文件行号");
}

#[test]
fn sql_ddl_yields_tables_and_views() {
    let objs = sql_objects(
        "CREATE TABLE users (id int);\ncreate table if not exists books(id int);\nCREATE VIEW v_books AS SELECT 1;\n",
    );
    let names: Vec<&str> = objs.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, vec!["users", "books", "v_books"]);
}

#[test]
fn make_targets_and_package_scripts_extracted() {
    let mk = make_targets("all: build\n\tcc -o x x.c\nrun: all\n\t./x\nVAR = 1\n");
    let names: Vec<&str> = mk.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, vec!["all", "run"]);
    let pkg = package_scripts(
        r#"{"name":"x","scripts":{"dev":"vite","build":"vite build"}}"#,
    );
    let pnames: Vec<&str> = pkg.iter().map(|(_, n)| n.as_str()).collect();
    assert!(pnames.contains(&"dev") && pnames.contains(&"build"));
}

#[test]
fn convention_entries_land_in_inventory_with_units() {
    let tmp = tempfile::tempdir().unwrap();
    touch(tmp.path(), "src/app.py", "import os\nV = os.getenv(\"APP_KEY\")\n");
    touch(tmp.path(), ".env.example", "APP_KEY=x\n");
    touch(tmp.path(), "db/schema.sql", "CREATE TABLE notes (id int);\n");
    touch(tmp.path(), "Makefile", "run:\n\tpython -m app\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let cfg: Vec<&str> = inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Config)
        .map(|e| e.canonical_name.as_str())
        .collect();
    assert!(cfg.contains(&"APP_KEY"), "{cfg:?}");
    assert!(inv
        .entries
        .iter()
        .any(|e| e.kind == EntryKind::Data && e.canonical_name == "notes"));
    assert!(inv
        .entries
        .iter()
        .any(|e| e.kind == EntryKind::Behavior && e.canonical_name == "run"));
    // 单元覆盖：约定面条目必须挂到单元，不得空着（否则永远没单元能覆盖它）
    assert!(
        inv.entries.iter().all(|e| !e.unit_id.is_empty()),
        "有条目缺 unit_id：{:?}",
        inv.entries
            .iter()
            .filter(|e| e.unit_id.is_empty())
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>()
    );
    // 同文件内的配置项应与该文件的模块同单元
    let app_unit = inv
        .entries
        .iter()
        .find(|e| e.kind == EntryKind::Module && e.path == "src/app.py")
        .map(|e| e.unit_id.clone())
        .unwrap();
    assert!(inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Config && e.path == "src/app.py")
        .all(|e| e.unit_id == app_unit));
}

// ---- T003 测试即可执行规格 ----

#[test]
fn test_specs_extracted_per_language() {
    let py = test_specs(
        Lang::Python,
        "def test_add() -> None:\n    assert 1\n\nasync def test_async():\n    pass\n\ndef helper():\n    pass\n",
    );
    let names: Vec<&str> = py.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(names, vec!["test_add", "test_async"]);

    let ts = test_specs(
        Lang::TsJs,
        "it('adds', () => {});\ntest(\"lists\", () => {});\n",
    );
    let tn: Vec<&str> = ts.iter().map(|(_, n)| n.as_str()).collect();
    assert_eq!(tn, vec!["adds", "lists"]);

    let go_src = "func TestAdd(t *testing.T) {}\nfunc helper() {}\n";
    let g = test_specs(Lang::Go, go_src);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].1, "TestAdd");

    let rs = "#[test]\nfn adds() {}\n\nfn helper() {}\n";
    let r = test_specs(Lang::Rust, rs);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].1, "adds");
}

#[test]
fn test_files_get_behavior_entries() {
    let tmp = tempfile::tempdir().unwrap();
    touch(
        tmp.path(),
        "tests/test_core.py",
        "def test_add():\n    assert True\n",
    );
    touch(tmp.path(), "src/app.py", "def run():\n    pass\n");
    let inv = build_inventory(tmp.path()).unwrap();
    let specs: Vec<&str> = inv
        .entries
        .iter()
        .filter(|e| e.id.starts_with("spec:"))
        .map(|e| e.canonical_name.as_str())
        .collect();
    assert_eq!(specs, vec!["test_add"]);
    assert!(
        inv.entries
            .iter()
            .filter(|e| e.id.starts_with("spec:"))
            .all(|e| !e.unit_id.is_empty()),
        "规格条目也要挂单元"
    );
}

#[test]
fn test_file_detection_covers_common_layouts() {
    assert!(is_test_file("tests/test_20_x.py"));
    assert!(is_test_file("src/core_test.py"));
    assert!(is_test_file("web/app.test.tsx"));
    assert!(is_test_file("pkg/server_test.go"));
    assert!(!is_test_file("src/core.py"));
    assert!(!is_test_file("src/testing_utils.py"));
}

#[test]
fn oversized_group_is_split_into_multiple_units() {
    // 大仓的 src/ 不能只当一个单元：否则滚动/检查点/逐单元审查全部退化
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..12 {
        touch(
            tmp.path(),
            &format!("src/mod_{i:02}.py"),
            "def run():\n    pass\n",
        );
    }
    let inv = build_inventory(tmp.path()).unwrap();
    let units: std::collections::BTreeSet<_> = inv
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Module)
        .map(|e| e.unit_id.clone())
        .collect();
    assert_eq!(units.len(), 2, "12 个模块应按 8 个上限切成 2 个单元: {units:?}");

    // 重复调用不漂移
    let mut again = inv.clone();
    assign_units(&mut again);
    let units2: std::collections::BTreeSet<_> = again
        .entries
        .iter()
        .filter(|e| e.kind == EntryKind::Module)
        .map(|e| e.unit_id.clone())
        .collect();
    assert_eq!(units, units2);
    // 同单元内模块仍在同一组，且每单元不超过上限
    for u in units {
        let n = inv
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Module && e.unit_id == u)
            .count();
        assert!(n <= MAX_UNIT_MODULES, "{u} 有 {n} 个模块");
    }
}
