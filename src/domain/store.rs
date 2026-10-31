//! 三层对象的内存存储 + JSON 持久化。
//!
//! 生命周期守卫集中在此：
//! - 系统级串行（decisions/003 实现口径）：任一时刻最多一个 Running run
//! - 任务状态机：Pending/Completed/Failed 可发起 run；Running/Archived 不可
//! - 重跑留档（Q004）：runs 按任务保留最新 N 条终态（[`DEFAULT_MAX_RUNS_PER_TASK`]），
//!   current_run 指向最新——自审 minor 114：旧口径「只增不减」使每次变更都要重写越来越大的
//!   整份 store JSON（写放大随 run 数线性增长）。

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::domain::auth::{AuthAccount, AuthEvent, MAX_AUTH_EVENTS};
use crate::domain::run::{Run, RunId, RunStatus};
use crate::domain::sop::SopRegistry;
use crate::domain::task::{Task, TaskStatus};
use crate::domain::{now_ms, Project};
use crate::error::{Context as _, DevforgeError, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Store {
    pub projects: Vec<Project>,
    pub tasks: Vec<Task>,
    pub runs: Vec<Run>,
    /// 并发 run 上限（P012 M4）：默认 1 = decisions/003 串行兼容；调度器/worktree 模式下调高。
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_runs: usize,
    /// 单任务保留的**终态** run 条数（自审 minor 114）：run 历史是有界遥测，
    /// 权威记录是事件流（logs/devforge.log.jsonl）；Running 的 run 永不被裁。
    #[serde(default = "default_max_runs_per_task")]
    pub max_runs_per_task: usize,
    /// 渠道熔断表（gap-B）：渠道（model 池条目 id）→ 冷却到期时刻。
    /// `#[serde(default)]`：旧 store.json 没有这一段也能读回（数据迁移方向：新二进制读旧文件）；
    /// 反向（新文件读旧二进制）由 serde 默认的「忽略未知字段」保证——回滚不会炸。
    #[serde(default)]
    pub channel_breakers: Vec<ChannelBreaker>,
    /// 通知表（A2 · 通知系统）：全量监控事件的落地面。
    /// `#[serde(default)]`：旧 store.json 没有这一段也能读回（同 channel_breakers 口径）。
    /// 上限裁剪见 `MAX_NOTIFICATIONS`：全量重写代价 O(通知数)，无界增长 = 写放大事故。
    #[serde(default)]
    pub notifications: Vec<Notification>,
    /// admin 账户（A1 · M-1）：单账户模型，None = 未初始化（登录端点据实回复）。
    /// `#[serde(default)]`：旧 store.json 无此段也能读回（同 channel_breakers 口径）。
    #[serde(default)]
    pub auth: Option<AuthAccount>,
    /// 认证审计事件（A1）：登录成功/失败与写操作；cap 裁剪同通知（无界 = 写放大）。
    #[serde(default)]
    pub auth_events: Vec<AuthEvent>,
    /// 持久化路径（复验 backlog：store 纯内存是设计错误——重启即蒸发）。
    /// None = 内存态（单测）；Some = 每个 &mut 变更方法尾调 persist()。
    #[serde(skip)]
    pub persist_path: Option<std::path::PathBuf>,
    /// PG 持久化连接串（P018 Phase 1）：Some = persist() 写 PG 单行 JSONB
    /// （store_state 表），load_pg 从 PG 重建。与 persist_path 互斥（config 决定，
    /// 启动时选一个；都设时 PG 优先）。serde skip：连接串不进任何持久化文件。
    #[serde(skip)]
    pub persist_pg: Option<String>,
}

fn default_max_concurrent() -> usize {
    1
}

/// 渠道熔断条目（gap-B · 派发前渠道熔断）：某个 model 池条目因渠道事故进入冷却。
///
/// 形状要点：
/// - `expires_at_ms` 存**绝对到期时刻**（unix ms），不存剩余秒数——剩余秒数跨重启就漂
///   （重启后必须按当前时间重算，裁决：熔断落 store.json，未到期继续熔断）；
/// - `last_seen_reason` 是最近一次触发的原因摘要（运维一眼看出这条为什么在冷却）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelBreaker {
    /// 熔断的渠道（model 池条目 id，如 `deepseek-flash`）。
    pub channel: String,
    /// 绝对到期时刻（unix ms）。
    pub expires_at_ms: u64,
    /// 最近一次触发的原因摘要。
    pub last_seen_reason: String,
}

/// 通知表上限（A2）：store 全量重写代价 O(通知数)，与 DEFAULT_MAX_RUNS_PER_TASK 同理
/// 必须有界——超出裁掉最旧的已读条目（未读优先保留，最新永远保留）。
pub const MAX_NOTIFICATIONS: usize = 200;

