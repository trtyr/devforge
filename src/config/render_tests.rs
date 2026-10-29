//! （ARC-R7 自 render.rs mod tests 平移，纯移动。）

use super::*;
use crate::config::installer::FakeInstaller;
use crate::config::manifest::ConfigManifest;
use crate::error::DevforgeError;

fn renderer(manifest: &ConfigManifest) -> Renderer<'_> {
    Renderer {
        manifest,
        installer: &FakeInstaller,
        skills_source: None,
    }
}

fn base_packages() -> Vec<String> {
    crate::config::manifest::BASE_PACKAGES
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn render_creates_directory_structure() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();

    assert_eq!(profile, root.path().join("plan-agent"));
    // AGENTS.md = role_prompt
    let agents_md = fs::read_to_string(profile.join("AGENTS.md")).unwrap();
    assert_eq!(agents_md, manifest.get_agent("plan-agent").unwrap().role_prompt);
    // settings.json 骨架来自模型池
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(profile.join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["defaultProvider"], "newapi");
    assert_eq!(settings["defaultModel"], "mokeywork/deepseek-flash");
    // skills：无源时用内置资产（P2-ISSUE-05 修复，不再占位）
    let skill_body = fs::read_to_string(profile.join("skills/plan-tree/SKILL.md")).unwrap();
    assert!(skill_body.starts_with("---"));
    assert!(skill_body.contains("目录约定"));
}

#[test]
fn render_empty_mcp_removes_stale_mcp_json() {
    // DR-12 迁移路径：清单把某 agent 的 mcp 清空后重渲染，旧 mcp.json 必须被清掉，
    // 否则「空 = 无 MCP」的语义对已存在的 profile 永远不生效
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = root.path().join("doc-review");
    fs::create_dir_all(&profile).unwrap();
    fs::write(
        profile.join("mcp.json"),
        r#"{"mcpServers":{"codegraph":{"command":"codegraph"}}}"#,
    )
    .unwrap();

    renderer(&manifest)
        .render_agent(root.path(), "doc-review")
        .unwrap();
    assert!(
        !profile.join("mcp.json").exists(),
        "mcp 清空后旧 mcp.json 必须被移除"
    );
}

#[test]
fn doc_contract_skill_renders_into_coding_profile() {
    // P008 T000a：契约必须真落到 profile 的 skills/ 下（而不只写在清单里）
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let r = Renderer {
        manifest: &manifest,
        installer: &FakeInstaller,
        skills_source: None,
    };
    let profile = r.render_agent(root.path(), "coding-agent").unwrap();
    let body = fs::read_to_string(profile.join("skills/doc-contract/SKILL.md")).unwrap();
    assert!(body.starts_with("---"));
    assert!(body.contains("必填"));
}

