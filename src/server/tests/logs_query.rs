//! A8 日志查询端点单测：RFC3339 解析器边界 + 过滤纯函数 + 端点过滤/分页。

#[allow(unused_imports)]
use super::*;

use crate::config::installer::FakeInstaller;
use crate::config::ConfigManifest;
use crate::domain::SopRegistry;
use crate::observability::events::EventBus;
use crate::server::handlers::{log_line_matches, rfc3339_zulu_to_ms, LogsQuery};
use crate::server::{build_router, ServerState, UiRelay};
use std::path::PathBuf;
use std::sync::Arc;

// ---------- 解析器 ----------

/// RFC3339 Zulu → unix ms：整秒、毫秒小数、跨日、闰日、畸形拒绝（相对性断言）。
#[test]
fn rfc3339_zulu_parses_to_unix_ms() {
    let base = rfc3339_zulu_to_ms("2026-10-31T00:00:00Z").unwrap();
    let with_frac = rfc3339_zulu_to_ms("2026-10-31T00:00:00.099597Z").unwrap();
    assert_eq!(with_frac - base, 99, "0.099597s = 99ms（微秒截断到毫秒）");
    let next_day = rfc3339_zulu_to_ms("2026-11-01T00:00:00Z").unwrap();
    assert_eq!(next_day - base, 86_400_000, "跨日差一天");
    // 畸形拒绝
    assert_eq!(rfc3339_zulu_to_ms(""), None);
    assert_eq!(rfc3339_zulu_to_ms("2026-10-31 00:00:00Z"), None, "缺 T");
    assert_eq!(rfc3339_zulu_to_ms("2026-13-31T00:00:00Z"), None, "月>12");
    assert_eq!(rfc3339_zulu_to_ms("2026-10-31T25:00:00Z"), None, "时>23");
    // 闰日
    assert!(rfc3339_zulu_to_ms("2028-02-29T12:00:00Z").is_some(), "闰年 2/29");
    assert_eq!(rfc3339_zulu_to_ms("2026-02-29T12:00:00Z"), None, "平年 2/29");
}

// ---------- 过滤纯函数 ----------

fn line_json(level: &str, component: &str, msg: &str, ts: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": ts,
        "level": level,
        "fields": {"message": msg, "component": component, "run_id": "run-xyz"},
        "target": "devforge::test",
    })
}

fn bare_query() -> LogsQuery {
    LogsQuery {
        level: None,
        component: None,
        q: None,
        run_id: None,
        since_ms: None,
        until_ms: None,
        offset: None,
        limit: None,
    }
}

#[test]
fn log_line_matches_filters() {
    let line = line_json(
        "WARN",
        "scheduler",
        "调度 tick 完成",
        "2026-10-31T15:10:07.099597Z",
    );
    let raw = line.to_string();
    let none = bare_query();
    assert!(log_line_matches(&line, &raw, &none, None, None));

    // level 下限：warn 匹配 WARN/ERROR，不匹配 INFO
    let warn = LogsQuery { level: Some("warn".into()), ..none.clone() };
    assert!(log_line_matches(&line, &raw, &warn, None, None));
    let info_line = line_json("INFO", "scheduler", "x", "2026-10-31T15:10:07Z");
    assert!(!log_line_matches(&info_line, &info_line.to_string(), &warn, None, None));
    let err_line = line_json("ERROR", "scheduler", "x", "2026-10-31T15:10:07Z");
    assert!(log_line_matches(&err_line, &err_line.to_string(), &warn, None, None));

    // component 精确
    let comp = LogsQuery { component: Some("scheduler".into()), ..none.clone() };
    assert!(log_line_matches(&line, &raw, &comp, None, None));
    let comp2 = LogsQuery { component: Some("engine".into()), ..none.clone() };
    assert!(!log_line_matches(&line, &raw, &comp2, None, None));

    // q 全行小写包含（中文字段值也命中）
    let q = LogsQuery { q: Some("调度 tick".into()), ..none.clone() };
    assert!(log_line_matches(&line, &raw, &q, None, None));
    let q2 = LogsQuery { q: Some("不存在的词".into()), ..none.clone() };
    assert!(!log_line_matches(&line, &raw, &q2, None, None));

    // run_id：整行包含
    let rid = LogsQuery { run_id: Some("run-xyz".into()), ..none.clone() };
    assert!(log_line_matches(&line, &raw, &rid, None, None));
    let rid2 = LogsQuery { run_id: Some("run-other".into()), ..none.clone() };
    assert!(!log_line_matches(&line, &raw, &rid2, None, None));

    // 时间窗：ts 无法解析的行保留（宁多勿丢）
    let bad_ts = line_json("INFO", "x", "y", "not-a-time");
    assert!(log_line_matches(
        &bad_ts,
        &bad_ts.to_string(),
        &none,
        Some(1),
        Some(2)
    ));
    let since = LogsQuery { since_ms: Some(1), ..none.clone() };
    assert!(log_line_matches(&line, &raw, &since, Some(1), None));
    let future_until = LogsQuery { until_ms: Some(1), ..none.clone() };
    assert!(!log_line_matches(&line, &raw, &future_until, None, Some(1)));
}

