//! 测试阶段的机械门（P009 T011 / D017-D019）。
//!
//! 口径：
//! - **回归优先**：test agent 先跑既有全套测试并把结果落 `evidence/regression.log`；
//!   日志退出码非 0 却声称「回归没问题」= 说谎，FAIL。
//! - **验收对账**：计划的每条验收标准都必须对到测试 `path:line`；缺一条即零覆盖（FAIL）。
//! - **回注带失败明细原文**：bug 打回 coding/plan 时把失败明细**逐条原文**（含 `path:line`）
//!   带给实现方——回注是 coding agent 的**输入面**，不给定位等于让它自己猜
//!   （D025 改写 D018/D019 的「不做定位」口径：两个路由目标同口径，交付面与日志面同源）。

use std::path::Path;

use crate::change::coding_gate::parse_evidence_log;
use crate::change::state::TestFailureContext;
use crate::error::{DevforgeError, Result};

/// 回归证据（相对黑板）。
pub const REGRESSION_LOG: &str = "evidence/regression.log";

/// 一条验收标准到测试的映射。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceMatch {
    pub criterion: String,
    pub test_ref: String,
}

/// 回归结果判定：日志退出码与 agent 的自述必须一致。
///
/// 自审 minor（87）：`claims_green` 在生产调用点**恒为 true**（测试阶段走到这里 =
/// agent 声称通过），`false` 只在单测里用来验「红着但没声称」的口径；返回的退出码在
/// 生产侧也只用于日志字段。别把它当成可变策略旋钮。
pub fn check_regression_claim(log_text: &str, claims_green: bool) -> Result<i32> {
    // 自审 minor（88）：走带标签的解析入口——旧口径复用基线门的单参解析，
    // 内层错误文本里出现 evidence/baseline-test.log，运维会找错文件。
    let ev = parse_evidence_log(log_text, REGRESSION_LOG, "回归").map_err(|e| {
        DevforgeError::Quality(format!(
            "回归日志 {REGRESSION_LOG} 不可解析（要写 `exit_code: N`）：{e}"
        ))
    })?;
    if ev.exit_code != 0 && claims_green {
        return Err(DevforgeError::Quality(format!(
            "回归日志退出码 {} 与「回归通过」的自述矛盾（命令：{}）",
            ev.exit_code,
            if ev.command.is_empty() { "未记录" } else { &ev.command }
        )));
    }
    Ok(ev.exit_code)
}

/// 读黑板上的回归日志并判定。
/// 引擎审计修复 #4：not_before 校验 mtime 不早于本轮开始（与 baseline/self-check 的
/// freshness 同口径）——上一轮残留的绿 log 不得给本轮提供通行证。
pub fn check_regression_log(
    blackboard: &Path,
    claims_green: bool,
    not_before: std::time::SystemTime,
) -> Result<i32> {
    let path = blackboard.join(REGRESSION_LOG);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "缺少回归证据 {}（测试阶段必须先跑既有全套）：{e}",
            path.display()
        ))
    })?;
    let mtime = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map_err(|e| {
            DevforgeError::Quality(format!(
                "回归证据 {} 的修改时间不可读：{e}",
                path.display()
            ))
        })?;
    if mtime < not_before {
        return Err(DevforgeError::Quality(format!(
            "回归证据 {} 是上一轮残留（mtime 早于本轮开始）——本轮必须重跑既有全套",
            path.display()
        )));
    }
    check_regression_claim(&text, claims_green)
}

