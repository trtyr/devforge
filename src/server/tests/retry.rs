//! （ARC-R6 自 server/tests.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;

#[tokio::test]
async fn quality_failure_path_is_not_retried_end_to_end() {
    // 走真实 run 路径（不是直接喂给 map_intervention）：引擎报 Quality → 一次就 Failed。
    // 自审 minor（51）：断点里的 attempt 是「重启记账用的尝试号」——**只在重试时写**，
    // 缺省/0 都表示「没有记过重启」，不是「尝试了一次」。
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
    assert_eq!(body["runs"][0]["status"], "failed");
    let key = crate::runstate::StateKey::new("p", "t", "doc-from-zero");
    let st = crate::runstate::load(&f.state.plans_root, &key);
    // 自审 minor（246）：区分「没写断点」与「写了但读不出来/身份不符」——
    // 旧口径 `unwrap_or(0)` 把两种情形折成一类，断点损坏可伪装成「没有重启」。
    match &st {
        crate::runstate::Loaded::Unusable(reason) => {
            panic!("断点不可用（判读失败）：{reason:?}")
        }
        // 质量失败可能根本不写断点（None），也可能写了但没记重启（Some(0)）；
        // 两者都不等于「记了重启」（≥1），但必须能区分于「读不出来」。
        other => assert!(
            other.state().map(|s| s.attempt).unwrap_or(0) == 0,
            "质量失败不得记重启尝试号：{:?}",
            other.state().map(|s| s.attempt)
        ),
    }
}

/// 源码守卫：协议违例上限 / 终审不通过不得再被当成基础设施故障。
///
/// 自审 major 纠偏：旧守卫的三条 stale 断言（coding/testing/doc-runner 的
/// 「stage-verdoc 块）判定为基础设施故障」旧文案）早已在目标源码中不存在——
/// `!contains` 恒真，整段删掉仍全绿（hollow）。文案随 D026 迁入共享门后，
/// 活守卫只保留仍命中真实源码的部分（doc_fix / review_gate / doc_wiring）。
/// 取源码中 `at` 起的一段窗口（按字符边界截断；自审 minor 247：旧口径按字节偏移切片，
/// 窗口边界落在多字节中文中间就 panic 而不是干净失败）。
fn text_window(text: &str, at: usize, len: usize) -> &str {
    let end = (at + len).min(text.len());
    let mut lo = at.min(text.len());
    while lo < text.len() && !text.is_char_boundary(lo) {
        lo += 1;
    }
    let mut hi = end;
    while hi < text.len() && !text.is_char_boundary(hi) {
        hi += 1;
    }
    &text[lo..hi]
}

