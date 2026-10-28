//! （2026-10-26 测试分离：自 src/audit/pipeline.rs 内联平移，纯移动零改动。）
use super::*;
use std::cell::RefCell;

/// 脚本化假会话：按序返回预设回复。
struct ScriptSession {
    replies: RefCell<std::collections::VecDeque<&'static str>>,
}
impl AuditSession for ScriptSession {
    fn ask(&mut self, _instruction: &str) -> Result<String> {
        self.replies
            .borrow_mut()
            .pop_front()
            .map(|s| s.to_string())
            .ok_or_else(|| DevforgeError::System("回复耗尽".into()))
    }
}
struct ScriptFactory {
    /// 回复队列（审计修复 #3：FIFO——压入序即消费序；LIFO 会让「违例→成功」
    /// 压序测试第一次就拿到成功回复，重试循环从未被走到，空洞绿）。
    replies: std::collections::VecDeque<std::collections::VecDeque<&'static str>>,
    /// open 次数（断言换实例逻辑真实发生）。
    opens: usize,
}
impl AuditSessionFactory for ScriptFactory {
    fn open(&mut self) -> Result<Box<dyn AuditSession>> {
        self.opens += 1;
        self.replies
            .pop_front()
            .map(|r| Box::new(ScriptSession { replies: RefCell::new(r) }) as Box<dyn AuditSession>)
            .ok_or_else(|| DevforgeError::System("会话耗尽".into()))
    }
}

const PLAN_REPLY: &str = "```json\n{\"priority\": [\"unit-001\"], \"skip\": [\"unit-002\"], \"cross_concerns\": [\"错误处理模式一致\"]}\n```";
const FINDINGS_REPLY: &str = r#"<audit-findings>
[{"path":"src/a.rs","line":1,"view":"correctness","severity":"major","claim":"c","evidence":"src/a.rs:1"}]
</audit-findings>"#;

fn unit(id: &str) -> AuditUnit {
    AuditUnit {
        id: id.into(),
        files: vec!["src/a.rs".into()],
        label: format!("label-{id}"),
        focus: Vec::new(),
        focus_range: None,
    }
}

/// 第三方 r2 C1：指令必须携带工作区**绝对路径**——审查会话 cwd 是黑板，
/// 只给仓内相对路径时审查官无法定位源码（旧版就死在里）。
#[test]
fn batch_instruction_carries_workspace_absolute_path() {
    let ins = batch_instruction(
        Path::new("/home/u/proj"),
        &unit("unit-001"),
        &["label-unit-001".to_string()],
        "预查：无",
    );
    assert!(
        ins.contains("代码工作区（绝对路径）= /home/u/proj"),
        "指令必须带工作区绝对路径：{ins}"
    );
    assert!(
        ins.contains("相对工作区"),
        "还需声明 path:line 的基准是工作区：{ins}"
    );
}

#[test]
fn stage0_plans_from_metadata_only() {
    // 自审 minor（17）：旧用例从不检查指令内容——「Stage 0 只喂元数据」的契约无断言保护，
    // 把整个文件内容塞进指令也照样绿。这里用记录型会话钉住「有元数据、无代码」。
    let seen = std::rc::Rc::new(RefCell::new(String::new()));
    struct RecordingSession {
        reply: &'static str,
        seen: std::rc::Rc<RefCell<String>>,
    }
    impl AuditSession for RecordingSession {
        fn ask(&mut self, instruction: &str) -> Result<String> {
            *self.seen.borrow_mut() = instruction.to_string();
            Ok(self.reply.to_string())
        }
    }
    let mut s = RecordingSession {
        reply: PLAN_REPLY,
        seen: seen.clone(),
    };
    let mut u = unit("unit-001");
    u.files = vec!["src/a.rs".into()];
    let plan = stage0_plan(&mut s, &[u, unit("unit-002")]).unwrap();
    assert_eq!(plan.priority, vec!["unit-001"]);
    assert_eq!(plan.skip, vec!["unit-002"]);
    assert_eq!(plan.cross_concerns.len(), 1);
    let instruction = seen.borrow().clone();
    assert!(
        instruction.contains("unit-001") && instruction.contains("label-unit-002"),
        "元数据必须在指令里：{instruction}"
    );
    assert!(
        !instruction.contains("src/a.rs") && !instruction.contains("fn ") && !instruction.contains("use "),
        "Stage 0 不得喂代码/文件路径：{instruction}"
    );
    assert!(
        instruction.contains("REPO-DATA 边界声明"),
        "仓库派生数据必须带边界声明（注入面）：{instruction}"
    );
}

