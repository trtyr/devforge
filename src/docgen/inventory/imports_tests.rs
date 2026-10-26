//! （2026-10-26 测试分离：自 src/docgen/inventory/imports.rs 内联平移，纯移动零改动。）
use super::*;

/// 自审 critical 回归：相对 import 解出的边必须与盘扫路径逐字一致——
/// 旧实现 `src/./helper.ts` 带 `.` 分量，与清单里的 `src/helper.ts` 永不相等，
/// TS/JS 相对依赖的边全部悬空（TS/JS 分支此前零用例，本用例兼补）。
#[test]
fn ts_js_relative_import_edges_are_normalized() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/helper.ts"), "export const x = 1;\n").unwrap();
    std::fs::write(
        tmp.path().join("src/main.ts"),
        "import { x } from './helper';\nconsole.log(x);\n",
    )
    .unwrap();
    let text = std::fs::read_to_string(tmp.path().join("src/main.ts")).unwrap();
    let deps = resolve_imports(Lang::TsJs, "src/main.ts", &text, tmp.path());
    assert_eq!(
        deps,
        vec!["src/helper.ts".to_string()],
        "单引号 ESM 边必须无 . 分量、无 '; 尾巴，与盘扫路径一致"
    );
}

/// 自审 critical 回归：模块路径以多字节字符开头时，旧实现 rest[1..] 切在
/// 字符中间直接 panic（门 0 崩溃）。
#[test]
fn ts_js_multibyte_module_path_does_not_panic() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src/工具")).unwrap();
    std::fs::write(tmp.path().join("src/工具/数量.ts"), "export const n = 1;\n").unwrap();
    std::fs::write(
        tmp.path().join("src/main.ts"),
        "import { n } from './工具/数量';\nconsole.log(n);\n",
    )
    .unwrap();
    let text = std::fs::read_to_string(tmp.path().join("src/main.ts")).unwrap();
    let deps = resolve_imports(Lang::TsJs, "src/main.ts", &text, tmp.path());
    assert_eq!(deps, vec!["src/工具/数量.ts".to_string()], "多字节路径边应可解析");
}

/// 双引号 ESM 不回归（旧实现对双引号碰巧能用，重写后必须仍然可用）。
#[test]
fn ts_js_double_quoted_import_still_resolves() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::write(tmp.path().join("src/helper.ts"), "export const x = 1;\n").unwrap();
    std::fs::write(
        tmp.path().join("src/main.ts"),
        "import { x } from \"./helper\";\n",
    )
    .unwrap();
    let text = std::fs::read_to_string(tmp.path().join("src/main.ts")).unwrap();
    let deps = resolve_imports(Lang::TsJs, "src/main.ts", &text, tmp.path());
    assert_eq!(deps, vec!["src/helper.ts".to_string()]);
}
