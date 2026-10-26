//! 变更 coding 阶段的两道机械门（P009 T010 / T010b）。
//!
//! - **回归前置**：动手前必须有过一次「既有测试全绿」的证据（`evidence/baseline-test.log`），
//!   日志里的退出码非 0 → 不许动代码。引擎不自己跑（那是 agent 的判断），只验证据。
//! - **实施步骤与按步提交**：`impl-steps.md` 的步骤数必须与基线之后的提交数、步骤号一一对上；
//!   对不上说明「说一套做一套」，直接 FAIL。
//!
//! 两者都是**质量门**：失败即 `DevforgeError::Quality`，不进自动重启。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{DevforgeError, Result};

/// 证据文件（相对黑板）。
pub const BASELINE_LOG: &str = "evidence/baseline-test.log";
/// 自证证据文件（相对黑板）：「先证后报」的机械门（形状与基线门一致）。
pub const SELF_CHECK_LOG: &str = "evidence/self-check.log";
/// 实施步骤文件（相对黑板）。
pub const IMPL_STEPS: &str = "impl-steps.md";
/// 提交信息里要求出现的 change-id 标记（便于机械归属）。
pub const COMMIT_PREFIX: &str = "change:";

/// 基线证据的结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineEvidence {
    pub exit_code: i32,
    pub command: String,
}

/// 解析证据日志：期望里面有命令行与退出码。
///
/// 允许的写法（任一）：`exit_code: 0` / `exit: 0` / `退出码: 0` / `退出码：0`。
///
/// `file` 与 `label` 是参数：同一套形状服务两道门（基线门 / 自证门），
/// 错误文案必须指向**各自**的证据文件——指向错文件 = 排查时找错地方。
fn parse_evidence_log(text: &str, file: &str, label: &str) -> Result<BaselineEvidence> {
    let mut exit_code = None;
    let mut command = String::new();
    for line in text.lines() {
        let mut t = line.trim();
        // F16：markdown 列表项容差——`- exit_code: 0` 与 `exit_code: 0` 语义相同，
        // 列表符号不参与语义（10-22 c7b 场：agent 用列表项写证据，解析器判死）。
        if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            t = rest.trim_start();
        }
        let lower = t.to_ascii_lowercase();
        if command.is_empty()
            && (lower.starts_with("command:") || t.starts_with("命令:") || t.starts_with("命令："))
        {
            command = t
                .split_once(':')
                .map(|(_, v)| v.trim().to_string())
                .or_else(|| t.split_once('：').map(|(_, v)| v.trim().to_string()))
                .unwrap_or_default();
            continue;
        }
        let candidates = [
            "exit_code:",
            "exit code:",
            "exit:",
            "退出码:",
            "退出码：",
        ];
        let mut found = false;
        for prefix in candidates {
            let l = if prefix.starts_with("exit") { lower.as_str() } else { t };
            if let Some(rest) = l.strip_prefix(prefix) {
                // F16b：值容差——`**0**`（粗体包裹）与 `0` 同义；尾随散文（`**0**（44 passed…）`）
                // 取前导整数。10-23 c7b 场：测试 agent 以列表项+粗体写回归日志，解析器判死。
                // 剥掉粗体后取前导（可选负号+）数字；无数字 = 模板/散文行，跳过继续扫。
                let v = rest.trim().trim_start_matches('*').trim_start();
                let mut ds = String::new();
                let mut cs = v.chars();
                if cs.clone().next() == Some('-') {
                    ds.push('-');
                    cs.next();
                }
                ds.extend(cs.take_while(|c| c.is_ascii_digit()));
                if let Ok(n) = ds.parse::<i32>() {
                    exit_code = Some(n);
                    found = true;
                    break;
                }
            }
        }
        if found {
            // 首中即停：主回归的退出码在文件最前（§1）；后面的 exit_code 行是
            // 反向验证的「能失败」证据（故意 101），不能覆盖主结果。
            break;
        }
    }
    let exit_code = exit_code.ok_or_else(|| {
        DevforgeError::Quality(format!(
            "{label}证据缺退出码（{file} 里要写 `exit_code: N`），无法判定{label}是否绿"
        ))
    })?;
    Ok(BaselineEvidence { exit_code, command })
}

