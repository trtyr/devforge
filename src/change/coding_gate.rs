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
        let t = line.trim();
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
            ("exit_code:", 10),
            ("exit code:", 10),
            ("exit:", 5),
            ("退出码:", 4),
            ("退出码：", 4),
        ];
        for (prefix, cut) in candidates {
            let l = if prefix.starts_with("exit") { lower.as_str() } else { t };
            if let Some(rest) = l.strip_prefix(prefix)
                && let Ok(n) = rest.trim().parse::<i32>()
            {
                exit_code = Some(n);
                let _ = cut;
                break;
            }
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
    let path = blackboard.join(BASELINE_LOG);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        DevforgeError::Quality(format!(
            "缺少基线证据 {}（动手前必须先跑一遍既有测试）：{e}",
            path.display()
        ))
    })?;
    let ev = parse_baseline_log(&text)?;
    if ev.exit_code != 0 {
        return Err(DevforgeError::Quality(format!(
            "基线不是绿的（exit_code={}，命令：{}）——不许在红基线上动代码",
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

/// 相对基线的改动文件（`git diff --name-only <baseline>`，含已提交与未提交、不含未跟踪）。
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
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
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
pub fn step_numbers_in(subject: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let lower = subject.to_ascii_lowercase();
    for marker in ["step", "步骤"] {
        let mut rest = lower.as_str();
        while let Some(pos) = rest.find(marker) {
            rest = &rest[pos + marker.len()..];
            let digits: String = rest
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse::<u32>()
                && !out.contains(&n)
            {
                out.push(n);
            }
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
    let expected: Vec<u32> = steps.iter().map(|s| s.number).collect();
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
}
