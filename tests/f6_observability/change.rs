//! （ARC-R6 自 f6_observability.rs 平移，纯移动。）

#[allow(unused_imports)]
use super::*;
#[allow(unused_imports)]
use super::{fixture::*, guards::*, milestone::*, panic::*, runlog::*, support::*};

// ---- change 侧：审查开庭 + 三道机械门 ----

/// change 侧的夹具（目标仓带 git + 黑板证据齐全）。
pub(super) struct ChangeFixture {
    _dir: tempfile::TempDir,
    ctx: ChangeCtx,
}

pub(super) fn change_fixture() -> ChangeFixture {
    let dir = tempfile::tempdir().expect("临时目录");
    let repo = dir.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("建仓目录");
    std::fs::write(repo.join("src/a.rs"), "// a\n").expect("写源文件");
    let blackboard = dir.path().join("bb");
    std::fs::create_dir_all(&blackboard).expect("建黑板");
    let change_dir = repo.join("docs/plantree/changes/change-f6");
    std::fs::create_dir_all(&change_dir).expect("建计划目录");
    std::fs::write(
        change_dir.join("change-plan.md"),
        "# 变更计划\n\n## 1 需求复述\n改 a。\n\n## 2 影响面\n- `src/a.rs:1`\n\n\
         ## 3 要改的\n| `src/a.rs` | 需求 |\n\n## 4 白名单\n`src/a.rs`\n\n\
         ## 5 验收标准\n- 给定 x=1 → 返回 2\n\n## 6 风险\n- 无\n",
    )
    .expect("写计划");
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "baseline",
        ],
    );
    let baseline = git_out(&repo, &["rev-parse", "HEAD"]);
    // 黑板证据：基线绿 + 自证绿 + 一步实施
    std::fs::create_dir_all(blackboard.join("evidence")).expect("建证据目录");
    std::fs::write(
        blackboard.join("evidence/baseline-test.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .expect("写基线证据");
    std::fs::write(
        blackboard.join("evidence/self-check.log"),
        "command: cargo test\nexit_code: 0\n",
    )
    .expect("写自证证据");
    // 证据 mtime 刷到未来 2s：模拟「agent 本轮实时写入」（证据门拒绝早于本轮开始的残留）
    for name in ["baseline-test.log", "self-check.log"] {
        let p = blackboard.join("evidence").join(name);
        std::fs::File::options()
            .write(true)
            .open(&p)
            .expect("开证据文件")
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .expect("刷 mtime");
    }
    std::fs::write(
        blackboard.join("impl-steps.md"),
        "# 实施步骤\n## 步骤 1 改 a\n",
    )
    .expect("写实施步骤");
    std::fs::write(repo.join("src/a.rs"), "// a changed\n").expect("改源文件");
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "change: change-f6 step 1",
        ],
    );
    ChangeFixture {
        ctx: ChangeCtx {
            repo,
            blackboard,
            change_dir,
            change_id: "change-f6".into(),
            baseline,
            requirement: "把 a 改成 b".into(),
            doc_level: DocLevel::None,
            contract: None,
        },
        _dir: dir,
    }
}

