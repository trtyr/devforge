//! （2026-10-26 测试分离：自 src/runstate/mod.rs 内联平移，纯移动零改动。）
use super::*;

fn key(sop: &str) -> StateKey {
    StateKey::new("proj", "t1", sop)
}

#[test]
fn missing_file_means_no_checkpoint() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(load(tmp.path(), &key("doc-from-zero")), Loaded::Missing);
}

#[test]
fn roundtrip_preserves_data_payload() {
    let tmp = tempfile::tempdir().unwrap();
    let k = key("doc-from-zero");
    let mut st = RunState::new(&k, serde_json::json!({"done": ["unit-src-01"]}));
    save(tmp.path(), &k, &mut st).unwrap();
    let got = load(tmp.path(), &k);
    assert_eq!(got.state().unwrap().data["done"][0], "unit-src-01");
}

#[test]
fn save_increments_nothing_but_stamps_time_and_wins_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let k = key("coding");
    // 故意给一个错的 sop，落盘时应被 key 纠正
    let mut st = RunState::new(&StateKey::new("x", "y", "z"), serde_json::json!({}));
    save(tmp.path(), &k, &mut st).unwrap();
    let got = load(tmp.path(), &k).state().unwrap().clone();
    assert_eq!((got.project.as_str(), got.task.as_str(), got.sop.as_str()), ("proj", "t1", "coding"));
    assert!(got.updated_at_ms > 0);
}

#[test]
fn truncated_json_is_unusable_not_fatal() {
    let tmp = tempfile::tempdir().unwrap();
    let k = key("doc-from-zero");
    let path = k.path(tmp.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "{\"schema_version\":1,\"sop\":\"doc-fr").unwrap();
    match load(tmp.path(), &k) {
        Loaded::Unusable(msg) => assert!(msg.detail.contains("解析失败"), "{msg:?}"),
        other => panic!("应判为不可用：{other:?}"),
    }
}

#[test]
fn identity_mismatch_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let writer = key("doc-from-zero");
    let mut st = RunState::new(&writer, serde_json::json!({}));
    save(tmp.path(), &writer, &mut st).unwrap();
    // 有人把文件拷到另一个 SOP 名下 → 必须拒绝续跑
    let stolen = StateKey::new("proj", "t1", "change");
    std::fs::create_dir_all(stolen.path(tmp.path()).parent().unwrap()).unwrap();
    std::fs::copy(writer.path(tmp.path()), stolen.path(tmp.path())).unwrap();
    match load(tmp.path(), &stolen) {
        Loaded::Unusable(msg) => assert!(msg.detail.contains("身份不符"), "{msg}"),
        other => panic!("身份不符必须拒绝：{other:?}"),
    }
}

#[test]
fn different_sops_of_same_task_do_not_clobber() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, b) = (key("doc-from-zero"), key("change"));
    assert_ne!(a.path(tmp.path()), b.path(tmp.path()));
    let mut sa = RunState::new(&a, serde_json::json!({"done": ["u1"]}));
    let mut sb = RunState::new(&b, serde_json::json!({"phase": "coding"}));
    save(tmp.path(), &a, &mut sa).unwrap();
    save(tmp.path(), &b, &mut sb).unwrap();
    assert_eq!(load(tmp.path(), &a).state().unwrap().data["done"][0], "u1");
    assert_eq!(load(tmp.path(), &b).state().unwrap().data["phase"], "coding");
}

#[test]
fn clear_removes_checkpoint_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let k = key("doc-from-zero");
    let mut st = RunState::new(&k, serde_json::json!({}));
    save(tmp.path(), &k, &mut st).unwrap();
    clear(tmp.path(), &k).unwrap();
    assert_eq!(load(tmp.path(), &k), Loaded::Missing);
    clear(tmp.path(), &k).unwrap(); // 再删一次不报错
}

#[test]
fn no_temp_file_left_behind_after_save() {
    let tmp = tempfile::tempdir().unwrap();
    let k = key("doc-from-zero");
    let mut st = RunState::new(&k, serde_json::json!({}));
    save(tmp.path(), &k, &mut st).unwrap();
    let dir = k.path(tmp.path()).parent().unwrap().to_path_buf();
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(TMP_SUFFIX))
        .collect();
    assert!(leftovers.is_empty(), "残留临时文件：{leftovers:?}");
}

#[test]
fn save_into_readonly_dir_reports_error() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let k = key("doc-from-zero");
    let dir = k.path(tmp.path()).parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    let mut perms = std::fs::metadata(&dir).unwrap().permissions();
    perms.set_mode(0o500); // r-x：不能建文件
    std::fs::set_permissions(&dir, perms.clone()).unwrap();
    let mut st = RunState::new(&k, serde_json::json!({}));
    let err = save(tmp.path(), &k, &mut st);
    // 恢复权限，避免 tempdir 清理失败
    let mut back = std::fs::metadata(&dir).unwrap().permissions();
    back.set_mode(0o700);
    std::fs::set_permissions(&dir, back).unwrap();
    if nix_is_root() {
        // 自审 minor（184）：root 无视权限位，旧口径整条断言被跳过（CI 常见 root 容器里
        // 这个错误分支等于没测）。改用**不可写目标**造同一出口：把 state.json 建成目录，
        // rename 到目录必然失败 → 必须报错且不留残渣。
        let k2 = key("doc-fix");
        let path2 = k2.path(tmp.path());
        std::fs::create_dir_all(&path2).unwrap();
        let mut st2 = RunState::new(&k2, serde_json::json!({}));
        let err2 = save(tmp.path(), &k2, &mut st2);
        assert!(
            err2.is_err(),
            "目标路径被目录占位时必须报错（root 下也要覆盖这条出口）"
        );
        let dir2 = path2.parent().unwrap();
        let leftovers: Vec<String> = std::fs::read_dir(dir2)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "失败路径不得残留临时文件：{leftovers:?}");
        return;
    }
    assert!(err.is_err(), "只读目录必须报错");
}

fn nix_is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim() == "0")
        .unwrap_or(false)
}
