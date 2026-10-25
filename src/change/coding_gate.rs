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
        .args(["diff", "--name-only", baseline])
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
        .args(["ls-files", "--others", "--exclude-standard"])
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
    // 流水线自身产物（计划文档/证据）不算代码改动——与 #10 纪律同口径：它们不进 coding
    // 提交、也不参与越界判定，否则每轮 run 自己写的 change-plan.md 会永久触发范围膨胀。
    files.retain(|f| !f.starts_with("docs/plantree/") && !f.starts_with("evidence/"));
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
        return Err(DevforgeError::Quality(format!(
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
    Ok(declared.len())
}

/// 黑板与实现步骤的路径（编排层用）。
pub fn impl_steps_path(blackboard: &Path) -> PathBuf {
    blackboard.join(IMPL_STEPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git 可执行");
        assert!(
            out.status.success(),
            "git {args:?} 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit(repo: &Path, msg: &str) {
        git(repo, &["add", "-A"]);
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@localhost",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                msg,
            ])
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    fn repo_with_baseline() -> (tempfile::TempDir, String) {
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "-q"]);
        std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();
        commit(tmp.path(), "devforge: baseline");
        let out = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (tmp, sha)
    }

    /// 复验 #2 回归：白名单外的未跟踪文件必须进入 changed_files（否则门禁三处盲区）。
    #[test]
    fn untracked_files_are_included_in_changed_files() {
        let (tmp, baseline) = repo_with_baseline();
        std::fs::write(tmp.path().join("outside.txt"), "secret\n").unwrap();
        let files = changed_files(tmp.path(), &baseline).unwrap();
        assert!(
            files.iter().any(|f| f == "outside.txt"),
            "未跟踪文件必须出现在变更集：{files:?}"
        );
    }

    /// 复验 #1 回归：非法基线（git diff 失败）必须 Err，不得静默空数组。
    #[test]
    fn invalid_baseline_is_error_not_empty() {
        let tmp = tempfile::tempdir().unwrap();
        git(tmp.path(), &["init", "-q"]);
        std::fs::write(tmp.path().join("a.txt"), "a\n").unwrap();
        let err = changed_files(tmp.path(), "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef")
            .unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn green_baseline_log_passes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(
            tmp.path().join(BASELINE_LOG),
            "command: cargo test\nexit_code: 0\n",
        )
        .unwrap();
        let ev = check_baseline_log(tmp.path()).unwrap();
        assert_eq!(ev.exit_code, 0);
        assert_eq!(ev.command, "cargo test");
    }

    #[test]
    fn red_baseline_blocks_coding() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(
            tmp.path().join(BASELINE_LOG),
            "command: pytest -q\nexit_code: 2\n",
        )
        .unwrap();
        let err = check_baseline_log(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("基线不是绿的"), "{msg}");
        assert!(msg.contains("pytest -q"), "错误里要带自己跑的命令：{msg}");
    }

    #[test]
    fn missing_log_is_quality_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let err = check_baseline_log(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("缺少基线证据"), "{err}");
        assert!(crate::error::DevforgeError::Quality(String::new()).is_quality_failure());
        assert!(err.is_quality_failure());
    }

    #[test]
    fn log_without_exit_code_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(tmp.path().join(BASELINE_LOG), "跑了一遍，看起来还行\n").unwrap();
        let err = check_baseline_log(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("缺退出码"), "{err}");
    }

    #[test]
    fn baseline_log_error_text_still_points_at_the_baseline_file() {
        // 回归路径的第二个调用方（testing_gate）依赖这套文案，本门不得改它
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(tmp.path().join(BASELINE_LOG), "command: cargo test\n").unwrap();
        let err = check_baseline_log(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("基线证据缺退出码"), "{msg}");
        assert!(msg.contains(BASELINE_LOG), "{msg}");
    }

    #[test]
    fn missing_self_check_log_is_quality_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let err = check_self_check(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(SELF_CHECK_LOG), "错误要指向自证证据文件：{msg}");
        assert!(
            !msg.contains(BASELINE_LOG),
            "自证门的错误不得指向基线文件：{msg}"
        );
    }

    #[test]
    fn self_check_without_exit_code_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(
            tmp.path().join(SELF_CHECK_LOG),
            "command: cargo test\n跑完了，看着没问题\n",
        )
        .unwrap();
        let err = check_self_check(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("退出码"), "{msg}");
        assert!(msg.contains(SELF_CHECK_LOG), "{msg}");
        assert!(
            !msg.contains(BASELINE_LOG),
            "自证门的错误不得指向基线文件：{msg}"
        );
    }

    #[test]
    fn red_self_check_is_rejected_with_exit_code() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(
            tmp.path().join(SELF_CHECK_LOG),
            "command: cargo test\nexit_code: 1\n",
        )
        .unwrap();
        let err = check_self_check(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("exit_code=1"), "{msg}");
        assert!(
            !msg.contains(BASELINE_LOG),
            "自证门的错误不得指向基线文件：{msg}"
        );
    }

    #[test]
    fn green_self_check_passes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(
            tmp.path().join(SELF_CHECK_LOG),
            "command: cargo test\nexit_code: 0\n455 passed\n",
        )
        .unwrap();
        let ev = check_self_check(tmp.path()).unwrap();
        assert_eq!(ev.exit_code, 0);
        assert_eq!(ev.command, "cargo test");
    }

    #[test]
    fn parse_baseline_log_keeps_its_single_argument_signature() {
        // 第二调用方（testing_gate）按这个签名调：签名一变就得连带改它（不在白名单内）
        let _f: fn(&str) -> Result<BaselineEvidence> = parse_baseline_log;
    }

    #[test]
    fn parse_accepts_markdown_list_item_evidence() {
        // F16：agent 用 markdown 列表项写证据（`- exit_code: 0`）语义与裸格式相同，
        // 列表符号不参与语义（10-22 c7b 场：列表项被判「缺退出码」→ 整场 Quality）
        let ev = parse_baseline_log("- command: `env -u X cargo test`\n- exit_code: 0\n").unwrap();
        assert_eq!(ev.exit_code, 0);
        assert_eq!(ev.command, "`env -u X cargo test`");
    }

    #[test]
    fn parse_still_rejects_list_item_without_exit_code() {
        // 容差只认列表符号，不放松语义：真缺退出码依旧拒
        let err = parse_baseline_log("- command: cargo test\n- 摘要：567 passed\n").unwrap_err();
        let crate::error::DevforgeError::Quality(msg) = &err else {
            panic!("应是质量失败：{err}");
        };
        assert!(msg.contains("缺退出码"), "{msg}");
    }

    #[test]
    fn parse_accepts_bold_list_item_exit_code() {
        // F16b：`- exit_code: **0**`（列表项+粗体）与 `exit_code: 0` 同义
        // （10-23 c7b 场：测试 agent 以列表项+粗体写回归日志，粗体未剥被判「缺退出码」）
        let ev = parse_baseline_log(
            "- command: `env -u X cargo test --offline`\n- exit_code: **0**\n- 摘要：578 passed\n",
        )
        .unwrap();
        assert_eq!(ev.exit_code, 0);
    }

    #[test]
    fn parse_bold_exit_code_with_prose_tail_takes_leading_int() {
        // 粗体后带散文：取前导整数
        let ev = parse_baseline_log("- exit_code: **0**（44 passed / 0 failed）\n").unwrap();
        assert_eq!(ev.exit_code, 0);
    }

    #[test]
    fn parse_first_exit_code_wins_over_later_reverse_validation() {
        // 首中即停：主回归在最前；后面的 exit_code 行是反向验证「能失败」的
        // 故意 101，不得覆盖主结果（否则永不绿）
        let ev = parse_baseline_log(
            "command: cargo test\nexit_code: 0\n\n## 反向验证\n- exit_code: **101**（故意破坏后必红）\n",
        )
        .unwrap();
        assert_eq!(ev.exit_code, 0);
    }

    #[test]
    fn regression_claim_text_is_unchanged() {
        // 回归路径的文案不得被本次改造带偏（质量失败载荷仍以「回归日志不可解析」开头）
        let err = crate::change::testing_gate::check_regression_claim("command: cargo test\n", true)
            .unwrap_err();
        let crate::error::DevforgeError::Quality(msg) = &err else {
            panic!("应是质量失败：{err}");
        };
        assert!(msg.starts_with("回归日志不可解析"), "{msg}");
    }

    #[test]
    fn decision_ledger_registers_d023() {
        // 决策台账同步（D023）：纪律供给口径 + change 路径的先证后报机械门 + 门只落 change 路径。
        // 放在本文件：白名单内不得新增测试文件，而本变更的门与该决策同源。
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("docs/plantree/plans/009-change-sop/decisions/README.md");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("决策台账不可读：{}", path.display()));
        assert!(text.contains("D023"), "台账未登记 D023");
        for anchor in ["coding-discipline", "evidence/self-check.log", "只落 change 路径"] {
            assert!(text.contains(anchor), "D023 缺登记点：{anchor}");
        }
    }

    #[test]
    fn self_check_and_baseline_logs_are_independent_files() {
        // 两道门各读各的文件：基线绿不代表自证绿，自证绿也不代表基线绿
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("evidence")).unwrap();
        std::fs::write(
            tmp.path().join(BASELINE_LOG),
            "command: cargo test\nexit_code: 0\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(SELF_CHECK_LOG),
            "command: cargo test\nexit_code: 1\n",
        )
        .unwrap();
        assert!(check_baseline_log(tmp.path()).is_ok());
        assert!(check_self_check(tmp.path()).is_err());
    }

    #[test]
    fn steps_are_parsed_from_headings() {
        let md = "# 实施步骤\n## 步骤 1 改接口\n## 步骤 2：补测试\n### Step 3 收尾\n";
        let steps = parse_steps(md);
        assert_eq!(
            steps.iter().map(|s| s.number).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(steps[1].title, "补测试");
    }

    #[test]
    fn step_count_matches_commits() {
        let (tmp, sha) = repo_with_baseline();
        std::fs::write(tmp.path().join("a.txt"), "a2\n").unwrap();
        commit(tmp.path(), "change: change-20261020-x step 1");
        std::fs::write(tmp.path().join("b.txt"), "b\n").unwrap();
        commit(tmp.path(), "change: change-20261020-x step 2");

        let steps = vec![
            Step { number: 1, title: "one".into() },
            Step { number: 2, title: "two".into() },
        ];
        let n = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn missing_step_commit_fails() {
        let (tmp, sha) = repo_with_baseline();
        std::fs::write(tmp.path().join("a.txt"), "a3\n").unwrap();
        commit(tmp.path(), "change: change-20261020-x step 1");

        let steps = vec![
            Step { number: 1, title: "one".into() },
            Step { number: 2, title: "two".into() },
        ];
        let err = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap_err();
        assert!(err.to_string().contains("对不上"), "{err}");
    }

    #[test]
    fn duplicate_step_or_missing_prefix_fails() {
        let (tmp, sha) = repo_with_baseline();
        commit(tmp.path(), "change: change-20261020-x step 1");
        commit(tmp.path(), "随手改一下");
        let steps = vec![Step { number: 1, title: "one".into() }];
        let err = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap_err();
        assert!(err.to_string().contains("无法机械归属"), "{err}");
    }

    #[test]
    fn extra_commit_without_step_number_fails() {
        let (tmp, sha) = repo_with_baseline();
        commit(tmp.path(), "change: change-20261020-x step 1");
        commit(tmp.path(), "change: change-20261020-x 顺手改");
        let steps = vec![Step { number: 1, title: "one".into() }];
        let err = check_step_commits(tmp.path(), &sha, "change-20261020-x", &steps).unwrap_err();
        assert!(err.to_string().contains("没写步骤号"), "{err}");
    }

    #[test]
    fn empty_impl_steps_is_quality_failure() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(IMPL_STEPS), "# 实施步骤\n随便写写\n").unwrap();
        let err = read_steps(tmp.path()).unwrap_err();
        assert!(err.to_string().contains("没有任何"), "{err}");
        assert!(err.is_quality_failure());
    }

    #[test]
    fn step_numbers_in_subject_handles_chinese_and_repeats() {
        assert_eq!(step_numbers_in("change: c1 step 3"), vec![3]);
        assert_eq!(step_numbers_in("change: c1 步骤 2"), vec![2]);
        assert_eq!(step_numbers_in("change: c1 step 2 step 2"), vec![2]);
    }

    #[test]
    fn step_numbers_ignore_digits_in_prose_after_a_marker() {
        // F15：正文数字不是步号——标记后必须紧跟数字（允许空白/冒号分隔），
        // 否则跳过该次出现，继续找下一个标记（不远处吞到别的数字）
        assert_eq!(
            step_numbers_in(
                "change: c1 step 8 —— 把「正在做什么」写进心跳的步骤字段…下一次 start 撞 409"
            ),
            vec![8]
        );
        assert_eq!(step_numbers_in("change: c1 步骤字段里提到 409"), Vec::<u32>::new());
    }

    #[test]
    fn eleven_steps_with_prose_digits_reconcile_cleanly() {
        // 模拟 c7b 场：11 步提交、其中一步标题正文含「409」——对账应恰好 1..=11
        let subjects: Vec<String> = (1..=11)
            .map(|n| {
                let extra = if n == 8 { "…下一次 start 撞 409" } else { "" };
                format!("change: c1 step {n} —— 第 {n} 步{extra}")
            })
            .collect();
        let mut declared: Vec<u32> = subjects
            .iter()
            .flat_map(|s| step_numbers_in(s))
            .collect();
        declared.sort_unstable();
        assert_eq!(declared, (1..=11).collect::<Vec<_>>());
    }
}