/// 通知条目（A2 · 通知系统）：全量监控事件的落地面。
///
/// 形状要点：
/// - `kind` 是机器可判的枚举字符串（前端按它选图标/颜色），`level` 是严重度
///   （error/warn/info——前端按它配色：红/琥珀/灰）；
/// - `read` 已读标记：未读数 = 列表里 read==false 的条数；
/// - `link` 可选跳转锚（如 `devforge/sr2-xxx` 任务页），前端点通知直接到现场；
/// - 创建时刻存 `created_at_ms`（unix ms，UTC 口径；前端本地渲染——时区纪律）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// 单调递增 id（store 内自增：max(existing)+1）。
    pub id: u64,
    /// 通知类别（machine 判别用）：agent_failed / ui_pending / channel_breaker /
    /// run_settled / integration_needs_repair。
    pub kind: String,
    /// 严重度：error / warn / info。
    pub level: String,
    /// 人读标题（一句话说清发生了什么）。
    pub title: String,
    /// 可选跳转锚（任务 id 等）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// 已读标记（未读数就是前端铃铛的角标）。
    pub read: bool,
    /// 创建时刻（unix ms，UTC 口径）。
    pub created_at_ms: u64,
}

/// 单任务默认保留的终态 run 条数（自审 minor 114）。///
/// 取 100 的理由：一次任务的正常重跑量级是个位数，100 条足够事后回看一整段迭代；
/// 再多的价值在事件流里（每条 run 的 gate/审查里程碑都在），而整份 store 每次变更
/// 全量重写的代价是 O(run 总数)——无界增长最终把每次状态迁移拖成写放大事故。
pub const DEFAULT_MAX_RUNS_PER_TASK: usize = 100;

fn default_max_runs_per_task() -> usize {
    DEFAULT_MAX_RUNS_PER_TASK
}

impl Default for Store {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            tasks: Vec::new(),
            runs: Vec::new(),
            notifications: Vec::new(),
            auth: None,
            auth_events: Vec::new(),
            max_concurrent_runs: default_max_concurrent(),
            max_runs_per_task: default_max_runs_per_task(),
            channel_breakers: Vec::new(),
            persist_path: None,
            persist_pg: None,
        }
    }
}

/// run 结束的结果（runner 驱动 store 收尾用）。
#[derive(Debug, Clone, PartialEq)]
pub enum FinishOutcome {
    Completed,
    Failed { reason: String },
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// 开启持久化：后续所有变更方法自动落盘（服务启动时用；单测不调即内存态）。
    pub fn with_persist(mut self, path: std::path::PathBuf) -> Self {
        self.persist_path = Some(path);
        self
    }

    /// 开启 PG 持久化（P018 Phase 1）：persist() 写 PG 单行 JSONB。
    pub fn with_persist_pg(mut self, url: impl Into<String>) -> Self {
        self.persist_pg = Some(url.into());
        self
    }

    /// 变更落盘（persist_path/persist_pg 均未设则空操作）。失败只 WARN 不阻断
    /// （可观测优先于强一致）。PG 模式（persist_pg Some）优先且不双写。
    fn persist(&self) {
        if let Some(url) = &self.persist_pg {
            if let Err(e) = self.save_pg(url) {
                tracing::warn!(
                    component = "store",
                    error = %e,
                    "store PG 落盘失败（内存态继续，重启将丢自上次成功落盘后的变更）"
                );
            }
            return;
        }
        if let Some(path) = &self.persist_path
            && let Err(e) = self.save(path)
        {
            tracing::warn!(
                component = "store",
                path = %path.display(),
                error = %e,
                "store 落盘失败（内存态继续，重启将丢自上次成功落盘后的变更）"
            );
        }
    }

    /// P018 Phase 1：全量写 PG（store_state 单行 JSONB，单语句原子）。
    fn save_pg(&self, url: &str) -> Result<()> {
        let mut conn = postgres::Client::connect(url, postgres::NoTls)
            .map_err(|e| DevforgeError::System(format!("连 PG 失败：{e}")))?;
        Self::ensure_pg_schema(&mut conn)?;
        // TEXT 参数 + ::jsonb cast——postgres crate 的 serde_json feature 不依赖，
        // 序列化字符串直接 cast（PG 校验 JSON 合法性，语义等同）
        let data = serde_json::to_string(self)
            .map_err(|e| DevforgeError::System(format!("序列化 store（PG）失败：{e}")))?;
        conn.execute(
            "INSERT INTO store_state (id, data) VALUES (1, $1::text::jsonb) \
             ON CONFLICT (id) DO UPDATE SET data = EXCLUDED.data, updated_at = now()",
            &[&data],
        )
        .map_err(|e| DevforgeError::System(format!("写 store_state 失败：{e}")))?;
        Ok(())
    }