/// 解析 `evidence/baseline-test.log`：期望里面有命令行与退出码。
///
/// 允许的写法（任一）：`exit_code: 0` / `exit: 0` / `退出码: 0` / `退出码：0`。
pub fn parse_baseline_log(text: &str) -> Result<BaselineEvidence> {
    parse_evidence_log(text, BASELINE_LOG, "基线")
}

/// 读证据文件并判定基线是否绿。
///
/// 自审 major（unit-011）：弱化版不再 pub——它传 UNIX_EPOCH 等于关掉新鲜度，
/// 生产只能用 `_fresh` 版；保留为测试底座，未来调用方拿不到（防再犯）。
#[cfg(test)]
pub fn check_baseline_log(blackboard: &Path) -> Result<BaselineEvidence> {
    check_evidence_fresh(
        blackboard.join(BASELINE_LOG),
        "缺少基线证据 {}（动手前必须先跑一遍既有测试）",
        "基线不是绿的",
        "不许在红基线上动代码",
        parse_baseline_log,
        std::time::SystemTime::UNIX_EPOCH,
    )
}

/// 基线门（绑定本轮）：证据文件 mtime 早于 `not_before`（本轮开始）视为上一轮残留
/// （复验 #6：上轮绿色证据不得通过本轮机械门）。
pub fn check_baseline_log_fresh(
    blackboard: &Path,
    not_before: std::time::SystemTime,
) -> Result<BaselineEvidence> {
    check_evidence_fresh(
        blackboard.join(BASELINE_LOG),
        "缺少基线证据 {}（动手前必须先跑一遍既有测试）",
        "基线不是绿的",
        "不许在红基线上动代码",
        parse_baseline_log,
        not_before,
    )
}

/// 自证门（绑定本轮）：同基线门，残留证据拒收。
pub fn check_self_check_fresh(
    blackboard: &Path,
    not_before: std::time::SystemTime,
) -> Result<BaselineEvidence> {
    check_evidence_fresh(
        blackboard.join(SELF_CHECK_LOG),
        "缺少自证证据 {}（提交前必须留下命令原文 + 退出码 + 摘要）",
        "自证不是绿的",
        "先证后报：证据红着不许提交",
        |text| parse_evidence_log(&text, SELF_CHECK_LOG, "自证"),
        not_before,
    )
}

/// 证据门通用实现：读文件 → mtime 不早于本轮开始 → 解析 → 退出码必须为 0。
fn check_evidence_fresh(
    path: std::path::PathBuf,
    missing_msg: &str,
    red_msg: &str,
    red_hint: &str,
    parse: fn(&str) -> Result<BaselineEvidence>,
    not_before: std::time::SystemTime,
) -> Result<BaselineEvidence> {
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "{}：{e}",
            missing_msg.replace("{}", &path.display().to_string())
        ))
    })?;
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    if mtime < not_before {
        return Err(DevforgeError::Quality(format!(
            "证据 {} 是上一轮残留（写入时间早于本轮开始）：本轮必须重跑重写，不许沿用旧绿色证据",
            path.display()
        )));
    }
    let ev = parse(&text)?;
    if ev.exit_code != 0 {
        return Err(DevforgeError::Quality(format!(
            "{}（exit_code={}，命令：{}）——{}",
            red_msg,
            ev.exit_code,
            if ev.command.is_empty() {
                "未记录".to_string()
            } else {
                ev.command
            },
            red_hint
        )));
    }
    Ok(ev)
}

/// 读自证证据文件并判定「先证后报」是否兑现（证据存在 + 退出码为 0）。
///
/// 与基线门同策略：失败即 Quality，不自动重问 Agent（证据红着 = 没证明）。
/// 自审 major（unit-011）：同基线门，弱化版收进测试底座，生产只用 `_fresh` 版。
#[cfg(test)]
pub fn check_self_check(blackboard: &Path) -> Result<BaselineEvidence> {
    let path = blackboard.join(SELF_CHECK_LOG);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "缺少自证证据 {}（提交前必须留下命令原文 + 退出码 + 摘要）：{e}",
            path.display()
        ))
    })?;
    let ev = parse_evidence_log(&text, SELF_CHECK_LOG, "自证")?;
    if ev.exit_code != 0 {
        return Err(DevforgeError::Quality(format!(
            "自证不是绿的（exit_code={}，命令：{}）——先证后报：证据红着不许提交",
            ev.exit_code,
            if ev.command.is_empty() {
                "未记录".to_string()
            } else {
                ev.command
            }
        )));
    }
    Ok(ev)
}