/// 自审 minor（9）：json 块取**末尾**那个（正文里的示例块不得劫持解析）。
#[test]
fn json_block_is_taken_from_the_tail() {
    let reply = "示例格式：\n```json\n{\"priority\": [\"unit-999\"]}\n```\n\n实际结果：\n```json\n{\"priority\": [\"unit-001\"]}\n```\n";
    let plan = parse_stage0(reply).unwrap();
    assert_eq!(
        plan.priority,
        vec!["unit-001"],
        "必须取末尾块（示例块不得劫持）"
    );
}

/// 自审 minor（10）：Stage 0 的非字符串条目不再静默丢弃，计数入报告。
#[test]
fn stage0_counts_malformed_entries() {
    let plan = parse_stage0(
        "```json\n{\"priority\": [\"unit-001\", 42, null], \"skip\": [], \"cross_concerns\": []}\n```",
    )
    .unwrap();
    assert_eq!(plan.priority, vec!["unit-001"]);
    assert_eq!(plan.parse_errors, 2, "畸形条目必须计数：{plan:?}");
}

/// 自审 minor（14）：进度文件损坏分支（旧口径零覆盖，删掉整段仍全绿）。
#[test]
fn corrupt_progress_file_is_warned_and_reset() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("audit-progress.json"), "{broken").unwrap();
    let p = Progress::load(dir.path());
    assert_eq!(p, Progress::default(), "损坏即整表重跑（空进度）");
    // 冷启动（文件不存在）静默且同样是空进度
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(Progress::load(empty.path()), Progress::default());
}

/// 自审 minor（7）：老版本进度文件缺字段不得被判「损坏」——serde default 要对称。
#[test]
fn legacy_progress_without_new_fields_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("audit-progress.json"),
        "{\"abandoned\": [\"unit-9：超时\"]}",
    )
    .unwrap();
    let p = Progress::load(dir.path());
    assert_eq!(p.abandoned, vec!["unit-9：超时".to_string()]);
    assert!(!p.stage0_done && p.done_batches.is_empty() && !p.stage2_done);
}

#[test]
fn stage1_reviews_batch_and_tags_batch_id() {
    let mut factory = ScriptFactory { replies: std::collections::VecDeque::from([[FINDINGS_REPLY].into()]), opens: 0 };
    let u = unit("unit-009");
    let findings = stage1_review_batch(
        &mut factory,
        Path::new("/ws"),
        &u,
        &["l1".to_string()],
        "预查：无",
    )
    .unwrap();
    assert_eq!(findings.findings.len(), 1);
    assert_eq!(
        findings.findings[0].batch, "unit-009",
        "发现应带批次溯源"
    );
    assert_eq!(factory.opens, 1, "无违例应只开一次会话");
}

#[test]
fn stage1_retries_once_on_protocol_violation_then_gives_up() {
    // FIFO：压入序即消费序——第一次真正拿到「违例」，重试循环才被真实走到
    // （审计修复 #3：LIFO 时代本用例是空洞绿，删掉重试循环仍过）。
    let mut factory = ScriptFactory {
        replies: std::collections::VecDeque::from([["违例"].into(), [FINDINGS_REPLY].into()]),
        opens: 0,
    };
    let u = unit("unit-001");
    let f = stage1_review_batch(&mut factory, Path::new("/ws"), &u, &[], "无").unwrap();
    assert_eq!(f.findings.len(), 1, "第一次违例重开后成功");
    assert_eq!(factory.opens, 2, "违例→成功必须恰好开两次会话（换实例）");
    // 两次全违例 → 放弃
    let mut factory = ScriptFactory {
        replies: std::collections::VecDeque::from([["违例2"].into(), ["违例3"].into()]),
        opens: 0,
    };
    assert!(stage1_review_batch(&mut factory, Path::new("/ws"), &u, &[], "无").is_err());
    assert_eq!(factory.opens, 2, "两次违例恰好开两次会话后放弃");
}

