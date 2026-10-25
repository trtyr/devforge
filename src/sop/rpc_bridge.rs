//! RPC 桥接：把 sop 的会话 trait 接到 engine::RpcClient 上。
//!
//! - [`RpcPlanSession`]：Plan Agent 会话（wait_settled_with_ui 处理 ask-user-question 子协议）
//! - [`RpcReviewSession`] / [`RpcReviewFactory`]：Review 每轮全新 spawn，Drop 即杀（用完即弃）

use crate::domain::RunId;
use crate::engine::client::RpcClient;
use crate::engine::process::SpawnSpec;
use crate::error::{Context as _, Result};
use crate::sop::planning::{PlanAgentSession, ReviewSession, ReviewSessionFactory, UserLink};

/// Plan Agent 的 RPC 会话（活：同一进程跨轮次复用）。
pub struct RpcPlanSession {
    client: RpcClient,
}

impl RpcPlanSession {
    pub fn new(client: RpcClient) -> Self {
        Self { client }
    }
}

impl PlanAgentSession for RpcPlanSession {
    fn converse(&mut self, message: &str, ui: &mut dyn UserLink) -> Result<String> {
        let id = self.client.prompt(message)?;
        let outcome = self
            .client
            .wait_settled_with_ui(|extra| {
                // UI 请求字段提取：真实线级形状优先 title（select dialog），
                // 兼容旧合成形状 request.question，兜底原始 JSON dump
                let question = extra
                    .get("title")
                    .and_then(|t| t.as_str())
                    .map(str::to_string)
                    .or_else(|| {
                        extra
                            .get("request")
                            .and_then(|r| r.get("question"))
                            .and_then(|q| q.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| {
                        let extra = serde_json::to_string(extra).unwrap_or_default();
                        format!("Agent 请求输入: {extra}")
                    });
                // select dialog 的选项透传给 SSE（前端可渲染按钮；MVP 由 AI/用户自由应答）
                let options: Vec<String> = extra
                    .get("options")
                    .and_then(|o| o.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|o| match o {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let refs: Vec<&str> = options.iter().map(String::as_str).collect();
                ui.ask(&question, &refs)
            })
            .ctx("Plan 对话（含 UI 子协议）")?;
        self.client.require_success(&outcome, &id)?;
        Ok(outcome.text)
    }
}

/// Review 的 RPC 会话（一次性实例，Drop 即杀进程——用完即弃 decisions/004）。
pub struct RpcReviewSession {
    client: RpcClient,
    /// 角色名（报错时指明是哪个审查会话）——C19-R2。
    role: String,
}

impl ReviewSession for RpcReviewSession {
    fn review(&mut self, instruction: &str) -> Result<String> {
        self.review_with_deadline(instruction, None)
    }

    /// 第三方 r3 M4：批次墙钟——到点让 wait_settled 返回 Quality（该批放弃，余批继续）。
    fn review_with_deadline(
        &mut self,
        instruction: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<String> {
        let id = self.client.prompt(instruction)?;
        let outcome = self.client.wait_settled_deadline(deadline)?;
        self.client.require_success(&outcome, &id)?;
        // C19-R2（e2e test_19 实证）：审查进程可能“静默死亡”——会话正常结束但
        // 未产出任何正文。空串不得当作合法审查结论（否那么保守解析会把它当成
        // “协议违例”并错误回注给被审 Agent），必须作为基础设施故障上抛。
        if outcome.text.trim().is_empty() {
            return Err(crate::error::DevforgeError::System(format!(
                "审查会话 `{}` 无有效回复（进程正常结束但未产出一字）——按基础设施故障处理，不作为审查结论",
                self.role
            )));
        }
        Ok(outcome.text)
    }
}

/// Review 会话工厂：每轮用新 spec spawn 全新 pi 进程。
///
/// 全仓审计 M3：spec 生成器与序号计数器放到 `Arc<Mutex<…>>` 后面，工厂副本共享它们——
/// 并行批次派生的多个工厂不会碰撞 session 子目录。
pub struct RpcReviewFactory {
    make_spec: std::sync::Arc<std::sync::Mutex<Box<dyn FnMut() -> SpawnSpec + Send>>>,
}

impl Clone for RpcReviewFactory {
    fn clone(&self) -> Self {
        Self { make_spec: self.make_spec.clone() }
    }
}

impl RpcReviewFactory {
    /// 每轮调用一次 spec 生成器（session dir / run 细分由生成器决定）。
    pub fn new(make_spec: impl FnMut() -> SpawnSpec + Send + 'static) -> Self {
        Self {
            make_spec: std::sync::Arc::new(std::sync::Mutex::new(Box::new(make_spec))),
        }
    }

    /// 便捷构造：同一 profile，每轮递增序号的 session 子目录（planning 沿用：cwd=服务器 cwd）。
    pub fn per_round(profile_dir: std::path::PathBuf, session_root: std::path::PathBuf) -> Self {
        let counter = std::sync::atomic::AtomicU32::new(0u32);
        Self::new(move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            SpawnSpec::new(
                profile_dir.clone(),
                "plan-review",
                RunId::generate().0,
                session_root.join(format!("review-{n}")),
            )
        })
    }

    /// P005：显式指定角色与子进程工作目录（编码审查的 cwd = 黑板，报告与树同址）。
    pub fn per_round_in(
        profile_dir: std::path::PathBuf,
        role: impl Into<String> + 'static,
        session_root: std::path::PathBuf,
        working_dir: std::path::PathBuf,
    ) -> Self {
        let counter = std::sync::atomic::AtomicU32::new(0u32);
        let role = role.into();
        Self::new(move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            SpawnSpec::new(
                profile_dir.clone(),
                role.clone(),
                RunId::generate().0,
                session_root.join(format!("review-{n}")),
            )
            .with_working_dir(working_dir.clone())
        })
    }
}

impl ReviewSessionFactory for RpcReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        let spec = (self
            .make_spec
            .lock()
            .expect("spec 生成器锁"))();
        let role = spec.role.clone();
        let client = RpcClient::spawn(spec)?;
        Ok(Box::new(RpcReviewSession { client, role }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DevforgeError;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    struct ScriptUser {
        answers: VecDeque<String>,
        asked: Vec<String>,
    }
    impl ScriptUser {
        fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|s| s.to_string()).collect(),
                asked: Vec::new(),
            }
        }
    }
    impl UserLink for ScriptUser {
        fn ask(&mut self, question: &str, _options: &[&str]) -> Result<String> {
            self.asked.push(question.into());
            self.answers
                .pop_front()
                .ok_or_else(|| DevforgeError::System("无更多回答".into()))
        }
    }

    fn write_script(dir: &Path, body: &str) -> PathBuf {
        let p = dir.join(format!("replay-{}.sh", uuid::Uuid::new_v4()));
        fs::write(&p, body).unwrap();
        p
    }

    /// 回放（模拟真 pi 时序）：先读走 prompt 行 → 抛 UI 请求 → 读 extension_ui_response（落盘留证）→ 回 ack + settled。
    fn ui_replay_script(dir: &Path, response_capture: &Path) -> PathBuf {
        let capture = response_capture.to_string_lossy().replace('\'', "");
        let body = format!(
            "read _cmd\n\
             cat <<'DEVFORGE_EOF'\n\
             {{\"id\":\"ui-1\",\"type\":\"extension_ui_request\",\"method\":\"select\",\"title\":\"规划完成了吗\",\"options\":[\"完成\",\"还没\"]}}\n\
             DEVFORGE_EOF\n\
             read _r\n\
             printf '%s\\n' \"$_r\" > '{capture}'\n\
             cat <<'DEVFORGE_EOF'\n\
             {{\"id\":\"req-1\",\"type\":\"response\",\"command\":\"prompt\",\"success\":true,\"data\":{{\"disposition\":\"started\"}}}}\n\
             {{\"type\":\"agent_settled\"}}\n\
             DEVFORGE_EOF\n"
        );
        write_script(dir, &body)
    }

    #[test]
    fn ui_request_goes_to_user_and_answer_flows_back() {
        let dir = tempfile::tempdir().unwrap();
        let capture = dir.path().join("captured-response.jsonl");
        let script = ui_replay_script(dir.path(), &capture);

        let spec = SpawnSpec {
            program: "sh".into(),
            profile_dir: dir.path().join("profile"),
            role: "plan-agent".into(),
            run_id: RunId::generate().0,
            session_dir: dir.path().to_path_buf(),
            working_dir: std::path::PathBuf::from("."),
            extra_args: vec![script.to_string_lossy().into_owned()],
        };
        // 裸 sh 跑回放脚本（pi 的 --mode 前缀 sh 不认）
        let mut cmd = Command::new("sh");
        cmd.arg(&script);
        let client = RpcClient::spawn_with_command(cmd, spec).unwrap();
        let mut session = RpcPlanSession::new(client);
        let mut user = ScriptUser::new(&["完成"]);

        let text = session.converse("开始规划", &mut user).unwrap();
        assert_eq!(text, ""); // 回放无 text_delta

        // 用户被问到的问题（宽松提取自 UI 请求）
        assert_eq!(user.asked.len(), 1);
        assert!(user.asked[0].contains("规划完成了吗"), "问句: {}", user.asked[0]);

        // 应答记录：同一 request id + 我们的回答（联调校准点的最小契约）
        let response = fs::read_to_string(&capture).unwrap();
        assert!(response.contains("ui-1"), "应带原 request id: {response}");
        assert!(response.contains("完成"), "应带用户回答: {response}");
    }

    /// 回放：进程正常结束但不产出任何正文（C19-R2 现场：审查官静默退出）。
    fn silent_review_script(dir: &Path) -> PathBuf {
        let body = "read _cmd\n\
                    cat <<'DEVFORGE_EOF'\n\
                    {\"id\":\"req-1\",\"type\":\"response\",\"command\":\"prompt\",\"success\":true,\"data\":{\"disposition\":\"started\"}}\n\
                    {\"type\":\"agent_settled\"}\n\
                    DEVFORGE_EOF\n";
        write_script(dir, body)
    }

    #[test]
    fn empty_review_reply_is_infrastructure_error() {
        // C19-R2：空回复不得当作「审查结论」（否则保守解析会误判为协议违例并回注被审 Agent）
        let dir = tempfile::tempdir().unwrap();
        let script = silent_review_script(dir.path());
        let spec = SpawnSpec {
            program: "sh".into(),
            profile_dir: dir.path().join("profile"),
            role: "coding-review".into(),
            run_id: RunId::generate().0,
            session_dir: dir.path().to_path_buf(),
            working_dir: PathBuf::from("."),
            extra_args: vec![script.to_string_lossy().into_owned()],
        };
        let mut cmd = Command::new("sh");
        cmd.arg(&script);
        let client = RpcClient::spawn_with_command(cmd, spec).unwrap();
        let mut session = RpcReviewSession {
            client,
            role: "coding-review".into(),
        };
        let err = session.review("开始审查").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("无有效回复"), "应报空回复基础设施故障: {msg}");
        assert!(msg.contains("coding-review"), "应指明角色: {msg}");
    }
}
