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