/// `impl-steps.md` 里的步骤（形如 `## 步骤 1` / `### 步骤 2`，也接受 `## Step 3`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub number: u32,
    pub title: String,
}

pub fn parse_steps(md: &str) -> Vec<Step> {
    let mut out = Vec::new();
    for line in md.lines() {
        let t = line.trim_start();
        if !t.starts_with('#') {
            continue;
        }
        let body = t.trim_start_matches('#').trim();
        let rest = body
            .strip_prefix("步骤")
            .or_else(|| body.strip_prefix("Step"))
            .or_else(|| body.strip_prefix("step"));
        let Some(rest) = rest else { continue };
        let rest = rest.trim_start_matches([' ', ':', '：', '.', '、']);
        let mut chars = rest.splitn(2, |c: char| !c.is_ascii_digit());
        let num = chars.next().unwrap_or("");
        let Ok(number) = num.parse::<u32>() else {
            continue;
        };
        let title = chars.next().unwrap_or("").trim().to_string();
        out.push(Step { number, title });
    }
    out
}

/// 读黑板上的 `impl-steps.md`。
pub fn read_steps(blackboard: &Path) -> Result<Vec<Step>> {
    let path = blackboard.join(IMPL_STEPS);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "缺少实施步骤 {}（先写步骤再动代码）：{e}",
            path.display()
        ))
    })?;
    let steps = parse_steps(&text);
    if steps.is_empty() {
        return Err(DevforgeError::Quality(format!(
            "{} 里没有任何 `## 步骤 N` 条目",
            path.display()
        )));
    }
    Ok(steps)
}

