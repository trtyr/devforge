//! A3 agent 配置只读端点单测：字段齐全性、模型池解引用、无 key 字段、SOP 清单。

#[allow(unused_imports)]
use super::*;

/// 端点照出 agents（字段齐全 + 模型解引用）+ sops 清单 + 模型池；无 key 类字段。
#[tokio::test]
async fn agents_config_lists_agents_with_model_resolution() {
    let f = fixture();
    let resp = send(&f.router, "GET", "/api/v1/config/agents", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;

    // agents 数量与 manifest 一致，字段齐全
    let expected = f.state.manifest.lock().unwrap().agents.len();
    let agents = body["agents"].as_array().expect("agents 数组");
    assert_eq!(agents.len(), expected);
    for a in agents {
        assert!(a["name"].as_str().is_some_and(|v| !v.is_empty()));
        assert!(a["role_prompt"].as_str().is_some(), "描述（role_prompt 全文）");
        assert!(a["extra_packages"].is_array());
        assert!(a["skills"].is_array());
        assert!(a["model"].as_str().is_some(), "模型绑定 id");
        // 模型解引用：model_detail 要么是池条目（id/provider/model_id/base_url），要么 null
        if let Some(d) = a["model_detail"].as_object() {
            assert!(d["provider"].is_string());
            assert!(d["model_id"].is_string());
            assert_eq!(d["id"], a["model"], "池条目 id 与绑定一致");
        }
        // 无 key 类字段（本视图不出现 api_key）
        assert!(a.get("api_key").is_none());
    }

    // sops 清单（剧本规划页复用）：id/name/description
    let sops = body["sops"].as_array().expect("sops 数组");
    assert!(!sops.is_empty(), "内置 SOP 至少 planning/coding/testing");
    for s in sops {
        assert!(s["id"].as_str().is_some_and(|v| !v.is_empty()));
        assert!(s["description"].as_str().is_some());
    }

    // 模型池全量（换模型候选列表）
    let pool = body["model_pool"].as_array().expect("model_pool 数组");
    let expected_pool = f.state.manifest.lock().unwrap().model_pool.len();
    assert_eq!(pool.len(), expected_pool);

    // 无 key 类字段（全局）
    let raw = serde_json::to_string(&body).unwrap();
    assert!(!raw.contains("api_key"), "A3 视图不得出现 api_key 字段：{raw}");
}
