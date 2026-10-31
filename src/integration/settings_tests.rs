//! A10 settings 读写单测：缺省回落、往返保真、原子写（无 tmp 残留）、损坏回落。

use super::*;

/// 文件缺失 = 全 off 缺省（「文件不存在 = 功能不存在」，ui-9 口径）。
#[test]
fn missing_file_defaults_to_all_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("integrations").join("settings.json");
    let s = load(&path);
    assert_eq!(s, IntegrationSettings::default());
    assert!(!s.auto_integrate_enabled);
    assert!(!s.gates.clippy);
    assert!(!s.gates.doc_lint);
}

/// 写 → 读往返保真；父目录自动创建；tmp 无残留（原子替换）。
#[test]
fn save_load_roundtrip_and_no_tmp_leftover() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    let s = IntegrationSettings {
        auto_integrate_enabled: true,
        gates: IntegrationGates { clippy: true, doc_lint: false },
    };
    save(&path, &s).unwrap();
    assert_eq!(load(&path), s, "往返保真");
    // 原子性外观：目录里只有 settings.json，无 .tmp 残留
    let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(entries, vec!["settings.json".to_string()], "无 tmp 残留：{entries:?}");
}

/// 损坏文件回落全 off（不炸读取方）。
#[test]
fn corrupt_file_falls_back_to_all_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = settings_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{{{ 损坏").unwrap();
    assert_eq!(load(&path), IntegrationSettings::default());
}

/// schema 序列化形态：顶层 auto_integrate_enabled + gates 嵌套（ui-9 裁决形状）。
#[test]
fn serialized_shape_matches_ui9_schema() {
    let s = IntegrationSettings {
        auto_integrate_enabled: true,
        gates: IntegrationGates { clippy: true, doc_lint: true },
    };
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["auto_integrate_enabled"], true);
    assert_eq!(v["gates"]["clippy"], true);
    assert_eq!(v["gates"]["doc_lint"], true);
    assert!(v.get("integrations").is_none(), "无多余嵌套");
}
