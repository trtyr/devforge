//! 四阶段审查编排（P013 M1）：Plan → Batch → Cross → (汇总交 M2)。
//!
//! 上下文效率设计（CodeCrow 实证）：
//! - Stage 0 只喂元数据（切片清单），喂代码会让 LLM 拉着代码不做规划；
//! - Stage 1 每批次**独立会话**（≤7 文件，批次间无污染、可并行；视角 = 批次
//!   会话提示词内检查清单，源码装一次综合全审——视角不是独立 agent）；
//! - Stage 2 输入是瘦身后 findings（只留 file/line/severity/reason/claim），
//!   结构性视角（V4 架构/V5 一致性）在此审，不重复装源码；
//! - 断点：批次粒度进度落黑板 progress.json（重启后跳过已完成批次）。

use std::path::Path;

use serde::Deserialize;

use crate::error::{Context as _, DevforgeError, Result};
use crate::sop::planning::{ReviewSession, ReviewSessionFactory};

use super::finding::{parse_findings, Finding};
use super::AuditUnit;

/// Stage 0 产物：审查计划（优先级分组 + 可跳过 + 跨单元假设）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReviewPlan {
    /// 优先审查的批次 id（含核心业务逻辑/复杂依赖）。
    pub priority: Vec<String>,
    /// 可跳过的批次 id（生成物/锁文件/明显机械代码）。
    pub skip: Vec<String>,
    /// 跨单元关注假设（Stage 2 逐一验证）。
    pub cross_concerns: Vec<String>,
}

/// Stage 2 产物：跨单元分析结论。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CrossAnalysis {
    /// 跨单元发现（V4 架构 / V5 一致性视角）。
    pub findings: Vec<Finding>,
    /// 对 Stage 0 假设的验证结论（假设 → 结论）。
    pub concern_verdicts: Vec<(String, String)>,
    /// 全仓审计 m4：被静默丢弃的条目数（单条反序列化失败 / 非二元组 verdict）——
    /// 旧实现只 `continue`，丢多少不可见；现计数进报告。
    #[serde(default)]
    pub parse_errors: usize,
}

/// 审计会话抽象（假 session 可注入；生产 = RpcReviewFactory 桥）。
pub trait AuditSession {
    fn ask(&mut self, instruction: &str) -> Result<String>;

    /// 带墙钟的 ask（第三方 r3 M4）：默认忽略墙钟（脚本替身无阻塞语义）；
    /// 真 RPC 会话覆写成「到点返回 Quality」。
    fn ask_deadline(
        &mut self,
        instruction: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<String> {
        let _ = deadline;
        self.ask(instruction)
    }
}

pub trait AuditSessionFactory {
    fn open(&mut self) -> Result<Box<dyn AuditSession>>;

    /// 全仓审计 M3：可选派生（并行批次用）。默认 `None` = 该工厂不支持并行，
    /// 编排层回落串行（测试替身多为脚本队列，天然不适合并发）。
    fn fork(&self) -> Option<Box<dyn AuditSessionFactory + Send>> {
        None
    }
}

/// 桥：ReviewSessionFactory（decisions/004 血统）→ AuditSessionFactory。
/// ask = review（同签名不同 trait，适配后编排层不依赖具体桥实现）。
pub struct AuditSessionFactoryBridge<F: ReviewSessionFactory>(pub F);

impl<F: ReviewSessionFactory + Clone + Send + 'static> AuditSessionFactory
    for AuditSessionFactoryBridge<F>
{
    fn open(&mut self) -> Result<Box<dyn AuditSession>> {
        Ok(Box::new(SessionBridge(self.0.open()?)))
    }

    /// 并行批次：派生共享 spec 生成器的工厂（session 目录序号由原子计数保证唯一）。
    fn fork(&self) -> Option<Box<dyn AuditSessionFactory + Send>> {
        Some(Box::new(AuditSessionFactoryBridge(self.0.clone())))
    }
}

pub struct SessionBridge(pub Box<dyn ReviewSession>);

impl AuditSession for SessionBridge {
    fn ask(&mut self, instruction: &str) -> Result<String> {
        self.0.review(instruction)
    }