#[test]
fn quality_failure_sites_do_not_use_system_error() {
    let doc_fix = include_str!("../../sop/doc/fix.rs");
    let at = doc_fix
        .find("缺 stage-verdict），判定为")
        .expect("doc_fix 协议违例消息应存在");
    let near = text_window(doc_fix, at, 120);
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
        let near = text_window(gate, at, 200);
        assert!(near.contains(needle), "{why}：{near}");
    }
    // ARC-R2/R3：终审文案随 run_doc_from_zero 迁入 sop/doc/wiring.rs，守卫扫描面跟迁移走
    let doc_wiring = include_str!("../../sop/doc/wiring.rs");
    for anchor in ["终审未通过", "终审输出协议违例"] {
        let at = doc_wiring
            .find(anchor)
            .unwrap_or_else(|| panic!("找不到 {anchor}"));
        let near = text_window(doc_wiring, at.saturating_sub(200), 400);
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

/// 自审 minor（248）：分类单点必须能穿透 `WithContext` 包装（root_cause）——
/// 旧口径只用例只喂裸变体，回归成直接 `matches!(e, Quality(_))`（不剥包装）也不会被发现，
/// 而这正是生产路径的真实形状（错误几乎都带着上下文出口）。
#[test]
fn context_wrapped_errors_are_classified_by_root_cause() {
    use crate::error::Context as _;

    let control = RunControl::new();
    let wrapped_quality = (|| -> crate::error::Result<()> {
        Err(crate::error::DevforgeError::Quality(
            "审查连续 4 轮输出协议违例".into(),
        ))
        .ctx("doc-from-zero 终审")
        .ctx("run 收尾")
    })()
    .unwrap_err();
    let outcome = crate::engine::policy::map_intervention(&control, "doc-engine", wrapped_quality)
        .expect("包装后的质量失败仍不得上抛");
    assert!(matches!(outcome, RunOutcome::Failed { .. }), "{outcome:?}");

    let wrapped_system = (|| -> crate::error::Result<()> {
        Err(crate::error::DevforgeError::System("上游 500".into())).ctx("RPC 调用")
    })()
    .unwrap_err();
    let infra = crate::engine::policy::map_intervention(&control, "doc-engine", wrapped_system);
    assert!(
        crate::engine::policy::is_infra_failure(&infra),
        "包装后的 System 仍必顶得上可重启：{infra:?}"
    );
}

#[tokio::test]
async fn infra_failures_are_retried_up_to_three_times() {
    // plan 010 T005：前两次 System → 第三次成功，run 终态 settled，attempt 落盘为 3
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
    // 自审 minor（249）：必须真断言终态——`wait_settled` 对 settled/failed 一视同仁，
    // 「重试后成功却落 failed」的回归在本用例里本来看不见。
    let body = body_json(
        send(&f.router, "GET", "/api/v1/projects/p/tasks/t/runs", None).await,
    )
    .await;
    assert_eq!(
        body["runs"][0]["status"], "settled",
        "重试后成功必须落 settled：{body}"
    );
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
    assert_eq!(body["runs"][0]["status"], "failed");
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
/// 为什么要跑**真实 run 路径**且**跑在本线程**：「自动重启 run」这条 WARN 在
/// `src/engine/policy.rs` 的 run_attempt_bookkeeping 落（自审 minor 52：ARC-R5 后已从
/// server/mod.rs 迁出，server 只是调用方），ERROR 条目也在重试层落——两处落点必须
/// 出现在**同一条日志流**里才有先后可言；而捕获器按线程分流（`observability::test_support`），
/// 经 HTTP `start` 的那条路会把 run 扔进别的线程 → 两处落点就散在不同缓冲里，看不到顺序。
///
/// **覆盖边界（自审 minor 251）**：本用例手工准入（只 `store.begin_run`），覆盖的是
/// `run_in_background` 及其之后的重试链；`start_task` 的安全段校验 / sop 覆盖 / 渲染闸门 /
/// 续跑门 `resume::start_gate` 不在范围内（那些由 start.rs 的用例守）。
#[tokio::test]
async fn the_infra_error_is_logged_before_the_restart_warning() {
    // 自审 minor（250）：夹具的故障注入在这里用不上（本用例跑真实 run + 手工准入），
    // 所以不建夹具引擎，只用它把项目/任务注册好——旧口径建了带注入的夹具却完全不生效。
    let f = fixture();
    send(&f.router, "POST", "/api/v1/projects", Some(json!({"name": "p", "local_path": "/tmp/p"}))).await;
    send(
        &f.router,
        "POST",
        "/api/v1/projects/p/tasks",
        Some(json!({"id": "t", "sop": "planning"})),
    )
    .await;
    // 引擎：第 1 次抛基础设施故障，第 2 次成功 → 必然发生一次「自动重启」
    let calls = Arc::new(Mutex::new(Vec::new()));
    let engine: crate::server::EngineBox = Box::new(MockEngine {
        calls: calls.clone(),
        hang_ms: 0,
        fail_system_times: Arc::new(Mutex::new(1)),
        quality_fail: false,
        panic_times: Arc::new(Mutex::new(0)),
    });
    let run = {
        let mut store = f.state.store.lock().expect("store 锁");
        store.begin_run("p", "t").expect("准入")
    };
    let text = crate::observability::test_support::capture_jsonl(|| {
        crate::server::run_in_background(f.state.clone(), "p".into(), "t".into(), engine, run);
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
    // 自审 minor（250）：引擎必须真跑了两次（旧口径只看日志，不断言引擎调用次数）
    assert_eq!(
        calls.lock().unwrap().len(),
        2,
        "一次故障 + 一次成功 = 引擎恰好被调用两次"
    );
}


