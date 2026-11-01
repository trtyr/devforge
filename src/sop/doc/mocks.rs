//! doc runner 的协议级测试 mock 簇（P009 口径：不 spawn 真 pi）。
//!
//! 两族：`MockAgent`（Rc 版，行为类测试——`run_pipeline` 的 agent 泛型无 Send 约束）
//! 与 `PipeAgent`/`PipeReviewFactory`（Arc/Mutex 版，DR-14 并行测试——要进审查线程）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::engine::client::ToolSubmission;
use crate::error::{DevforgeError, Result};
use crate::runstate::StateStore;
use crate::sop::doc::state::{bump_review, mark_done};
use crate::sop::planning::{PlanAgentSession, ReviewSession, ReviewSessionFactory, UserLink};

pub struct NoUser;
impl UserLink for NoUser {
    fn ask(&mut self, q: &str, _: &[&str]) -> Result<String> {
        Err(DevforgeError::System(format!("测试不应有用户交互: {q}")))
    }
}

pub struct MockAgent {
    pub turns: VecDeque<String>,
    pub received: Rc<RefCell<Vec<String>>>,
    /// AF-5：交卷工具提交脚本（队首对应当前回合；空/缺 = 该回合无提交 → 文本回退）。
    pub submissions: RefCell<VecDeque<Vec<ToolSubmission>>>,
}
impl MockAgent {
    pub fn new(turns: &[&str]) -> Self {
        Self {
            turns: turns.iter().map(|s| s.to_string()).collect(),
            received: Rc::new(RefCell::new(Vec::new())),
            submissions: RefCell::new(Default::default()),
        }
    }

    /// AF-5：带交卷工具提交脚本的 mock（与 turns 按回合对齐）。
    pub fn with_submissions(turns: &[&str], submissions: Vec<Vec<ToolSubmission>>) -> Self {
        Self {
            turns: turns.iter().map(|s| s.to_string()).collect(),
            received: Rc::new(RefCell::new(Vec::new())),
            submissions: RefCell::new(submissions.into()),
        }
    }
}
impl PlanAgentSession for MockAgent {
    fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        self.received.borrow_mut().push(message.into());
        Ok(self.turns.pop_front().expect("MockAgent 轮次耗尽"))
    }
    fn take_tool_submissions(&mut self) -> Vec<ToolSubmission> {
        self.submissions.borrow_mut().pop_front().unwrap_or_default()
    }
}

/// AF-5 测试用：构造一条 submit_unit 提交。
pub fn submit_unit(unit: &str) -> ToolSubmission {
    ToolSubmission {
        tool: "submit_unit".into(),
        args: serde_json::json!({ "unit": unit }),
    }
}

/// R6-P56/P66 测试用：构造一条 submit_verdict 裁决提交（args 形状与
/// protocol::parse_tool_verdict 对齐——PASS 无需 issues，FAIL 必须非空）。
pub fn submit_verdict(verdict: &str, issues: &[&str]) -> ToolSubmission {
    ToolSubmission {
        tool: "submit_verdict".into(),
        args: serde_json::json!({ "verdict": verdict, "issues": issues }),
    }
}

/// 无闸门审查工厂：裁决立即返回（行为类测试用）
pub fn pipe_factory(replies: &[&str]) -> PipeReviewFactory {
    PipeReviewFactory {
        replies: Arc::new(Mutex::new(
            replies.iter().map(|s| s.to_string()).collect(),
        )),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: no_gates(),
        submissions: Arc::new(Mutex::new(Default::default())),
        deadlines: Arc::new(Mutex::new(Vec::new())),
    }
}

/// R6-P56/P66：带审查侧工具提交脚本的工厂——第 N 次开会话的会话拿到第 N 条
/// 提交脚本（该会话每次 take_tool_submissions 吐出同一条脚本的全部提交）。
pub fn pipe_factory_with_submissions(
    replies: &[&str],
    submissions: Vec<Vec<ToolSubmission>>,
) -> PipeReviewFactory {
    PipeReviewFactory {
        replies: Arc::new(Mutex::new(
            replies.iter().map(|s| s.to_string()).collect(),
        )),
        instructions: Arc::new(Mutex::new(Vec::new())),
        opens: Arc::new(Mutex::new(0)),
        gates: no_gates(),
        submissions: Arc::new(Mutex::new(submissions.into())),
        deadlines: Arc::new(Mutex::new(Vec::new())),
    }
}

pub fn state_path(tmp: &tempfile::TempDir) -> StateStore {
    StateStore::new(
        tmp.path().join("plans"),
        crate::runstate::StateKey::new("p", "t", crate::sop::doc::resume::DOC_SOP),
    )
}