#[test]
fn dual_records_carry_base_plus_extra() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();

    // 记录 1：settings.json packages = 四件套 + ask-user-question
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(profile.join("settings.json")).unwrap())
            .unwrap();
    let pkgs: Vec<&str> = settings["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(pkgs.len(), 5, "四件套+1 差异化: {pkgs:?}");
    assert!(pkgs.contains(&"npm:@juicesharp/rpiv-ask-user-question"));

    // 记录 2：npm/package.json dependencies 同步
    let pkg_json: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(profile.join("npm/package.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(pkg_json["dependencies"].as_object().unwrap().len(), 5);
}

// ---------- CAL-19-RENDER：并发渲染与幂等跳过 ----------

#[derive(Default)]
struct CountingInstaller(AtomicUsize);

impl PackageInstaller for CountingInstaller {
    fn install(&self, dir: &Path, packages: &[String]) -> Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        FakeInstaller.install(dir, packages)
    }
}

struct FailingInstaller;

impl PackageInstaller for FailingInstaller {
    fn install(&self, _dir: &Path, packages: &[String]) -> Result<()> {
        Err(DevforgeError::Config(format!(
            "故意失败（{} 个包）",
            packages.len()
        )))
    }
}

/// 自审 major 回归：render_all 的并发契约必须有真断言——旧「parallel」测试
/// 只验顺序与计数，把 render_all 改成串行实现 27 个测试仍全绿。
/// 会合点设计：4 路各等 in_flight 达到 4 再放行——真并发恰好 4、串行实现
/// 等不到（3s 超时放行让用例失败而非挂死）。
#[test]
fn render_all_actually_runs_concurrently_up_to_cap() {
    use std::time::{Duration, Instant};

    struct RendezvousInstaller {
        in_flight: std::sync::atomic::AtomicUsize,
        max_in_flight: std::sync::atomic::AtomicUsize,
        inner: FakeInstaller,
    }
    impl PackageInstaller for RendezvousInstaller {
        fn install(&self, profile_dir: &Path, packages: &[String]) -> Result<()> {
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            // 数字 4 对应 render.rs 的 MAX_RENDER_CONCURRENCY=4（私有常量，改动时同步）
            let deadline = Instant::now() + Duration::from_secs(3);
            while self.in_flight.load(Ordering::SeqCst) < 4 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let r = self.inner.install(profile_dir, packages);
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            r
        }
    }

    let manifest = ConfigManifest::builtin();
    assert!(
        manifest.agents.len() >= 4,
        "内置 agent 数须 ≥ 并发上限，本测试前提不成立"
    );
    let root = tempfile::tempdir().unwrap();
    let inst = RendezvousInstaller {
        in_flight: std::sync::atomic::AtomicUsize::new(0),
        max_in_flight: std::sync::atomic::AtomicUsize::new(0),
        inner: FakeInstaller,
    };
    let r = Renderer {
        manifest: &manifest,
        installer: &inst,
        skills_source: None,
    };
    r.render_all(root.path()).unwrap();
    let max = inst.max_in_flight.load(Ordering::SeqCst);
    assert_eq!(
        max, 4,
        "render_all 必须真并发到上限（实测峰值在飞 {max}）——CAL-19-RENDER"
    );
}

#[test]
fn render_all_parallel_preserves_manifest_order() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let counter = CountingInstaller::default();
    let r = Renderer {
        manifest: &manifest,
        installer: &counter,
        skills_source: None,
    };
    let rendered = r.render_all(root.path()).unwrap();
    let expected: Vec<PathBuf> = manifest
        .agents
        .iter()
        .map(|a| root.path().join(&a.name))
        .collect();
    assert_eq!(rendered, expected, "并发渲染返回值须与清单顺序一致");
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        manifest.agents.len(),
        "每个 Agent 恰好装一次"
    );
    for a in &manifest.agents {
        assert!(
            root.path().join(&a.name).join("AGENTS.md").exists(),
            "{} 渲染缺失",
            a.name
        );
    }
}

#[test]
fn render_all_propagates_install_failure() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let r = Renderer {
        manifest: &manifest,
        installer: &FailingInstaller,
        skills_source: None,
    };
    let err = r.render_all(root.path()).unwrap_err();
    assert!(
        matches!(err, DevforgeError::Config(_)),
        "安装失败必须上抛而非吞掉: {err}"
    );
}

#[test]
fn second_render_skips_install_when_packages_already_present() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let counter = CountingInstaller::default();
    let r = Renderer {
        manifest: &manifest,
        installer: &counter,
        skills_source: None,
    };
    r.render_agent(root.path(), "plan-agent").unwrap();
    let after_first = counter.0.load(Ordering::SeqCst);
    r.render_agent(root.path(), "plan-agent").unwrap();
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        after_first,
        "已装齐（settings + npm 双记录）不得重装"
    );
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.path().join("plan-agent/settings.json")).unwrap(),
    )
    .unwrap();
    assert!(
        !settings["packages"].as_array().unwrap().is_empty(),
        "跳过安装时 packages 记录必须回写进 settings"
    );
}

