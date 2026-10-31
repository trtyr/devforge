//! 旋钮生效值端点单测（F14/T205 ftask-1）：声明表全量覆盖 + 来源标注。

#[allow(unused_imports)]
use super::*;

/// 端点照出全部旋钮；file 覆盖可见、未覆盖=default（「这值哪来的」可答）。
///
/// 写成同步用例 + 自建 runtime：旋钮锁必须覆盖整段并发窗口，而 `MutexGuard`
/// 不得跨 await（clippy::await_holding_lock）——锁在同步帧里持有，异步段只跑
/// HTTP 往返（与 pending.rs 的 rounds 用例同一手法）。
#[test]
fn knob_effective_endpoint_lists_all_knobs_with_sources() {
    let _guard = crate::config::knobs::test_lock(); // 全局旋钮单例：并行用例整段串行
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("测试 runtime");
    rt.block_on(async {
        let f = fixture();
        crate::config::knobs::init(Some(&serde_json::json!({ "max_concurrent": 1 })));
        let resp = send(&f.router, "GET", "/api/v1/config/knobs", None).await;
        assert_eq!(resp.status(), 200);
        let body = body_json(resp).await;
        let knobs = body["knobs"].as_array().expect("knobs 应为数组");
        assert!(!knobs.is_empty());
        let mc = knobs
            .iter()
            .find(|k| k["key"] == "max_concurrent")
            .expect("应有 max_concurrent");
        assert_eq!(mc["value"], 1);
        assert_eq!(mc["source"], "file");
        assert!(!mc["doc"].as_str().unwrap_or_default().is_empty(), "带语义说明");
        let stall = knobs
            .iter()
            .find(|k| k["key"] == "stall_warn_ms")
            .expect("应有 stall_warn_ms");
        assert_eq!(stall["source"], "default");
    });
    crate::config::knobs::init(None); // 还原，不污染其他用例
}
