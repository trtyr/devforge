
// ── P018 Phase 1：PG 持久化分支（连本机 PG；不可达则 skip——不假装绿）──

use super::*;

/// pg 用例互斥锁：四个用例共享 devforge_test 库且都 drop 表，并行 = 互删竞态
/// （上轮挂 3 这轮挂 2 的 flaky 正是这个）。串行执行，锁存活期 = 用例全程。
static PG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 测试用连接串（unix socket + peer 认证；devforge_test 库，每用例清表）。
const PG_TEST_URL: &str = "postgresql:///devforge_test?host=/var/run/postgresql";

/// 尝试连测试库；连不上（PG 停了/库没建）返回 None，用例打印说明后跳过。
fn pg_test_conn() -> Option<postgres::Client> {
    match postgres::Client::connect(PG_TEST_URL, postgres::NoTls) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!(
                "skip：PG 测试库不可达（{e}）——启动 PG 并创建 devforge_test 库后重跑"
            );
            None
        }
    }
}

fn drop_store_state(conn: &mut postgres::Client) {
    conn.execute("DROP TABLE IF EXISTS store_state", &[]).unwrap();
}

#[test]
fn pg_persist_and_load_roundtrip() {
    let _pg_guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut conn) = pg_test_conn() else { return };
    drop_store_state(&mut conn);

    let out_dir = tempfile::tempdir().unwrap();
    let mut store = Store::new().with_persist_pg(PG_TEST_URL);
    store
        .create_project(crate::domain::Project::new("proj-pg", out_dir.path().join("repo"), 1700000000000))
        .unwrap();
    // create_project 尾调 persist → 单行 JSONB 已落 PG
    let loaded = Store::load_pg(PG_TEST_URL).unwrap();
    assert_eq!(loaded.projects.len(), 1, "PG 重建应含项目");
    assert_eq!(loaded.projects[0].name, "proj-pg");
    // 连接串回填（续跑 persist 开启）
    assert_eq!(loaded.persist_pg.as_deref(), Some(PG_TEST_URL));
}

#[test]
fn pg_load_on_empty_database_gives_fresh_store() {
    let _pg_guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut conn) = pg_test_conn() else { return };
    drop_store_state(&mut conn);
    let loaded = Store::load_pg(PG_TEST_URL).unwrap();
    assert!(loaded.projects.is_empty());
    assert!(loaded.tasks.is_empty());
    assert_eq!(loaded.persist_pg.as_deref(), Some(PG_TEST_URL));
}

#[test]
fn pg_persist_updates_same_row_not_append() {
    let _pg_guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut conn) = pg_test_conn() else { return };
    drop_store_state(&mut conn);
    let mut store = Store::new().with_persist_pg(PG_TEST_URL);
    let tmp1 = tempfile::tempdir().unwrap();
    let tmp2 = tempfile::tempdir().unwrap();
    store
        .create_project(crate::domain::Project::new(
            "p1",
            tmp1.path().to_path_buf(),
            1700000000000,
        ))
        .unwrap();
    store
        .create_project(crate::domain::Project::new(
            "p2",
            tmp2.path().to_path_buf(),
            1700000000000,
        ))
        .unwrap();
    let n: i64 = conn
        .query_one("SELECT count(*) FROM store_state", &[])
        .unwrap()
        .get(0);
    assert_eq!(n, 1, "单行整存：多次 persist 不追加行");
    let loaded = Store::load_pg(PG_TEST_URL).unwrap();
    assert_eq!(loaded.projects.len(), 2);
}

#[test]
fn pg_unreachable_url_is_a_hard_error_on_load() {
    let _pg_guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // PG 模式启动要求可达：不可达 = 硬错误（不静默回落文件模式）
    let err = Store::load_pg("postgresql:///no_such_db_xyz?host=/var/run/postgresql").unwrap_err();
    assert!(err.to_string().contains("连 PG 失败"), "{err}");
}
