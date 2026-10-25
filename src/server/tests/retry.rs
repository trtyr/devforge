//! （ARC-R6 自 server/tests.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{crud::*, http::*, observe::*, start::*};

#[tokio::test]
async fn quality_failure_path_is_not_retried_end_to_end() {
    // 走真实 run 路径（不是直接喂给 map_intervention）：引擎报 Quality → 一次就 Failed，attempt 恒为 1
    let f = fixture_with_quality_failure();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "doc-from-zero"})),
    )
    .await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t").await;

    assert_eq!(f.calls.lock().unwrap().len(), 1, "质量失败不得自动重启");
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    assert_eq!(body["runs"][0]["status"], "Failed");
    let key = crate::runstate::StateKey::new("p", "t", "doc-from-zero");
    let attempt = crate::runstate::load(&f.state.plans_root, &key)
        .state()
        .map(|s| s.attempt)
        .unwrap_or(0);
    assert_eq!(attempt, 0, "没有重启就不会写 attempt>0");
}

/// 源码守卫：协议违例上限 / 终审不通过不得再被当成基础设施故障。
#[test]
fn quality_failure_sites_do_not_use_system_error() {
    // 逐条比对旧文本片段，而不是泛搜「基础设施故障」——审查空回复属于真基础设施故障，保留 System 是对的
    let stale = [
        ("coding", include_str!("../../sop/coding.rs"), "stage-verdict/next 块），判定为基础设施故障"),
        ("testing", include_str!("../../sop/testing.rs"), "stage-verdict/next），判定为基础设施故障"),
        (
            "doc_from_zero",
            include_str!("../../sop/doc/runner.rs"),
            "doc-verdict/route 块），判定为基础设施故障",
        ),
    ];
    for (name, src, needle) in stale {
        assert!(
            !src.contains(needle),
            "{name} 的协议违例上限仍走 System，会被自动重启"
        );
    }
    let doc_fix = include_str!("../../sop/doc/fix.rs");
    let at = doc_fix
        .find("缺 stage-verdict），判定为")
        .expect("doc_fix 协议违例消息应存在");
    let near = &doc_fix[at..(at + 120).min(doc_fix.len())];
    assert!(near.contains("质量失败"), "doc_fix 应判质量失败：{near}");
    // D026：违例上限与实例失败上限的分层（迁到共享门后，这三个文件里不再有旧文案，
    // 守卫必须扩到新模块，否则「绿但没守任何东西」）。
    let gate = include_str!("../../sop/review_gate.rs");
    for (anchor, needle, why) in [
        (
            "DevforgeError::Quality",
            "输出协议违例",
            "协议违例上限必须判 Quality（不自动重启）",
        ),
        (
            "DevforgeError::System",
            "判定为基础设施故障",
            "审查实例失败上限必须判 System（可重启）",
        ),
    ] {
        let at = gate.find(anchor).expect("共享门里应有对应的错误构造");
        let near = &gate[at..(at + 200).min(gate.len())];
        assert!(near.contains(needle), "{why}：{near}");
    }
    // ARC-R2/R3：终审文案随 run_doc_from_zero 迁入 sop/doc/wiring.rs，守卫扫描面跟迁移走
    let doc_wiring = include_str!("../../sop/doc/wiring.rs");
    for anchor in ["终审未通过", "终审输出协议违例"] {
        let at = doc_wiring
            .find(anchor)
            .unwrap_or_else(|| panic!("找不到 {anchor}"));
        let near = &doc_wiring[at.saturating_sub(200)..(at + 200).min(doc_wiring.len())];
        assert!(
            near.contains("DevforgeError::Quality"),
            "{anchor} 必须返回 Quality（不被重试），附近：{near}"
        );
    }
}

#[test]
fn quality_error_becomes_failed_and_is_not_retried() {
    // plan 010 T006/D004：质量失败→明确的 Failed（不进重启分支）；System 仍向上抛（可重启）
    // ARC-R5：map_intervention 是横切策略自由函数（engine/policy.rs），不再挂在引擎上
    let control = RunControl::new();
    let outcome = crate::engine::policy::map_intervention(
        &control,
        "doc-engine",
        crate::error::DevforgeError::Quality("审查连续 4 轮输出协议违例".into()),
    )
    .expect("质量失败不得作为错误上抛");
    match outcome {
        RunOutcome::Failed { reason } => assert!(reason.contains("违例"), "{reason}"),
        other => panic!("应为 Failed，实际 {other:?}"),
    }

    let infra = crate::engine::policy::map_intervention(
        &control,
        "doc-engine",
        crate::error::DevforgeError::System("上游 500".into()),
    );
    assert!(infra.is_err(), "系统错误必须上抛给重试层");
    assert!(
        crate::engine::policy::is_infra_failure(&infra),
        "System 才是可重启故障"
    );
}