pub(super) fn git(repo: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

pub(super) fn git_out(repo: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git 可执行");
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
pub(super) fn the_change_review_open_carries_the_role_and_the_round() {
    // 错实现形态：change 侧审查开庭不落 review.open（或缺角色名/轮次）→ 本用例必须变红
    let cap = Capture::new();
    let f = change_fixture();
    let agent = ScriptedAgent::new(&[
        "基线绿了\n<stage-complete>1</stage-complete>",
        "改完了\n<stage-complete>1</stage-complete>",
    ]);
    let factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    let mut session = ProdChangeSession {
        plan_agent: Box::new(ScriptedAgent::new(&[])),
        plan_review: Box::new(ScriptedFactory::new(&[])),
        coding_agent: Box::new(agent),
        coding_review: Box::new(factory),
        test_agent: Box::new(ScriptedAgent::new(&[])),
        test_review: Box::new(ScriptedFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    };
    {
        use devforge::change::orchestrator::ChangeSession as _;
        session.run_coding(&mut NoUser, 0, None).expect("coding 段应通过");
    }
    let text = cap.lines();
    let opens = lines_with(&text, "review.open");
    assert_eq!(opens.len(), 1, "一次开庭一条：{text:?}");
    assert!(
        opens[0].contains("\"role\":\"change-coding-review\""),
        "{opens:?}"
    );
    assert!(opens[0].contains("\"round\":1"), "首审轮次 = 1：{opens:?}");
    assert!(
        opens[0].contains("\"target\":\"devforge::change::session\""),
        "target 必须是发出它的模块：{opens:?}"
    );
}

/// 造一个 change coding 段的 session（脚本化 agent + 脚本化审查门）。
pub(super) fn coding_session(
    f: &ChangeFixture,
    agent_replies: &[&str],
    review_replies: &[&str],
) -> ProdChangeSession {
    ProdChangeSession {
        plan_agent: Box::new(ScriptedAgent::new(&[])),
        plan_review: Box::new(ScriptedFactory::new(&[])),
        coding_agent: Box::new(ScriptedAgent::new(agent_replies)),
        coding_review: Box::new(ScriptedFactory::new(review_replies)),
        test_agent: Box::new(ScriptedAgent::new(&[])),
        test_review: Box::new(ScriptedFactory::new(&[])),
        ctx: f.ctx.clone(),
        plan_review_instances: 0,
    }
}

#[test]
pub(super) fn the_baseline_gate_logs_the_green_and_the_red_verdict() {
    // 错实现形态：①红基线不落门禁里程碑（只落绿路径）②退出码只埋在错误文本里
    //   （按字段过滤看不见「退出码多少」）③门禁里程碑落成 WARN/ERROR → 本用例必须变红
    // 绿：判定结论 passed=true，退出码按字段带出
    {
        let cap = Capture::new();
        let f = change_fixture();
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[STAGE_REVIEW_PASS_FINISH],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            session
                .run_coding(&mut NoUser, 0, None)
                .expect("coding 段应通过");
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.baseline");
        assert_eq!(gates.len(), 1, "绿基线也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.baseline\""), "{line}");
        assert!(line.contains("\"passed\":true"), "绿基线的判定结论要落绿：{line}");
        assert!(
            line.contains("\"exit_code\":0"),
            "绿基线也要按字段带出退出码：{line}"
        );
    }
    // 红：判定结论 passed=false，退出码按字段带出（红时含退出码）
    {
        let cap = Capture::new();
        let f = change_fixture();
        // 基线证据写成红（退出码 1）→ 门卫判红
        std::fs::write(
            f.ctx.blackboard.join("evidence/baseline-test.log"),
            "command: cargo test\nexit_code: 1\n",
        )
        .expect("写红基线证据");
        // 红证据也要 fresh 才能走到「基线红」分支而非残留拒收
        std::fs::File::options()
            .write(true)
            .open(f.ctx.blackboard.join("evidence/baseline-test.log"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
        let mut session = coding_session(
            &f,
            &["跑了，红了\n<stage-complete>1</stage-complete>"],
            &[],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            let err = session
                .run_coding(&mut NoUser, 0, None)
                .expect_err("红基线应拦住 coding 轮");
            assert!(
                err.to_string().contains("基线不是绿的"),
                "红基线的报错应说明基线不绿：{err}"
            );
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.baseline");
        assert_eq!(gates.len(), 1, "红基线也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.baseline\""), "{line}");
        assert!(line.contains("\"passed\":false"), "判定结论要落红：{line}");
        assert!(
            line.contains("\"exit_code\":1"),
            "红时要把退出码按字段带出来（不能只埋在错误文本里）：{line}"
        );
    }
}

#[test]
pub(super) fn the_self_check_gate_logs_the_green_and_the_red_verdict() {
    // 错实现形态：①自证门红时不落门禁里程碑（只落绿路径）②红路径缺判定结论
    //   ③门禁里程碑落成 WARN/ERROR → 本用例必须变红
    // 绿：判定结论 passed=true
    {
        let cap = Capture::new();
        let f = change_fixture();
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[STAGE_REVIEW_PASS_FINISH],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            session
                .run_coding(&mut NoUser, 0, None)
                .expect("coding 段应通过");
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.self_check");
        assert_eq!(gates.len(), 1, "绿自证也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.self_check\""), "{line}");
        assert!(line.contains("\"passed\":true"), "绿自证的判定结论要落绿：{line}");
    }
    // 红：判定结论 passed=false
    {
        let cap = Capture::new();
        let f = change_fixture();
        // 自证证据写成红（退出码 1）→ 自证门判红
        std::fs::write(
            f.ctx.blackboard.join("evidence/self-check.log"),
            "command: cargo test\nexit_code: 1\n",
        )
        .expect("写红自证证据");
        // 红自证也要 fresh 才能走到「自证红」分支而非残留拒收
        std::fs::File::options()
            .write(true)
            .open(f.ctx.blackboard.join("evidence/self-check.log"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(2))
            .unwrap();
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            let err = session
                .run_coding(&mut NoUser, 0, None)
                .expect_err("红自证应拦住 coding 轮");
            assert!(
                err.to_string().contains("自证不是绿的"),
                "红自证的报错应说明自证没过：{err}"
            );
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.self_check");
        assert_eq!(gates.len(), 1, "红自证也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.self_check\""), "{line}");
        assert!(line.contains("\"passed\":false"), "判定结论要落红：{line}");
    }
}

#[test]
pub(super) fn the_step_commits_gate_logs_the_green_and_the_red_verdict() {
    // 错实现形态：①步提交对账不落门禁里程碑（只落绿路径）②缺步骤数
    //   ③门禁里程碑落成 WARN/ERROR → 本用例必须变红
    // 绿：判定结论 passed=true + 步骤数
    {
        let cap = Capture::new();
        let f = change_fixture();
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[STAGE_REVIEW_PASS_FINISH],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            session
                .run_coding(&mut NoUser, 0, None)
                .expect("coding 段应通过");
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.step_commits");
        assert_eq!(gates.len(), 1, "绿步提交对账也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.step_commits\""), "{line}");
        assert!(line.contains("\"passed\":true"), "绿对账的判定结论要落绿：{line}");
        assert!(line.contains("\"steps\":1"), "步提交对账要带步骤数：{line}");
    }
    // 红：判定结论 passed=false + 步骤数
    {
        let cap = Capture::new();
        let f = change_fixture();
        // 实施步骤写两步，但只有一个提交 → 对账判红
        std::fs::write(
            f.ctx.blackboard.join("impl-steps.md"),
            "# 实施步骤\n## 步骤 1 改 a\n## 步骤 2 改 b\n",
        )
        .expect("写两步的实施步骤");
        let mut session = coding_session(
            &f,
            &[
                "基线绿了\n<stage-complete>1</stage-complete>",
                "改完了\n<stage-complete>1</stage-complete>",
            ],
            &[],
        );
        {
            use devforge::change::orchestrator::ChangeSession as _;
            let err = session
                .run_coding(&mut NoUser, 0, None)
                .expect_err("步骤与提交对不上应拦住 coding 轮");
            assert!(
                err.to_string().contains("对不上"),
                "对账失败的报错应说明步骤与提交对不上：{err}"
            );
        }
        let text = cap.lines();
        let gates = lines_with(&text, "gate.step_commits");
        assert_eq!(gates.len(), 1, "红对账也要落一条门禁里程碑：{text:?}");
        let line = gates[0];
        assert!(line.contains("\"level\":\"INFO\""), "门禁里程碑是 INFO：{line}");
        assert!(line.contains("\"event\":\"gate.step_commits\""), "{line}");
        assert!(line.contains("\"passed\":false"), "判定结论要落红：{line}");
        assert!(line.contains("\"steps\":2"), "红对账也要带步骤数：{line}");
    }
}

#[test]
pub(super) fn an_agent_turn_logs_exactly_one_turn_end() {
    // 错实现形态：①交卷不落 agent.turn_end（或缺角色名/耗时）②同一轮落两条
    // （change 侧另落一份）→ 本用例必须变红
    let cap = Capture::new();
    let dir = tempfile::tempdir().expect("临时目录");
    let script = r#"read line; printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"hello"}}'; printf '%s\n' '{"type":"agent_settled"}'; sleep 1"#;
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(script);
    let spec = SpawnSpec::new(
        dir.path().join("profile"),
        "f6-test-agent",
        "run-f6-turn",
        dir.path().join("sessions"),
    );
    let mut client = RpcClient::spawn_with_command(cmd, spec).expect("spawn 假 agent");
    {
        client.prompt("干活").expect("发 prompt");
        let outcome = client.wait_settled().expect("等交卷");
        assert!(outcome.text.contains("hello"), "应收到流式文本");
    }
    let _ = client.kill();
    let text = cap.lines();
    let turns = lines_with(&text, "agent.turn_end");
    assert_eq!(turns.len(), 1, "一轮只落一条交卷事件：{text:?}");
    assert!(
        turns[0].contains("\"role\":\"f6-test-agent\""),
        "要带角色名：{turns:?}"
    );
    assert!(turns[0].contains("turn_ms"), "要带本轮耗时：{turns:?}");
    assert!(turns[0].contains("chars"), "要带产出字符数：{turns:?}");
    // 落点唯一：change 侧不另落交卷事件
    let change_session =
        std::fs::read_to_string(repo_root().join("src/change/session.rs")).expect("读 change 侧源码");
    assert!(
        !change_session.contains("agent.turn_end"),
        "交卷事件落点必须唯一（change 侧不得另落）"
    );
}

#[test]
pub(super) fn every_milestone_is_info_with_component_event_and_the_emitting_module_target() {
    // 错实现形态：里程碑落成 WARN/ERROR、删掉 component、或所有里程碑共用同一个 target
    // （包装函数）→ 本用例必须变红
    let cap = Capture::new();
    let plan = ScriptedAgent::new(&["好了\n<plan-complete>"]);
    let mut factory = ScriptedFactory::new(&[PLAN_REVIEW_PASS]);
    devforge::sop::planning::PlanningLoop::new(plan, &mut factory)
        .run(&mut NoUser)
        .expect("规划循环应通过");
    let text = cap.lines();
    let milestones = lines_with(&text, "\"event\":");
    assert!(!milestones.is_empty(), "应有里程碑条目：{text:?}");
    for line in milestones {
        assert!(line.contains("\"level\":\"INFO\""), "级别必须是 INFO：{line}");
        assert!(line.contains("\"component\":"), "要有 component：{line}");
        assert!(line.contains("\"event\":"), "要有 event：{line}");
        assert!(line.contains("\"target\":"), "要有 target：{line}");
    }
    // 不同模块的里程碑不共用同一个 target
    let before_coding = cap.lines().len();
    let agent = ScriptedAgent::new(&["阶段 1 完成\n<stage-complete>1</stage-complete>"]);
    let mut factory = ScriptedFactory::new(&[STAGE_REVIEW_PASS_FINISH]);
    devforge::sop::coding::CodingSopLoop::new(agent, &mut factory)
        .run(&mut NoUser, Path::new("/w"), "p", "t")
        .expect("编码循环应通过");
    let coding_text = cap.lines()[before_coding..].to_vec();
    let target_of = |lines: &[&str]| -> String {
        lines[0]
            .split("\"target\":\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("")
            .to_string()
    };
    let planning_target = target_of(&lines_with(&text, "review.open"));
    let coding_target = target_of(&lines_with(&coding_text, "review.open"));
    assert!(!planning_target.is_empty(), "target 要有值");
    assert!(
        planning_target != coding_target,
        "不同模块的里程碑不共用同一个 target：{planning_target} vs {coding_target}"
    );
    assert!(planning_target.contains("planning"), "{planning_target}");
    assert!(coding_target.contains("coding"), "{coding_target}");
}

// ============================================================ 需求 3：死亡取证（墓碑）


