//! 心跳文件（run 活性证据）：`root/heartbeat/<runid>.json`。
//!
//! 口径（Q8/§5）：
//! - **覆盖写**（幂等收敛）：状态文件只留「最后一次状态」，不追加成第二份真相；
//! - **无活动 run 时不写**；run 正常结束**保留**最后一次状态（不删——那是死亡现场的证据）；
//! - 内容含**阶段 / 步骤 / 时间戳**三要素（外加 run id / attempt / pid）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::RunSnapshot;

/// 心跳文件的内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub run_id: String,
    /// 阶段（change = plan/coding/testing/done；其他 SOP = sop 名）。
    pub phase: String,
    /// 步骤（阶段内更细的一格）。
    pub step: String,
    /// 当前尝试次数。
    pub attempt: u32,
    /// 最后一次写入的时刻（毫秒）。
    pub timestamp_ms: u64,
    /// 写盘进程 pid（异常死亡后据此判断「谁写的」）。
    pub pid: u32,
}

/// 心跳文件路径：`root/heartbeat/<runid>.json`。
pub fn heartbeat_path(root: &Path, run_id: &str) -> PathBuf {
    root.join("heartbeat").join(format!("{run_id}.json"))
}

/// 写心跳（**覆盖写**）。
pub fn write(root: &Path, snap: &RunSnapshot) -> std::io::Result<()> {
    let path = heartbeat_path(root, &snap.run_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let hb = Heartbeat {
        run_id: snap.run_id.clone(),
        phase: snap.phase.clone(),
        step: snap.step.clone(),
        attempt: snap.attempt,
        timestamp_ms: crate::domain::now_ms(),
        pid: std::process::id(),
    };
    let body = serde_json::to_vec_pretty(&hb)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    // F18b：原子写（临时文件 + rename）——覆盖写在多写者并发（watchdog tick /
    // 阶段转换 / run 收尾终态刷）下会被读者撞见半截/空文件，resume6g 测试阶段
    // 实测 serde「EOF while parsing a value」。rename 同目录内原子替换，读者要么
    // 看旧整文件、要么看新整文件。
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

/// 读心跳（取证/测试用）。
pub fn read(root: &Path, run_id: &str) -> std::io::Result<Heartbeat> {
    let text = std::fs::read_to_string(heartbeat_path(root, run_id))?;
    serde_json::from_str(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(phase: &str, step: &str) -> RunSnapshot {
        RunSnapshot {
            run_id: "run-1".into(),
            phase: phase.into(),
            step: step.into(),
            attempt: 1,
            started_at_ms: crate::domain::now_ms(),
        }
    }

    #[test]
    fn heartbeat_file_carries_phase_step_and_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &snap("plan", "review:change-plan-review")).unwrap();
        let hb = read(dir.path(), "run-1").unwrap();
        assert_eq!(hb.phase, "plan");
        assert_eq!(hb.step, "review:change-plan-review");
        assert!(hb.timestamp_ms > 0, "心跳必须带时间戳");
        assert_eq!(hb.run_id, "run-1");
        let path = heartbeat_path(dir.path(), "run-1");
        assert!(path.exists(), "心跳文件应落在 root/heartbeat/ 下");
        assert!(
            path.to_string_lossy().contains("heartbeat"),
            "心跳落点目录名要对：{path:?}"
        );
    }

    #[test]
    fn heartbeat_is_overwritten_not_appended() {
        // 错实现形态：心跳用追加写 → 文件里出现两份真相（解析失败 / 读到旧状态）
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &snap("plan", "step-1")).unwrap();
        write(dir.path(), &snap("coding", "step-2")).unwrap();
        let hb = read(dir.path(), "run-1").unwrap();
        assert_eq!(hb.phase, "coding", "后写的状态必须覆盖先写的");
        assert_eq!(hb.step, "step-2");
    }

    #[test]
    fn writing_a_heartbeat_creates_the_heartbeat_dir() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("nested").join("devforge");
        write(&root, &snap("plan", "s")).unwrap();
        assert!(root.join("heartbeat").join("run-1.json").exists());
    }
}