/// 自审 major 回归：撤包方向——manifest 移除包后重渲染，settings 与 npm 依赖
/// 都不得残留已撤包（旧口径只抹 settings，npm 依赖留着，双记录自此自相矛盾）。
#[test]
fn package_removal_reconciles_both_records() {
    let mut manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let counter = CountingInstaller::default();
    let removed = "npm:pi-to-be-removed";
    manifest
        .agents
        .iter_mut()
        .find(|a| a.name == "plan-agent")
        .unwrap()
        .extra_packages
        .push(removed.into());
    let r = Renderer {
        manifest: &manifest,
        installer: &counter,
        skills_source: None,
    };
    r.render_agent(root.path(), "plan-agent").unwrap();

    // 撤包：从清单移除，重渲染（期望集回落到 base_packages）
    let desired = manifest.base_packages.clone();
    // r 的不可变借用已随最后一次使用（render_agent）结束（NLL），可以直接改清单
    manifest
        .agents
        .iter_mut()
        .find(|a| a.name == "plan-agent")
        .unwrap()
        .extra_packages
        .clear();
    let before = counter.0.load(Ordering::SeqCst);
    let r = Renderer {
        manifest: &manifest,
        installer: &counter,
        skills_source: None,
    };
    r.render_agent(root.path(), "plan-agent").unwrap();

    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.path().join("plan-agent/settings.json")).unwrap(),
    )
    .unwrap();
    let pkgs: Vec<&str> = settings["packages"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        !pkgs.iter().any(|p| p.contains("pi-to-be-removed")),
        "settings 不得残留已撤包：{pkgs:?}"
    );
    let npm: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(root.path().join("plan-agent/npm/package.json")).unwrap(),
    )
    .unwrap();
    let deps = npm["dependencies"].as_object().unwrap();
    assert!(
        !deps.keys().any(|k| k.contains("pi-to-be-removed")),
        "npm 依赖不得残留已撤包（双记录对账）：{deps:?}"
    );
    assert_eq!(
        deps.len(),
        desired.len(),
        "npm 依赖应恰好等于清单期望集：{deps:?}"
    );
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        before,
        "撤包不需要重装（对账即可）"
    );
}

#[test]
fn manifest_package_change_forces_reinstall() {
    let mut manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let counter = CountingInstaller::default();
    Renderer {
        manifest: &manifest,
        installer: &counter,
        skills_source: None,
    }
    .render_agent(root.path(), "plan-agent")
    .unwrap();
    let before = counter.0.load(Ordering::SeqCst);
    manifest
        .agents
        .iter_mut()
        .find(|a| a.name == "plan-agent")
        .unwrap()
        .extra_packages
        .push("npm:pi-newpkg".into());
    Renderer {
        manifest: &manifest,
        installer: &counter,
        skills_source: None,
    }
    .render_agent(root.path(), "plan-agent")
    .unwrap();
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        before + 1,
        "清单新增包 → 已装记录不覆盖 → 必须重装"
    );
}

#[test]
fn missing_npm_record_forces_reinstall() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let counter = CountingInstaller::default();
    let r = Renderer {
        manifest: &manifest,
        installer: &counter,
        skills_source: None,
    };
    r.render_agent(root.path(), "plan-agent").unwrap();
    let before = counter.0.load(Ordering::SeqCst);
    fs::remove_file(root.path().join("plan-agent/npm/package.json")).unwrap();
    r.render_agent(root.path(), "plan-agent").unwrap();
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        before + 1,
        "双记录缺失（目录损坏）必须重装"
    );
}

#[test]
fn review_agent_gets_no_extra_packages() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-review").unwrap();
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(profile.join("settings.json")).unwrap())
            .unwrap();
    let pkgs = settings["packages"].as_array().unwrap();
    assert_eq!(pkgs.len(), 4, "review 无差异化包: {pkgs:?}");
}

#[test]
fn review_profiles_render_reduced_thinking_level() {
    // 性能剖析 2026-10-19：审查官每轮 11-22s（输出 token 占大头）→ 降 medium
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    for (agent, want) in [
        ("coding-review", "medium"),
        ("test-review", "medium"),
        ("doc-review", "medium"),
        ("plan-review", "medium"),
        ("coding-agent", "high"),
        ("test-agent", "high"),
    ] {
        let dir = renderer(&manifest).render_agent(root.path(), agent).unwrap();
        let settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            settings["defaultThinkingLevel"], want,
            "{agent} 思考档位应为 {want}"
        );
    }
}

