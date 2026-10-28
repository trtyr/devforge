//! （2026-10-26 测试分离：自 src/audit/runner.rs 内联平移，纯移动零改动。）
use super::*;

/// 复验 M3 缺陷①回归：断点续跑时已落盘的 findings 从黑板回读，
/// 不随进程死亡丢失；进度里但文件缺失的历史批次跳过不崩。
fn sample_finding() -> Finding {
    Finding {
        path: "src/x.rs".into(),
        line: 1,
        view: crate::audit::finding::View::Correctness,
        severity: crate::audit::finding::Severity::Major,
        claim: "测试发现".into(),
        evidence: "src/x.rs:1".into(),
        batch: "unit-001".into(),
    }
}

fn unit(id: &str) -> AuditUnit {
    AuditUnit {
        id: id.into(),
        files: vec!["src/a.rs".into()],
        label: format!("label-{id}"),
        focus: Vec::new(),
        focus_range: None,
    }
}

#[test]
fn persisted_findings_are_read_back_on_resume() {
    let dir = tempfile::tempdir().unwrap();
    let findings_dir = dir.path().join("audit-findings");
    std::fs::create_dir_all(&findings_dir).unwrap();
    let f = sample_finding();
    std::fs::write(
        findings_dir.join("unit-001.json"),
        serde_json::to_vec_pretty(&vec![f]).unwrap(),
    )
    .unwrap();
    let u1 = unit("unit-001");
    let u2 = unit("unit-002");
    let units = vec![&u1, &u2];
    let (got, recovered, bad) = load_persisted_findings(&findings_dir, &units);
    assert_eq!(got.len(), 1, "盘上有文件的批次回读");
    assert_eq!(got[0].batch, "unit-001");
    assert_eq!(recovered, vec!["unit-001".to_string()], "文件 = 完成事实");
    assert!(bad.is_empty(), "缺失 = 未完成，不是坏");
}

/// 2026-10-25 事故回归：done 打勾丢失（崩溃在 join 前）但 findings 文件在盘
/// ——对账必须以文件为准恢复完成态，不得把已完成批次重审一遍（当时白烧 40 分钟）。
#[test]
fn findings_file_without_done_mark_is_recovered_not_rereviewed() {
    let dir = tempfile::tempdir().unwrap();
    let findings_dir = dir.path().join("audit-findings");
    std::fs::create_dir_all(&findings_dir).unwrap();
    let mut f = sample_finding();
    f.line = 2;
    f.claim = "已审完但打勾丢了".into();
    f.evidence = "src/x.rs:2".into();
    std::fs::write(
        findings_dir.join("unit-001.json"),
        serde_json::to_vec_pretty(&vec![f]).unwrap(),
    )
    .unwrap();
    let u1 = unit("unit-001");
    let units = vec![&u1];
    let (got, recovered, bad) = load_persisted_findings(&findings_dir, &units);
    assert_eq!(recovered, vec!["unit-001".to_string()], "文件在盘 = 已完成");
    assert_eq!(got.len(), 1);
    assert!(bad.is_empty());
}

// ---------- 审计修复 #1/#5 回归 ----------