/// 相对基线的改动文件（`git diff --name-only <baseline>`，含已提交与未提交；
/// 并入 `git ls-files --others --exclude-standard` 的未跟踪文件——复验 #2：
/// 否则新建白名单外文件且不 git add，边界门/步提交对账/报告 diff 三处同时盲区）。
pub fn changed_files(repo: &Path, baseline: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .args(["-c", "core.quotePath=false", "diff", "--name-only", baseline])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git diff 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "git diff --name-only {baseline} 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let mut files: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    let untracked = Command::new("git")
        .args([
            "-c",
            "core.quotePath=false",
            "ls-files",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git ls-files 失败：{e}")))?;
    if !untracked.status.success() {
        return Err(DevforgeError::System(format!(
            "git ls-files --others 失败：{}",
            String::from_utf8_lossy(&untracked.stderr).trim()
        )));
    }
    for line in String::from_utf8_lossy(&untracked.stdout).lines() {
        let f = line.trim();
        if !f.is_empty() && !files.iter().any(|x| x == f) {
            files.push(f.to_string());
        }
    }
    // 流水线自身产物（计划文档/证据/文档域）不算代码改动——与 #10 纪律同口径：它们不进 coding
    // 提交、也不参与越界判定，否则每轮 run 自己写的 change-plan.md 会永久触发范围膨胀。
    // 2026-10-25 起计划已落台账根（仓外），这里保留过滤是为了**迁移前的旧仓**（计划还在
    // docs/plantree/changes/ 下）不会被误判为越界。
    // 引擎审计修复 #3：docs/contexts/（含 essential 档的 _meta 盘点/词表）同样是 SOP
    // 自产文档域——已有 git 仓基线只记 HEAD 不提交，这些 untracked 文件会让每次
    // doc_level≠none 的 change 必踩范围膨胀 FAIL。
    files.retain(|f| {
        !f.starts_with("docs/plantree/")
            && !f.starts_with("docs/contexts/")
            && !f.starts_with("evidence/")
    });
    Ok(files)
}

/// 基线之后的提交（`git log --format=%s`）。
pub fn commits_since(repo: &Path, baseline: &str) -> Result<Vec<String>> {
    let out = Command::new("git")
        .args(["log", "--format=%s", &format!("{baseline}..HEAD")])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git log 失败：{e}")))?;
    if !out.status.success() {
        // 自审 major：与 changed_files 的 git 失败分类对齐（System 可重启）——
        // 基线 SHA 被外部 rebase 吹掉是基础设施态，不是 agent 的质量失败。
        return Err(DevforgeError::System(format!(
            "git log {baseline}..HEAD 失败：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// 提交信息里声明的步骤号（`change: <change-id> step 2` / `步骤 2`）。
///
/// F15：数字必须**紧跟**标记（允许空白/冒号/顿号分隔）——正文里任意位置的数字
/// 不是步号（10-22 c7b 场：「…写进心跳的步骤字段…下一次 start 撞 409」
/// 被「步骤字段」误中 + 远处吞到 409，产生幻影步号 409）。
pub fn step_numbers_in(subject: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let lower = subject.to_ascii_lowercase();
    for marker in ["step", "步骤"] {
        let mut rest = lower.as_str();
        while let Some(pos) = rest.find(marker) {
            let after = &rest[pos + marker.len()..];
            let digits: String = after
                .chars()
                .skip_while(|c| matches!(c, ' ' | ':' | '：' | '.' | '、'))
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse::<u32>()
                && !out.contains(&n)
            {
                out.push(n);
            }
            rest = after;
        }
    }
    out
}

/// 校验「步骤 ↔ 提交」一一对应。
///
/// 规则：每个步骤号恰好在一个提交里被声明；不在 `impl-steps.md` 里的步骤号 = 多出来的提交；
/// 声明了但 `COMMIT_PREFIX` 缺失的提交也算不合格（无法机械归属）。
pub fn check_step_commits(
    repo: &Path,
    baseline: &str,
    change_id: &str,
    steps: &[Step],
) -> Result<usize> {
    let commits = commits_since(repo, baseline)?;
    let mut declared: Vec<u32> = Vec::new();
    for subject in &commits {
        if !subject.contains(change_id) || !subject.contains(COMMIT_PREFIX.trim_end_matches(':')) {
            return Err(DevforgeError::Quality(format!(
                "提交「{subject}」缺 change-id 或 `{COMMIT_PREFIX}` 前缀，无法机械归属"
            )));
        }
        let nums = step_numbers_in(subject);
        if nums.is_empty() {
            return Err(DevforgeError::Quality(format!(
                "提交「{subject}」没写步骤号（应含 `step N`）"
            )));
        }
        // 自审 major：一歩一提交是模块文档承诺的不变量——一个提交声明多个步号
        //（"step 1 步骤 2"）会绕过对账，必须硬门拒绝。
        if nums.len() > 1 {
            return Err(DevforgeError::Quality(format!(
                "提交「{subject}」声明了 {} 个步号（{nums:?}）——一歩一提交，一次只声明一个步骤",
                nums.len()
            )));
        }
        for n in nums {
            if declared.contains(&n) {
                return Err(DevforgeError::Quality(format!(
                    "步骤 {n} 被提交了不止一次（一歩一提交）"
                )));
            }
            declared.push(n);
        }
    }
    declared.sort_unstable();
    // 复验 #6：expected 同步排序——impl-steps 写成「步骤 2、步骤 1」时不应误报
    let mut expected: Vec<u32> = steps.iter().map(|s| s.number).collect();
    expected.sort_unstable();
    if declared != expected {
        return Err(DevforgeError::Quality(format!(
            "实施步骤与提交对不上：impl-steps 有 {expected:?}，提交声明 {declared:?}"
        )));
    }
    // 自审 major：返回**提交数**而非声明步号数——消费方把它当提交数记事件，
    // 声明数恒等于步数，实际提交数永远不可观测。
    Ok(commits.len())
}

/// 黑板与实现步骤的路径（编排层用）。
pub fn impl_steps_path(blackboard: &Path) -> PathBuf {
    blackboard.join(IMPL_STEPS)
}

#[cfg(test)]
#[path = "coding_gate_tests.rs"]
mod tests;