#[test]
fn api_key_written_provider_keyed() {
    let mut manifest = ConfigManifest::builtin();
    manifest.api_key = Some("sk-test-123".into());
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    let auth: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(profile.join("auth.json")).unwrap()).unwrap();
    // P2-ISSUE-01 修复：pi 期望 provider 键控形状
    assert_eq!(auth["newapi"]["type"], "api_key");
    assert_eq!(auth["newapi"]["key"], "sk-test-123"); // 明文，decisions/001
    assert!(auth.get("api_key").is_none(), "旧错误形状不得残留");
}

#[test]
fn models_json_carries_provider_and_budget() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    let models: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(profile.join("models.json")).unwrap()).unwrap();
    // P2-ISSUE-03 修复：baseUrl + 窗口/预算
    let newapi = &models["providers"]["newapi"];
    assert_eq!(newapi["api"], "openai-completions");
    assert_eq!(newapi["baseUrl"], "https://newapi.trtyr.top/v1");
    let list = newapi["models"].as_array().unwrap();
    assert_eq!(list.len(), 4, "池内同 provider 的模型全量分发（含 person-deepseek-flash）");
    assert!(
        list.iter()
            .any(|m| m["id"] == "MiniMax-M3.1-Flash" && m["contextWindow"] == 1_000_000),
        "doc-agent 用的 MiniMax 必须在 models.json 里: {list:?}"
    );
    assert!(list
        .iter()
        .any(|m| m["id"] == "mokeywork/deepseek-flash"
            && m["contextWindow"] == 1_000_000
            && m["maxTokens"] == 65_536));
}

#[test]
fn settings_carry_runtime_keys() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(profile.join("settings.json")).unwrap(),
    )
    .unwrap();
    // P2-ISSUE-04 修复：运行时生存配置
    assert_eq!(settings["retry"]["maxRetries"], 3);
    assert_eq!(settings["retry"]["baseDelayMs"], 5_000);
    // 错实现形态：漏发 `retry.provider.timeoutMs`（中转挂起时无请求绝对上界，agent 挂死）
    assert_eq!(settings["retry"]["provider"]["timeoutMs"], 600_000);
    assert_eq!(settings["httpIdleTimeoutMs"], 60_000);
    assert_eq!(settings["defaultThinkingLevel"], "high");
    assert_eq!(settings["hideThinkingBlock"], true);
    assert_eq!(settings["compaction"]["enabled"], false);
    assert_eq!(settings["compaction"]["reserveTokens"], 16_384);
    assert_eq!(settings["compaction"]["keepRecentTokens"], 20_000);
    // 模型层键共存
    assert_eq!(settings["defaultProvider"], "newapi");
    assert_eq!(settings["defaultModel"], "mokeywork/deepseek-flash");
}

/// 清单 JSON（取自 builtin）改出「runtime 段只写一个字段」的形状——部分段/旧清单升级场景。
fn manifest_json_with_runtime(runtime: serde_json::Value) -> serde_json::Value {
    let mut v = serde_json::to_value(ConfigManifest::builtin()).unwrap();
    v["runtime"] = runtime;
    v
}

