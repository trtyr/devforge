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
    let mut s = ScriptSession { replies: RefCell::new([PLAN_REPLY].into()) };
    let plan = stage0_plan(&mut s, &[unit("unit-001"), unit("unit-002")]).unwrap();
    assert_eq!(plan.priority, vec!["unit-001"]);
    assert_eq!(plan.skip, vec!["unit-002"]);
    assert_eq!(plan.cross_concerns.len(), 1);
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
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].batch, "unit-009", "发现应带批次溯源");
    assert_eq!(factory.opens, 1, "无违例应只开一次会话");
}

#[test]
fn stage1_retries_once_on_protocol_violation_then_gives_up() {
    // FIFO：压入序即消费序——第一次真正拿到「违例」，重试循环才被真实走到
    // （审计修复 #3：LIFO 时代本用例是空洞绿，删掉重试循环仍过）。
    let mut factory = ScriptFactory {
        replies: std::collections::VecDeque::from([["违例".into()].into(), [FINDINGS_REPLY].into()]),
        opens: 0,
    };
    let u = unit("unit-001");
    let f = stage1_review_batch(&mut factory, Path::new("/ws"), &u, &[], "无").unwrap();
    assert_eq!(f.len(), 1, "第一次违例重开后成功");
    assert_eq!(factory.opens, 2, "违例→成功必须恰好开两次会话（换实例）");
    // 两次全违例 → 放弃
    let mut factory = ScriptFactory {
        replies: std::collections::VecDeque::from([["违例2".into()].into(), ["违例3".into()].into()]),
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
