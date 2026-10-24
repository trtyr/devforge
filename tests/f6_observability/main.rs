//! F6 靶的验收对账测试（黑盒口径，plan 010 · change-20261022-c7-1）。
//!
//! 口径：只经**公开面**观察——`devforge::` 的导出面、HTTP 端点、日志条目、落盘文件；
//! 不读私有状态、不改产品代码。每条用例首行写「错实现形态」：把实现改坏时本用例必须变红。
//!
//! 覆盖面 = 变更计划 §5 的验收标准（逐条对账见黑板 `testing/acceptance.md`）。
//! ARC-R6 拆分：support 采集器+mocks / fixture 服务器夹具 / runlog run 级日志契约 /
//! milestone 里程碑与 review_open / change change 侧 / panic hook 与心跳 / guards 源码守卫。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, MutexGuard, Once, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};

use devforge::change::gate0::DocLevel;
use devforge::change::orchestrator::{
    ChangeRun, ChangeSession, CodingTurn, PlanTurn, TestingTurn, run as run_change,
};
use devforge::change::session::{ChangeCtx, ProdChangeSession};
use devforge::domain::run::Run;
use devforge::domain::runner::{RunControl, RunEngine, RunOutcome};
use devforge::domain::sop::SopRegistry;
use devforge::domain::store::Store;
use devforge::domain::task::Task;
use devforge::engine::client::RpcClient;
use devforge::engine::process::SpawnSpec;
use devforge::engine::policy::{RetryPolicy, run_with_retry};
use devforge::error::{Context as _, DevforgeError, Result};
use devforge::observability as obs;
use devforge::runstate::{StateKey, StateStore};
use devforge::sop::planning::{PlanAgentSession, ReviewSession, ReviewSessionFactory, UserLink};

// ============================================================ 日志面捕获

static LOG: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static ACC: OnceLock<Mutex<String>> = OnceLock::new();
static CAPTURE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub mod support;
mod change;
mod fixture;
mod guards;
mod milestone;
mod panic;
mod runlog;