/// 错实现形态：runtime 段只写部分字段时，其余字段回落到零值/空串（serde 缺省丢失）
/// → 产物 retry 键值错、思维层错。
#[test]
fn a_partial_runtime_segment_fills_the_pinned_defaults() {
    let v = manifest_json_with_runtime(serde_json::json!({ "provider_timeout_ms": 90_000 }));
    let manifest: ConfigManifest = serde_json::from_value(v).unwrap();
    assert_eq!(manifest.runtime.max_retries, 3);
    assert_eq!(manifest.runtime.base_delay_ms, 5_000);
    assert_eq!(manifest.runtime.provider_timeout_ms, 90_000);
    assert_eq!(manifest.runtime.http_idle_timeout_ms, 60_000);
    assert_eq!(manifest.runtime.default_thinking_level, "high");
    assert!(manifest.runtime.hide_thinking_block);
    assert!(!manifest.runtime.compaction.enabled);
    assert_eq!(manifest.runtime.compaction.reserve_tokens, 16_384);
    assert_eq!(manifest.runtime.compaction.keep_recent_tokens, 20_000);

    // 缺省填好后渲染产物照旧带齐三键（没裸奔）
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(profile.join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["retry"]["maxRetries"], 3);
    assert_eq!(settings["retry"]["baseDelayMs"], 5_000);
    assert_eq!(settings["retry"]["provider"]["timeoutMs"], 90_000);
}

/// 错实现形态：渲染器仍写死缺省常量（操作员改清单无效）→ 产物三键仍是 3/5000/600000。
#[test]
fn runtime_key_values_come_from_the_manifest_segment() {
    let mut manifest = ConfigManifest::builtin();
    manifest.runtime.max_retries = 7;
    manifest.runtime.base_delay_ms = 1_234;
    manifest.runtime.provider_timeout_ms = 90_000;
    manifest.runtime.http_idle_timeout_ms = 30_000;
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(profile.join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["retry"]["maxRetries"], 7);
    assert_eq!(settings["retry"]["baseDelayMs"], 1_234);
    assert_eq!(settings["retry"]["provider"]["timeoutMs"], 90_000);
    assert_eq!(settings["httpIdleTimeoutMs"], 30_000);
}

/// 错实现形态：思维层三键仍写死在渲染器（清单改不动）→ 产物三键不变。
#[test]
fn the_thinking_layer_is_distributed_from_the_runtime_segment() {
    let mut manifest = ConfigManifest::builtin();
    manifest.runtime.default_thinking_level = "medium".into();
    manifest.runtime.compaction.enabled = true;
    manifest.runtime.hide_thinking_block = false;
    let root = tempfile::tempdir().unwrap();
    // plan-agent 自身无档位覆盖（thinking_level = None）→ 落到 runtime 段的全局档位
    let profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(profile.join("settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["defaultThinkingLevel"], "medium");
    assert_eq!(settings["compaction"]["enabled"], true);
    assert_eq!(settings["hideThinkingBlock"], false);
}

/// 错实现形态：优先级反过来（runtime 全局盖过 Agent 覆盖）→ 审查 Agent 的 medium 档位丢失。
#[test]
fn the_agent_thinking_level_wins_over_the_runtime_segment() {
    let mut manifest = ConfigManifest::builtin();
    manifest.runtime.default_thinking_level = "low".into();
    let root = tempfile::tempdir().unwrap();
    // 审查 Agent 自身有档位覆盖（Some("medium")）→ Agent 覆盖赢
    let review = renderer(&manifest).render_agent(root.path(), "plan-review").unwrap();
    let rsettings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(review.join("settings.json")).unwrap()).unwrap();
    assert_eq!(rsettings["defaultThinkingLevel"], "medium");
    // 无档位覆盖的 Agent（None）→ 落到 runtime 段的值
    let agent = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(agent.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["defaultThinkingLevel"], "low");
}

/// 错实现形态：第二次渲染丢了运行时层或叠了重复条目 → 两份产物不再相等。
#[test]
fn rendering_the_same_manifest_twice_is_byte_identical() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let r = renderer(&manifest);
    r.render_agent(root.path(), "plan-agent").unwrap();
    let first = fs::read_to_string(root.path().join("plan-agent/settings.json")).unwrap();
    r.render_agent(root.path(), "plan-agent").unwrap();
    let second = fs::read_to_string(root.path().join("plan-agent/settings.json")).unwrap();
    assert_eq!(first, second, "同一清单连续渲染两次，settings.json 必须逐字节相等");

    let settings: serde_json::Value = serde_json::from_str(&second).unwrap();
    let retry = settings["retry"].as_object().unwrap();
    assert_eq!(retry.len(), 3, "retry 下恰好三键，不得重复堆叠: {retry:?}");
    let pkgs: Vec<&str> = settings["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(!pkgs.is_empty(), "packages 记录不得丢失");
    let mut uniq = pkgs.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(uniq.len(), pkgs.len(), "packages 不得出现重复项: {pkgs:?}");
}

/// 错实现形态：渲染器只做增量合并（把手工键/被改坏的值留在产物里）
/// → 注入键仍在、timeoutMs 仍是 1。
#[test]
fn re_rendering_drops_hand_injected_keys_and_restores_manifest_values() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    renderer(&manifest)
        .render_agent(root.path(), "plan-agent")
        .unwrap();
    let path = root.path().join("plan-agent/settings.json");

    // 手工注入：自定义键 + 把请求绝对超时改坏
    let mut injected: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    injected["handInjectedKey"] = serde_json::json!("hand-made");
    injected["retry"]["provider"]["timeoutMs"] = serde_json::json!(1);
    fs::write(&path, serde_json::to_string_pretty(&injected).unwrap()).unwrap();

    renderer(&manifest)
        .render_agent(root.path(), "plan-agent")
        .unwrap();
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(
        after.get("handInjectedKey").is_none(),
        "手工注入的键必须在重渲染后消失（渲染器独占）：{after}"
    );
    assert_eq!(
        after["retry"]["provider"]["timeoutMs"], 600_000,
        "被改坏的请求绝对超时必须恢复为清单值"
    );
    assert!(
        !after["packages"].as_array().unwrap().is_empty(),
        "packages 记录必须在重渲染后仍在"
    );
}

