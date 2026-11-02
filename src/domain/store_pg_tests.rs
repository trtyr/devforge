
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

/// R7-P68 后续（PG 实体化）：清全部 PG 表——blob 与实体/KV 表都清，
/// 只清 blob 会残留上一用例的实体行（空库用例假数据、行数断言漂移）。
fn drop_store_state(conn: &mut postgres::Client) {
    for table in [
        "store_state",
        "pg_projects",
        "pg_tasks",
        "pg_runs",
        "pg_issues",
        "pg_notifications",
        "pg_auth_events",
        "pg_channel_breakers",
        "pg_kv",
    ] {
        conn.execute(&format!("DROP TABLE IF EXISTS {table}"), &[]).unwrap();
    }
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
    // 实体表化：每项目一行（p1/p2 两行）——多次 persist 同 id upsert 不追加
    let n: i64 = conn
        .query_one("SELECT count(*) FROM pg_projects", &[])
        .unwrap()
        .get(0);
    assert_eq!(n, 2, "实体表：每实体一行，多次 persist 不追加行");
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

#[test]
fn migrate_json_to_pg_roundtrip_verifies() {
    let _g = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut conn) = pg_test_conn() else { return };
    drop_store_state(&mut conn);

    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("store.json");
    let mut s = Store::new();
    s.create_project(crate::domain::Project::new(
        "mig",
        dir.path().to_path_buf(),
        1700000000000,
    ))
    .unwrap();
    s.save(&src).unwrap();

    let rep = Store::migrate_json_to_pg(&src, PG_TEST_URL).unwrap();
    assert!(rep.verified, "roundtrip 比对应通过：{rep:?}");
    assert_eq!(rep.projects, 1);
    let back = Store::load_pg(PG_TEST_URL).unwrap();
    assert_eq!(back.projects.len(), 1);
    assert_eq!(back.projects[0].name, "mig");
}

/// R2-05/R3-09：并发 persist 走长连接——Arc<Store> 双线程各写多次，全部成功
/// 且数据最终一致（load_pg 回读，无连接竞态炸）。
#[test]
fn concurrent_persist_shares_one_connection() {
    let _pg_guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut conn) = pg_test_conn() else { return };
    drop_store_state(&mut conn);

    let store = std::sync::Arc::new(std::sync::Mutex::new(
        Store::new().with_persist_pg(PG_TEST_URL),
    ));
    let handles: Vec<_> = (0..2)
        .map(|worker| {
            let store = store.clone();
            std::thread::spawn(move || {
                for i in 0..3 {
                    let mut st = store.lock().unwrap();
                    st.create_project(crate::domain::project::Project::new(
                        format!("proj-{worker}-{i}"),
                        std::path::PathBuf::from(format!("/tmp/p{worker}{i}")),
                        1,
                    ))
                    .unwrap(); // create_project 尾调 persist → save_pg 长连接
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    // 连接复用且数据落齐：6 个项目全部可从 PG 读回
    let loaded = Store::load_pg(PG_TEST_URL).unwrap();
    for worker in 0..2 {
        for i in 0..3 {
            assert!(
                loaded
                    .projects
                    .iter()
                    .any(|p| p.name == format!("proj-{worker}-{i}")),
                "并发写入的项目应全部落 PG"
            );
        }
    }
}

/// P018 Phase 2（t9）：旧 blob 一次性迁移——store_state 有行且实体表空时，
/// load_pg 把 blob 数据全量写入实体表，然后 blob 改名 store_state_legacy
/// 保留现场（不 DROP：数据不出库），blob 从此退出读序。
#[test]
fn blob_legacy_is_migrated_to_entity_tables_on_load() {
    let _pg_guard = PG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(mut conn) = pg_test_conn() else { return };
    drop_store_state(&mut conn);
    // 手工种旧格式 blob（store_state 一行 + 实体表不存在/为空）
    conn.execute(
        "CREATE TABLE IF NOT EXISTS store_state (\
         id INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),\
         data JSONB NOT NULL,\
         updated_at TIMESTAMPTZ NOT NULL DEFAULT now())",
        &[],
    )
    .unwrap();
    let mut legacy = Store::new();
    legacy
        .create_project(crate::domain::project::Project::new(
            "legacy-proj",
            std::path::PathBuf::from("/tmp/legacy"),
            1,
        ))
        .unwrap();
    let data = serde_json::to_string(&legacy).unwrap();
    conn.execute(
        "INSERT INTO store_state (id, data) VALUES (1, $1::text::jsonb)",
        &[&data],
    )
    .unwrap();

    // load_pg 触发一次性迁移
    let loaded = Store::load_pg(PG_TEST_URL).unwrap();
    assert_eq!(loaded.projects.len(), 1, "blob 数据应迁入实体表并读回");
    assert_eq!(loaded.projects[0].name, "legacy-proj");

    // blob 改名收尾：store_state 不存在、store_state_legacy 保留现场
    let blob_gone: i64 = conn
        .query_one(
            "SELECT count(*) FROM information_schema.tables WHERE table_name = 'store_state'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(blob_gone, 0, "blob 应已改名退出读序");
    let legacy_kept: i64 = conn
        .query_one(
            "SELECT count(*) FROM information_schema.tables WHERE table_name = 'store_state_legacy'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(legacy_kept, 1, "legacy 表应保留现场（数据不出库）");

    // 迁移后续写：实体表路径正常（persist 再次落 PG、load 再读回一致）
    let mut store = loaded;
    store
        .create_project(crate::domain::project::Project::new(
            "post-migration",
            std::path::PathBuf::from("/tmp/post"),
            1,
        ))
        .unwrap();
    let reloaded = Store::load_pg(PG_TEST_URL).unwrap();
    assert_eq!(reloaded.projects.len(), 2, "迁移后续写应落实体表");
}
