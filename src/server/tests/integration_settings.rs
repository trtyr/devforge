//! A10 集成开关端点单测：GET 缺失回落全 off、PUT 写后读一致、PUT 挂写审计、
//! 损坏文件 GET 不炸。

#[allow(unused_imports)]
use super::*;

/// GET：文件缺失 → 全 off 缺省（ui-9「文件不存在 = 功能不存在」）。
#[tokio::test]
async fn get_returns_all_off_when_file_missing() {
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/integrations/settings", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["auto_integrate_enabled"], false);
    assert_eq!(body["gates"]["clippy"], false);
    assert_eq!(body["gates"]["doc_lint"], false);
}

/// PUT → GET 往返一致；文件真落在 plans_root/integrations/settings.json；
/// PUT 挂写操作审计。
#[tokio::test]
async fn put_then_get_roundtrips_and_audits() {
    let f = fixture();
    let resp = send(
        &f.router,
        "PUT",
        "/api/v1/integrations/settings",
        Some(json!({
            "auto_integrate_enabled": true,
            "gates": {"clippy": true, "doc_lint": false}
        })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    assert_eq!(body["auto_integrate_enabled"], true);
    assert_eq!(body["gates"]["clippy"], true);

    // 文件真实落盘（plans_root/integrations/settings.json）
    let file = f.state.plans_root.join("integrations").join("settings.json");
    assert!(file.exists(), "settings.json 应落盘：{}", file.display());
    let on_disk = crate::integration::settings::load(&file);
    assert!(on_disk.auto_integrate_enabled);
    assert!(on_disk.gates.clippy);
    assert!(!on_disk.gates.doc_lint);

    // GET 读回一致
    let body = body_json(send(&f.router, "GET", "/api/v1/integrations/settings", None).await).await;
    assert_eq!(body["auto_integrate_enabled"], true);
    assert_eq!(body["gates"]["doc_lint"], false);

    // 写审计
    let store = f.state.store.lock().unwrap();
    let last = store.auth_events.last().unwrap();
    assert_eq!(last.kind, "write_op");
    assert!(last.detail.contains("put_integration_settings"), "{}", last.detail);
    assert!(last.detail.contains("auto_integrate=true"), "{}", last.detail);
}

/// 损坏文件 GET 不炸（回落全 off）；PUT 覆盖损坏文件后恢复正常。
#[tokio::test]
async fn corrupt_file_falls_back_then_put_repairs() {
    let f = fixture();
    let file = f.state.plans_root.join("integrations").join("settings.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{{{ 损坏").unwrap();

    let resp = send(&f.router, "GET", "/api/v1/integrations/settings", None).await;
    assert_eq!(resp.status(), 200, "损坏文件 GET 不炸");
    let body = body_json(resp).await;
    assert_eq!(body["auto_integrate_enabled"], false);

    let resp = send(
        &f.router,
        "PUT",
        "/api/v1/integrations/settings",
        Some(json!({
            "auto_integrate_enabled": true,
            "gates": {"clippy": false, "doc_lint": false}
        })),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let on_disk = crate::integration::settings::load(&file);
    assert!(on_disk.auto_integrate_enabled, "PUT 覆盖损坏文件后恢复正常");
}