/// 测试阶段入口快照：基线以来 tracked 文件的改动集 + 未跟踪的新产品文件。
///
/// 自审 major：旧口径只看 `git diff baseline`——测试 agent 新建且未 git add 的
/// 产品源文件完全逃过「不改产品代码」门（与 coding 侧复验 #2 同款盲区）。
/// 现在并入 untracked 扫描：带源码扩展名、不在 tests/ docs/ evidence/ 豁免前缀、
/// 不在常见构建产物目录里的新文件也算改动集（源码扩展名单与 inventory 同源）。
pub fn snapshot_tracked_changes(
    repo: &Path,
    baseline: &str,
) -> Result<std::collections::BTreeSet<String>> {
    // 自审 major：baseline 先过形状门（hex）再进 git 参数——防参数注入/垃圾锚点。
    let well_formed = baseline.len() >= 7
        && baseline.len() <= 64
        && baseline.chars().all(|c| c.is_ascii_hexdigit());
    if !well_formed {
        // 自审 minor（90）：形状非法属于**确定性配置错误**（重启不会变好），
        // 旧口径归 System（可重试）会白耗重试预算、并把终态错标成基础设施故障。
        return Err(DevforgeError::Config(format!(
            "基线 sha 形状非法（{baseline:?}）——拒绝当作 git 参数"
        )));
    }
    let mut set = std::collections::BTreeSet::new();
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "core.quotePath=false",
            "diff",
            "--name-only",
            baseline,
            "--",
        ])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git diff 失败：{e}")))?;
    if !out.status.success() {
        // fail-closed：快照失败 = 本阶段机械门未生效，必须 Err 让本轮测试作废，
        // 绝不把「查不了」当「查过、干净」。
        //（自审 minor 91：旧注释与日志文案仍写着「待改为 fail-closed」，与已落地的
        // fail-closed 实现矛盾，会误导排障。）
        return Err(DevforgeError::System(format!(
            "测试阶段产品代码快照失败（git 基线 {baseline}）：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let t = line.trim();
        if !t.is_empty() {
            set.insert(t.to_string());
        }
    }
    // untracked 产品源文件（未 add 的新文件不进 diff）
    let out = std::process::Command::new("git")
        .args(["-c", "core.quotePath=false", "status", "--porcelain", "-uall"])
        .current_dir(repo)
        .output()
        .map_err(|e| DevforgeError::System(format!("执行 git status 失败：{e}")))?;
    if !out.status.success() {
        return Err(DevforgeError::System(format!(
            "测试阶段产品代码快照失败（git status）：{}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Some(rest) = line.strip_prefix("?? ") else {
            continue;
        };
        let path = rest.trim();
        if is_exempt_or_junk(path) {
            continue;
        }
        set.insert(path.to_string());
    }
    Ok(set)
}

/// 豁免前缀（tests/ docs/ evidence/）+ 常见构建/缓存产物目录。
fn is_exempt_or_junk(path: &str) -> bool {
    const EXEMPT: [&str; 3] = ["tests/", "docs/", "evidence/"];
    const JUNK_DIRS: [&str; 6] = [
        "node_modules/",
        "target/",
        "dist/",
        "__pycache__/",
        ".pytest_cache/",
        ".venv/",
    ];
    if EXEMPT.iter().any(|p| path.starts_with(p)) {
        return true;
    }
    if JUNK_DIRS
        .iter()
        .any(|p| path.contains(&format!("{p}")))
    {
        return true;
    }
    // 带源码扩展名 = 产品代码 → 不豁免（进快照）；无源码扩展名（report.html、
    // .log 之类的运行产物）→ 当噪音跳过。
    // 注意：SOURCE_EXTENSIONS 存裸扩展名（无点），必须补点比对。
    !crate::docgen::inventory::SOURCE_EXTENSIONS
        .iter()
        .any(|(e, _)| path.ends_with(&format!(".{e}")))
}

/// 测试阶段出口校验（引擎审计修复 #6）：测试期间新增的产品代码改动 = Quality。
/// 「不改产品代码」不再只是 prompt 约束——tests/ 与流水线产物（docs/evidence）之外
/// 的新增 tracked 改动一律拒收。
pub fn check_no_product_code_edits(
    before: &std::collections::BTreeSet<String>,
    after: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let new_edits: Vec<&String> = after
        .difference(before)
        .filter(|f| {
            !f.starts_with("tests/")
                && !f.starts_with("docs/")
                && !f.starts_with("evidence/")
        })
        .collect();
    if !new_edits.is_empty() {
        return Err(DevforgeError::Quality(format!(
            "测试阶段修改了产品代码（测试 Agent 只准写测试）：{}——发现 bug 回注，不自己修",
            new_edits
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("、")
        )));
    }
    // 自审 major：还原检测——入口快照里有、出口没了的产品路径 = 测试阶段把
    // coding 的成果 checkout 回基线/删掉。旧口径只看「新增项」，回退完全隐形。
    let vanished: Vec<&String> = before
        .difference(after)
        .filter(|f| {
            !f.starts_with("tests/")
                && !f.starts_with("docs/")
                && !f.starts_with("evidence/")
        })
        .collect();
    if !vanished.is_empty() {
        return Err(DevforgeError::Quality(format!(
            "测试阶段删除/还原了产品代码（coding 的改动不得被测试阶段抹掉）：{}——发现 bug 回注，不自己修",
            vanished
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("、")
        )));
    }
    Ok(())
}

/// 解析验收对账表：`- <验收标准> → <测试引用 path:line>`（按**最后一个** `→` 切，标准本身可能含箭头）。
pub fn parse_acceptance_table(text: &str) -> Vec<AcceptanceMatch> {
    let mut out = Vec::new();
    for line in text.lines() {
        let raw = line.trim_start();
        // 自审 minor（92）：旧口径把**任何**含 → 的行都当验收项——说明文字/散文里的
        // 箭头会被解析成验收标准并触发不可恢复的硬失败。只认列表项。
        if !(raw.starts_with("- ") || raw.starts_with("* ") || raw.starts_with("+ ")) {
            continue;
        }
        let t = raw.trim_start_matches(['-', '*', '+']).trim();
        let Some((left, right)) = t.rsplit_once('→') else {
            continue;
        };
        let criterion = left.trim().to_string();
        let test_ref = right.trim().trim_matches('`').to_string();
        if !criterion.is_empty() && !test_ref.is_empty() {
            out.push(AcceptanceMatch {
                criterion,
                test_ref,
            });
        }
    }
    out
}

/// 生成回注给 coding/plan 的消息：失败明细**逐条原文**（含 `path:line`）。
///
/// `target` 只影响措辞（`coding` / `plan`），两个目标同口径：明细同源
/// （`failures` 是 test agent 给的原文，不剥定位、不改写、不截断）。
pub fn route_back_message(target: &str, failures: &[String]) -> String {
    format!(
        "测试阶段未通过，按 <route>{target}</route> 回去处理。\n失败明细：\n{}\n",
        failure_details(failures)
    )
}

/// 失败明细渲染：issues 逐条原文，一行一条。
///
/// 回注消息与 coding 送达块共用这一份渲染 → 同一失败在**四个**出口同源（不漂移）：
/// route_back_message（coding/plan 两个路由目标）+ coding_failure_block（送达块）+
/// 编排层的 fix.reinject 事件字段（自审 minor 23：旧注释写「三个出口」）。
pub fn failure_details(failures: &[String]) -> String {
    failures
        .iter()
        .map(|f| format!("- {f}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// coding 送达块的标题行（逐字可查：验收按这句话对账）。
pub const FAILURE_BLOCK_TITLE: &str = "上一轮测试失败明细如下（逐条原文）";

/// coding 送达块：上一轮测试失败的明细（逐条原文）。
///
/// 这是回注内容在 **coding agent 输入面**上的形态；明细与日志面/失败原因文本同源。
pub fn coding_failure_block(failure: &TestFailureContext) -> String {
    format!(
        "{}：\n{}",
        FAILURE_BLOCK_TITLE,
        failure_details(&failure.issues)
    )
}

#[cfg(test)]
#[path = "testing_gate_tests.rs"]
mod tests;