    fn ask_deadline(
        &mut self,
        instruction: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<String> {
        self.0.review_with_deadline(instruction, deadline)
    }
}

/// 通用提取：回复末尾的 ```json 代码块（Stage 0/2 用，findings 用专用协议）。
fn extract_json_block(reply: &str) -> Result<&str, String> {
    let start = reply
        .find("```json")
        .ok_or_else(|| "缺 ```json 块".to_string())?;
    let body = &reply[start + 7..];
    let end = body
        .find("```")
        .ok_or_else(|| "```json 块未闭合".to_string())?;
    Ok(body[..end].trim())
}

/// Stage 0：元数据规划——只喂切片清单（id/文件数/标签），不喂代码。
/// 单发入口（测试/兼容）；生产走 runner 的换实例重试包装（全仓审计 M2）。
pub fn stage0_plan(
    session: &mut dyn AuditSession,
    units: &[AuditUnit],
) -> Result<ReviewPlan> {
    let reply = session.ask(&stage0_instruction(units))?;
    parse_stage0(&reply)
}

fn stage0_instruction(units: &[AuditUnit]) -> String {
    let meta: Vec<serde_json::Value> = units
        .iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "files": u.files.len(),
                "label": u.label,
            })
        })
        .collect();
    let instruction = format!(
        "你是审计规划官。以下是全仓审计的批次清单（仅元数据，无代码）。\
         产出审查计划，输出一个 ```json 块：{{\"priority\": [\"unit-XXX\"...], \
         \"skip\": [\"unit-XXX\"...], \"cross_concerns\": [\"假设\"...]}}。\n\
         priority = 应优先深审的批次（核心业务逻辑/复杂依赖）；skip = 只可跳生成物/锁文件（引擎二次校验，非生成物一律忽略）\n\
         （生成物/锁文件/纯声明无逻辑）；cross_concerns = 跨批次的结构性关注假设\n\
         （如「错误处理模式在各 SOP 是否一致」「事件命名是否统一」）。\n\n批次清单：\n{meta:?}"
    );
    instruction
}

