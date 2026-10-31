//! A9 SOP 只读序列化单测：全量清单 + agent 出场表守卫（映射名必须存在于
//! manifest.agents——编排器改名时这里红，防漂移）。

#[allow(unused_imports)]
use super::*;

use std::collections::BTreeSet;

/// 出场映射守卫：每个 SOP 的 agents 必须都是 manifest.agents 的真名（空出场 =
/// 声明缺失也要红——7 个 SOP 全部应有出场）。
#[tokio::test]
async fn sops_endpoint_lists_all_with_agent_appearances() {
    let f = fixture();
    let manifest_agents: BTreeSet<String> =
        f.state.manifest.lock().unwrap().agents.iter().map(|a| a.name.clone()).collect();

    let resp = send(&f.router, "GET", "/api/v1/sops", None).await;
    assert_eq!(resp.status(), 200);
    let body = body_json(resp).await;
    let sops = body["sops"].as_array().unwrap();

    // 全量：registry 7 个内置 SOP
    assert_eq!(sops.len(), 7, "内置 SOP 全量：{sops:?}");

    let known_ids: BTreeSet<&str> = [
        "planning",
        "coding",
        "testing",
        "doc-fix",
        "doc-from-zero",
        "change",
        "code-review",
    ]
    .into_iter()
    .collect();

    for s in sops {
        let id = s["id"].as_str().unwrap();
        assert!(known_ids.contains(id), "SOP id 在内置集合内：{id}");
        assert!(s["name"].as_str().is_some_and(|v| !v.is_empty()));
        assert!(s["description"].as_str().is_some_and(|v| !v.is_empty()));
        let agents = s["agents"].as_array().unwrap_or_else(|| panic!("{id} 缺 agents"));
        assert!(!agents.is_empty(), "{id} 的出场表不得为空（声明缺失也要红）");
        for a in agents {
            let name = a.as_str().unwrap();
            assert!(
                manifest_agents.contains(name),
                "出场 agent `{name}`（SOP {id}）必须是 manifest.agents 的真名——编排器改名未同步映射"
            );
        }
    }

    // change 是最长的编排（6 agent）
    let change = sops.iter().find(|s| s["id"] == "change").unwrap();
    assert_eq!(change["agents"].as_array().unwrap().len(), 6);
}
