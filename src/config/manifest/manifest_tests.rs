//! （ARC-R7 自 manifest.rs mod tests 平移，纯移动。）

use super::*;

#[test]
fn change_plan_prompts_carry_six_blocks_and_forbidden_items() {
    // P009 T006：六块与「不写实施顺序 / 不写回归命令 / codegraph-first」必须在提示词里
    for block in crate::change::plan_audit::PLAN_BLOCKS {
        assert!(
            CHANGE_PLAN_AGENT_PROMPT.contains(block),
            "change-plan 提示词缺块：{block}"
        );
    }
    for anchor in ["结构性事实一律走 codegraph", "不写实施顺序", "不写回归命令", "path:line"] {
        assert!(
            CHANGE_PLAN_AGENT_PROMPT.contains(anchor),
            "change-plan 提示词缺纪律：{anchor}"
        );
    }
    // 审查侧：只审计划、不看 diff、不跑命令、不写文件
    for anchor in ["不看 diff", "不跑命令", "不写文件", "<stage-verdict>"] {
        assert!(
            CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
            "change-plan-review 提示词缺纪律：{anchor}"
        );
    }
    // 问询环的四个锚点，且标记字面量与机械门常量**逐字相同**（改一边不改另一边 = 门空转）
    for anchor in [
        crate::change::plan_audit::VERDICT_MUST_ASK,
        crate::change::plan_audit::VERDICT_SELF,
        crate::change::plan_audit::VERDICT_ANSWER_FIELD,
        "ask-user-question",
    ] {
        assert!(
            CHANGE_PLAN_AGENT_PROMPT.contains(anchor),
            "change-plan 提示词缺问询契约锚点：{anchor}"
        );
    }
    // 审查侧两条新审点 + 同一批标记字面量
    for anchor in ["留痕核对", "分类恰当性", "结构缺失"] {
        assert!(
            CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
            "change-plan-review 提示词缺新审点：{anchor}"
        );
    }
    for anchor in [
        crate::change::plan_audit::VERDICT_MUST_ASK,
        crate::change::plan_audit::VERDICT_ANSWER_FIELD,
    ] {
        assert!(
            CHANGE_PLAN_REVIEW_PROMPT.contains(anchor),
            "change-plan-review 提示词缺标记字面量：{anchor}"
        );
    }
}

/// 需求 2 的落点：change 六件套逐个钉住供给（谁有 ask 包、谁没有）。
///
/// 错实现形态：给某个 agent 顺手加包/删包 → 本用例必须变红。
#[test]
fn change_six_agents_have_pinned_supplies() {
    let m = ConfigManifest::builtin();
    let ask = "npm:@juicesharp/rpiv-ask-user-question";
    // 干活方要能问用户（ask 包）；审查方不与用户交互（无包）。
    let expect: [(&str, &[&str]); 6] = [
        ("change-plan-agent", &[ask]),
        ("change-plan-review", &[]),
        ("change-coding-agent", CODING_AGENT_EXTRA),
        ("change-coding-review", &[]),
        ("change-test-agent", CODING_AGENT_EXTRA),
        ("change-test-review", &[]),
    ];
    for (name, packages) in expect {
        let a = m.get_agent(name).unwrap();
        assert_eq!(
            a.extra_packages,
            packages.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "{name} 的差异化包与钉死的供给不符：{:?}",
            a.extra_packages
        );
    }
}

/// 其余九个 Agent 的差异化包逐字段钉死：改一处不许误伤别处。
///
/// 错实现形态：顺手给别的 agent 加包/删包 → 本用例必须变红。
#[test]
fn other_agents_supplies_are_unchanged() {
    let m = ConfigManifest::builtin();
    let expect: [(&str, &[&str]); 9] = [
        ("plan-agent", &[PLAN_AGENT_EXTRA]),
        ("plan-review", &[]),
        ("coding-agent", CODING_AGENT_EXTRA),
        ("coding-review", CODING_REVIEW_EXTRA),
        ("test-agent", TEST_AGENT_EXTRA),
        ("test-review", &[]),
        ("doc-fix", CODING_AGENT_EXTRA),
        ("doc-agent", CODING_AGENT_EXTRA),
        ("doc-review", &[]),
    ];
    for (name, packages) in expect {
        let a = m.get_agent(name).unwrap();
        assert_eq!(
            a.extra_packages,
            packages.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            "{name} 的差异化包被误改了：{:?}",
            a.extra_packages
        );
    }
}

