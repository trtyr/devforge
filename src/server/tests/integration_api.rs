//! （2026-10-27 新增：集成队列端点此前零测试覆盖——findings 118/120/214。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{http::*, observe::*, retry::*, start::*};

fn queue_path(f: &Fixture, project: &str) -> std::path::PathBuf {
    crate::integration::MergeQueue::queue_path(&f.state.plans_root, project)
}

#[tokio::test]
async fn integration_queue_round_trip_and_validation() {
    let f = fixture();
    // 夹具的 store 是空的：先建项目（端点要求项目存在）
    {
        let mut store = f.state.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .create_project(crate::domain::project::Project::new(
                "devforge",
                f.state.plans_root.join("repo"),
                0,
            ))
            .unwrap();
    }
    let hex = "a".repeat(40);
    let body = json!({
        "task_id": "t1",
        "frozen_sha": hex,
        "base_rev": "b".repeat(40),
        "owned_paths": ["src/x.rs"],
    });

    // 项目不存在 → 404（findings 214：旧口径会对不存在的项目建出队列目录）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/ghost/integration/queue",
        Some(body.clone()),
    )
    .await;
    assert_eq!(resp.status(), 404);
    assert!(!queue_path(&f, "ghost").exists(), "不得为不存在的项目建目录");

    // 正常入队 → 201，队列落盘
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/devforge/integration/queue",
        Some(body.clone()),
    )
    .await;
    assert_eq!(resp.status(), 201);
    let q = crate::integration::MergeQueue::load(&f.state.plans_root, "devforge").unwrap();
    assert_eq!(q.candidates.len(), 1);

    // 同一 (task_id, frozen_sha) 且仍 Pending → 去重（findings 120）
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/devforge/integration/queue",
        Some(body.clone()),
    )
    .await;
    assert_eq!(resp.status(), 201);
    let q = crate::integration::MergeQueue::load(&f.state.plans_root, "devforge").unwrap();
    assert_eq!(q.candidates.len(), 1, "重复入队必须去重");

    // frozen_sha 非 40 位十六进制 → 400（git 参数注入面）
    let mut bad = body.clone();
    bad["frozen_sha"] = json!("--upload-pack=evil");
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/devforge/integration/queue",
        Some(bad),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // task_id 含路径分隔符 → 400（会被拼进产物文件名）
    let mut bad = body.clone();
    bad["task_id"] = json!("../evil");
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/projects/devforge/integration/queue",
        Some(bad),
    )
    .await;
    assert_eq!(resp.status(), 400);

    // 查队列：depth 与候选可见
    let resp = send(
        &f.router,
        "GET",
        "/api/v1/projects/devforge/integration/queue",
        None,
    )
    .await;
    assert_eq!(resp.status(), 200);
    let got = body_json(resp).await;
    assert_eq!(got["depth"], 1);
    assert_eq!(got["candidates"][0]["task_id"], "t1");
}

#[tokio::test]
async fn unknown_ui_answer_is_404_not_500() {
    // findings 211：客户端重复/过期应答不得被当成服务端故障
    let f = fixture();
    let resp = send(
        &f.router,
        "POST",
        "/api/v1/ui/nope-123/answer",
        Some(json!({"value": "x"})),
    )
    .await;
    assert_eq!(resp.status(), 404);
}

// ---------- 2026-10-27：日志读取的 run 归属 + 分页（findings 209/210）----------

fn write_log(dir: &std::path::Path, lines: &[serde_json::Value]) -> std::path::PathBuf {
    let p = dir.join("devforge.log.jsonl");
    let body: String = lines
        .iter()
        .map(|v| format!("{v}\n"))
        .collect();
    std::fs::write(&p, body).unwrap();
    p
}

#[test]
fn log_events_are_attributed_to_their_run_only() {
    // 错实现形态：不过滤 run_id（旧口径回全局流，响应里的 run 字段是误导标签）→ 红
    let dir = tempfile::tempdir().unwrap();
    let p = write_log(
        dir.path(),
        &[
            json!({"fields": {"event": "agent.step", "run_id": "run-a"}}),
            json!({"fields": {"event": "agent.step", "run_id": "run-b"}}),
            json!({"fields": {"event": "server.ready"}}), // 无归属
            json!({"fields": {"event": "agent.turn_end", "run_id": "run-a"}}),
        ],
    );
    let (total, page) =
        crate::server::handlers::read_log_events_at(&p, Some("run-a"), None, 0, 100);
    assert_eq!(total, 2, "只有 run-a 的两条：{page:?}");
    assert!(page.iter().all(|v| v["fields"]["run_id"] == "run-a"));
    // 事件名子串过滤与 run 归属叠加
    let (total, _) = crate::server::handlers::read_log_events_at(&p, Some("run-a"), Some("turn_end"), 0, 100);
    assert_eq!(total, 1);
}

#[test]
fn log_events_pagination_is_stable_and_bounded() {
    // 分页边界 + 坏行跳过 + 文件缺失返回空（不得 5xx）
    let dir = tempfile::tempdir().unwrap();
    let mut lines: Vec<serde_json::Value> = (0..5)
        .map(|i| json!({"fields": {"event": "agent.step", "run_id": "r", "i": i}}))
        .collect();
    lines.push(json!({"not_a_log_line": true}));
    lines.push(json!({"fields": {"event": "agent.step", "run_id": "r", "i": 5}}));
    let p = dir.path().join("devforge.log.jsonl");
    std::fs::write(&p, lines.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
    let (total, page) = crate::server::handlers::read_log_events_at(&p, Some("r"), None, 2, 2);
    assert_eq!(total, 6, "坏行不计入");
    assert_eq!(page.len(), 2);
    assert_eq!(page[0]["fields"]["i"], 2);
    assert_eq!(page[1]["fields"]["i"], 3);
    // 文件缺失
    let (total, page) = crate::server::handlers::read_log_events_at(
        &dir.path().join("missing.jsonl"),
        Some("r"),
        None,
        0,
        10,
    );
    assert_eq!(total, 0);
    assert!(page.is_empty());
}
