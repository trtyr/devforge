//! extension_assets 对账门禁（2026-10-27 自审 major）：
//! 运行件清单是手写常量——资产目录新增运行件而忘加清单 = 静默漏发，
//! path-guard 的整条路径边界随之消失（fail-open）。目录 vs 清单硬对账。

use super::*;

#[test]
fn runtime_assets_dir_is_fully_covered_by_the_constant() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/extensions/path-guard");
    // 开发件白名单：不分发（pi 只加载 index.ts 入口）
    const DEV_ONLY: &[&str] = &["match.test.ts"];

    let mut listed: Vec<&str> = PATH_GUARD_FILES.iter().map(|(p, _)| *p).collect();
    listed.sort();
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .expect("path-guard 资产目录应存在")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".ts") && !DEV_ONLY.contains(&n.as_str()))
        .collect();
    on_disk.sort();
    assert_eq!(
        on_disk, listed,
        "资产目录与分发清单不一致（目录新增运行件必须同步进 PATH_GUARD_FILES，否则静默漏发）"
    );
}

#[test]
fn listed_files_exist_and_are_nonempty() {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/extensions/path-guard");
    for (p, body) in PATH_GUARD_FILES {
        assert!(dir.join(p).exists(), "清单里的 {p} 在资产目录不存在");
        assert!(!body.is_empty(), "{p} 内嵌正文为空");
    }
}

#[test]
fn index_ts_entry_is_always_distributed() {
    // pi 只加载 index.ts 入口——清单少了它 = 整个插件不加载
    assert!(PATH_GUARD_FILES.iter().any(|(p, _)| *p == "index.ts"));
}

/// 自审 minor（33）：样例集「双端共同驱动」必须真有两端入口——旧口径只有 Rust 侧在跑，
/// TS 端（`match.test.ts`）只是躺着，插件运行时判定与 Rust 契约的同构性零护栏。
/// 这里把 TS 端也拉进 `cargo test`：node 不可用时大声跳过（不伪装成通过）。
#[test]
fn ts_side_sample_suite_runs_against_the_same_cases() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/extensions/path-guard");
    let probe = std::process::Command::new("node").arg("--version").output();
    let Ok(probe) = probe else {
        eprintln!("跳过：本机无 node，TS 侧样例集未执行（不是通过）");
        return;
    };
    assert!(probe.status.success(), "node --version 应成功");
    let out = std::process::Command::new("node")
        .arg("--test")
        .arg(dir.join("match.test.ts"))
        .current_dir(&dir)
        .output()
        .expect("运行 node --test");
    assert!(
        out.status.success(),
        "TS 侧样例集必须与 Rust 侧同判：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