/// Stage 0 成功后，批次会话 ask 失败（System 类）：整次审计必须 Err，
/// 错误不得伪装成 Quality，批次发现不落盘、报告不生成（无「成功审计」假象）。
#[test]
fn batch_instance_failure_aborts_whole_run_not_abandoned() {
    struct PlanOnlySession;
    impl super::super::pipeline::AuditSession for PlanOnlySession {
        fn ask(&mut self, _: &str) -> crate::error::Result<String> {
            Ok("```json\n{\"priority\": [], \"skip\": [], \"cross_concerns\": []}\n```"
                .into())
        }
    }
    struct FailAskSession;
    impl super::super::pipeline::AuditSession for FailAskSession {
        fn ask(&mut self, _: &str) -> crate::error::Result<String> {
            Err(crate::error::DevforgeError::System("RPC 挂死模拟".into()))
        }
    }
    /// 第一次 open 给 Stage 0 会话（成功），之后全部给 ask 失败的批次会话。
    struct Stage0OkThenFailFactory { opens: usize }
    impl super::super::pipeline::AuditSessionFactory for Stage0OkThenFailFactory {
        fn open(&mut self) -> crate::error::Result<Box<dyn super::super::pipeline::AuditSession>> {
            self.opens += 1;
            if self.opens == 1 {
                Ok(Box::new(PlanOnlySession))
            } else {
                Ok(Box::new(FailAskSession))
            }
        }
    }
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut task = crate::domain::task::Task::new("audit-self", "p-audit", "code-review", 1);
    task.workspace = ws.path().to_path_buf();
    let control = crate::domain::runner::RunControl::new();
    assert!(!control.is_cancelled());
    let mut factory = Stage0OkThenFailFactory { opens: 0 };
    let err = run_audit(&task, root.path(), &control, &mut factory).unwrap_err();
    assert!(
        !matches!(err, crate::error::DevforgeError::Quality(_)),
        "实例失败不得伪装成 Quality：{err:?}"
    );
    let blackboard = root.path().join("p-audit").join("audit-self");
    assert!(
        !blackboard.join("audit-report.json").exists(),
        "实例失败打断时不得生成报告（无成功假象）"
    );
    let findings_dir = blackboard.join("audit-findings");
    let leaked: usize = std::fs::read_dir(&findings_dir)
        .map(|d| d.flatten().count())
        .unwrap_or(0);
    assert_eq!(leaked, 0, "被打断的批次不得有发现落盘（abandoned 不含该批的文件面证据）");
}

/// 审计修复 #5：cancel 置位 → run_audit 入口即中断，不碰 factory。
#[test]
fn cancelled_control_aborts_audit_before_any_session() {
    struct PanicFactory;
    impl super::super::pipeline::AuditSessionFactory for PanicFactory {
        fn open(&mut self) -> crate::error::Result<Box<dyn super::super::pipeline::AuditSession>> {
            panic!("cancel 置位后不得开任何会话");
        }
    }
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut task = crate::domain::task::Task::new("audit-self", "p-audit", "code-review", 1);
    task.workspace = ws.path().to_path_buf();
    let control = crate::domain::runner::RunControl::new();
    control.cancel();
    let mut factory = PanicFactory;
    let err = run_audit(&task, root.path(), &control, &mut factory).unwrap_err();
    assert!(err.to_string().contains("干预"), "应报干预中断：{err:?}");
}

