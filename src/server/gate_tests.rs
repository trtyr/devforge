//! （2026-10-26 测试分离：自 src/server/gate.rs 内联平移，纯移动零改动。）
use super::RenderGate;

// ---------- 010 F6 靶：渲染闸门 ----------

#[tokio::test]
async fn the_gate_waits_for_the_required_profiles_and_ignores_the_rest() {
    // 错实现形态：闸门不等所需 profile（start 立刻开跑 → 门卫判未渲染 → 盲重启）
    // 或等整轮渲染（start 变成分钟级阻塞点）→ 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let gate = std::sync::Arc::new(RenderGate::new());
    gate.begin_render();
    let root = dir.path().to_path_buf();
    let g = gate.clone();
    let waiter = tokio::spawn(async move {
        g.wait_for_profiles(&root, &["plan-agent", "plan-review"]).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!waiter.is_finished(), "所需 profile 未就绪时闸门必须等");
    // 所需 profile 就绪（其它 profile 仍缺、渲染仍在途）→ 放行
    for p in ["plan-agent", "plan-review"] {
        let d = dir.path().join(p);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("settings.json"), "{}").unwrap();
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
        .await
        .expect("所需 profile 就绪后闸门应放行")
        .unwrap();
    assert!(
        gate.is_in_flight(),
        "放行不改变渲染在途状态（闸门不等整轮渲染）"
    );
}

#[tokio::test]
async fn the_gate_releases_when_the_render_finishes_even_if_the_profile_is_missing() {
    // 错实现形态：渲染结束也不放行（start 永远挂着）→ 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let gate = std::sync::Arc::new(RenderGate::new());
    gate.begin_render();
    let root = dir.path().to_path_buf();
    let g = gate.clone();
    let waiter = tokio::spawn(async move {
        g.wait_for_profiles(&root, &["plan-agent"]).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(!waiter.is_finished(), "渲染仍在途且 profile 未就绪 → 继续等");
    gate.end_render();
    tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
        .await
        .expect("渲染结束后闸门应放行（成败由门卫复查决定）")
        .unwrap();
}

#[tokio::test]
async fn the_gate_keeps_waiting_while_another_render_is_still_in_flight() {
    // 错实现形态：任意一轮渲染结束就放行（`rx.changed()` 分支直接 return）→ 并发两轮渲染时
    //   第一轮结束就把 run 放出去（所需 profile 还没落盘）→ 本用例必须变红
    let dir = tempfile::tempdir().unwrap();
    let gate = std::sync::Arc::new(RenderGate::new());
    gate.begin_render(); // 第 1 轮渲染在途
    gate.begin_render(); // 第 2 轮渲染在途
    let root = dir.path().to_path_buf();
    let g = gate.clone();
    let waiter = tokio::spawn(async move {
        g.wait_for_profiles(&root, &["plan-agent"]).await;
    });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert!(!waiter.is_finished(), "两轮渲染都在途时闸门必须等");
    // 第 1 轮结束：仍有渲染在途、所需 profile 未就绪 → 不得放行
    gate.end_render();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !waiter.is_finished(),
        "仍有渲染在途时不得放行（所需 profile 还没落盘）"
    );
    // 所需 profile 就绪 → 放行（放行条件的另一半）
    let d = dir.path().join("plan-agent");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("settings.json"), "{}").unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), waiter)
        .await
        .expect("所需 profile 就绪后闸门应放行")
        .unwrap();
}

#[tokio::test]
async fn the_gate_does_not_wait_when_no_render_is_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let gate = RenderGate::new();
    tokio::time::timeout(
        std::time::Duration::from_millis(200),
        gate.wait_for_profiles(dir.path(), &["plan-agent"]),
    )
    .await
    .expect("无渲染在途时不应等待");
}