#[test]
fn builtin_has_eleven_agents_and_four_base_packages() {
    let m = ConfigManifest::builtin();
    assert_eq!(m.base_packages, BASE_PACKAGES.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(
        m.agents.len(),
        16,
        "规划双 + 编码双 + 测试双 + 文档三 + 变更规划双 + 变更编码双 + 变更测试双 + 审计单（P007/P008/P009/P013）"
    );
    // 差异化包仅 Plan Agent
    let pa = m.get_agent("plan-agent").unwrap();
    assert_eq!(pa.extra_packages, vec![PLAN_AGENT_EXTRA.to_string()]);
    let pr = m.get_agent("plan-review").unwrap();
    assert!(pr.extra_packages.is_empty());
    // P005：编码双 Agent 的 MCP 与差异化包
    let ca = m.get_agent("coding-agent").unwrap();
    assert_eq!(ca.mcp.len(), 2, "codegraph + context7");
    assert_eq!(ca.mcp[0].name, "codegraph");
    assert_eq!(ca.mcp[0].env.get("CODEGRAPH_TELEMETRY").map(String::as_str), Some("0"));
    assert_eq!(ca.extra_packages.len(), 3);
    assert_eq!(ca.model, "deepseek-flash"); // 编码 → DeepSeek（沿规划期拍板）
    let cr = m.get_agent("coding-review").unwrap();
    assert_eq!(cr.mcp.len(), 2);
    assert_eq!(cr.extra_packages, vec!["npm:pi-lens".to_string()]);
    assert_eq!(cr.model, "grok-4.7"); // 审查 → Grok（沿规划期拍板）
    // 规划双 Agent 无 MCP（P001 行为不回归）
    assert!(pa.mcp.is_empty() && pr.mcp.is_empty());
    // P007：测试双 Agent（D007 供给即纪律）
    let ta = m.get_agent("test-agent").unwrap();
    assert_eq!(ta.extra_packages, TEST_AGENT_EXTRA.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(ta.mcp.len(), 1, "仅 context7——codegraph 是代码事实层，黑盒不给");
    assert_eq!(ta.mcp[0].name, "context7");
    assert_eq!(ta.skills, vec!["python-best-practices".to_string()]);
    assert_eq!(ta.model, "deepseek-flash");
    let tr = m.get_agent("test-review").unwrap();
    assert!(tr.extra_packages.is_empty() && tr.mcp.is_empty() && tr.skills.is_empty());
    assert_eq!(tr.model, "grok-4.7");
    // P007：文档双 Agent（doc-fix=coding 家族供给；doc-review 极简）
    let df = m.get_agent("doc-fix").unwrap();
    assert_eq!(df.extra_packages, ca.extra_packages, "doc-fix 沿用 coding-agent 家族供给");
    assert_eq!(df.mcp.len(), 2);
    assert_eq!(df.model, "deepseek-flash");
    let dr = m.get_agent("doc-review").unwrap();
    // P008 T000a：doc-review 也拿 doc-contract（它是审查用规则书，不是写作技能）
    assert!(dr.extra_packages.is_empty());
    assert_eq!(dr.skills, vec!["doc-contract".to_string()]);
    // DR-12（10-23 用户拍板，推翻 D012/D018）：审查官不读码——事实由引擎确定性面兑底；
    // 实测有图时审查官人肉重推导（61 次调用/14.5min）且读码探索失控触发协议违例双轮
    assert!(dr.mcp.is_empty(), "doc-review 不得持有读码权限: {:?}", dr.mcp);
    assert_eq!(dr.model, "grok-4.7");

    // P009：变更 SOP 规划双 Agent（D009/D012/D016）
    let cpa = m.get_agent("change-plan-agent").unwrap();
    assert_eq!(cpa.model, "deepseek-flash", "干活方与 plan/coding 同档");
    assert_eq!(cpa.skills, vec!["plan-tree".to_string()]);
    assert_eq!(cpa.mcp.len(), 1, "只给 codegraph（D009）");
    assert_eq!(cpa.mcp[0].name, "codegraph");
    assert_eq!(
        cpa.extra_packages,
        vec![PLAN_AGENT_EXTRA.to_string()],
        "问询环的供给层：change-plan-agent 必须拿 ask 包（与 MVP plan-agent 同包）"
    );
    let cpr = m.get_agent("change-plan-review").unwrap();
    assert_eq!(cpr.model, "grok-4.7", "审查固定 grok");
    assert_eq!(cpr.mcp.len(), 1, "审计划要核调用关系（D012）");
    assert_eq!(cpr.mcp[0].name, "codegraph");
    assert!(cpr.extra_packages.is_empty());
    // MVP 的两个规划 Agent 不受影响（“平行不干扰”）
    assert_eq!(pa.model, "deepseek-flash");
    assert!(pa.mcp.is_empty() && pr.mcp.is_empty(), "MVP 规划双 Agent 仍无 MCP");

    // P009：变更编码双 Agent（D014/D016）
    let cca = m.get_agent("change-coding-agent").unwrap();
    assert_eq!(cca.model, "deepseek-flash");
    assert_eq!(cca.mcp.len(), 2, "codegraph + context7");
    assert_eq!(cca.mcp[1].name, "context7", "改真代码要查第三方 API");
    assert_eq!(cca.extra_packages, ca.extra_packages, "与 MVP coding 同档供给");
    assert_eq!(cca.skills.len(), 5, "四语言 best-practices + coding-discipline");
    let ccr = m.get_agent("change-coding-review").unwrap();
    assert_eq!(ccr.model, "grok-4.7");
    assert_eq!(ccr.mcp.len(), 1, "审查只给 codegraph（D016）");
    assert_eq!(ccr.mcp[0].name, "codegraph");
    assert!(ccr.extra_packages.is_empty());

    // P009：变更测试双 Agent（黑盒，D019：不给 codegraph）
    let cta = m.get_agent("change-test-agent").unwrap();
    assert_eq!(cta.model, "deepseek-flash");
    assert!(cta.mcp.is_empty(), "测试 agent 不得拿到代码事实层");
    let ctr = m.get_agent("change-test-review").unwrap();
    assert_eq!(ctr.model, "grok-4.7");
    assert!(ctr.mcp.is_empty() && ctr.skills.is_empty());
    // 测试提示词必须包含「回归优先 / 验收对账 / 不改产品代码」三条纪律
    for anchor in ["回归优先", "验收对照表", "不改产品代码", "对照表里只准有验收项"] {
        assert!(CHANGE_TEST_AGENT_PROMPT.contains(anchor), "缺纪律：{anchor}");
    }
    assert!(CHANGE_TEST_REVIEW_PROMPT.contains("缺一条即 FAIL"));

    // P008 D018：doc-agent = 干活方（与 coding-agent 同档供给）
    let da = m.get_agent("doc-agent").unwrap();
    assert_eq!(
        da.model, "minimax-m3.1-flash",
        "doc-agent 自 2026-10-20 起用 MiniMax-M3.1-Flash"
    );
    assert_eq!(da.extra_packages, ca.extra_packages);
    assert_eq!(da.mcp.len(), 2, "codegraph + context7");
    assert_eq!(da.mcp[0].name, "codegraph");
    assert_eq!(da.mcp[1].name, "context7");
    assert!(da.skills.iter().any(|s| s == "doc-contract"));
    assert_eq!(da.thinking_level, None, "干活方用全局 high");
}

#[test]
fn doc_contract_skill_is_shared_by_four_profiles() {
    // P008 T000a：一份契约四处共用，不允许某处漏掉（否则就是四份措辞漂移的起点）
    let m = ConfigManifest::builtin();
    for name in ["coding-agent", "coding-review", "doc-fix", "doc-review"] {
        let a = m.get_agent(name).unwrap();
        assert!(
            a.skills.iter().any(|s| s == "doc-contract"),
            "{name} 缺少 doc-contract skill"
        );
    }
}

#[test]
fn coding_discipline_skill_goes_to_exactly_the_four_coding_agents() {
    // 纪律唯一事实源：只发给写代码与审代码的四个 Agent（4 正 + 11 负 = 15）
    let m = ConfigManifest::builtin();
    for name in [
        "coding-agent",
        "coding-review",
        "change-coding-agent",
        "change-coding-review",
    ] {
        let a = m.get_agent(name).unwrap();
        assert!(
            a.skills.iter().any(|s| s == "coding-discipline"),
            "{name} 缺少 coding-discipline skill"
        );
    }
    for name in [
        "plan-agent",
        "plan-review",
        "change-plan-agent",
        "change-plan-review",
        "test-agent",
        "test-review",
        "change-test-agent",
        "change-test-review",
        "doc-fix",
        "doc-agent",
        "doc-review",
    ] {
        let a = m.get_agent(name).unwrap();
        assert!(
            !a.skills.iter().any(|s| s == "coding-discipline"),
            "{name} 不该拿 coding-discipline skill"
        );
    }
    assert_eq!(m.agents.len(), 16, "4 正 + 12 负 = 16（code-review 不带 coding-discipline，P013）");
}

#[test]
fn coding_prompts_carry_discipline_anchors() {
    // 写代码方：纪律 skill + 行为断言 + 先证后报
    for anchor in ["coding-discipline", "行为断言", "先证后报"] {
        assert!(
            CODING_AGENT_PROMPT.contains(anchor),
            "MVP coding 提示词缺：{anchor}"
        );
        assert!(
            CHANGE_CODING_AGENT_PROMPT.contains(anchor),
            "change coding 提示词缺：{anchor}"
        );
    }
    // 行为断言要两头成立（补齐「只做内部重构仍绿」这半边）
    for prompt in [CODING_AGENT_PROMPT, CHANGE_CODING_AGENT_PROMPT] {
        assert!(
            prompt.contains("只做内部重构"),
            "行为断言判据缺「只做内部重构仍绿」这半边"
        );
    }
    // 审代码方：自证核对审查面（先证后报）
    assert!(
        CODING_REVIEW_PROMPT.contains("先证后报"),
        "MVP review 提示词缺自证核对面"
    );
    assert!(
        CHANGE_CODING_REVIEW_PROMPT.contains("先证后报"),
        "change review 提示词缺自证核对面"
    );
}

/// 代码生成器：把 6 个 change agents 导出成 e2e 夹具（漂移时重跑刷新）。
/// `cargo test --lib dump_change_agents_fixture -- --ignored`
#[test]
#[ignore = "代码生成器：重建 tests/e2e/fixtures/change_agents.json"]
fn dump_change_agents_fixture() {
    let m = ConfigManifest::builtin();
    let agents: Vec<&AgentManifest> = m
        .agents
        .iter()
        .filter(|a| a.name.starts_with("change-"))
        .collect();
    assert_eq!(agents.len(), 6, "change agents 应有 6 个");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/e2e/fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("change_agents.json"),
        serde_json::to_string_pretty(&agents).unwrap(),
    )
    .unwrap();
}

#[test]
fn change_agents_fixture_matches_builtin() {
    // e2e 用这份夹具把 change 六件套装进 manifest；漂移 = e2e 测的不是内置提示词
    let m = ConfigManifest::builtin();
    let expected: Vec<&AgentManifest> = m
        .agents
        .iter()
        .filter(|a| a.name.starts_with("change-"))
        .collect();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/e2e/fixtures/change_agents.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("夹具缺失，重跑 dump_change_agents_fixture：{}", path.display()));
    let actual: Vec<AgentManifest> = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        serde_json::to_string(&actual).unwrap(),
        serde_json::to_string(&expected).unwrap(),
        "change agents 夹具与 builtin 漂移——重跑 dump_change_agents_fixture"
    );
}