// ---------- 端点 ----------

fn state_with_logs_dir(logs_dir: &std::path::Path, exempt: bool) -> Arc<ServerState> {
    let bus = Arc::new(EventBus::new());
    let ui = Arc::new(UiRelay::new(bus.clone()));
    let mut st = ServerState::new(
        crate::domain::store::Store::new(),
        SopRegistry::builtin(),
        ConfigManifest::builtin(),
        Box::new(|| -> Box<dyn crate::domain::RunEngine + Send> {
            unreachable!("logs 单测不触发引擎")
        }),
        bus,
        ui,
        Arc::new(FakeInstaller),
        PathBuf::from("/tmp/logs-profiles"),
        PathBuf::from("/tmp/logs-plans"),
        PathBuf::from("/tmp/logs-sessions"),
    );
    let shared = Arc::get_mut(&mut st).expect("路由构建前应独占");
    shared.logs_root = logs_dir.to_path_buf();
    shared
        .auth_loopback_exempt
        .store(exempt, std::sync::atomic::Ordering::Relaxed);
    st
}

/// 端点：临时日志文件 → level/component 过滤 + 分页 + 损坏行跳过 + timestamp_ms 附加。
#[tokio::test]
async fn logs_query_filters_and_paginates() {
    let dir = tempfile::tempdir().unwrap();
    let st = state_with_logs_dir(dir.path(), true);
    let router = build_router(st.clone());
    let write_line = |v: serde_json::Value| serde_json::to_string(&v).unwrap();
    let content = [
        write_line(serde_json::json!({"timestamp": "2026-10-31T10:00:00.000000Z", "level": "INFO", "fields": {"message": "启动", "component": "main"}, "target": "devforge::main"})),
        write_line(serde_json::json!({"timestamp": "2026-10-31T11:00:00.000000Z", "level": "WARN", "fields": {"message": "渠道告警", "component": "scheduler", "run_id": "run-42"}, "target": "devforge::scheduler"})),
        write_line(serde_json::json!({"timestamp": "2026-10-31T12:00:00.000000Z", "level": "ERROR", "fields": {"message": "渠道失败 run-42", "component": "engine"}, "target": "devforge::engine"})),
        "半写损坏行 {{{".to_string(),
    ]
    .join("\n")
        + "\n";
    std::fs::create_dir_all(&st.logs_root).unwrap();
    std::fs::write(crate::logging::log_file_path(&st.logs_root, "devforge"), content).unwrap();

    // 全量（豁免开，无凭据）：3 条合法行，损坏行跳过；timestamp_ms 附加字段
    let resp = send(&router, "GET", "/api/v1/logs", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(3), "损坏行不计入：{body}");
    let lines = body["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 3);
    assert!(lines[0]["timestamp_ms"].as_u64().is_some(), "附加解析时间戳");

    // level=warn → 2 条（WARN+ERROR）
    let resp = send(&router, "GET", "/api/v1/logs?level=warn", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(2));

    // component=engine → 1 条
    let resp = send(&router, "GET", "/api/v1/logs?component=engine", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(1));
    assert_eq!(body["lines"][0]["level"], "ERROR");

    // run_id=run-42 → 2 条（WARN+ERROR 都带）
    let resp = send(&router, "GET", "/api/v1/logs?run_id=run-42", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(2));

    // 分页：offset=1&limit=1 → total 仍是 3，返回第 2 条
    let resp = send(&router, "GET", "/api/v1/logs?offset=1&limit=1", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(3));
    assert_eq!(body["lines"].as_array().unwrap().len(), 1);
    assert_eq!(body["lines"][0]["level"], "WARN", "文件序旧→新切片");
}

/// 时间窗：since_ms/until_ms 按解析出的时间戳过滤。
#[tokio::test]
async fn logs_query_time_window() {
    let dir = tempfile::tempdir().unwrap();
    let st = state_with_logs_dir(dir.path(), true);
    let router = build_router(st.clone());
    let content = [
        serde_json::json!({"timestamp": "2026-10-31T10:00:00.000000Z", "level": "INFO", "fields": {"message": "a"}, "target": "t"}),
        serde_json::json!({"timestamp": "2026-10-31T12:00:00.000000Z", "level": "INFO", "fields": {"message": "b"}, "target": "t"}),
    ]
    .iter()
    .map(|v| v.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\n";
    std::fs::create_dir_all(&st.logs_root).unwrap();
    std::fs::write(crate::logging::log_file_path(&st.logs_root, "devforge"), content).unwrap();

    let t11 = rfc3339_zulu_to_ms("2026-10-31T11:00:00Z").unwrap();
    let t13 = rfc3339_zulu_to_ms("2026-10-31T13:00:00Z").unwrap();

    // since 11:00 → 只剩 12:00 那条
    let resp = send(&router, "GET", &format!("/api/v1/logs?since_ms={t11}"), None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(1));
    assert_eq!(body["lines"][0]["fields"]["message"], "b");

    // until 11:00 → 只剩 10:00 那条
    let resp = send(&router, "GET", &format!("/api/v1/logs?until_ms={t11}"), None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(1));
    assert_eq!(body["lines"][0]["fields"]["message"], "a");

    // 全窗外（13:00 之后）→ 0
    let resp = send(&router, "GET", &format!("/api/v1/logs?since_ms={t13}"), None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(0));

    // 日志文件缺失（logs_root 指向空目录）→ 0 匹配不报错
    let empty_dir = tempfile::tempdir().unwrap();
    let st2 = state_with_logs_dir(empty_dir.path(), true);
    let router2 = build_router(st2);
    let resp = send(&router2, "GET", "/api/v1/logs", None).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(body_json(resp).await["total_matched"].as_u64(), Some(0));
}

// ---------- 轮转感知（收尾批 2026-11-01） ----------

/// 轮转归并：当前文件 + .1/.2 历史副本按 mtime 旧→新归并，分页跨文件连续；
/// .gz 压缩副本跳过（读不了，files 列表如实反映）；坏行照旧跳过。
#[tokio::test]
async fn logs_query_scans_rotated_files_in_mtime_order() {
    let dir = tempfile::tempdir().unwrap();
    let st = state_with_logs_dir(dir.path(), true);
    let router = build_router(st.clone());
    std::fs::create_dir_all(&st.logs_root).unwrap();

    let line = |msg: &str| {
        serde_json::json!({"timestamp": "2026-10-31T10:00:00.000000Z", "level": "INFO", "fields": {"message": msg}, "target": "t"})
            .to_string()
    };
    // .2（最旧）→ .1 → 当前（最新），写盘顺序保证 mtime 单调
    let oldest = st.logs_root.join("devforge.log.jsonl.2");
    let mid = st.logs_root.join("devforge.log.jsonl.1");
    let cur = crate::logging::log_file_path(&st.logs_root, "devforge");
    std::fs::write(&oldest, format!("{}\n", line("from-rotated-2"))).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&mid, format!("{}\nbroken-line\n{}\n", line("from-rotated-1"), line("mid-second"))).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&cur, format!("{}\n{}\n", line("from-current-a"), line("from-current-b"))).unwrap();
    // gz 副本：读不了，必须被跳过（不炸不计入）
    std::fs::write(st.logs_root.join("devforge.log.jsonl.3.gz"), b"\x1f\x8b garbage").unwrap();

    // 全量：5 条合法行（gz 与 broken-line 不计），旧→新归并
    let resp = send(&router, "GET", "/api/v1/logs", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(5), "{body}");
    let msgs: Vec<&str> = body["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["fields"]["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        msgs,
        vec!["from-rotated-2", "from-rotated-1", "mid-second", "from-current-a", "from-current-b"],
        "跨文件旧→新归并：{msgs:?}"
    );
    // files 列表如实反映扫描范围（无 .gz）
    let files: Vec<&str> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(files.len(), 3, "gz 不在扫描列表：{files:?}");
    assert!(!files.iter().any(|f| f.ends_with(".gz")), "{files:?}");

    // 跨文件分页：offset=2&limit=2 → mid-second + from-current-a
    let resp = send(&router, "GET", "/api/v1/logs?offset=2&limit=2", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(5), "total 跨文件累计");
    let msgs: Vec<&str> = body["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["fields"]["message"].as_str().unwrap())
        .collect();
    assert_eq!(msgs, vec!["mid-second", "from-current-a"], "分页跨文件连续：{msgs:?}");
}

/// 单文件行为不变：无轮转文件时与旧口径一致（files 只含当前文件）。
#[tokio::test]
async fn logs_query_single_file_behavior_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let st = state_with_logs_dir(dir.path(), true);
    let router = build_router(st.clone());
    std::fs::create_dir_all(&st.logs_root).unwrap();
    std::fs::write(
        crate::logging::log_file_path(&st.logs_root, "devforge"),
        format!("{}\n", serde_json::json!({"timestamp": "2026-10-31T10:00:00.000000Z", "level": "INFO", "fields": {"message": "only"}, "target": "t"})),
    )
    .unwrap();

    let resp = send(&router, "GET", "/api/v1/logs", None).await;
    let body = body_json(resp).await;
    assert_eq!(body["total_matched"].as_u64(), Some(1));
    assert_eq!(body["files"].as_array().unwrap().len(), 1);
    assert_eq!(body["lines"][0]["fields"]["message"], "only");
}
