//! （2026-10-26 测试分离：自 src/change/change_id.rs 内联平移，纯移动零改动。）
use super::*;

#[test]
fn slugify_normalizes_and_bounds() {
    assert_eq!(slugify("Add Book API"), "add-book-api");
    assert_eq!(slugify("修复：分页越界!!"), "change");
    assert_eq!(slugify("  __a__b__  "), "a-b");
    assert_eq!(slugify(&"x".repeat(100)).len(), 40);
}

#[test]
fn same_task_reuses_change_id() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, dir_a) =
        assign_change_id(tmp.path(), "p", "t1", "add book api", "20261020").unwrap();
    let (b, dir_b) =
        assign_change_id(tmp.path(), "p", "t1", "completely different wording", "20261021")
            .unwrap();
    assert_eq!(a, b, "同一任务必须幂等复用");
    assert_eq!(dir_a, dir_b);
    assert_eq!(a, "change-20261020-add-book-api");
    assert!(dir_a.is_dir());
}

#[test]
fn different_tasks_get_different_ids_and_dirs() {
    let tmp = tempfile::tempdir().unwrap();
    let (a, _) = assign_change_id(tmp.path(), "p", "t1", "same wording", "20261020").unwrap();
    let (b, _) = assign_change_id(tmp.path(), "p", "t2", "same wording", "20261020").unwrap();
    assert_eq!(a, b, "id 文本可以相同");
    assert_ne!(
        change_root(tmp.path(), "p", "t1"),
        change_root(tmp.path(), "p", "t2"),
        "不同任务的目录必须隔离"
    );
}

#[test]
fn path_shape_is_plans_project_task_change_id() {
    let tmp = tempfile::tempdir().unwrap();
    let (id, dir) = assign_change_id(tmp.path(), "proj", "task9", "x", "20261020").unwrap();
    let expected = tmp
        .path()
        .join("proj")
        .join("task9")
        .join("change")
        .join(&id);
    assert_eq!(dir, expected);
}

#[test]
fn marker_survives_deleted_dir_without_renumbering() {
    let tmp = tempfile::tempdir().unwrap();
    let (id, dir) = assign_change_id(tmp.path(), "p", "t", "x", "20261020").unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let (id2, dir2) = assign_change_id(tmp.path(), "p", "t", "y", "20261021").unwrap();
    assert_eq!(id, id2, "目录被清也不换号");
    assert!(dir2.is_dir(), "应重建目录");
}

#[test]
fn date_segment_is_zero_padded() {
    assert_eq!(date_segment(2026, 10, 5), "20261005");
}

#[test]
fn civil_date_conversion_matches_known_dates() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(18_262), (2020, 1, 1));
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
}

#[test]
fn env_override_wins_for_today() {
    // 不能并行改全局 env（其它测试会读到）——只验拼接函数本身
    assert_eq!(date_segment(2026, 10, 20), "20261020");
}

#[test]
fn validate_rejects_weird_ids() {
    assert!(validate_change_id("change-20261020-x").is_ok());
    assert!(validate_change_id("Change-20261020-x").is_err());
    assert!(validate_change_id("../evil").is_err());
}