/// 审计修复 #2 回归：Stage 2 跨单元发现（LLM 二次生成）同样过核验——
/// 穿越路径进 dropped_ghosts，不进报告 kept；真实跨单元发现照常保留。
#[test]
fn stage2_findings_pass_verify_ghosts_go_to_dropped() {
    use super::super::pipeline::{AuditSession, AuditSessionFactory};
    struct ScriptedSession {
        replies: std::collections::VecDeque<&'static str>,
    }
    impl AuditSession for ScriptedSession {
        fn ask(&mut self, _: &str) -> crate::error::Result<String> {
            self.replies
                .pop_front()
                .map(|s| s.to_string())
                .ok_or_else(|| crate::error::DevforgeError::System("回复耗尽".into()))
        }
    }
    /// 按会话序切换脚本：会话 1 = Stage 0，会话 2 = 批次，会话 3 = Stage 2。
    struct SequencedFactory {
        scripts: std::collections::VecDeque<std::collections::VecDeque<&'static str>>,
    }
    impl AuditSessionFactory for SequencedFactory {
        fn open(&mut self) -> crate::error::Result<Box<dyn AuditSession>> {
            let s = self
                .scripts
                .pop_front()
                .ok_or_else(|| crate::error::DevforgeError::System("会话耗尽".into()))?;
            Ok(Box::new(ScriptedSession { replies: s }))
        }
    }
    const PLAN: &str = "```json\n{\"priority\": [], \"skip\": [], \"cross_concerns\": []}\n```";
    const FINDINGS: &str = r#"<audit-findings>
[{"path":"src/a.rs","line":1,"view":"correctness","severity":"major","claim":"c","evidence":"src/a.rs:1"}]
</audit-findings>"#;
    const CROSS_GHOST: &str = "```json\n{\"findings\": [\n{\"path\":\"../../etc/passwd\",\"line\":1,\"view\":\"correctness\",\"severity\":\"critical\",\"claim\":\"越界发现\",\"evidence\":\"../../etc/passwd:1\"},\n{\"path\":\"src/a.rs\",\"line\":1,\"view\":\"security\",\"severity\":\"minor\",\"claim\":\"真实跨单元发现\",\"evidence\":\"src/a.rs:1\"}], \"concern_verdicts\": []}\n```";
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut task = crate::domain::task::Task::new("audit-self", "p-audit", "code-review", 1);
    task.workspace = ws.path().to_path_buf();
    let control = crate::domain::runner::RunControl::new();
    let mut factory = SequencedFactory {
        scripts: [PLAN, FINDINGS, CROSS_GHOST]
            .into_iter()
            .map(|s| [s].into_iter().collect())
            .collect(),
    };
    run_audit(&task, root.path(), &control, &mut factory).unwrap();
    let blackboard = root.path().join("p-audit").join("audit-self");
    let v: serde_json::Value = serde_json::from_slice(
        &std::fs::read(blackboard.join("audit-report.json")).unwrap(),
    )
    .unwrap();
    let kept = v["findings"]["kept"].as_array().unwrap();
    assert!(
        !kept.iter().any(|f| f["path"].as_str().unwrap().contains("..")),
        "穿越发现不得进 kept：{kept:?}"
    );
    let dropped = v["findings"]["dropped_ghosts"].as_array().unwrap();
    assert!(
        dropped.iter().any(|d| d[0].as_str().unwrap().contains("..")),
        "Stage 2 穿越发现应进 dropped_ghosts：{dropped:?}"
    );
}

/// 审计修复 #6：预查报告含本批文件的 unwrap/吞错/埋点/行数机检事实。
#[test]
fn precheck_report_counts_machine_facts() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(
        ws.path().join("a.rs"),
        "fn f() {\n    let x = vec![1].pop().unwrap(); // 行尾注释不跳过\n    let _ = std::fs::read_to_string(\"x\");\n    tracing::info!(\"hello\");\n}\n",
    )
    .unwrap();
    let report = precheck_report(ws.path(), &["a.rs".to_string()], None);
    assert!(report.contains("a.rs"), "{report}");
    assert!(report.contains("unreachable 1 处"), "{report}");
    assert!(report.contains("吞错 1 处"), "{report}");
    assert!(report.contains("tracing 事件 1 处"), "{report}");
    assert!(report.contains("共 5 行"), "{report}");
}

// ---------- 全仓审计（第六份）回归 ----------

struct SSess {
    replies: std::collections::VecDeque<&'static str>,
}
impl crate::audit::pipeline::AuditSession for SSess {
    fn ask(&mut self, _: &str) -> crate::error::Result<String> {
        self.replies
            .pop_front()
            .map(|s| s.to_string())
            .ok_or_else(|| crate::error::DevforgeError::System("回复耗尽".into()))
    }
}
struct SFactory {
    scripts: std::collections::VecDeque<Vec<&'static str>>,
}
impl crate::audit::pipeline::AuditSessionFactory for SFactory {
    fn open(&mut self) -> crate::error::Result<Box<dyn crate::audit::pipeline::AuditSession>> {
        let s = self
            .scripts
            .pop_front()
            .ok_or_else(|| crate::error::DevforgeError::System("会话耗尽".into()))?;
        Ok(Box::new(SSess {
            replies: s.into_iter().collect(),
        }))
    }
}
fn mk(scripts: Vec<Vec<&'static str>>) -> SFactory {
    SFactory {
        scripts: scripts.into_iter().collect(),
    }
}

/// 测试用并发度覆盖（避免改环境变量影响并行跑的其他用例）。
static CONCURRENCY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub(super) fn concurrency_override() -> Option<usize> {
    match CONCURRENCY.load(std::sync::atomic::Ordering::SeqCst) {
        0 => None,
        n => Some(n),
    }
}
fn set_concurrency(n: usize) {
    CONCURRENCY.store(n, std::sync::atomic::Ordering::SeqCst);
}