/// 种一个「已有 done」的断点（续跑类用例公用）。
pub fn seed_done(store: &StateStore, done: &[&str]) {
    let mut state = store.fresh_state();
    for u in done {
        mark_done(&mut state, u, &[]);
    }
    if !done.is_empty() {
        bump_review(&mut state); // 与旧 fixture 一致：已完成单元落过一次审查轮次
    }
    store.save(&mut state).unwrap();
}

// ── DR-14 流水线并行测试（Send 版 mock）──

#[derive(Clone)]
pub struct PipeAgent {
    replies: Arc<Mutex<VecDeque<String>>>,
    pub received: Arc<Mutex<Vec<String>>>,
}
impl PipeAgent {
    pub fn new(replies: &[&str]) -> (Self, Arc<Mutex<Vec<String>>>) {
        let received = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                replies: Arc::new(Mutex::new(
                    replies.iter().map(|s| s.to_string()).collect(),
                )),
                received: received.clone(),
            },
            received,
        )
    }
}
impl PlanAgentSession for PipeAgent {
    fn converse(&mut self, message: &str, _: &mut dyn UserLink) -> Result<String> {
        self.received.lock().unwrap().push(message.into());
        let popped = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default();
        Ok(popped)
    }
}

#[derive(Clone)]
pub struct PipeReviewFactory {
    pub replies: Arc<Mutex<VecDeque<String>>>,
    pub instructions: Arc<Mutex<Vec<String>>>,
    /// 自审 major：open() 次数——「空回复重试必须换新实例」的契约此前无任何
    /// 断言观测（instructions 记在 review()，同实例调两次也一样是 2）。
    pub opens: Arc<Mutex<usize>>,
    // 闸门：每个审查会话开庭后等放行再出裁决（模拟真实审查耗时，测试才能确定性断言重叠）
    pub gates: Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>>,
    /// R6-P66：审查侧工具提交脚本（按会话开启序分配；空 = 该会话无提交 → 文本降级）。
    pub submissions: Arc<Mutex<VecDeque<Vec<ToolSubmission>>>>,
    /// R6-P59：每次 review_with_deadline 传入的墙钟（None = 传了 None——
    /// 用例断言「显式墙钟已传入」用；不记录 = 墙钟路径没走到）。
    pub deadlines: Arc<Mutex<Vec<Option<std::time::Instant>>>>,
}
impl ReviewSessionFactory for PipeReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        *self.opens.lock().unwrap() += 1;
        Ok(Box::new(PipeReviewSession {
            replies: self.replies.clone(),
            instructions: self.instructions.clone(),
            gate: self.gates.clone(),
            submissions: self.submissions.clone(),
            deadlines: self.deadlines.clone(),
        }))
    }
}
struct PipeReviewSession {
    replies: Arc<Mutex<VecDeque<String>>>,
    instructions: Arc<Mutex<Vec<String>>>,
    gate: Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>>,
    submissions: Arc<Mutex<VecDeque<Vec<ToolSubmission>>>>,
    deadlines: Arc<Mutex<Vec<Option<std::time::Instant>>>>,
}
impl ReviewSession for PipeReviewSession {
    fn review(&mut self, instruction: &str) -> Result<String> {
        self.instructions.lock().unwrap().push(instruction.into());
        if let Some(rx) = self.gate.lock().unwrap().pop_front() {
            let _ = rx.recv();
        }
        Ok(self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default())
    }

    fn take_tool_submissions(&mut self) -> Vec<crate::engine::client::ToolSubmission> {
        self.submissions
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default()
    }

    /// R6-P59：记录传入的墙钟并按文本路径出结果（脚本替身无阻塞语义，但
    /// 「墙钟确实被传入」这件事要用例可断言）。
    fn review_with_deadline(
        &mut self,
        instruction: &str,
        deadline: Option<std::time::Instant>,
    ) -> Result<String> {
        self.deadlines.lock().unwrap().push(deadline);
        self.review(instruction)
    }
}

/// 事件驱动放行：每个闸门等 watch 闭包达标后放行；10s 死线强制全放（防测试悬挂）
pub fn drive_gates(
    watch: Vec<(Box<dyn Fn() -> usize + Send>, usize)>,
    gate_txs: Vec<std::sync::mpsc::Sender<()>>,
) {
    std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        for (tx, (count_fn, threshold)) in gate_txs.into_iter().zip(watch) {
            while std::time::Instant::now() < deadline && count_fn() < threshold {
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            let _ = tx.send(());
        }
    });
}

pub fn gate_pair() -> (
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<()>,
) {
    std::sync::mpsc::channel()
}

pub fn no_gates() -> Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>> {
    Arc::new(Mutex::new(Default::default()))
}
