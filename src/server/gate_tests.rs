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
        // 自审 minor（115）：就绪判据 = 渲染完成标记（settings.json 在装包前就写了）
        std::fs::write(d.join(crate::config::render::RENDER_READY_MARKER), "fp").unwrap();
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
        // 自审 minor（115）：就绪判据 = 渲染完成标记（settings.json 在装包前就写了）
        std::fs::write(d.join(crate::config::render::RENDER_READY_MARKER), "fp").unwrap();
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

// ---------- 2026-10-27 自审 major：渲染单飞 + 提交合并 ----------

#[test]
fn concurrent_submits_coalesce_instead_of_spawning_a_second_render() {
    // 错实现形态：在途时另起渲染线程（两个线程非原子写同一 profiles_root）→ 红
    let gate = RenderGate::new();
    assert!(gate.try_begin_or_coalesce(), "首个提交应取得渲染权");
    assert!(gate.is_in_flight());
    assert!(!gate.try_begin_or_coalesce(), "在途时第二个提交必须被合并");
    assert!(gate.is_in_flight(), "合并不得改变在途计数");
}

#[test]
fn a_coalesced_submit_is_never_lost() {
    // 错实现形态：判完 dirty 到归零之间有窗口 → 合并进来的提交永久丢掉
    //（配置提交了但 profile 永不更新、无报错）→ 本用例必须变红
    let gate = RenderGate::new();
    assert!(gate.try_begin_or_coalesce());
    assert!(!gate.try_begin_or_coalesce(), "提交 A 合并进在跑的轮");
    assert!(gate.finish_round_or_continue(), "轮末必须发现合并请求并再跑一轮");
    assert!(
        gate.is_in_flight(),
        "重渲期间仍算在途（不得出现空窗让别的线程插队）"
    );
    assert!(!gate.finish_round_or_continue(), "无新提交时收工");
    assert!(!gate.is_in_flight(), "收工后归零");
}

#[test]
fn idle_gate_grants_the_render_to_the_next_submit() {
    // 收工后新提交必须拿到渲染权（否则之后的配置永不生效）
    let gate = RenderGate::new();
    assert!(gate.try_begin_or_coalesce());
    assert!(!gate.finish_round_or_continue());
    assert!(gate.try_begin_or_coalesce(), "收工后的新提交应取得渲染权");
    assert!(gate.is_in_flight());
}