#[test]
fn prompts_carry_protocol_and_workflow() {
    let manifest = ConfigManifest::builtin();
    let review = manifest.get_agent("plan-review").unwrap();
    assert!(review.role_prompt.contains("<verdict>PASS</verdict>"));
    assert!(review.role_prompt.contains("<verdict>FAIL</verdict>"));
    assert!(review.role_prompt.contains("快速审查"));
    let plan = manifest.get_agent("plan-agent").unwrap();
    assert!(plan.role_prompt.contains("小步快跑"));
}

#[test]
fn skills_copied_from_source_when_given() {
    let manifest = ConfigManifest::builtin();
    // 造假 skill 源
    let skill_src = tempfile::tempdir().unwrap();
    let plan_tree = skill_src.path().join("plan-tree");
    fs::create_dir_all(&plan_tree).unwrap();
    fs::write(plan_tree.join("SKILL.md"), "# plan-tree skill 正文").unwrap();

    let root = tempfile::tempdir().unwrap();
    let r = Renderer {
        manifest: &manifest,
        installer: &FakeInstaller,
        skills_source: Some(skill_src.path()),
    };
    let profile = r.render_agent(root.path(), "plan-agent").unwrap();
    let copied = fs::read_to_string(profile.join("skills/plan-tree/SKILL.md")).unwrap();
    assert_eq!(copied, "# plan-tree skill 正文");
}

#[test]
fn render_all_covers_every_agent() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let rendered = renderer(&manifest).render_all(root.path()).unwrap();
    assert_eq!(
        rendered.len(),
        16,
        "规划双 + 编码双 + 测试双 + 文档三 + 变更规划双 + 变更编码双 + 变更测试双 + 审计单（P007/P008/P009/P013）"
    );
    for name in [
        "plan-agent",
        "plan-review",
        "coding-agent",
        "coding-review",
        "test-agent",
        "test-review",
        "doc-fix",
        "doc-agent",
        "doc-review",
    ] {
        assert!(root.path().join(name).is_dir());
    }
}

