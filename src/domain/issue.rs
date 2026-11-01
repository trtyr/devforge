//! Issue 实体（P020 反馈系统）：跨 run 的持久反馈层，带 task 转化闭环。
//!
//! 定位（调研裁决，见 plantree plans/020-feedback-issues）：不是 Jira 克隆——
//! 固定六态状态机（不可配置工作流）、无排序算法、单运营者；核心价值 =
//! **issue → task 闭环**（转化后由 run 收尾驱动状态自动流转）。
//! key = `<PROJECT>-<n>` 项目前缀序（2026-11-01 拍板；key 不可变）。

use serde::{Deserialize, Serialize};

/// 反馈来源：操作员手工 / agent（submit_issue 交卷工具）/ 系统自动（M3 终判留痕）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSource {
    Operator,
    Agent,
    System,
}

/// 六态状态机（✅ 2026-11-01 拍板原案）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// 新报未分诊。
    Open,
    /// 已确认要修。
    Triaged,
    /// 已转 task（在流水线里）。
    Queued,
    /// 关联 run Settled 自动落（或人工确认修复）。
    Resolved,
    /// 不修（有效但不做）。
    Rejected,
    /// 记录不修。
    Wontfix,
}

impl IssueStatus {
    /// 是否终态（终态仅可翻案回 Open）。
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            IssueStatus::Resolved | IssueStatus::Rejected | IssueStatus::Wontfix
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Info,
    Minor,
    Major,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCategory {
    Bug,
    Flow,
    Model,
    Feature,
}

/// 时间线条目（append-only，不删不改）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub at_ms: u64,
    /// 操作者：`operator` / `agent:<sop>` / `system`。
    pub actor: String,
    /// 动作词：created / transitioned / converted / auto_resolved / reopened /
    /// regression_failed / note。
    pub action: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    /// `<PROJECT>-<n>`（如 `DEVFORGE-3`）——不可变。
    pub key: String,
    /// 归属项目（refs 锚，创建即定）。
    pub project: String,
    pub title: String,
    pub body: String,
    pub source: IssueSource,
    pub status: IssueStatus,
    pub severity: IssueSeverity,
    pub category: IssueCategory,
    /// 转化后指向 task id（Queued 态必有）。
    #[serde(default)]
    pub task_ref: Option<String>,
    /// 报告时关联的 run（evidence 锚）。
    #[serde(default)]
    pub run_refs: Vec<String>,
    /// 结构化上下文（失败原因/日志指针/对象引用——创建时自动附）。
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub timeline: Vec<TimelineEntry>,
    /// 乐观锁（PATCH 并发；409 口径）。
    pub version: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl Issue {
    /// 新建（key 由 Store 按项目序分配——不在这里算，调用方传入）。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        key: String,
        project: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
        source: IssueSource,
        severity: IssueSeverity,
        category: IssueCategory,
        run_refs: Vec<String>,
        now_ms: u64,
    ) -> Self {
        Self {
            key,
            project: project.into(),
            title: title.into(),
            body: body.into(),
            source,
            status: IssueStatus::Open,
            severity,
            category,
            task_ref: None,
            run_refs,
            evidence: Vec::new(),
            timeline: vec![TimelineEntry {
                at_ms: now_ms,
                actor: match source {
                    IssueSource::Operator => "operator".into(),
                    IssueSource::Agent => "agent".into(),
                    IssueSource::System => "system".into(),
                },
                action: "created".into(),
                detail: String::new(),
            }],
            version: 1,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    /// 人工状态转移合法性（PATCH 通道）。引擎专用通道（auto_resolve/auto_reopen）
    /// 不走这里——Queued → Resolved 只能由 run Settled 联动触发，人工不得手标。
    pub fn can_transition(&self, to: IssueStatus) -> bool {
        use IssueStatus::*;
        matches!(
            (self.status, to),
            (Open, Triaged)
                | (Open, Rejected)
                | (Open, Wontfix)
                | (Triaged, Queued)
                | (Triaged, Rejected)
                | (Triaged, Wontfix)
                | (Queued, Open) // 手动回炉（如取消 task 后翻案）
                | (Queued, Rejected)
                | (Queued, Wontfix)
                | (Resolved, Open) // 复发重开
                | (Rejected, Open) // 翻案
                | (Wontfix, Open) // 翻案
        )
    }

    /// 追加时间线条目并 bump updated_at（唯一合法的 timeline 写入口）。
    pub fn note(&mut self, actor: &str, action: &str, detail: impl Into<String>, at_ms: u64) {
        self.timeline.push(TimelineEntry {
            at_ms,
            actor: actor.into(),
            action: action.into(),
            detail: detail.into(),
        });
        self.updated_at_ms = at_ms;
    }
}

// ---------- P020-k5：agent submit_issue 交卷载荷 ----------

/// agent submit_issue 落库回调（server 注入；引擎事件流捕获，免 API 鉴权——
/// agent 子进程没有 token，引擎持有 store，解析后的载荷经此回调进第三实体）。
pub type IssueSink = std::sync::Arc<dyn Fn(AgentIssueReport) + Send + Sync>;

/// submit_issue 工具参数的结构化载荷（project/task/run 由引擎侧补齐）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentIssueReport {
    pub project: String,
    pub task_id: String,
    pub run_id: String,
    pub title: String,
    pub body: String,
    pub severity: IssueSeverity,
    pub category: IssueCategory,
}

impl AgentIssueReport {
    /// 工具 args → 载荷。校验失败（缺 title/body、枚举越界）返回 None，调用方
    /// WARN 跳过——宁丢单条上报不炸 run。
    pub fn parse_args(
        args: &serde_json::Value,
        project: &str,
        task_id: &str,
        run_id: &str,
    ) -> Option<Self> {
        #[derive(Deserialize)]
        struct Args {
            title: String,
            #[serde(default)]
            body: String,
            severity: IssueSeverity,
            category: IssueCategory,
        }
        let a: Args = serde_json::from_value(args.clone()).ok()?;
        let title = a.title.trim().to_string();
        if title.is_empty() {
            return None;
        }
        Some(Self {
            project: project.into(),
            task_id: task_id.into(),
            run_id: run_id.into(),
            title,
            body: a.body,
            severity: a.severity,
            category: a.category,
        })
    }
}

#[cfg(test)]
#[path = "issue_tests.rs"]
mod tests;