/// 支持并行的工厂：脚本队列共享，fork 出的子工厂取下一个脚本（模拟真实多进程）。
struct SharedScriptFactory {
    scripts: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<Vec<&'static str>>>>,
}
impl crate::audit::pipeline::AuditSessionFactory for SharedScriptFactory {
    fn open(&mut self) -> crate::error::Result<Box<dyn crate::audit::pipeline::AuditSession>> {
        let s = self
            .scripts
            .lock()
            .expect("scripts 锁")
            .pop_front()
            .ok_or_else(|| crate::error::DevforgeError::System("会话耗尽".into()))?;
        Ok(Box::new(SSess {
            replies: s.into_iter().collect(),
        }))
    }
    fn fork(&self) -> Option<Box<dyn crate::audit::pipeline::AuditSessionFactory + Send>> {
        Some(Box::new(SharedScriptFactory {
            scripts: self.scripts.clone(),
        }))
    }
}

fn audit_task() -> (tempfile::TempDir, tempfile::TempDir, crate::domain::task::Task) {
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.rs"), "fn main() {}\n").unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut task = crate::domain::task::Task::new("audit-x", "p-audit", "code-review", 1);
    task.workspace = ws.path().to_path_buf();
    (ws, root, task)
}

const P0: &str = "```json\n{\"priority\": [], \"skip\": [], \"cross_concerns\": []}\n```";
const F1: &str = r#"<audit-findings>
[{"path":"a.rs","line":1,"view":"correctness","severity":"major","claim":"c","evidence":"fn main() {} （a.rs:1）"}]
</audit-findings>"#;
const C0: &str = "```json\n{\"findings\": [], \"concern_verdicts\": []}\n```";
const JUNK: &str = "我先想一下……（这里没有 json 块）";

/// 全仓审计 M5：空仓/错目录不得 Completed + 空报告（假成功最难排查）。
#[test]
fn empty_workspace_fails_instead_of_fake_success() {
    let empty = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut task = crate::domain::task::Task::new("t", "p", "code-review", 1);
    task.workspace = empty.path().to_path_buf();
    let control = crate::domain::runner::RunControl::new();
    let mut f = mk(vec![vec![P0]]);
    let err = run_audit(&task, root.path(), &control, &mut f).unwrap_err();
    assert!(format!("{err}").contains("无可审源码"), "{err}");
}

/// 全仓审计 M2：Stage 0 首次输出没 json 块 → 换实例重试后继续，不报废整次。
#[test]
fn stage0_format_jitter_is_retried() {
    let (_ws, root, task) = audit_task();
    let control = crate::domain::runner::RunControl::new();
    let mut f = mk(vec![vec![JUNK], vec![P0], vec![F1], vec![C0]]);
    run_audit(&task, root.path(), &control, &mut f).unwrap();
}

/// 全仓审计 M2：Stage 2（全部批次跑完之后）同理。
#[test]
fn stage2_format_jitter_is_retried() {
    let (_ws, root, task) = audit_task();
    let control = crate::domain::runner::RunControl::new();
    let mut f = mk(vec![vec![P0], vec![F1], vec![JUNK], vec![C0]]);
    run_audit(&task, root.path(), &control, &mut f).unwrap();
}

/// 第三方 r3 M2：skip 指向非生成物批次 → 被忽略，该批仍进审查（自审不能再
/// 用一句「生成物」把 src/audit 跳过）。旧的「skip 全量拒收」用例已并到这里：
/// skip 只能命中生成物批次，而切入器只切源码面，那条 100% 分支现在只是防御性兑底。
#[test]
fn skip_of_core_unit_is_ignored_and_still_reviewed() {
    let (_ws, root, task) = audit_task();
    let control = crate::domain::runner::RunControl::new();
    let skip_core =
        "```json\n{\"priority\": [], \"skip\": [\"unit-001\"], \"cross_concerns\": []}\n```";
    // 先 skip 后正常审：若 skip 被采纳，第二/三次会话不会被消费 → 脚本耗尽时报错
    let mut f = mk(vec![vec![skip_core], vec![F1], vec![C0]]);
    run_audit(&task, root.path(), &control, &mut f)
        .expect("核心批次被 skip 时应忽略 skip 并照审");
    let progress: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            root.path()
                .join("p-audit")
                .join("audit-x")
                .join("audit-progress.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        progress["done_batches"].as_array().unwrap().len(),
        1,
        "unit-001 必须真被审（不能被 skip 掉）：{progress}"
    );
}