#[test]
fn coding_agent_profile_has_mcp_json() {
    // P005 T003：pi 原生 schema，codegraph（含遥测关闭）+ context7
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "coding-agent").unwrap();
    let mcp: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(profile.join("mcp.json")).unwrap()).unwrap();
    let cg = &mcp["mcpServers"]["codegraph"];
    assert_eq!(cg["command"], "codegraph");
    assert_eq!(cg["args"][0], "serve");
    assert_eq!(cg["args"][1], "--mcp");
    assert_eq!(cg["env"]["CODEGRAPH_TELEMETRY"], "0"); // decisions/013 卫生条款
    let c7 = &mcp["mcpServers"]["context7"];
    assert_eq!(c7["command"], "npx");
    assert!(c7["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "@upstash/context7-mcp"));
    // 规划双 Agent 无 MCP：不生成文件（P001 行为不回归）
    let plan_profile = renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    assert!(!plan_profile.join("mcp.json").exists());
}

#[test]
fn coding_packages_carry_todo_and_lens() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let profile = renderer(&manifest).render_agent(root.path(), "coding-agent").unwrap();
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(profile.join("settings.json")).unwrap(),
    )
    .unwrap();
    let pkgs: Vec<&str> = settings["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(pkgs.len(), 7, "四件套+ask+todo+lens: {pkgs:?}");
    for must in [
        "npm:@juicesharp/rpiv-ask-user-question",
        "npm:@juicesharp/rpiv-todo",
        "npm:pi-lens",
    ] {
        assert!(pkgs.contains(&must), "缺 {must}: {pkgs:?}");
    }
    // review 只有诊断层，无 todo/ask（无状态一轮）
    let review = renderer(&manifest).render_agent(root.path(), "coding-review").unwrap();
    let rsettings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(review.join("settings.json")).unwrap(),
    )
    .unwrap();
    let rpkgs: Vec<&str> = rsettings["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(rpkgs.len(), 5, "四件套+lens: {rpkgs:?}");
    assert!(rpkgs.contains(&"npm:pi-lens"));
    assert!(!rpkgs.contains(&"npm:@juicesharp/rpiv-todo"));
}

#[test]
fn coding_prompts_carry_protocol_and_discipline() {
    let manifest = ConfigManifest::builtin();
    let coder = manifest.get_agent("coding-agent").unwrap();
    // 阶段完成协议（T002 schema 对齐）
    assert!(coder.role_prompt.contains("<stage-complete>N</stage-complete>"));
    // 决策链锚点抽查：伴写/单测禁 e2e/git/偏差留痕/codegraph-first/刚需
    assert!(coder.role_prompt.contains("严禁写端到端测试"));
    assert!(coder.role_prompt.contains("绝不 push"));
    assert!(coder.role_prompt.contains("coding/blockers/"));
    assert!(coder.role_prompt.contains("projectPath"));
    assert!(coder.role_prompt.contains("AI-friendly"));
    assert!(coder.role_prompt.contains("勘察"));
    let reviewer = manifest.get_agent("coding-review").unwrap();
    assert!(reviewer.role_prompt.contains("<stage-verdict>PASS或FAIL</stage-verdict>"));
    assert!(reviewer.role_prompt.contains("<next>fix或advance或finish</next>"));
    assert!(reviewer.role_prompt.contains("完全基于文档写测试"));
    assert!(reviewer.role_prompt.contains("对照实际代码与文档"));
}

#[test]
fn unknown_agent_rejected() {
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    let err = renderer(&manifest)
        .render_agent(root.path(), "ghost")
        .unwrap_err();
    assert!(matches!(err, DevforgeError::Config(_)));
}

#[test]
fn base_packages_exactly_four_verbatim() {
    assert_eq!(base_packages().len(), 4);
    assert!(base_packages().contains(&"git:github.com/NVlabs/SoL-Pi".to_string()));
}

// ---------- P012 · pathguard 配置渲染（topic 07） ----------