/// Stage 0 反应解析（纯函数）：重试只需重开会话，不再重复组装指令。
pub fn parse_stage0(reply: &str) -> Result<ReviewPlan> {
    let json_text =
        extract_json_block(reply).map_err(DevforgeError::Quality)?;
    let v: serde_json::Value =
        serde_json::from_str(json_text).map_err(|e| DevforgeError::Quality(format!("ReviewPlan JSON 非法：{e}")))?;
    let arr = |key: &str| -> Vec<String> {
        v.get(key)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(ReviewPlan {
        priority: arr("priority"),
        skip: arr("skip"),
        cross_concerns: arr("cross_concerns"),
    })
}

/// Stage 1 单批次指令（工作区路径 + 六视角检查清单 + 预查报告 + 跨批次路径）。
///
/// 第三方 r2 C1：审查会话的 cwd 是**黑板**（`builtin.rs` 把 `working_dir` 设为 task 目录），
/// 不是被审仓——旧指令只给仓内相对路径，审查官无法定位源码（只能猜行号）。
/// 口径对齐 `sop::doc::fix::doc_review_instruction`：「黑板 = 当前目录；代码工作区 = 绝对路径」。
pub fn batch_instruction(
    workspace: &Path,
    unit: &AuditUnit,
    all_unit_labels: &[String],
    precheck_report: &str,
) -> String {
    let others: Vec<&String> = all_unit_labels.iter().filter(|l| **l != unit.label).collect();
    // r3 m5：超大文件分片——把「本批看哪段」写进指令，否则 1000+ 行的编排内核
    // 整包丢给一个会话，并行只提吞吐、降不了装载量。
    let focus_line = if unit.focus.is_empty() {
        String::new()
    } else {
        format!(
            "本批是**超大文件分片**：同一文件的其它符号段由其它批次负责（不要重复报同一处）；\
             重点符号：\n- {}\n",
            unit.focus.join("\n- ")
        )
    };
    format!(
        "代码工作区（绝对路径）= {}；当前目录是共享黑板（两者不同）。\n\
         读源码请用工作区路径拼下面列出的相对路径；你输出的 `path:line` 里的 path 也是相对工作区的。\n\
         审查批次 `{}`，文件：{:?}。\n\
         {focus_line}引擎预查报告（参考计数，非裁决依据——简化扫描含字符串/test 代码命中）：\n{precheck_report}\n\
         其他批次（路径感知，不在本批审查范围）：{others:?}\n\
         按你的角色提示词执行六视角审查并输出 <audit-findings> 协议块。",
        workspace.display(),
        unit.label,
        unit.files,
    )
}

/// Stage 1 执行：单批次审查 + 协议解析（协议违例重开一次，再违例放弃该批次）。
pub fn stage1_review_batch(
    factory: &mut dyn AuditSessionFactory,
    workspace: &Path,
    unit: &AuditUnit,
    all_unit_labels: &[String],
    precheck_report: &str,
) -> Result<Vec<Finding>> {
    stage1_review_batch_with_deadline(
        factory,
        workspace,
        unit,
        all_unit_labels,
        precheck_report,
        None,
    )
}

/// 带墙钟的批次审查（第三方 r3 M4）：`deadline` 到点 → 会话返回 Quality，
/// 该批被 runner 记入 abandoned 后继续（「活死批」不得占着 worker 不理）。
pub fn stage1_review_batch_with_deadline(
    factory: &mut dyn AuditSessionFactory,
    workspace: &Path,
    unit: &AuditUnit,
    all_unit_labels: &[String],
    precheck_report: &str,
    deadline: Option<std::time::Instant>,
) -> Result<Vec<Finding>> {
    let mut last_err = String::new();
    for attempt in 0..2 {
        let mut session = factory.open().ctx("开审计会话")?;
        let instruction = batch_instruction(workspace, unit, all_unit_labels, precheck_report);
        let reply = match session.ask_deadline(&instruction, deadline) {
            Ok(r) => r,
            // 审计修复 #1：ask 失败保持原分类上抛（System/Agent 等基础设施故障）——
            // 不包 Quality：实例失败该打断整次审计而非伪装成质量失败被放弃。
            Err(e) => return Err(e),
        };
        drop(session);
        match parse_findings(&reply) {
            Ok(mut findings) => {
                for f in &mut findings {
                    f.batch = unit.id.clone();
                }
                return Ok(findings);
            }
            Err(e) => {
                last_err = format!("第 {} 次协议违例：{e}", attempt + 1);
                tracing::warn!(
                    component = "audit-engine",
                    unit = %unit.id,
                    attempt = attempt + 1,
                    "findings 协议违例，换实例重审"
                );
            }
        }
    }
    // 两次违例：放弃批次（不编造发现）——诚实失败优于噪声
    tracing::warn!(component = "audit-engine", unit = %unit.id, "批次两次协议违例，放弃");
    Err(DevforgeError::Quality(format!(
        "批次 {} 两次协议违例，已放弃：{last_err}",
        unit.id
    )))
}

/// Stage 2 执行：跨单元分析（输入瘦身 findings + 跨单元假设）。
pub fn stage2_cross(
    session: &mut dyn AuditSession,
    findings: &[Finding],
    cross_concerns: &[String],
) -> Result<CrossAnalysis> {
    let reply = session.ask(&stage2_instruction(findings, cross_concerns))?;
    parse_stage2(&reply)
}

fn stage2_instruction(findings: &[Finding], cross_concerns: &[String]) -> String {
    let slim: Vec<serde_json::Value> = findings
        .iter()
        .map(|f| {
            serde_json::json!({
                "file": f.path, "line": f.line, "severity": f.severity,
                "view": f.view, "claim": f.claim,
            })
        })
        .collect();
    let instruction = format!(
        "你是审计的跨单元分析官。以下是瘦身后发现清单（location/severity/claim）\n\
         与全仓关系图（codegraph explore 可查）。任务：\n\
         1. V5 一致性：同一能力在多处实现的强度是否对齐（日志/错误处理/重试/事件命名）；\n\
         2. V4 架构：发现里反映的结构性问题归因（重复根因合并指出）；\n\
         3. 验证跨单元假设：逐条给结论。\n\
         输出一个 ```json 块：{{\"findings\": [同 audit-findings 条目形态], \
         \"concern_verdicts\": [[\"假设\", \"结论\"]...]}}。\n\n\
         发现清单：{slim:?}\n\n跨单元假设：{cross_concerns:?}"
    );
    instruction
}

/// Stage 2 反应解析（纯函数）：重试只需重开会话，不再重复组装指令。
pub fn parse_stage2(reply: &str) -> Result<CrossAnalysis> {
    let json_text =
        extract_json_block(reply).map_err(DevforgeError::Quality)?;
    let v: serde_json::Value = serde_json::from_str(json_text)
        .map_err(|e| DevforgeError::Quality(format!("跨单元分析 JSON 非法：{e}")))?;
    let mut out = CrossAnalysis::default();
    if let Some(arr) = v.get("findings").and_then(|x| x.as_array()) {
        for item in arr {
            let mut f: Finding = match serde_json::from_value(item.clone()) {
                Ok(f) => f,
                Err(e) => {
                    // 单条坏不拖垮整批（Stage 2 是汇总性质），但**要计数留痕**（m4）
                    tracing::warn!(component = "audit-engine", error = %e, "Stage 2 单条发现解析失败，已跳过");
                    out.parse_errors += 1;
                    continue;
                }
            };
            f.batch = "cross".into();
            out.findings.push(f);
        }
    }
    if let Some(arr) = v.get("concern_verdicts").and_then(|x| x.as_array()) {
        for pair in arr {
            let ok = pair
                .as_array()
                .filter(|both| both.len() == 2)
                .and_then(|both| Some((both[0].as_str()?, both[1].as_str()?)));
            match ok {
                Some((a, b)) => out.concern_verdicts.push((a.to_string(), b.to_string())),
                None => {
                    // m4：非二元组/非字符串的 verdict 不静默丢——计数入报告
                    tracing::warn!(component = "audit-engine", item = %pair, "Stage 2 假设结论形态非法，已跳过");
                    out.parse_errors += 1;
                }
            }
        }
    }
    Ok(out)
}

/// 断点：进度落黑板（批次粒度）。
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, Deserialize)]
pub struct Progress {
    pub stage0_done: bool,
    pub done_batches: Vec<String>,
    pub stage2_done: bool,
    /// 全仓审计 m13：放弃批次**入断点**——旧实现只活在本次 run 的局部变量，
    /// 报告里的 abandoned 不反映历史（重跑会重试，口径不完整）。
    #[serde(default)]
    pub abandoned: Vec<String>,
}

impl Progress {
    pub fn load(blackboard: &Path) -> Self {
        match std::fs::read_to_string(blackboard.join("audit-progress.json"))
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        {
            Ok(p) => p,
            Err(e) => {
                // 审计修复 #8：文件不存在 = 冷启动（静默）；存在但读/解析失败 = 损坏，
                // 整表重跑代价大，必须 WARN 留痕（与 change::state::load 口径一致，复验 #5 同构）。
                if std::path::Path::new(blackboard)
                    .join("audit-progress.json")
                    .exists()
                {
                    tracing::warn!(
                        component = "audit-engine",
                        error = %e,
                        "进度文件损坏，断点作废整表重跑"
                    );
                }
                Self::default()
            }
        }
    }
    pub fn save(&self, blackboard: &Path) -> Result<()> {
        std::fs::write(
            blackboard.join("audit-progress.json"),
            serde_json::to_vec_pretty(self).ctx("序列化进度")?,
        )
        .ctx("写进度")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
}
