//! 回注失败上下文送达 coding agent —— 验收对账测试
//! change-20261021-coding-agent-c4-path-line-coding-src-cha（计划 §5 验收标准 1–17）
//!
//! 黑盒口径：只经**公开 API** 观察行为——`ChangeState` / `ChangeSession` / `run_change` /
//! `coding_kickoff` / `route_back_message` / 断点文件 / 仓内文档与常量——不窥探内部实现；
//! 每条用例首行写明「错实现形态」——写不出错实现形态的断言当场不写（防假绿）。
//!
//! 与仓内 `src/change/**` 的 `#[cfg(test)]` 单测相互独立：那一组在实现内部就地断言，
//! 本组从**仓外**（`tests/` 集成测试）看公开面，专门证伪「送达通道 / 清除语义 / 口径翻转」
//! 这三件事是否真的落到对外可观测的行为上。

// ARC-R6：本文件只作 target root（mod 声明 + 共享 use），use 项由子 mod 经 super::* 链式使用
#![allow(unused_imports)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use devforge::change::boundary::ImpactGap;
use devforge::change::gate0::DocLevel;
use devforge::change::orchestrator::{
    ChangeRun, ChangeSession, CodingTurn, PlanTurn, TestingTurn, run as run_change,
};
use devforge::change::session::{ChangeCtx, ProdChangeSession, coding_kickoff};
use devforge::change::state::{
    CHANGE_SOP, ChangePhase, ChangeState, TestFailureContext, load, save,
};
use devforge::change::testing_gate::{FAILURE_BLOCK_TITLE, route_back_message};
use devforge::error::{DevforgeError, Result};
use devforge::runstate::{StateKey, StateStore};
use devforge::sop::planning::{PlanAgentSession, ReviewSession, ReviewSessionFactory, UserLink};

mod context;
mod guards;
mod support;