/// 全仓审计 M7/M8：JSON 的 cross_analysis 只写核验后集合（幽灵单放到 dropped），
/// 跨层重复只算一次（summary 不因子与本地重复而虚高）。
#[test]
fn cross_layer_duplicates_count_once_and_json_excludes_ghosts() {
    let (_ws, root, task) = audit_task();
    // 本地与跨层各一条同位置同主张（evidence 必须引原文，否则被核验器当猜行号丢掉）
    let cross_dup = r#"```json
{"findings": [{"path":"a.rs","line":1,"view":"correctness","severity":"major","claim":"c","evidence":"fn main() {} （a.rs:1）"},{"path":"../x.rs","line":1,"view":"correctness","severity":"critical","claim":"ghost","evidence":"../x.rs:1"}], "concern_verdicts": []}
```"#;
    let control = crate::domain::runner::RunControl::new();
    let mut f = mk(vec![vec![P0], vec![F1], vec![cross_dup]]);
    run_audit(&task, root.path(), &control, &mut f).unwrap();
    let v: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            root.path()
                .join("p-audit")
                .join("audit-x")
                .join("audit-report.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let cross = v["cross_analysis"]["findings"].as_array().unwrap();
    assert!(
        !cross.iter().any(|f| f["path"].as_str().unwrap().contains("..")),
        "JSON 不得含未核验幽灵：{cross:?}"
    );
    assert_eq!(v["severity_summary"]["major"], 1, "跨层重复不得重复计数：{v}");
    assert_eq!(v["severity_summary"]["critical"], 0, "幽灵不得入汇总：{v}");
    assert!(
        v["cross_analysis"]["dropped_ghosts"]
            .as_array()
            .unwrap()
            .len()
            == 1,
        "幽灵应单列在 cross_analysis.dropped_ghosts：{v}"
    );
}

/// 全仓审计 m2：断点批次发现文件损坏 → 摘掉打勾**重审**（不得永久丢）。
#[test]
fn corrupt_findings_file_is_rereviewed() {
    let (_ws, root, task) = audit_task();
    let blackboard = root.path().join("p-audit").join("audit-x");
    std::fs::create_dir_all(blackboard.join("audit-findings")).unwrap();
    std::fs::write(
        blackboard.join("audit-progress.json"),
        r#"{"stage0_done":true,"done_batches":["unit-001"],"stage2_done":false}"#,
    )
    .unwrap();
    std::fs::write(
        blackboard.join("audit-stage0.json"),
        r#"{"priority":[],"skip":[],"cross_concerns":[]}"#,
    )
    .unwrap();
    std::fs::write(blackboard.join("audit-findings/unit-001.json"), b"{broken").unwrap();
    let control = crate::domain::runner::RunControl::new();
    // Stage 0 走断点；批次因发现损坏而重审（消费 1 个会话）；Stage 2 正常
    let mut f = mk(vec![vec![F1], vec![C0]]);
    run_audit(&task, root.path(), &control, &mut f).unwrap();
    let findings: serde_json::Value = serde_json::from_slice(
        &std::fs::read(blackboard.join("audit-findings/unit-001.json")).unwrap(),
    )
    .unwrap();
    assert!(
        findings.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "损坏的发现文件应被重审结果覆盖：{findings}"
    );
}

/// 全仓审计 M3：工厂支持 fork 时批次并行，两批都落盘。
#[test]
fn parallel_batches_review_every_unit() {
    let ws = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(ws.path().join("src")).unwrap();
    std::fs::create_dir_all(ws.path().join("tools")).unwrap();
    std::fs::write(ws.path().join("src/a.rs"), "fn a() {}\n").unwrap();
    std::fs::write(ws.path().join("tools/b.rs"), "fn b() {}\n").unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut task = crate::domain::task::Task::new("audit-par", "p-audit", "code-review", 1);
    task.workspace = ws.path().to_path_buf();
    set_concurrency(2);
    let control = crate::domain::runner::RunControl::new();
    let scripts = std::sync::Arc::new(std::sync::Mutex::new(
        std::collections::VecDeque::from(vec![vec![P0], vec![F1], vec![F1], vec![C0]]),
    ));
    let mut f = SharedScriptFactory { scripts };
    let outcome = run_audit(&task, root.path(), &control, &mut f);
    set_concurrency(0);
    outcome.unwrap();
    let progress: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            root.path()
                .join("p-audit")
                .join("audit-par")
                .join("audit-progress.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        progress["done_batches"].as_array().unwrap().len(),
        2,
        "并行路径下两批都应完成：{progress}"
    );
}

/// 第三方 r3 M1：worker 内 panic 收成该批 System（不再向主线程扔 usize::MAX 哨兵），
/// 且同批已成功的兄弟批次发现已落盘（兄弟失败不带走已完成批次）。
#[test]
fn parallel_panic_is_contained_and_successful_batch_persisted() {
    struct LabelSession;
    impl crate::audit::pipeline::AuditSession for LabelSession {
        fn ask(&mut self, instruction: &str) -> crate::error::Result<String> {
            // 只匹「本批」那一行——指令里的「其他批次」列表也会列出别的 label
            if instruction.contains("审查批次 `label-unit-002`") {
                panic!("模拟审查线程 panic");
            }
            Ok(F1.to_string())
        }
    }
    struct LabelFactory;
    impl crate::audit::pipeline::AuditSessionFactory for LabelFactory {
        fn open(
            &mut self,
        ) -> crate::error::Result<Box<dyn crate::audit::pipeline::AuditSession>> {
            Ok(Box::new(LabelSession))
        }
        fn fork(
            &self,
        ) -> Option<Box<dyn crate::audit::pipeline::AuditSessionFactory + Send>> {
            Some(Box::new(LabelFactory))
        }
    }

    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("a.rs"), "fn a() {}\n").unwrap();
    let findings_dir = tempfile::tempdir().unwrap();
    let units = [
        AuditUnit {
            id: "unit-001".into(),
            files: vec!["a.rs".into()],
            label: "label-unit-001".into(),
            focus: Vec::new(),
            focus_range: None,
        },
        AuditUnit {
            id: "unit-002".into(),
            files: vec!["a.rs".into()],
            label: "label-unit-002".into(),
            focus: Vec::new(),
            focus_range: None,
        },
    ];
    let refs: Vec<&AuditUnit> = units.iter().collect();
    let labels: Vec<String> = units.iter().map(|u| u.label.clone()).collect();
    let control = crate::domain::runner::RunControl::new();
    set_concurrency(2);
    let results = run_stage1_parallel(
        &LabelFactory,
        &refs,
        &labels,
        ws.path(),
        2,
        &control,
        findings_dir.path(),
    );
    set_concurrency(0);
    assert_eq!(results.len(), 2, "两批都要有结果（panic 不得吞掉其他批）");
    let panic_res = results
        .iter()
        .find(|(_, id, _)| id == "unit-002")
        .expect("unit-002 必须在结果里");
    let msg = format!(
        "{:?}",
        panic_res.2.as_ref().expect_err("panic 批必须是 Err")
    );
    assert!(msg.contains("panic"), "panic 要收成带说明的 System：{msg}");
    assert!(
        findings_dir.path().join("unit-001.json").is_file(),
        "兄弟批失败不得带走已完成批次的发现（r3 M1 回潮点）"
    );
}

/// 第三方 r3 M4：0 = 不限时限（旧行为），非 0 = 给未来截止点。
#[test]
fn batch_deadline_zero_means_unlimited() {
    assert!(batch_deadline_from_ms(0).is_none());
    let at = batch_deadline_from_ms(1_000).expect("非 0 应有截止点");
    assert!(at > std::time::Instant::now());
}

/// r3 m5 遗留收口：分片预查只数本段行（否则同文件多分片看到相同计数）。
#[test]
fn precheck_counts_only_the_focus_range() {
    let ws = tempfile::tempdir().unwrap();
    // 第 1–3 行有 unwrap，第 4–6 行干净
    std::fs::write(
        ws.path().join("a.rs"),
        "let a = x.unwrap();\nlet b = y.unwrap();\nlet c = z.unwrap();\nlet d = 1;\nlet e = 2;\nlet f = 3;\n",
    )
    .unwrap();
    let whole = precheck_report(ws.path(), &["a.rs".to_string()], None);
    assert!(whole.contains("共 6 行"), "{whole}");
    assert!(whole.contains("unwrap·expect·panic·todo·unreachable 3 处"), "{whole}");
    let tail = precheck_report(ws.path(), &["a.rs".to_string()], Some((4, 6)));
    assert!(tail.contains("第 4–6 行"), "{tail}");
    assert!(tail.contains("共 3 行"), "{tail}");
    assert!(tail.contains("unreachable 0 处"), "分片不得看到整文件计数：{tail}");
}

/// 全仓审计 m1：Stage 0 断点产物损坏 → 清标记重算，不硬失败。
#[test]
fn corrupt_stage0_checkpoint_is_recomputed() {
    let (_ws, root, task) = audit_task();
    let blackboard = root.path().join("p-audit").join("audit-x");
    std::fs::create_dir_all(&blackboard).unwrap();
    std::fs::write(blackboard.join("audit-stage0.json"), b"not json").unwrap();
    std::fs::write(
        blackboard.join("audit-progress.json"),
        r#"{"stage0_done":true,"done_batches":[],"stage2_done":false}"#,
    )
    .unwrap();
    let control = crate::domain::runner::RunControl::new();
    let mut f = mk(vec![vec![P0], vec![F1], vec![C0]]);
    run_audit(&task, root.path(), &control, &mut f).unwrap();
}

// ── 2026-10-27 自审 audit/runner 批回归 ──

/// 自审 minor（30）：目录名启发式不得单独成立——手写源码放在 dist/ 或命名为 *.snap
/// 也必须照审（旧口径整批静默跳过，与「宁可多审」相悖）。
#[test]
fn skippable_unit_requires_a_generated_marker_inside_generated_dirs() {
    let unit = |files: &[&str]| AuditUnit {
        id: "u".into(),
        files: files.iter().map(|s| s.to_string()).collect(),
        label: "u".into(),
        focus: Vec::new(),
        focus_range: None,
    };
    // 纯生成物目录 + 生成物标记 → 可跳
    assert!(skippable_unit(&unit(&["dist/app.bundle.js"])));
    assert!(skippable_unit(&unit(&["node_modules/x/index.d.ts"])));
    // 手写源码放在生成物目录里 → 不跳（旧口径会跳过）
    assert!(!skippable_unit(&unit(&["dist/handwritten.rs"])));
    assert!(!skippable_unit(&unit(&["vendor/core.rs"])));
    // 后缀类（.lock / .min.js）独立成立
    assert!(skippable_unit(&unit(&["Cargo.lock"])));
    assert!(skippable_unit(&unit(&["web/app.min.js"])));
    // 普通源码 → 不跳
    assert!(!skippable_unit(&unit(&["src/a.rs"])));
}

/// 自审 minor（31）：断点里的 abandoned 历史重复项在读出时去重（报告计数不虚高）。
#[test]
fn abandoned_history_is_deduped_by_unit() {
    let mut progress = crate::audit::pipeline::Progress::default();
    progress.abandoned.push("unit-001：超时".into());
    progress.abandoned.push("unit-001：协议违例".into());
    progress.abandoned.push("unit-002：空回复".into());
    let mut all = progress.abandoned.clone();
    all.sort();
    all.dedup_by(|a, b| {
        let key = |s: &String| s.split('：').next().unwrap_or(s).to_string();
        key(a) == key(b)
    });
    assert_eq!(all.len(), 2, "同一单元只留一条：{all:?}");
}
