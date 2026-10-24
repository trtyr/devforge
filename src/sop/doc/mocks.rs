//! doc runner 的协议级测试 mock 簇（P009 口径：不 spawn 真 pi）。
//!
//! 两族：`MockAgent`（Rc 版，行为类测试——`run_pipeline` 的 agent 泛型无 Send 约束）
//! 与 `PipeAgent`/`PipeReviewFactory`（Arc/Mutex 版，DR-14 并行测试——要进审查线程）。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

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
}
impl MockAgent {
    pub fn new(turns: &[&str]) -> Self {
        Self {
            turns: turns.iter().map(|s| s.to_string()).collect(),
            received: Rc::new(RefCell::new(Vec::new())),
        }
    }
}
impl PlanAgentSession for MockAgent {
    fn converse(&mut self, message: &str, _ui: &mut dyn UserLink) -> Result<String> {
        self.received.borrow_mut().push(message.into());
        Ok(self.turns.pop_front().expect("MockAgent 轮次耗尽"))
    }
}

/// 无闸门审查工厂：裁决立即返回（行为类测试用）
pub fn pipe_factory(replies: &[&str]) -> PipeReviewFactory {
    PipeReviewFactory {
        replies: Arc::new(Mutex::new(
            replies.iter().map(|s| s.to_string()).collect(),
        )),
        instructions: Arc::new(Mutex::new(Vec::new())),
        gates: no_gates(),
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
    // 闸门：每个审查会话开庭后等放行再出裁决（模拟真实审查耗时，测试才能确定性断言重叠）
    pub gates: Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>>,
}
impl ReviewSessionFactory for PipeReviewFactory {
    fn open(&mut self) -> Result<Box<dyn ReviewSession>> {
        Ok(Box::new(PipeReviewSession {
            replies: self.replies.clone(),
            instructions: self.instructions.clone(),
            gate: self.gates.clone(),
        }))
    }
}
struct PipeReviewSession {
    replies: Arc<Mutex<VecDeque<String>>>,
    instructions: Arc<Mutex<Vec<String>>>,
    gate: Arc<Mutex<VecDeque<std::sync::mpsc::Receiver<()>>>>,
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
