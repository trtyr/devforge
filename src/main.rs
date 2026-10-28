//! devforge 入口（应用层）：起 API 服务器（P003 服务器形态）。

use anyhow::Context as _;
use std::sync::Arc;

/// 解析运行根（纯函数；`home` 惰性求值——DEVFORGE_HOME 已设时完全不读 HOME）。
///
/// 口径：`DEVFORGE_HOME` 已设（trim 后非空）→ 它就是运行根；否则才读 `HOME` 回落
/// `$HOME/.devforge`；两者皆未设（或只有空白）→ fail fast，错误文案同时点名
/// `DEVFORGE_HOME` 与 `HOME` 给出双出路。
/// 与 `observability::resolve_root` 同源（DEVFORGE_HOME 优先、空串视作未设），
/// 保证运行根与观测根（心跳/墓碑落点）同根。
fn resolve_runtime_root(
    devforge_home: Option<&str>,
    home: impl FnOnce() -> Option<String>,
) -> anyhow::Result<std::path::PathBuf> {
    if let Some(d) = devforge_home.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(std::path::PathBuf::from(d));
    }
    let home = home();
    if let Some(h) = home.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(std::path::PathBuf::from(h).join(".devforge"));
    }
    anyhow::bail!(
        "无法确定运行根：DEVFORGE_HOME 与 HOME 均未设置（或只有空白）——\
         请设置 DEVFORGE_HOME=<运行根路径>，或设置 HOME 后使用 $HOME/.devforge"
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // doc-lint：引擎侧确定性门禁（D008/D012）。退出码可判：0 = 通过。
    // 形态：`devforge doc-lint [docs_dir]`，默认检查 `docs/contexts`。
    if args.first().map(String::as_str) == Some("doc-lint") {
        let dir = args
            .get(1)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("docs/contexts"));
        // --repo <root>：启用实体存在性/同名异写检查（以源码语料为真相源）
        let repo = args
            .iter()
            .position(|a| a == "--repo")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from);
        let report = match repo {
            Some(repo) => {
                let index = devforge::docgen::lint::EntityIndex::build(&repo, None)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?;
                devforge::docgen::lint::lint_docs_with_entities(&dir, &index)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
            }
            None => devforge::docgen::lint::lint_docs(&dir)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?,
        };
        for v in report.violations() {
            println!("{}", v.render());
        }
        println!("{}", report.summary());
        std::process::exit(if report.passed() { 0 } else { 1 });
    }

    if args.first().map(String::as_str) == Some("doc-audit") {
        // 五项审计里的机械部分可复算入口（覆盖 + 跨模块重复）。
        // 形态：`devforge doc-audit <docs_dir> --repo <root> [--level complete]`，退出码 1 = 覆盖不完整。
        let Some(docs) = args.get(1) else {
            eprintln!("用法: devforge doc-audit <docs_dir> --repo <root> [--level essential|complete|detailed]");
            std::process::exit(2);
        };
        let docs = std::path::PathBuf::from(docs);
        let repo = args
            .iter()
            .position(|a| a == "--repo")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let level = args
            .iter()
            .position(|a| a == "--level")
            .and_then(|i| args.get(i + 1))
            .map(|s| devforge::docgen::audit::DocLevel::parse(s))
            .unwrap_or(devforge::docgen::audit::DocLevel::Complete);
        let inv_path = docs.join("_meta/inventory.json");
        let inv = devforge::docgen::inventory::Inventory::load(&inv_path)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let cov = devforge::docgen::audit::coverage(&inv, &docs, level)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        print!("{}", cov.render());
        let dups = devforge::docgen::audit::duplicate_mentions(&docs, &inv)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let dup_content = devforge::docgen::audit::duplicated_content(&docs)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if dups.is_empty() {
            println!("同名出现（提示级）：无");
        } else {
            println!("同名出现（提示级，不单独判 FAIL）：");
            for d in &dups {
                println!("- {d}");
            }
        }
        if dup_content.is_empty() {
            println!("跨模块重复正文：无");
        } else {
            println!("跨模块重复正文（同一段解释被两篇以上具体描述）：");
            for d in &dup_content {
                println!("- {d}");
            }
        }
        let index = devforge::docgen::lint::EntityIndex::build(&repo, Some(&inv))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let lint = devforge::docgen::lint::lint_docs_with_entities(&docs, &index)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        for v in lint.violations() {
            println!("{}", v.render());
        }
        println!("{}", lint.summary());
        std::process::exit(if cov.passed() && lint.passed() { 0 } else { 1 });
    }

    if args.first().map(String::as_str) == Some("doc-inventory") {
        // 清单（分母）机械抽取 + 三道闭包校验（D002/D013）。
        // 形态：`devforge doc-inventory <repo> [--out <dir>]`，退出码 1 = 闭包不过。
        let Some(repo) = args.get(1) else {
            eprintln!("用法: devforge doc-inventory <repo> [--out <dir>]");
            std::process::exit(2);
        };
        let repo = std::path::PathBuf::from(repo);
        let out_dir = args
            .iter()
            .position(|a| a == "--out")
            .and_then(|i| args.get(i + 1))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| repo.join("docs/contexts/_meta"));
        let inv = devforge::docgen::inventory::build_inventory(&repo)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let path = out_dir.join("inventory.json");
        inv.save(&path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let findings = devforge::docgen::inventory::check_closures(&repo, &inv)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let modules = inv.entries.iter().filter(|e| matches!(e.kind, devforge::docgen::inventory::EntryKind::Module)).count();
        let symbols = inv.entries.iter().filter(|e| matches!(e.kind, devforge::docgen::inventory::EntryKind::Symbol)).count();
        let entries = inv.entries.iter().filter(|e| matches!(e.kind, devforge::docgen::inventory::EntryKind::Entry)).count();
        let configs = inv.entries.iter().filter(|e| matches!(e.kind, devforge::docgen::inventory::EntryKind::Config)).count();
        let data = inv.entries.iter().filter(|e| matches!(e.kind, devforge::docgen::inventory::EntryKind::Data)).count();
        let behaviors = inv.entries.iter().filter(|e| matches!(e.kind, devforge::docgen::inventory::EntryKind::Behavior)).count();
        println!(
            "清单：{} 条目（module {modules} / symbol {symbols} / entry {entries} / config {configs} / data {data} / behavior {behaviors}），单元水位 {}",
            inv.entries.len(),
            inv.meta.unit_seq
        );
        println!("写入：{}", path.display());
        if !inv.meta.unsupported_languages.is_empty() {
            println!("未支持符号抽取的语言（缺口登记）：{}", inv.meta.unsupported_languages.join(", "));
        }
        for f in &findings {
            println!("{}:{} [{}] {}", f.path, f.line, f.closure.as_str(), f.message);
        }
        if findings.is_empty() {
            println!("闭包校验：三道全过");
            std::process::exit(0);
        }
        println!("闭包校验：{} 项不通过", findings.len());
        std::process::exit(1);
    }

    // 运行根推导口径：DEVFORGE_HOME 优先 → HOME/.devforge → 双未设 fail fast；
    // 与 default_plantree_root 同源（台账根与运行根必须同根，否则台账与运行状态分家）。
    // 自审 major：旧口径先无条件读 HOME 且 `?` 直接退进程——即便显式设了 DEVFORGE_HOME
    // 也强依赖 HOME（无 HOME 的最小环境里起不来），与本段宣称的优先级正好相反。
    // 现在 DEVFORGE_HOME 已设时完全不碰 HOME，HOME 只在兜底分支被读。
    let root = resolve_runtime_root(
        std::env::var("DEVFORGE_HOME").ok().as_deref(),
        || std::env::var("HOME").ok(),
    )?;

    devforge::logging::init(&root.join("logs"), "devforge")
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    // 观测根（心跳/墓碑的落点）：与运行根同源（DEVFORGE_HOME 优先 → HOME/.devforge，
    // 双未设时回落 CWD/.devforge，Q4）——心跳/墓碑与清单/台账落在同一目录树
    let obs_root = devforge::observability::root();
    devforge::observability::set_root(obs_root.clone());
    // 死亡取证：进程启动即装 panic hook（panic 原文 + 位置 + 当时阶段写进墓碑文件）
    devforge::observability::tombstone::install_hook(obs_root);
    tracing::info!(component = "main", "devforge 启动");

    // 配置清单：真源落盘 ~/.devforge/manifest.json（无则用内置默认生成）
    let manifest_path = root.join("manifest.json");
    let manifest = if manifest_path.exists() {
        devforge::config::ConfigManifest::load(&manifest_path).map_err(|e| anyhow::anyhow!(e.to_string()))?
    } else {
        let m = devforge::config::ConfigManifest::builtin();
        m.save(&manifest_path).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        m
    };

    // 目录约定（decisions/002/006 + 原则 §2/§4）
    let profiles_root = root.join("profiles");
    let plans_root = root.join("plans");
    let sessions_root = root.join("sessions");

    // 铁律守卫（用户 10-25 拍板）：最终产物禁入临时目录——root 命中 /tmp、/var/tmp
    // 及一切 tmpfs 挂载点直接拒绝启动（fail fast，重启即蒸发的教训已付过一次）。
    for (label, path) in [("--root", &root), ("plans_root", &plans_root)] {
        if let Some(reason) = devforge::domain::store::Store::tmp_path_violation(path) {
            anyhow::bail!(
                "{label} = {} 落在临时目录（{reason}）：最终产物禁入非持久位置，\n\
                 请改用持久路径（如 ~/.devforge 或数据盘目录）后重启",
                path.display()
            );
        }
    }

    // 生产引擎：规划 SOP 的 RPC 接线
    // 失活收割阈值（006 decisions/002）：DEVFORGE_RUN_IDLE_MS 显式开启（ms，>0），
    // 默认 None = 永不因无进展自动杀 run（零硬限制哲学；慢而活着的 run 不误伤）
    let idle_ms = std::env::var("DEVFORGE_RUN_IDLE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|n| *n > 0);
    let bus = std::sync::Arc::new(devforge::observability::events::EventBus::new());
    let ui = Arc::new(devforge::server::UiRelay::new(bus.clone()));
    let engine_profiles = profiles_root.clone();
    let engine_sessions = sessions_root.clone();
    let engine_plans = plans_root.clone();
    let engine_ui = ui.clone();
    let engine_factory = Box::new(move || -> Box<dyn devforge::domain::runner::RunEngine + Send> {
        Box::new(devforge::engine::builtin::BuiltinRpcEngine {
            profile_root: engine_profiles.clone(),
            session_root: engine_sessions.clone(),
            plans_root: engine_plans.clone(),
            ui: engine_ui.clone(),
            codegraph_bin: std::path::PathBuf::from("codegraph"),
            idle_ms,
        })
    });

    let mut state = devforge::server::ServerState::new(
        // store 持久化（复验 backlog 设计错误修复）：启动加载 --root/store.json，
        // 变更自动落盘——重启不再蒸发 projects/tasks/runs。
        devforge::domain::Store::load(&root.join("store.json"))
            .map(|s| s.with_persist(root.join("store.json")))
            .unwrap_or_else(|e| {
                // 地基批次 #3：备份要如实——旧实现先 warn「已备份为 .bak」再 `let _ = rename`，
                // rename 失败（权限/跨设备）无日志、且文件不存在（首启）也走这条谎言。
                let path = root.join("store.json");
                let bak = root.join("store.json.bak");
                if path.exists() {
                    match std::fs::rename(&path, &bak) {
                        Ok(()) => tracing::warn!(
                            component = "main",
                            error = %e,
                            bak = %bak.display(),
                            "store.json 不可用，已备份为 .bak 后按空仓冷启动"
                        ),
                        Err(re) => tracing::error!(
                            component = "main",
                            error = %e,
                            rename_error = %re,
                            path = %path.display(),
                            "store.json 不可用且备份失败——坏文件仍在原处，下次启动会再次命中"
                        ),
                    }
                } else {
                    tracing::warn!(
                        component = "main",
                        error = %e,
                        "store.json 不存在（首次启动），按空仓冷启动"
                    );
                }
                devforge::domain::Store::new().with_persist(path)
            }),
        devforge::domain::sop::SopRegistry::builtin(),
        manifest,
        engine_factory,
        bus,
        ui,
        std::sync::Arc::new(devforge::config::installer::PiInstallBridge),
        profiles_root,
        plans_root,
        sessions_root,
    );
    // P3-ISSUE-02 修复：真源路径交给状态，PUT 替换时同步落盘
    *state
        .manifest_path
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(manifest_path);
    // 自审 critical：启动对账 store 里的僵尸 Running——进程崩溃/被杀后遗留的 Running
    // run 会永久占死并发槽、任务停在 Running 无法再发起，无自愈即整机拒绝服务。
    let zombies = state
        .store
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .reconcile_zombie_runs();
    if zombies > 0 {
        tracing::warn!(
            component = "server",
            zombies,
            "启动对账：中断遗留的 Running run/任务已按失败收场，并发槽已释放"
        );
    }
    // P3-ISSUE-03 修复：token 配置入口（CLI --token > DEVFORGE_TOKEN 环境变量；
    // 未设即放行，Q003）+ run 超时（P8-ISSUE-01）：默认不限时（零硬限制），
    // DEVFORGE_RUN_TIMEOUT_MS 显式开启（两者在 Arc 分享前经 get_mut 注入）
    // 外部审计 #9（2026-10-28）：此处 Arc 尚未分享（下面才交给 server），get_mut
    // 恒成功；若未来有人把注入点挪到分享之后，必须炸出来而不是静默丢掉已配置的
    // token/超时——回环地址下 DR-9 闸不覆盖，静默丢失 = 鉴权无声关闭。
    let Some(s) = std::sync::Arc::get_mut(&mut state) else {
        panic!("启动注入 token/超时时 Arc 已被分享——注入点必须保持在服务启动之前");
    };
    // 自审 minor（46）：env 名与解析都在 server::http 里（单一契约来源）
    if let Some(t) = devforge::server::resolve_startup_token(
        devforge::server::parse_token_flag(&args),
        devforge::server::http::token_from_env(),
    ) {
        s.token = Some(t);
    }
    if let Ok(ms) = std::env::var("DEVFORGE_RUN_TIMEOUT_MS")
        && let Ok(n) = ms.parse::<u64>()
        && n > 0
    {
        s.run_timeout_ms = Some(n);
    }

    let addr: std::net::SocketAddr = std::env::var("DEVFORGE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:7888".into())
        .parse()
        .context("解析 DEVFORGE_ADDR")?;
    // DR-9 L5：远程可视的安全闸——绑非回环地址（暴露给局域网/tailnet，供 Win10 等外部机器
    // 查看全部行为）必须配 token，否则拒绝启动：无鉴权的全行为可视端点不能裸奔。
    if !addr.ip().is_loopback() && state.token.is_none() {
        anyhow::bail!(
            "DEVFORGE_ADDR 绑定非回环地址 {addr} 但未配置 token：远程可视必须带鉴权
            （--token <值> 或 DEVFORGE_TOKEN 环境变量），拒绝裸奔启动"
        );
    }
    // 引擎 #2：启动对账——上一进程遗留的自家子进程（Ours）直接 SIGTERM 清场，Dead 条目
    // 从台账清掉，Foreign 绝不动（红线：只认 DEVFORGE_MANAGED 标记）。
    // 此前 reconcile/kill_orphan 只在单测里被调用，崩溃孤儿会继续改目标仓。
    let (killed, cleared, foreign) = devforge::engine::ledger::reconcile_all(&state.plans_root);
    if killed + cleared + foreign > 0 {
        tracing::warn!(
            component = "startup",
            event = "ledger.reconcile",
            killed,
            cleared,
            foreign,
            "启动对账：清掉遗留子进程与已死条目（Foreign 一律不杀）"
        );
    }
    devforge::server::serve(state, addr)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
