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
                // UI 请求的具体字段形状联调校准（P001 roadmap）；MVP 宽松提取
                let question = extra
                    .get("request")
                    .and_then(|r| r.get("question"))
                    .and_then(|q| q.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        let extra = serde_json::to_string(extra).unwrap_or_default();
                        format!("Agent 请求输入: {extra}")
                    });
                ui.ask(&question, &[])
            })
            .ctx("Plan 对话（含 UI 子协议）")?;
        self.client.require_success(&outcome, &id)?;
        Ok(outcome.text)
    }
}

/// Review 的 RPC 会话（一次性实例，Drop 即杀进程——用完即弃 decisions/004）。
pub struct RpcReviewSession {
    client: RpcClient,
}

impl ReviewSession for RpcReviewSession {
    fn review(&mut self, instruction: &str) -> Result<String> {
        let id = self.client.prompt(instruction)?;
        let outcome = self.client.wait_settled()?; // Review 永不碰用户——无 UI 处理
        self.client.require_success(&outcome, &id)?;
        Ok(outcome.text)
    }
}

/// Review 会话工厂：每轮用新 spec spawn 全新 pi 进程。
pub struct RpcReviewFactory {
    make_spec: Box<dyn FnMut() -> SpawnSpec>,
}

impl RpcReviewFactory {
    /// 每轮调用一次 spec 生成器（session dir / run 细分由生成器决定）。
    pub fn new(make_spec: impl FnMut() -> SpawnSpec + 'static) -> Self {
        Self {
            make_spec: Box::new(make_spec),
        }
    }

    /// 便捷构造：同一 profile，每轮递增序号的 session 子目录。
    pub fn per_round(profile_dir: std::path::PathBuf, session_root: std::path::PathBuf) -> Self {
        let counter = std::cell::Cell::new(0u32);
        Self::new(move || {
            let n = counter.get();
            counter.set(n + 1);
            SpawnSpec::new(
                profile_dir.clone(),
                "plan-review",
                RunId::generate().0,
                session_root.join(format!("review-{n}")),
            )
        })
    }
}

impl ReviewSessionFactory for RpcReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        let spec = (self.make_spec)();
        let client = RpcClient::spawn(spec)?;
        Ok(Box::new(RpcReviewSession { client }))
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
             {{\"id\":\"ui-1\",\"type\":\"extension_ui_request\",\"request\":{{\"question\":\"规划完成了吗\"}}}}\n\
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
}