#[test]
fn doc_agent_prompt_carries_protocol_and_guards() {
    // D009/D010/D016/D020 的硬约束锚点
    assert!(DOC_AGENT_PROMPT.contains("只写 `docs/contexts/`"));
    assert!(DOC_AGENT_PROMPT.contains("codegraph-first"));
    assert!(DOC_AGENT_PROMPT.contains("事实与推断分离"));
    assert!(DOC_AGENT_PROMPT.contains("🟢"));
    assert!(DOC_AGENT_PROMPT.contains("path:line"));
    assert!(DOC_AGENT_PROMPT.contains("gaps.md"));
    assert!(DOC_AGENT_PROMPT.contains("N/A"));
    assert!(DOC_AGENT_PROMPT.contains("<doc-complete>unit-xxx</doc-complete>"));
    assert!(DOC_AGENT_PROMPT.contains("devforge doc-lint"));
    assert!(DOC_AGENT_PROMPT.contains("按 route 回去修"));
    // t8：三阶段流水与证据要求
    assert!(DOC_AGENT_PROMPT.contains("阶段流水（事实 / 推断分离"));
    assert!(DOC_AGENT_PROMPT.contains("_meta/facts/<unit>.md"));
    // t12/t13：汇编层只合成、图用 Mermaid 源码（D005）
    assert!(DOC_AGENT_PROMPT.contains("汇编层只合成"));
    assert!(DOC_AGENT_PROMPT.contains("Mermaid 源码"));
    assert!(DOC_AGENT_PROMPT.contains("总览 / 架构 / 数据流 / Runbook"));
    assert!(DOC_AGENT_PROMPT.contains("严禁写“为什么”"));
    assert!(DOC_AGENT_PROMPT.contains("不得静默省略"));
}