#[test]
fn path_permissions_render_to_pathguard_json_with_escalation_guard() {
    // deny-first 口径的形状验证：deny 原样进档；profile 目录恒追加在 deny_write（防提权）
    let mut manifest = ConfigManifest::builtin();
    let target = "plan-agent";
    let agent = manifest
        .agents
        .iter_mut()
        .find(|a| a.name == target)
        .expect("内置清单应有 plan-agent");
    agent.path_permissions = Some(crate::config::manifest::PathPermissions {
        read: vec!["**".into()],
        write: vec!["src/**".into(), "docs/**".into()],
        deny_read: vec![".env".into(), "**/secrets/**".into()],
        deny_write: vec!["assets/prompts/**".into()],
    });
    let root = tempfile::tempdir().unwrap();
    renderer(&manifest)
        .render_agent(root.path(), target)
        .expect("渲染应成功");

    let raw = std::fs::read_to_string(
        root.path().join(target).join(".pi").join("pathguard.json"),
    )
    .expect("pathguard.json 应存在");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["read"][0], "**");
    assert_eq!(v["write"][1], "docs/**");
    assert_eq!(v["deny_read"][0], ".env");
    assert_eq!(v["deny_write"][0], "assets/prompts/**");
    // 提权防护：profile 目录恒在 deny_write（插件不可改自己的边界）
    let profile_dir = root.path().join(target);
    let dir_str = profile_dir.to_string_lossy().to_string();
    let deny_write = v["deny_write"].as_array().unwrap();
    assert!(
        deny_write
            .iter()
            .any(|d| d.as_str().unwrap().starts_with(&format!("{dir_str}/**"))),
        "profile 目录应在 deny_write：{raw}"
    );
}

#[test]
fn agents_without_path_permissions_get_no_pathguard_file() {
    // 老清单兼容：无 pathPermissions → 不生成 pathguard.json（存量 profile 不受影响）
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    renderer(&manifest)
        .render_agent(root.path(), "plan-agent")
        .expect("渲染应成功");
    assert!(
        !root.path().join("plan-agent").join(".pi").join("pathguard.json").exists(),
        "无声明不应生成 pathguard.json"
    );
}

#[test]
fn path_permissions_distribute_plugin_files_matching_embedded_assets() {
    // 启用 agent：分发三件运行件且逐字节与内嵌资产一致（产品化：非源码部署同产物）
    let mut manifest = ConfigManifest::builtin();
    let target = "plan-agent";
    let agent = manifest
        .agents
        .iter_mut()
        .find(|a| a.name == target)
        .expect("内置清单应有 plan-agent");
    agent.path_permissions = Some(crate::config::manifest::PathPermissions::default());
    let root = tempfile::tempdir().unwrap();
    renderer(&manifest).render_agent(root.path(), target).unwrap();

    for (rel, content) in crate::config::extension_assets::PATH_GUARD_FILES {
        let written = std::fs::read_to_string(
            root.path().join(target).join("extensions").join("path-guard").join(rel),
        )
        .unwrap_or_else(|e| panic!("分发件 {rel} 应存在：{e}"));
        assert_eq!(written, *content, "分发件 {rel} 应与内嵌资产逐字节一致");
    }
}

#[test]
fn agents_without_path_permissions_get_no_plugin_dir() {
    // None：不分发（零痕迹）
    let manifest = ConfigManifest::builtin();
    let root = tempfile::tempdir().unwrap();
    renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    assert!(
        !root.path().join("plan-agent").join("extensions").exists(),
        "无声明不应创建 extensions 目录"
    );
}

#[test]
fn plugin_distribution_does_not_depend_on_skills_source() {
    // 反回退：skills_source=None（生产默认，走内嵌资产）时插件照常分发——
    // 防止实现回退成「目录复制」导致非源码部署失效
    let mut manifest = ConfigManifest::builtin();
    let agent = manifest
        .agents
        .iter_mut()
        .find(|a| a.name == "plan-agent")
        .unwrap();
    agent.path_permissions = Some(crate::config::manifest::PathPermissions::default());
    let root = tempfile::tempdir().unwrap();
    // renderer() helper 的 skills_source 恒为 None（生产默认形态）
    renderer(&manifest).render_agent(root.path(), "plan-agent").unwrap();
    assert!(
        root.path()
            .join("plan-agent")
            .join("extensions")
            .join("path-guard")
            .join("index.ts")
            .exists(),
        "skills_source=None 时插件仍应分发"
    );
}