/// 2026-10-25 拍板回归：单批**空回复**与墙钟超时同层——换实例重试后按该批放弃
/// （Quality），不得像真基础设施故障那样打断整场审计。
#[test]
fn empty_reply_is_batch_abandon_not_whole_run_abort() {
    struct EmptySession;
    impl AuditSession for EmptySession {
        fn ask(&mut self, _: &str) -> Result<String> {
            Ok(String::new())
        }
    }
    struct EmptyFactory;
    impl AuditSessionFactory for EmptyFactory {
        fn open(&mut self) -> Result<Box<dyn AuditSession>> {
            Ok(Box::new(EmptySession))
        }
    }
    let mut f = EmptyFactory;
    let err = stage1_review_batch(&mut f, Path::new("/ws"), &unit("unit-001"), &[], "无")
        .unwrap_err();
    assert!(
        matches!(err, DevforgeError::Quality(_)),
        "空回复应归 Quality（放弃层）：{err:?}"
    );
    assert!(err.to_string().contains("已放弃"), "{err}");
}

/// 审计修复 #1 回归：ask 失败保持原分类（System）上抛——不包 Quality，
/// 否则基础设施故障会被 runner 当成「协议违例放弃」写成成功审计。
#[test]
fn instance_failure_keeps_original_error_classification() {
    struct FailSession;
    impl AuditSession for FailSession {
        fn ask(&mut self, _: &str) -> Result<String> {
            Err(DevforgeError::System("RPC 挂死模拟".into()))
        }
    }
    struct FailFactory;
    impl AuditSessionFactory for FailFactory {
        fn open(&mut self) -> Result<Box<dyn AuditSession>> {
            Ok(Box::new(FailSession))
        }
    }
    let mut f = FailFactory;
    let err = stage1_review_batch(&mut f, Path::new("/ws"), &unit("unit-001"), &[], "无").unwrap_err();
    assert!(
        matches!(err, DevforgeError::System(_)),
        "ask 失败应保持 System 分类：{err:?}"
    );
}

#[test]
fn stage2_cross_verifies_concerns() {
    let mut s = ScriptSession {
        replies: RefCell::new([r#"```json
{"findings": [{"path":"src/x.rs","line":3,"view":"consistency","severity":"minor","claim":"模式不一致","evidence":"src/x.rs:3"}], "concern_verdicts": [["错误处理模式一致", "不一致：change 吞错而 doc 不吞"]]}
```"#].into()),
    };
    let out = stage2_cross(&mut s, &[finding_for_cross()], &["假设A".into()]).unwrap();
    assert_eq!(out.findings.len(), 1);
    assert_eq!(out.findings[0].batch, "cross");
    assert_eq!(out.concern_verdicts[0].0, "错误处理模式一致");
}

fn finding_for_cross() -> Finding {
    Finding {
        path: "src/a.rs".into(),
        line: 1,
        view: crate::audit::finding::View::Correctness,
        severity: crate::audit::finding::Severity::Major,
        claim: "c".into(),
        evidence: "e".into(),
        batch: "unit-001".into(),
    }
}

#[test]
fn progress_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = Progress::default();
    p.done_batches.push("unit-001".into());
    p.stage0_done = true;
    p.save(dir.path()).unwrap();
    let loaded = Progress::load(dir.path());
    assert_eq!(p, loaded);
    assert_eq!(Progress::load(&dir.path().join("nope")), Progress::default());
}