#[test]
fn coding_agent_prompt_has_preflight_audit() {
    // 2026-10-19 实测：10 轮审查 5 轮打回——文档契约/绝对词/断言强度三类白旗缺口
    assert!(CODING_AGENT_PROMPT.contains("提交前预审"));
    assert!(CODING_AGENT_PROMPT.contains("绝对词"));
    assert!(CODING_AGENT_PROMPT.contains("断言必须能失败"));
    assert!(CODING_AGENT_PROMPT.contains("预审证据"));
    assert!(CODING_AGENT_PROMPT.contains("isVisible()"));
    // T000c：契约 + doc-lint 硬门禁（文档不能只靠自述）
    assert!(CODING_AGENT_PROMPT.contains("doc-contract"));
    assert!(CODING_AGENT_PROMPT.contains("devforge doc-lint"));
    assert!(CODING_AGENT_PROMPT.contains("退出码必须 0"));
    assert!(CODING_REVIEW_PROMPT.contains("doc-lint 复核"));
    assert!(CODING_REVIEW_PROMPT.contains("自己跑一遍"));
}

#[test]
fn testing_prompts_carry_protocol_and_discipline() {
    // 黑盒硬禁令锚点（D001）
    assert!(TEST_AGENT_PROMPT.contains("禁止读取任何实现文件内容"));
    assert!(TEST_AGENT_PROMPT.contains("路径可以看，内容不能读"));
    assert!(TEST_AGENT_PROMPT.contains("白名单"));
    assert!(TEST_AGENT_PROMPT.contains("<stage-complete>"));
    assert!(TEST_AGENT_PROMPT.contains("coverage-matrix"));
    assert!(TEST_AGENT_PROMPT.contains("pytest-rerunfailures"));
    assert!(TEST_AGENT_PROMPT.contains("tests/integration"));
    assert!(TEST_AGENT_PROMPT.contains("Runbook"));
    assert!(TEST_AGENT_PROMPT.contains("依赖获取"), "测试依赖获取口径缺失");
    assert!(TEST_AGENT_PROMPT.contains("findings"));
    assert!(TEST_AGENT_PROMPT.contains("严禁写单元测试"));
    // reroute 协议锚点（D003）
    assert!(TEST_REVIEW_PROMPT.contains("<reroute>"));
    assert!(TEST_REVIEW_PROMPT.contains("test-side"));
    assert!(TEST_REVIEW_PROMPT.contains("doc-side"));
    assert!(TEST_REVIEW_PROMPT.contains("<stage-verdict>"));
    assert!(TEST_REVIEW_PROMPT.contains("<next>"));
    assert!(TEST_REVIEW_PROMPT.contains("业务关系"));
    assert!(TEST_REVIEW_PROMPT.contains("极端用户面"));
    assert!(TEST_REVIEW_PROMPT.contains("coverage-matrix"));
    // 省时硬纪律（2026-10-19 实测：审查占编码墙钟 68%）：环境复用/只跑一遍/一次列全
    assert!(CODING_REVIEW_PROMPT.contains("环境复用"));
    assert!(CODING_REVIEW_PROMPT.contains("测试只跑一遍"));
    assert!(CODING_REVIEW_PROMPT.contains("问题一次列全"));
    assert!(CODING_REVIEW_PROMPT.contains("批量调用"));
    assert!(CODING_REVIEW_PROMPT.contains("报告密度"));
    assert!(CODING_REVIEW_PROMPT.contains("不复述代码"));
    assert!(TEST_REVIEW_PROMPT.contains("批量调用"));
    assert!(TEST_REVIEW_PROMPT.contains("环境复用"));
    assert!(TEST_REVIEW_PROMPT.contains("问题一次列全"));
    // 测试自身可靠性锚点（2026-10-19 自指递归事故）
    assert!(TEST_AGENT_PROMPT.contains("禁止在套件内运行整套测试"));
    assert!(TEST_AGENT_PROMPT.contains("进程组"));
    assert!(TEST_AGENT_PROMPT.contains("passed / failed / skipped"));
    assert!(TEST_AGENT_PROMPT.contains("短超时"));
    assert!(TEST_REVIEW_PROMPT.contains("一票否决"));
    assert!(TEST_REVIEW_PROMPT.contains("发现挂死风险先停跑"));
    // 文档双 Agent 锚点（D004）
    assert!(DOC_FIX_PROMPT.contains("只改文档，不改代码"));
    assert!(DOC_FIX_PROMPT.contains("第三方"));
    assert!(DOC_FIX_PROMPT.contains("Runbook"));
    assert!(DOC_FIX_PROMPT.contains("findings"));
    assert!(DOC_REVIEW_PROMPT.contains("plan tree"));
    assert!(DOC_REVIEW_PROMPT.contains("Runbook"));
    assert!(DOC_REVIEW_PROMPT.contains("findings"));
    assert!(DOC_REVIEW_PROMPT.contains("<stage-verdict>"));
}

#[test]
fn model_resolution_from_pool() {
    let m = ConfigManifest::builtin();
    let pa = m.get_agent("plan-agent").unwrap();
    let model = m.resolve_model(pa).unwrap();
    assert_eq!(model.provider, "newapi");
    assert_eq!(model.model_id, "mokeywork/deepseek-flash");
    // 固定分配：审查 Agent → Grok 4.7（用户拍板 2026-10-17）
    let rv = m.get_agent("plan-review").unwrap();
    assert_eq!(m.resolve_model(rv).unwrap().model_id, "mokeywork/grok-4.7");
}

#[test]
fn unknown_model_rejected() {
    let mut m = ConfigManifest::builtin();
    m.agents[0].model = "ghost-model".into();
    let err = m.resolve_model(&m.agents[0]).unwrap_err();
    assert!(err.to_string().contains("不在模型池中"));
}

#[test]
fn manifest_json_roundtrip() {
    let m = ConfigManifest::builtin();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("manifest.json");
    m.save(&path).unwrap();
    let loaded = ConfigManifest::load(&path).unwrap();
    assert_eq!(m, loaded);
}