#[tokio::test]
async fn infra_failures_are_retried_up_to_three_times() {
    // plan 010 T005：前两次 System → 第三次成功，run 终态 Settled，attempt 落盘为 3
    let f = fixture_with_failures(2);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t").await;

    assert_eq!(f.calls.lock().unwrap().len(), 3, "两次故障 + 一次成功");
    let key = crate::runstate::StateKey::new("p", "t", "planning");
    let st = crate::runstate::load(&f.state.plans_root, &key);
    assert_eq!(
        st.state().map(|s| s.attempt),
        Some(3),
        "attempt 必须落盘（续跑审计）"
    );
}

#[tokio::test]
async fn persistent_infra_failures_fail_after_four_attempts() {
    // 不无限：连续故障只试 4 次（首次 + 3 重试），终态 Failed
    let f = fixture_with_failures(10);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    let resp = send(&f.router, "POST", "/api/v1/projects/p/tasks/t/start", None).await;
    assert_eq!(resp.status(), 202);
    wait_settled(&f, "p", "t").await;

    assert_eq!(f.calls.lock().unwrap().len(), 4, "首次 + 3 次重试，不得无限重跑");
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    assert_eq!(body["runs"][0]["status"], "Failed");
    let key = crate::runstate::StateKey::new("p", "t", "planning");
    let st = crate::runstate::load(&f.state.plans_root, &key);
    assert_eq!(st.state().map(|s| s.attempt), Some(4));
}

/// 验收（计划 §5）· 「先记错误、再决定重启」：第 1 次基础设施故障的 ERROR 条目必须出现在
/// 「基础设施故障，自动重启 run」WARN **之前**，且条目带 `attempt=1`。
///
/// 错实现形态：把重试层的逐次落盘挪到终态（只在尝试耗尽后落一条）→ 现场只剩「重启了」，
/// 看不出「为什么重启」→ 本用例必须变红。
///
/// 为什么要跑**真实 run 路径**且**跑在本线程**：「自动重启 run」这条 WARN 只在服务端落
/// （`src/server/mod.rs`），ERROR 条目只在重试层落（`src/engine/policy.rs`）——两处落点必须
/// 出现在**同一条日志流**里才有先后可言；而捕获器按线程分流（`observability::test_support`），
/// 经 HTTP `start` 的那条路会把 run 扔进别的线程 → 两处落点就散在不同缓冲里，看不到顺序。
#[tokio::test]
async fn the_infra_error_is_logged_before_the_restart_warning() {
    let f = fixture_with_failures(1);
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    // 引擎：第 1 次抛基础设施故障，第 2 次成功 → 必然发生一次「自动重启」
    let engine: crate::server::EngineBox = Box::new(MockEngine {
        calls: Arc::new(Mutex::new(Vec::new())),
        hang_ms: 0,
        fail_system_times: Arc::new(Mutex::new(1)),
        quality_fail: false,
        panic_times: Arc::new(Mutex::new(0)),
    });
    let text = crate::observability::test_support::capture_jsonl(|| {
        crate::server::run_in_background(f.state.clone(), "p".into(), "t".into(), engine);
    });
    let lines: Vec<&str> = text.lines().collect();
    let err_at = lines
        .iter()
        .position(|l| l.contains("run.attempt_failed") && l.contains("\"attempt\":1"))
        .unwrap_or_else(|| panic!("第 1 次失败应落一条 ERROR 条目：{text}"));
    let restart_at = lines
        .iter()
        .position(|l| l.contains("自动重启 run"))
        .unwrap_or_else(|| panic!("基础设施故障应触发自动重启：{text}"));
    assert!(
        err_at < restart_at,
        "错误条目必须先于「自动重启 run」落盘（先记错误、再决定重启）：{text}"
    );
    let err_line = lines[err_at];
    assert!(err_line.contains("\"level\":\"ERROR\""), "非质量类失败落 ERROR：{err_line}");
    assert!(err_line.contains("\"component\":\"run-retry\""), "{err_line}");
    let restart_line = lines[restart_at];
    assert!(restart_line.contains("\"level\":\"WARN\""), "{restart_line}");
    assert!(
        restart_line.contains("\"attempt\":2"),
        "重启的是第 2 次尝试：{restart_line}"
    );
}


