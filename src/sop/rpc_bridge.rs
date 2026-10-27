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
        // 引擎批次 #1（critical）：审查子进程的 `DEVFORGE_MANAGED` 必须是**父 run-id**——
        // 旧实现每次 spawn 用 `RunId::generate()`，而收割端（ledger）按父 run-id 匹配，
        // cancel/超时/熔断三条杀路径全部漏杀。这里在**工厂构造时**（= run 线程）取一次。
        let managed = crate::observability::current_run_id().unwrap_or_else(|| RunId::generate().0);
        Self::new(move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            SpawnSpec::new(
                profile_dir.clone(),
                "plan-review",
                managed.clone(),
                session_root.join(format!("review-{n}")),
            )
        })
    }

    /// P005：显式指定角色与子进程工作目录（编码审查的 cwd = 黑板，报告与树同址）。
    /// `ledger_path`：spawn 台账显式落点（自审 major：熔断/对账的事实源不能靠
    /// working_dir 涌现推导——change 审查的 cwd 是 task 层黑板，台账必须回 project 层）。
    pub fn per_round_in(
        profile_dir: std::path::PathBuf,
        role: impl Into<String> + 'static,
        session_root: std::path::PathBuf,
        working_dir: std::path::PathBuf,
        ledger_path: std::path::PathBuf,
    ) -> Self {
        let counter = std::sync::atomic::AtomicU32::new(0u32);
        let role = role.into();
        // 同上：子进程标记跟父 run 走（否则审查 pi 杀不掉）。会话目录仍按 review-{n} 区分。
        let managed = crate::observability::current_run_id().unwrap_or_else(|| RunId::generate().0);
        Self::new(move || {
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            SpawnSpec::new(
                profile_dir.clone(),
                role.clone(),
                managed.clone(),
                session_root.join(format!("review-{n}")),
            )
            .with_working_dir(working_dir.clone())
            .with_ledger_path(ledger_path.clone())
        })
    }
}

impl ReviewSessionFactory for RpcReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        let spec = (self
            .make_spec
            .lock()
            .unwrap_or_else(|e| e.into_inner()))();
        let role = spec.role.clone();
        let client = RpcClient::spawn(spec)?;
        Ok(Box::new(RpcReviewSession { client, role }))
    }
}

#[cfg(test)]
#[path = "rpc_bridge_tests.rs"]
mod tests;
