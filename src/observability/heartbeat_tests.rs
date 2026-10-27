//! （2026-10-26 测试分离：自 src/observability/heartbeat.rs 内联平移，纯移动零改动。）
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

/// 自审 critical 回归：同一 run 的多个写者并发写心跳，不得丢更新/产出半截文件
/// （旧实现共用固定 tmp 名 → write/rename 互踩，rename ENOENT 或读者撞见空文件）。
#[test]
fn concurrent_writers_do_not_corrupt_or_lose_the_heartbeat() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::thread::scope(|scope| {
        for i in 0..8 {
            let root = root.clone();
            scope.spawn(move || {
                for n in 0..25 {
                    let phase = format!("p{i}-{n}");
                    write(&root, &snap(&phase, "step")).expect("并发写心跳不得失败");
                }
            });
        }
    });
    // 文件必须可解析（无半截 JSON），且临时文件不得残留
    let hb = read(&root, "run-1").expect("心跳文件必须可解析");
    assert!(
        hb.phase.starts_with('p'),
        "应保留最后一次完整写入，实际 {}",
        hb.phase
    );
    let leftovers: Vec<_> = std::fs::read_dir(root.join("heartbeat"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "不得残留临时文件：{leftovers:?}");
}

// ── 2026-10-27 自审 heartbeat 批回归 ──

/// 自审 minor（160）：文档化的并发原子性不变量（读者要么看旧整文件、要么看新整文件）
/// 此前零测试——旧用例全是单线程顺序写。这里多线程并发写 + 并发读，任何一次读到
/// 半截 JSON 都算违约。
#[test]
fn concurrent_writers_never_expose_a_partial_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    let readers: Vec<_> = (0..3)
        .map(|_| {
            let root = root.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut reads = 0usize;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    match super::read(&root, "run-concurrent") {
                        Ok(hb) => assert_eq!(hb.run_id, "run-concurrent"),
                        // 文件尚未创建是允许的（首个写者还没 rename 完成）
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => panic!("读到半截/坏文件（原子性违约）：{e}"),
                    }
                    reads += 1;
                }
                reads
            })
        })
        .collect();

    let writers: Vec<_> = (0..4)
        .map(|i| {
            let root = root.clone();
            std::thread::spawn(move || {
                for n in 0..25 {
                    let snap = RunSnapshot {
                        run_id: "run-concurrent".into(),
                        phase: format!("phase-{i}"),
                        step: format!("step-{n}"),
                        attempt: n,
                        started_at_ms: 0,
                    };
                    super::write(&root, &snap).expect("并发写心跳必须成功");
                }
            })
        })
        .collect();

    for w in writers {
        w.join().unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let total: usize = readers.into_iter().map(|r| r.join().unwrap()).sum();
    assert!(total > 0, "读线程应当真的读到过内容");

    // 收尾：不得残留临时文件
    let leftovers: Vec<String> = std::fs::read_dir(root.join("heartbeat"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "不得残留临时文件：{leftovers:?}");
    // 最终文件必须是完整可解析的
    let hb = super::read(&root, "run-concurrent").expect("终态心跳必须可解析");
    assert_eq!(hb.run_id, "run-concurrent");
}

/// 自审 minor（159）：run_id 形状非法时不得构造出穿越路径。
#[test]
fn bad_run_id_cannot_escape_the_heartbeat_dir() {
    let root = std::path::Path::new("/root");
    for bad in ["../evil", "a/b", "a\\b", "a b", ""] {
        let p = super::heartbeat_path(root, bad);
        assert!(
            p.starts_with("/root/heartbeat"),
            "非法 run_id 不得逃出心跳目录：{bad} → {}",
            p.display()
        );
    }
    assert_eq!(
        super::heartbeat_path(root, "run-ok"),
        std::path::PathBuf::from("/root/heartbeat/run-ok.json")
    );
}
