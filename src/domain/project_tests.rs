//! （2026-10-26 测试分离：自 src/domain/project.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn project_holds_manually_designated_path() {
    let p = Project::new("devforge", PathBuf::from("/home/me/code/devforge"), 1);
    assert_eq!(p.name, "devforge");
    assert_eq!(p.local_path, PathBuf::from("/home/me/code/devforge"));
}

#[test]
fn ensure_local_dir_creates_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("from-zero-project");
    let p = Project::new("fresh", target.clone(), 1);
    p.ensure_local_dir().unwrap();
    assert!(target.is_dir());
    p.ensure_local_dir().unwrap(); // 幂等
    assert!(target.is_dir());
}

// ── 2026-10-27 自审 project 批回归 ──

/// 自审 minor（29）：空路径必须先拒——`create_dir_all("")` 返回 Ok，
/// 旧口径会「静默成功但什么也没建」。
#[test]
fn empty_local_path_is_refused_not_silently_ok() {
    let p = Project::new("empty-path", PathBuf::new(), 1);
    let err = p.ensure_local_dir().unwrap_err();
    assert!(err.to_string().contains("本地路径为空"), "{err}");
    assert!(err.is_config_error(), "应是配置错误：{err:?}");
}

/// 自审 minor（14）：对外视图不得包含主机绝对路径。
#[test]
fn public_view_hides_the_host_absolute_path() {
    let p = Project::new("devforge", PathBuf::from("/home/me/secret-layout/devforge"), 7);
    let v = p.public_view();
    let text = v.to_string();
    assert!(!text.contains("secret-layout"), "不得回显主机目录布局：{text}");
    assert_eq!(v["name"], "devforge");
    assert_eq!(v["created_at_ms"], 7);
    assert_eq!(v["has_local_path"], true);
    // 空路径的项目同样可以序列化（只是 has_local_path = false）
    let empty = Project::new("no-path", PathBuf::new(), 1).public_view();
    assert_eq!(empty["has_local_path"], false);
}
