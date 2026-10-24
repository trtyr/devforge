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
    assert_eq!(list.len(), 3, "池内同 provider 的模型全量分发");
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
    assert_eq!(settings["httpIdleTimeoutMs"], 60_000);
    assert_eq!(settings["defaultThinkingLevel"], "high");
    assert_eq!(settings["compaction"]["enabled"], false);
    // 模型层键共存
    assert_eq!(settings["defaultProvider"], "newapi");
    assert_eq!(settings["defaultModel"], "mokeywork/deepseek-flash");
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
        15,
        "规划双 + 编码双 + 测试双 + 文档三 + 变更规划双 + 变更编码双 + 变更测试双（P007/P008/P009）"
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
