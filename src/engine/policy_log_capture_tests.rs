//! （2026-10-26 测试分离：自 src/engine/policy.rs 内联平移，纯移动零改动。）
//! 「原始错误全量可见」的行为断言：钉**日志面**（JSONL 条目的级别与字段），
//! 不是钉内部实现（不数函数被调了几次）。
//!
//! 错实现形态（任一命中即红）：
//! ① 基础设施故障落 WARN（应 ERROR）/ 质量失败落 ERROR（应 WARN）；
//! ② 落盘时缺 `error_chain`（source 链看不见）/ 缺 `attempt` / `retryable`；
//! ③ 尝试耗尽时少一条 `run.attempt_exhausted`（或次数字段错）。

use super::*;

use crate::observability::test_support::capture_jsonl;

#[test]
fn infra_failures_are_logged_as_error_with_the_full_error_text() {
    let text = capture_jsonl(|| {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |n| {
            Err(DevforgeError::System(format!("第 {n} 次挂了")))
        });
        assert_eq!(attempts, 4);
        assert!(result.is_err());
    });
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("run.attempt_failed"))
        .collect();
    assert_eq!(lines.len(), 4, "4 次尝试应各落一条尝试级条目：{text}");
    for (i, line) in lines.iter().enumerate() {
        let attempt = i + 1;
        assert!(line.contains("\"level\":\"ERROR\""), "基础设施故障应落 ERROR：{line}");
        assert!(line.contains("\"component\":\"run-retry\""), "{line}");
        assert!(line.contains("\"event\":\"run.attempt_failed\""), "{line}");
        assert!(line.contains(&format!("\"attempt\":{attempt}")), "{line}");
        assert!(line.contains("\"retryable\":true"), "第 1 次失败后仍会重启：{line}");
        assert!(line.contains("第"), "缺原始错误全文：{line}");
        assert!(line.contains("error_chain"), "缺 source 链字段：{line}");
    }
    let exhausted: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("run.attempt_exhausted"))
        .collect();
    assert_eq!(exhausted.len(), 1, "应有且仅有一条终态条目：{text}");
    assert!(exhausted[0].contains("\"attempts\":4"), "终态条目要带尝试次数：{}", exhausted[0]);
    assert!(exhausted[0].contains("\"level\":\"ERROR\""), "{}", exhausted[0]);
}

/// 引擎审计修复 #3 回归：is_infra_failure 分类矩阵——剥 WithContext 看根因；
/// 子进程死亡（EOF 与 try_wait 退出）同归 System 同重启；协议违例/质量/配置不重启。
#[test]
fn infra_failure_classification_matrix() {
    let io_err = std::io::Error::other("临时资源耗尽");
    let cases: Vec<(crate::error::DevforgeError, bool)> = vec![
        (DevforgeError::System("子进程已退出".into()), true),
        (DevforgeError::Io(io_err), true),
        // spawn 的 .ctx() 包装：剥开看根因 Io → 仍算基础设施（此前不重启 = 漏自愈）
        (
            DevforgeError::WithContext {
                context: "spawn pi (role=plan-agent)".into(),
                source: Box::new(DevforgeError::Io(
                    std::io::Error::other("boom"),
                )),
            },
            true,
        ),
        (
            DevforgeError::WithContext {
                context: "写台账".into(),
                source: Box::new(DevforgeError::System("盘满".into())),
            },
            true,
        ),
        (DevforgeError::RpcProtocol("坏记录".into()), false),
        (DevforgeError::Quality("审查打回".into()), false),
        (DevforgeError::Config("缺 API key".into()), false),
        (DevforgeError::agent("x", "子 Agent 决策失败"), false),
    ];
    for (e, want) in cases {
        let r: crate::error::Result<RunOutcome> = Err(e);
        assert_eq!(
            is_infra_failure(&r),
            want,
            "分类漂移：{r:?}"
        );
    }
}

#[test]
fn quality_failures_are_logged_as_warn_and_are_not_retried() {
    let text = capture_jsonl(|| {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
            Err(DevforgeError::Quality("审查打回：缺验收标准".into()))
        });
        assert_eq!(attempts, 1, "质量失败不得重启");
        assert!(result.is_err());
    });
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("run.attempt_failed"))
        .collect();
    assert_eq!(lines.len(), 1, "质量失败应落一条条目：{text}");
    assert!(lines[0].contains("\"level\":\"WARN\""), "质量失败应落 WARN：{}", lines[0]);
    assert!(lines[0].contains("\"retryable\":false"), "质量失败不重启：{}", lines[0]);
    assert!(lines[0].contains("\"attempt\":1"), "{}", lines[0]);
    assert!(lines[0].contains("缺验收标准"), "缺原始错误全文：{}", lines[0]);
    assert!(lines[0].contains("error_chain"), "缺 source 链字段：{}", lines[0]);
}

/// 审计修复 #2 回归：配置类失败（缺 API key 等 success:false）不进自动重启——
/// 错实现形态：command_failure 返回 System → is_infra_failure 命中 → 盲重启 4 次
/// （退避 10s/30s/90s），配置问题重启永远不会变好。
#[test]
fn config_failures_are_not_retried_and_logged_as_warn() {
    let text = capture_jsonl(|| {
        let (result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
            Err(DevforgeError::Config("缺 API key（配置问题）".into()))
        });
        assert_eq!(attempts, 1, "配置失败不得重启：{attempts}");
        assert!(result.is_err());
    });
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("run.attempt_failed"))
        .collect();
    assert_eq!(lines.len(), 1, "配置失败应落一条条目：{text}");
    assert!(lines[0].contains("\"level\":\"WARN\""), "{}", lines[0]);
    assert!(lines[0].contains("\"retryable\":false"), "配置失败不重启：{}", lines[0]);
    assert!(lines[0].contains("缺 API key"), "缺原始错误全文：{}", lines[0]);
}

#[test]
fn the_retry_layer_updates_the_run_context_attempt() {
    // 错实现形态：重试层不更新 run 上下文 → `map_intervention` 读到的 attempt 不是本次的
    let id = "run-attempt";
    // 观测根按 run 注入到临时目录 → 不写用户 HOME
    let dir = tempfile::tempdir().unwrap();
    crate::observability::begin_run_in(dir.path().to_path_buf(), id, "coding");
    let (_result, attempts) = run_with_retry(&RetryPolicy::fast(), &RunControl::new(), |_| {
        Err(DevforgeError::System("挂了".into()))
    });
    assert_eq!(attempts, 4);
    let snap = crate::observability::current().expect("run 上下文应存在");
    assert_eq!(snap.attempt, 4, "run 上下文的 attempt 应与本次尝试一致");
    assert_eq!(crate::observability::attempt(), 4, "attempt 读取口应给同一个值");
    crate::observability::end_run(id);
}