    /// store_state 表幂等建表（persist/load 各自调用，IF NOT EXISTS 幂等）。
    fn ensure_pg_schema(conn: &mut postgres::Client) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS store_state (\
             id INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),\
             data JSONB NOT NULL,\
             updated_at TIMESTAMPTZ NOT NULL DEFAULT now())",
            &[] as &[&(dyn postgres::types::ToSql + Sync)],
        )
        .map_err(|e| DevforgeError::System(format!("建 store_state 表失败：{e}")))?;
        Ok(())
    }

    /// P018 Phase 1：从 PG 重建 Store（启动入口；PG 不可达 = 硬错误，服务不启动——
    /// 不静默回落文件模式，双真相分叉比崩溃更危险）。空库 = 全新 Store（persist 已开）。
    pub fn load_pg(url: &str) -> Result<Self> {
        let mut conn = postgres::Client::connect(url, postgres::NoTls).map_err(|e| {
            DevforgeError::System(format!(
                "连 PG 失败（PG 模式启动要求可达，不做静默回落）：{e}"
            ))
        })?;
        Self::ensure_pg_schema(&mut conn)?;
        let row = conn
            .query_opt(
                "SELECT data::text FROM store_state WHERE id = 1",
                &[] as &[&(dyn postgres::types::ToSql + Sync)],
            )
            .map_err(|e| DevforgeError::System(format!("读 store_state 失败：{e}")))?;
        let Some(row) = row else {
            let store = Self {
                persist_pg: Some(url.to_string()),
                ..Self::default()
            };
            return Ok(store);
        };
        let data: String = row.get(0);
        let mut store: Store = serde_json::from_str(&data)
            .map_err(|e| DevforgeError::System(format!("解析 store（PG）失败：{e}")))?;
        store.persist_pg = Some(url.to_string());
        Ok(store)
    }

    // ---------- 项目层 ----------

    /// 自审 major（跨域汇入点）：project/task 名是**目录名**——plans_root/<project>/<task>/、
    /// 会话目录、spawn 台账、集成队列全都拿它拼路径。名不安全（空/./..、含分隔符）=
    /// 路径穿越 + 任意目录创建/覆写。在域层唯一入口拦截，所有消费方一次性安全。
    pub(crate) fn validate_path_name(name: &str) -> Result<()> {
        let path_ok = !name.is_empty()
            && name != "."
            && name != ".."
            && !name.contains('/')
            && !name.contains('\\')
            && !name.contains('\0');
        if !path_ok {
            return Err(DevforgeError::Store(format!(
                "非法请求：名称 `{name}` 含非法路径字符（不可为空/./..，不可含分隔符）"
            )));
        }
        // 自审 minor（19）：文档承诺 kebab-case（`mvp-from-zero`），旧口径只挡路径穿越、
        // 字符集零校验——`My.Project` / `项目` 这类名能进仓，而它同时是目录名与 API 键。
        let kebab_ok = name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !name.starts_with('-')
            && !name.ends_with('-');
        if !kebab_ok {
            // 自审 minor（45）：客户端输入类校验错误统一前缀「非法请求：」——
            // HTTP 层据此映射 400；旧口径靠中文关键词子串猜，kebab 文案不在关键词表里
            // 就静默落进 500（已文档化的契约被违反）。
            return Err(DevforgeError::Store(format!(
                "非法请求：名称 `{name}` 不是 kebab-case（只允许 a-z0-9 与 -，首尾不得为 -）"
            )));
        }
        Ok(())
    }

    /// 回滚插入（P1-ISSUE-01：目录落地失败时不留半注册项目）。
    pub fn remove_project(&mut self, name: &str) {
        // 自审 minor（115）：必须级联清理——旧口径只删项目记录，遗留的孤儿任务仍参与
        // running_run_count / list_tasks（占并发槽、出现在别人项目列表里）。
        self.projects.retain(|p| p.name != name);
        let orphan_task_ids: Vec<String> = self
            .tasks
            .iter()
            .filter(|t| t.project == name)
            .map(|t| t.id.clone())
            .collect();
        self.tasks.retain(|t| t.project != name);
        self.runs
            .retain(|r| r.task_project != name || !orphan_task_ids.contains(&r.task_id));
        tracing::info!(
            component = "store",
            event = "store.project_removed",
            project = %name,
            tasks = orphan_task_ids.len(),
            "项目已删除（含级联任务）"
        );
        self.persist();
    }

    pub fn create_project(&mut self, project: Project) -> Result<&Project> {
        Self::validate_path_name(&project.name)?;
        if self.projects.iter().any(|p| p.name == project.name) {
            return Err(DevforgeError::Store(format!(
                "项目 `{}` 已存在",
                project.name
            )));
        }
        self.projects.push(project);
        self.persist();
        // 自审 minor（29）：库代码不留 expect（rust-strict 口径）——push 后必有，用错误出口。
        self.projects
            .last()
            .ok_or_else(|| DevforgeError::Store("项目登记失败（内部不变量）".into()))
    }

    pub fn get_project(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    pub fn list_projects(&self) -> &[Project] {
        &self.projects
    }

    // ---------- 任务层 ----------

    /// 创建任务：项目须存在、任务 id 项目内唯一、SOP 须已内置注册。
    /// 契约（P012 M0）创建时提交不可变；None = 无契约（不做边界终校验）。
    /// 依赖图（P012 T4.1）：依赖须本项目内存在且无环（含自依赖）。
    pub fn create_task(
        &mut self,
        registry: &SopRegistry,
        project: &str,
        id: impl Into<String>,
        sop: impl Into<String>,
        contract: Option<crate::domain::contract::WorkItemContract>,
        deps: crate::domain::task::TaskDeps,
    ) -> Result<&Task> {
        let sop = sop.into();
        registry.validate(&sop)?;
        Self::validate_path_name(project)?;
        if self.get_project(project).is_none() {
            return Err(DevforgeError::Store(format!("项目 `{project}` 不存在")));
        }
        let id = id.into();
        Self::validate_path_name(&id)?;
        if self
            .tasks
            .iter()
            .any(|t| t.project == project && t.id == id)
        {
            return Err(DevforgeError::Store(format!(
                "任务 `{id}` 在项目 `{project}` 下已存在"
            )));
        }
        // 依赖校验（T4.1）：存在性 + 无环（把待建节点也放进图一起检）。
        for dep in &deps.depends_on {
            if dep == &id {
                return Err(DevforgeError::Store(format!(
                    "任务 `{id}` 不能依赖自身"
                )));
            }
            if !self
                .tasks
                .iter()
                .any(|t| t.project == project && &t.id == dep)
            {
                return Err(DevforgeError::Store(format!(
                    "依赖任务 `{dep}` 在项目 `{project}` 下不存在"
                )));
            }
        }
        Self::assert_acyclic(self, project, &id, &deps.depends_on)?;
        let mut task = Task::new(id, project, sop, now_ms());
        task.contract = contract;
        task.depends_on = deps.depends_on;
        task.exclusive = deps.exclusive;
        // P005 decisions/001：任务快照项目登记的代码工作区（编码 SOP 用；黑板由引擎建）
        task.workspace = self
            .get_project(project)
            .map(|p| p.local_path.clone())
            .unwrap_or_default();
        let id = task.id.clone();
        self.tasks.push(task);
        self.persist();
        self.tasks
            .iter()
            .find(|t| t.project == project && t.id == id)
            .ok_or_else(|| DevforgeError::Store("任务登记失败（内部不变量）".into()))
    }

    /// 环检测（T4.1）：假设 `new_id` 带 `depends_on` 建入后，项目图不得有环。
    ///
    /// 实现是 **Kahn 拓扑排序**（入度归零计数），不是 DFS 三色标记——2026-10-27 自审
    /// minor 116 把注释与实现对齐：旧注释说 DFS，读者会按 DFS 去修。悬空依赖
    /// （目标不在本项目图内）不进节点计数，不得把 visited 撑大造成「有环」误报。
    fn assert_acyclic(
        &self,
        project: &str,
        new_id: &str,
        new_deps: &[String],
    ) -> Result<()> {
        use std::collections::{HashMap, VecDeque};
        let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for t in self.tasks.iter().filter(|t| t.project == project) {
            edges.entry(&t.id).or_default().extend(
                t.depends_on.iter().map(|s| s.as_str()),
            );
        }
        edges.entry(new_id).or_default().extend(
            new_deps.iter().map(|s| s.as_str()),
        );
        // Kahn：入度归零计数 = 无环；残图非空 = 有环（比较基准 = 图内节点数）
        let mut indegree: HashMap<&str, usize> = edges.keys().map(|k| (*k, 0)).collect();
        for outs in edges.values() {
            for d in outs {
                *indegree.entry(d).or_insert(0) += 1;
            }
        }
        let mut queue: VecDeque<&str> = indegree
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(k, _)| *k)
            .collect();
        let mut visited = 0usize;
        while let Some(n) = queue.pop_front() {
            visited += 1;
            if let Some(outs) = edges.get(n) {
                for d in outs {
                    let Some(e) = indegree.get_mut(d) else {
                        // 自审 minor（116）：悬空依赖（目标不在本项目图内）不得当作节点参与
                        // 计数——旧口径会把它算进 visited，导致「存在环」误报。
                        continue;
                    };
                    *e -= 1;
                    if *e == 0 {
                        queue.push_back(d);
                    }
                }
            }
        }
        if visited != edges.len() {
            return Err(DevforgeError::Store(format!(
                "依赖图存在环：加入 `{new_id}` 后项目 `{project}` 的 DAG 无拓扑序"
            )));
        }
        Ok(())
    }

    pub fn get_task(&self, project: &str, id: &str) -> Option<&Task> {
        self.tasks
            .iter()
            .find(|t| t.project == project && t.id == id)
    }

    fn get_task_mut(&mut self, project: &str, id: &str) -> Result<&mut Task> {
        self.tasks
            .iter_mut()
            .find(|t| t.project == project && t.id == id)
            .ok_or_else(|| DevforgeError::Store(format!("任务 `{project}/{id}` 不存在")))
    }

    /// 置任务状态（P012 M4 调度器专用：Blocked ⇄ Pending 恢复联动；其他调用方勿用）。
    pub fn set_task_status(
        &mut self,
        project: &str,
        id: &str,
        status: TaskStatus,
    ) -> Result<()> {
        self.get_task_mut(project, id)?.status = status;
        tracing::debug!(
            component = "store",
            event = "store.task_status",
            project = %project,
            task = %id,
            status = ?status,
            "任务状态变更"
        );
        self.persist();
        Ok(())
    }

    pub fn list_tasks(&self, project: &str) -> Vec<&Task> {
        self.tasks.iter().filter(|t| t.project == project).collect()
    }

    // ---------- 渠道熔断（gap-B · 派发前渠道熔断） ----------

    /// 记一条渠道熔断（upsert：同一渠道取**较晚**的到期时刻，原因摘要刷新为最新一条）。
    ///
    /// 取较晚到期而不是覆盖：同一条渠道先后两次事故（新失败又来一次）时，冷却窗口
    /// 取两次里更晚的那个，不因为后一次来得早就把已定的窗口缩短。
    pub fn record_channel_breaker(&mut self, channel: &str, expires_at_ms: u64, reason: &str) {
        match self
            .channel_breakers
            .iter_mut()
            .find(|b| b.channel == channel)
        {
            Some(existing) => {
                existing.expires_at_ms = existing.expires_at_ms.max(expires_at_ms);
                existing.last_seen_reason = reason.to_string();
            }
            None => self.channel_breakers.push(ChannelBreaker {
                channel: channel.to_string(),
                expires_at_ms,
                last_seen_reason: reason.to_string(),
            }),
        }
        tracing::warn!(
            component = "store",
            event = "store.channel_breaker_recorded",
            channel,
            expires_at_ms,
            "渠道进入熔断冷却"
        );
        self.persist();
    }

    /// 某渠道剩余冷却毫秒（0 = 没有熔断条目，或条目已到期）。
    pub fn channel_breaker_remaining_ms(&self, channel: &str, now_ms: u64) -> u64 {
        self.channel_breakers
            .iter()
            .find(|b| b.channel == channel)
            .map(|b| b.expires_at_ms.saturating_sub(now_ms))
            .unwrap_or(0)
    }

    /// 清掉已到期的熔断条目，返回清掉的条数（0 = 无变更，不落盘）。
    pub fn purge_expired_channel_breakers(&mut self, now_ms: u64) -> usize {
        let before = self.channel_breakers.len();
        self.channel_breakers
            .retain(|b| b.expires_at_ms.saturating_sub(now_ms) > 0);
        let purged = before - self.channel_breakers.len();
        if purged > 0 {
            tracing::info!(
                component = "store",
                event = "store.channel_breakers_purged",
                purged,
                "已到期的渠道熔断条目被清除（冷却自然解除）"
            );
            self.persist();
        }
        purged
    }

    /// 活跃（未到期）熔断条目，按渠道 id 排序（输出稳定，便于对账与断言）。
    pub fn active_channel_breakers(&self, now_ms: u64) -> Vec<ChannelBreaker> {
        let mut active: Vec<ChannelBreaker> = self
            .channel_breakers
            .iter()
            .filter(|b| b.expires_at_ms.saturating_sub(now_ms) > 0)
            .cloned()
            .collect();
        active.sort_by(|a, b| a.channel.cmp(&b.channel));
        active
    }

    // ---------- 通知（A2 · 通知系统） ----------

    /// 记录一条通知：id 自增、未创建即未读；超上限时裁最旧的**已读**条目
    /// （未读优先保留，最新永远保留——铃铛角标不许因裁剪丢事件）。
    /// 返回新通知的 id（调用方用它发 SSE）。
    pub fn push_notification(
        &mut self,
        kind: &str,
        level: &str,
        title: &str,
        link: Option<&str>,
        now_ms: u64,
    ) -> u64 {
        let id = self
            .notifications
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.notifications.push(Notification {
            id,
            kind: kind.to_string(),
            level: level.to_string(),
            title: title.to_string(),
            link: link.map(|s| s.to_string()),
            read: false,
            created_at_ms: now_ms,
        });
        self.trim_notifications();
        self.persist();
        id
    }

    /// 未读数（前端铃铛角标）。
    pub fn unread_notification_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    /// 标记单条已读；id 不存在时静默（幂等——重试的已读请求不该报错）。
    /// 返回是否真的改了状态（变了才落盘）。
    pub fn mark_notification_read(&mut self, id: u64) -> bool {
        let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        if n.read {
            return false;
        }
        n.read = true;
        self.persist();
        true
    }

    /// 全部已读；无未读时零变更不落盘。返回实际标记的条数。
    pub fn mark_all_notifications_read(&mut self) -> usize {
        let mut changed = 0;
        for n in &mut self.notifications {
            if !n.read {
                n.read = true;
                changed += 1;
            }
        }
        if changed > 0 {
            self.persist();
        }
        changed
    }

    /// 超上限裁剪：优先裁最旧的**已读**，未读保留；返回裁掉条数（0 = 无变更不落盘）。
    fn trim_notifications(&mut self) -> usize {
        if self.notifications.len() <= MAX_NOTIFICATIONS {
            return 0;
        }
        let overflow = self.notifications.len() - MAX_NOTIFICATIONS;
        // 已读的按 id 升序（最旧优先）裁；不够再裁最旧的未读（保底，避免无界）。
        let mut read_idx: Vec<usize> = self
            .notifications
            .iter()
            .enumerate()
            .filter(|(_, n)| n.read)
            .map(|(i, _)| i)
            .collect();
        read_idx.sort_unstable();
        let mut to_remove: Vec<usize> = read_idx.into_iter().take(overflow).collect();
        if to_remove.len() < overflow {
            let mut unread_idx: Vec<usize> = self
                .notifications
                .iter()
                .enumerate()
                .filter(|(i, n)| !n.read && !to_remove.contains(i))
                .map(|(i, _)| i)
                .collect();
            unread_idx.sort_unstable();
            to_remove.extend(unread_idx.into_iter().take(overflow - to_remove.len()));
        }
        let removed = to_remove.len();
        let remove_set: std::collections::HashSet<usize> = to_remove.into_iter().collect();
        let mut i = 0usize;
        self.notifications.retain(|_| {
            let keep = !remove_set.contains(&i);
            i += 1;
            keep
        });
        if removed > 0 {
            self.persist();
        }
        removed
    }

    // ---------- 项目归档/删除（A6 · P019） ----------

    /// 归档/取消归档项目：存在性校验；状态无变化 = 0（不落盘）。
    pub fn set_project_archived(&mut self, name: &str, archived: bool) -> Result<usize> {
        let p = self
            .projects
            .iter_mut()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                DevforgeError::Store(format!("项目 `{name}` 不存在"))
            })?;
        if p.archived == archived {
            return Ok(0);
        }
        p.archived = archived;
        self.persist();
        Ok(1)
    }

    /// 删除项目（A6）：**零任务才可删**——有任务（任何状态）拒绝（409 语义，
    /// 防误删连带任务/run 历史）；返回删除的项目名。
    pub fn delete_project(&mut self, name: &str) -> Result<String> {
        if !self.projects.iter().any(|p| p.name == name) {
            return Err(DevforgeError::Store(format!("项目 `{name}` 不存在")));
        }
        if self.tasks.iter().any(|t| t.project == name) {
            return Err(DevforgeError::Store(format!(
                "项目 `{name}` 仍有任务（先处理任务再删除，或改为归档）"
            )));
        }
        self.projects.retain(|p| p.name != name);
        self.persist();
        Ok(name.to_string())
    }

    // ---------- 认证（A1 · M-1） ----------

    /// 当前账户（None = 未初始化）。
    pub fn auth_account(&self) -> Option<&AuthAccount> {
        self.auth.as_ref()
    }

    /// 设置/重置 admin 账户（初始化与改密同路）：返回 1 = 落盘，0 = 无变更不落盘。
    pub fn set_auth_account(
        &mut self,
        username: &str,
        password_hash: &str,
        role: crate::domain::auth::Role,
        now_ms: u64,
    ) -> usize {
        let new = AuthAccount {
            username: username.to_string(),
            password_hash: password_hash.to_string(),
            role,
            created_at_ms: now_ms,
        };
        if self.auth.as_ref() == Some(&new) {
            return 0;
        }
        self.auth = Some(new);
        self.persist();
        1
    }

    /// 记录一条认证审计事件：id 自增；超上限裁最旧（无已读概念，FIFO）；
    /// 返回新事件 id。
    pub fn push_auth_event(&mut self, kind: &str, actor: &str, detail: &str, now_ms: u64) -> u64 {
        let id = self
            .auth_events
            .last()
            .map(|e| e.id + 1)
            .unwrap_or(1);
        self.auth_events.push(AuthEvent {
            id,
            kind: kind.to_string(),
            actor: actor.to_string(),
            detail: detail.to_string(),
            at_ms: now_ms,
        });
        let overflow = self.auth_events.len().saturating_sub(MAX_AUTH_EVENTS);
        if overflow > 0 {
            self.auth_events.drain(0..overflow);
        }
        self.persist();
        id
    }

    /// 变更任务的 SOP（P005 跨 SOP 触发：同任务先规划后编码）。
    /// 校验注册 + 任务存在 + 状态可发起（Running/Archived 下禁改）。
    pub fn set_task_sop(
        &mut self,
        registry: &SopRegistry,
        project: &str,
        id: &str,
        sop: impl Into<String>,
    ) -> Result<()> {
        let sop = sop.into();
        registry.validate(&sop)?;
        let task = self.get_task_mut(project, id)?;
        if !task.can_begin_run() {
            return Err(DevforgeError::Store(format!(
                "任务 `{project}/{id}` 当前状态（{:?}）不可变更 SOP",
                task.status
            )));
        }
        task.sop = sop;
        self.persist();
        Ok(())
    }

    // ---------- Run 生命周期 ----------

    /// 系统级串行守卫（decisions/003 实现口径）：任一时刻最多一个 Running run。
    pub fn has_running_run(&self) -> bool {
        self.running_run_count() > 0
    }

    /// 当前 Running run 数（P012 M4 并发调度用）。
    pub fn running_run_count(&self) -> usize {
        self.runs
            .iter()
            .filter(|r| r.status == RunStatus::Running)
            .count()
    }

    /// 发起 run：守卫（并发上限 + 任务状态机）通过后创建新 run 并挂到任务上。
    /// 并发上限：`max_concurrent_runs`（默认 1 = decisions/003 串行兼容；M4 调度器调高）。
    pub fn begin_run(&mut self, project: &str, task_id: &str) -> Result<Run> {
        // 自审 minor（118）：0 不得等于「永久拒绝」——调度器侧对同一字段做了 .max(1)，
        // 两端口径必须一致（0 视为 1 = 串行兼容）。
        let cap = self.max_concurrent_runs.max(1);
        if self.running_run_count() >= cap {
            return Err(DevforgeError::Store(format!(
                "已达并发上限 {cap}（P012 M4：worktree 隔离下允许多 run）"
            )));
        }
        let task = self.get_task_mut(project, task_id)?;
        if !task.can_begin_run() {
            let status = task.status;
            return Err(DevforgeError::Store(format!(
                "任务 `{project}/{task_id}` 状态为 {status:?}，不可发起 run"
            )));
        }
        // 自审 major：DAG 契约在领域层强制——依赖未就绪（非 Completed）直接拒，
        // 不再依赖调度器自觉（绕过调度器的直接 start 路径也能守住不变量）。
        // 依赖清单取走后 task 借用即终结（NLL），后续查 tasks 无冲突。
        let deps = task.depends_on.clone();
        for dep in &deps {
            let ready = self
                .tasks
                .iter()
                .find(|t| t.project == project && t.id == *dep)
                .map(|t| t.status == TaskStatus::Completed)
                .unwrap_or(false);
            if !ready {
                return Err(DevforgeError::Store(format!(
                    "任务 `{project}/{task_id}` 的依赖 `{dep}` 未就绪（未完成），不可发起 run"
                )));
            }
        }
        let run = Run::new(project, task_id);
        let run_id = run.id.clone();
        self.runs.push(run);
        let task = self.get_task_mut(project, task_id)?;
        task.status = TaskStatus::Running;
        task.current_run = Some(run_id.clone());
        task.runs.push(run_id.clone());
        // 自审 minor（114）：run 历史有界——超出保留额度的**终态** run 在这里剪掉，
        // 否则每次变更都要重写越来越大的整份 store JSON。
        self.prune_runs(project, task_id);
        // 自审 minor（117）：域层状态迁移此前零可观测（只有 persist 失败一条 WARN）
        tracing::info!(
            component = "store",
            event = "store.run_begun",
            project = %project,
            task = %task_id,
            run_id = %run_id.0,
            "run 已发起"
        );
        self.persist();
        self.runs
            .last()
            .cloned()
            .ok_or_else(|| DevforgeError::Store("run 登记失败（内部不变量）".into()))
    }

    /// 单任务最终状态 run 裁剪（自审 minor 114）：只留最新 `max_runs_per_task` 条。
    ///
    /// 三条不变量：
    /// - **Running 永不裁**（裁了会让并发槽计算与 cancel 找不到 run）；
    /// - 刚发起的 run 必在保留集（它是本任务 `current_run`）；
    /// - `task.runs`（id 清单）与 `self.runs`（全集）同步裁，不得一边有一边没。
    fn prune_runs(&mut self, project: &str, task_id: &str) {
        let keep = self.max_runs_per_task.max(1);
        // 本任务的 run 按发生顺序（self.runs 是 push 顺序）取出 id
        let ids: Vec<RunId> = self
            .runs
            .iter()
            .filter(|r| r.task_project == project && r.task_id == task_id)
            .map(|r| r.id.clone())
            .collect();
        // 额度只算**终态**条目（Running 不算额度）——先数清再裁，否则会一边减一边
        // 把仍在跑的 run 当额度用（旧写法就踩了这个坑）。
        let is_running = |store: &Self, id: &RunId| {
            store
                .runs
                .iter()
                .find(|r| &r.id == id)
                .map(|r| r.status == RunStatus::Running)
                .unwrap_or(false)
        };
        let mut remaining_terminal = ids.iter().filter(|id| !is_running(self, id)).count();
        if remaining_terminal <= keep {
            return;
        }
        // 从最旧的终态开始裁，留最新 keep 条
        let mut doomed: Vec<RunId> = Vec::new();
        for id in &ids {
            if remaining_terminal <= keep {
                break;
            }
            if is_running(self, id) {
                continue;
            }
            doomed.push(id.clone());
            remaining_terminal -= 1;
        }
        if doomed.is_empty() {
            return;
        }
        let doomed_set: std::collections::BTreeSet<String> =
            doomed.iter().map(|id| id.0.clone()).collect();
        self.runs.retain(|r| !doomed_set.contains(&r.id.0));
        if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|t| t.project == project && t.id == task_id)
        {
            task.runs.retain(|id| !doomed_set.contains(&id.0));
        }
        tracing::info!(
            component = "store",
            event = "store.runs_pruned",
            project = %project,
            task = %task_id,
            pruned = doomed.len(),
            keep,
            "run 历史超出保留额度，已裁剪最早的终态 run（事件流不受影响）"
        );
    }

    /// 收尾 run：更新 run 终态与任务状态。
    pub fn finish_run(&mut self, run_id: &RunId, outcome: FinishOutcome) -> Result<()> {
        // 自审 major：先在作用域内改 run 终态并取出任务坐标，落下借用后再 persist
        // 与改任务——旧结构 run 借用跨 persist，任务查不到时还会跳过落盘。
        let (task_project, task_id) = {
            let run = self
                .runs
                .iter_mut()
                .find(|r| &r.id == run_id)
                .ok_or_else(|| DevforgeError::Store(format!("run `{}` 不存在", run_id.0)))?;
            if run.status != RunStatus::Running {
                return Err(DevforgeError::Store(format!(
                    "run `{}` 状态为 {:?}，不可收尾",
                    run_id.0, run.status
                )));
            }
            run.status = match outcome {
                FinishOutcome::Completed => RunStatus::Settled,
                FinishOutcome::Failed { .. } => RunStatus::Failed,
            };
            run.ended_at_ms = Some(now_ms());
            // 自审 major：失败原因落进 run 记录（旧实现收下 FinishOutcome 的 reason
            // 只落状态，原因既不落库也不进日志，重启后无法复盘）。
            if let FinishOutcome::Failed { reason } = &outcome {
                run.reason = Some(reason.clone());
            }
            (run.task_project.clone(), run.task_id.clone())
        };

        // 先落盘：即使任务随后查不到，run 终态也已在盘（两边不再分歧）。
        self.persist();

        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.project == task_project && t.id == task_id);
        let Some(task) = task else {
            return Err(DevforgeError::Store(format!(
                "run `{}` 的任务 `{task_project}/{task_id}` 不存在（run 终态已落盘，任务状态未动）",
                run_id.0
            )));
        };
        task.status = match outcome {
            FinishOutcome::Completed => TaskStatus::Completed,
            FinishOutcome::Failed { .. } => TaskStatus::Failed,
        };
        tracing::info!(
            component = "store",
            event = "store.run_finished",
            project = %task_project,
            task = %task_id,
            run_id = %run_id.0,
            completed = matches!(outcome, FinishOutcome::Completed),
            "run 已收尾"
        );
        // 自审 minor（114）：收尾当场裁一次——否则本任务的终态 run 会多留一条到下次
        // begin_run 才被剪，而这期间每次变更都在重写多出来的那份体积。
        self.prune_runs(&task_project, &task_id);
        self.persist();
        Ok(())
    }

    /// 启动对账（自审 critical）：进程崩溃/被杀后 store 里遗留的 Running run 会永久
    /// 占死并发槽（running_run_count 只认终态），任务也停在 Running 无法再发起 run
    /// ——无自愈即整机拒绝服务，只能手改 store.json。load 后调用一次：Running run
    /// 判中断失败、Running 任务复位为 Failed，让下一次 begin_run 正常走。
    /// 返回对账条数（供启动日志汇总；0 表示无需对账也不落盘）。
    pub fn reconcile_zombie_runs(&mut self) -> usize {
        let mut n = 0;
        for run in self
            .runs
            .iter_mut()
            .filter(|r| r.status == RunStatus::Running)
        {
            run.status = RunStatus::Failed;
            run.ended_at_ms = Some(now_ms());
            n += 1;
        }
        for task in self
            .tasks
            .iter_mut()
            .filter(|t| t.status == TaskStatus::Running)
        {
            task.status = TaskStatus::Failed;
            n += 1;
        }
        if n > 0 {
            self.persist();
        }
        n
    }

    pub fn get_run(&self, run_id: &RunId) -> Option<&Run> {
        self.runs.iter().find(|r| &r.id == run_id)
    }

    pub fn list_runs(&self) -> &[Run] {
        &self.runs
    }

    // ---------- 持久化（JSON 单文件，MVP 口径） ----------

    /// 铁律守卫（用户 10-25 拍板）：路径命中临时目录返回违反原因，否则 None。
    /// 覆盖 /tmp、/var/tmp、/dev/shm、/run 及其子路径（前缀判定，含恰等）。
    pub fn tmp_path_violation(path: &std::path::Path) -> Option<String> {
        // 自审 minor（119）：旧口径只做字符串前缀判定——相对路径或含符号链接的路径可以
        // 绕过（`./x` 指到 /tmp、或 /var/tmp 的软链）。现在先尽力 canonicalize（不存在时
        // canonicalize 父目录再拼文件名），相对路径直接判违规（无法证明落点）。
        let canonical = match std::fs::canonicalize(path) {
            Ok(c) => c,
            Err(_) => match (path.parent(), path.file_name()) {
                (Some(parent), Some(name)) => std::fs::canonicalize(parent)
                    .map(|c| c.join(name))
                    .unwrap_or_else(|_| path.to_path_buf()),
                _ => path.to_path_buf(),
            },
        };
        if !canonical.is_absolute() {
            return Some(format!(
                "路径 `{}` 不是绝对路径（无法证明落点，按临时路径处理）",
                path.display()
            ));
        }
        let p = canonical.to_string_lossy();
        for tmp in ["/tmp", "/var/tmp", "/dev/shm", "/run"] {
            if p == tmp || p.starts_with(&format!("{tmp}/")) {
                return Some(format!("位于临时目录 {tmp}（非持久，重启即清）"));
            }
        }
        None
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .ctx("序列化 store")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ctx("创建 store 目录")?;
        }
        // 自审 major：原子写（tmp + sync + rename，同 runstate::StateStore 口径）——
        // 旧实现截断式 write，崩溃/掉电产出半截 store.json，下次启动静默冷启动
        // 丢全部三层对象。临时名按写者唯一（多进程同根时互不踩踏）。
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = path.with_extension(format!(
            "json.{}.{seq}.tmp",
            std::process::id()
        ));
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)
                .ctx(format!("建 store 临时文件 {}", tmp.display()))?;
            f.write_all(json.as_bytes()).ctx("写 store 临时文件")?;
            f.sync_all().ctx("刷盘 store")?;
        }
        let renamed = std::fs::rename(&tmp, path).ctx(format!("落盘 store {}", path.display()));
        if renamed.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        renamed
    }

    pub fn load(path: &Path) -> Result<Self> {
        let json = std::fs::read_to_string(path)
            .ctx(format!("读取 store {}", path.display()))?;
        let store: Store = serde_json::from_str(&json).ctx("解析 store")?;
        // 自审 major：加载路径同样过名字安檢——域层「唯一入口」断言不能被
        // 被篡改/损坏的 store.json 绕过（穿越名经 load 直接进内存）。
        for p in &store.projects {
            Self::validate_path_name(&p.name)?;
        }
        for t in &store.tasks {
            Self::validate_path_name(&t.id)?;
            Self::validate_path_name(&t.project)?;
            // 自审 major：task.sop 同样被拼进 plans_root 下的路径段（SOP 状态目录），
            // 篡改过的 store.json 不得借它越界。
            Self::validate_path_name(&t.sop)?;
        }
        Ok(store)
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "store_pg_tests.rs"]
mod pg_tests;
