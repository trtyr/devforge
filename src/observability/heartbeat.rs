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
    //
    // 自审 critical：临时文件名**必须每个写者唯一**。旧实现固定 `{run}.json.tmp`——
    // 同一 run 的 watchdog 线程与工作线程同时写时，一方 rename 走文件，另一方
    // write 到一半被 rename → 丢更新；甚至 rename ENOENT。这里用 pid + 进程内
    // 单调序号，保证同目录并发写者互不踩踏。
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}.{seq}.tmp", std::process::id()));
    std::fs::write(&tmp, body)?;
    let renamed = std::fs::rename(&tmp, &path);
    if renamed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    renamed
}

/// 读心跳（取证/测试用）。
pub fn read(root: &Path, run_id: &str) -> std::io::Result<Heartbeat> {
    let text = std::fs::read_to_string(heartbeat_path(root, run_id))?;
    serde_json::from_str(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
#[path = "heartbeat_tests.rs"]
mod tests;
