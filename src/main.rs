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
    // 引擎 #2 启动对账：上一进程遗留的自家子进程（Ours）直接 SIGTERM 清场，Dead 条目
    // 从台账清掉，Foreign 绝不动（红线：只认 DEVFORGE_MANAGED 标记）。
    // 此前 reconcile/kill_orphan 只在单测里被调用，崩溃孤儿会继续改目标仓。
    //
    // 活动集与 store 可信度是**启动对账策略层**的输入（两道安全阀的判定依据）；
    // 本步先把调用点适配新签名，启动侧的真实接线在下一步落地。
    let report = devforge::engine::ledger::reconcile_all(
        &state.plans_root,
        &std::collections::BTreeSet::new(),
        true,
    );
    if report.killed + report.cleared + report.foreign > 0 {
        tracing::warn!(
            component = "startup",
            event = "ledger.reconcile",
            killed = report.killed,
            cleared = report.cleared,
            foreign = report.foreign,
            "启动对账：清掉遗留子进程与已死条目（Foreign 一律不杀）"
        );
    }
    devforge::server::serve(state, addr)
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_runtime_root;
    use std::path::PathBuf;

    /// 需求：DEVFORGE_HOME 已设时**不要求 HOME 存在**。
    /// 断言钉的是对外行为（解析结果 + HOME 根本没被读）——
    /// 换成「先无条件读 HOME」的旧实现：HOME 闭包被调用 → 这条红。
    #[test]
    fn devforge_home_alone_resolves_without_reading_home() {
        let mut home_read = false;
        let root = resolve_runtime_root(Some("/x/devforge"), || {
            home_read = true;
            Some("/home/u".to_string())
        })
        .expect("DEVFORGE_HOME 已设时必须解析成功，不依赖 HOME");
        assert_eq!(root, PathBuf::from("/x/devforge"));
        assert!(!home_read, "DEVFORGE_HOME 已设时不得读取 HOME");
    }

    /// 需求点 2：DEVFORGE_HOME 与 HOME 同时存在时，root 仍是 DEVFORGE_HOME。
    /// 换成「优先 HOME」的错实现：root 变成 $HOME/.devforge → 这条红。
    #[test]
    fn devforge_home_wins_over_home() {
        let root = resolve_runtime_root(Some("/x/devforge"), || Some("/home/u".to_string()))
            .expect("DEVFORGE_HOME 与 HOME 同时存在时必须解析成功");
        assert_eq!(
            root,
            PathBuf::from("/x/devforge"),
            "DEVFORGE_HOME 优先于 HOME"
        );
    }

    /// 只设 HOME 时回落 $HOME/.devforge。
    /// 换成「只认 DEVFORGE_HOME、忽略 HOME」的错实现：解析失败 → 这条红。
    #[test]
    fn home_is_the_fallback_root() {
        let root = resolve_runtime_root(None, || Some("/home/u".to_string()))
            .expect("仅设 HOME 时必须解析成功");
        assert_eq!(root, PathBuf::from("/home/u").join(".devforge"));
    }

    /// 空串/纯空白视作未设（与 observability::resolve_root 同口径）。
    /// 换成旧 var_os 口径（空串 = 已设）：root 变成空路径 → 这条红。
    #[test]
    fn blank_devforge_home_falls_back_to_home() {
        for blank in ["", "   "] {
            let root = resolve_runtime_root(Some(blank), || Some("/home/u".to_string()))
                .expect("DEVFORGE_HOME 只有空白时回落 HOME 必须成功");
            assert_eq!(
                root,
                PathBuf::from("/home/u").join(".devforge"),
                "DEVFORGE_HOME={blank:?} 视作未设，应回落 $HOME/.devforge"
            );
        }
    }

    /// 双未设 fail fast，错误文案同时点名两个变量名（给出双出路）。
    /// 换成只点 HOME 的旧文案（"读取 HOME 环境变量"）：缺 DEVFORGE_HOME → 这条红。
    #[test]
    fn missing_both_fails_fast_naming_both_vars() {
        let cases: [(Option<&str>, Option<String>); 3] = [
            (None, None),
            (Some("   "), None),
            (None, Some("  ".to_string())),
        ];
        for (devforge_home, home) in cases {
            let err = resolve_runtime_root(devforge_home, || home.clone())
                .expect_err("DEVFORGE_HOME 与 HOME 都没有时必须 fail fast");
            let msg = err.to_string();
            assert!(
                msg.contains("DEVFORGE_HOME"),
                "错误文案须点名 DEVFORGE_HOME：{msg}"
            );
            assert!(msg.contains("HOME"), "错误文案须点名 HOME：{msg}");
        }
    }
}

/// 验收测试模块（计划 §5 的 11 条验收标准 → ac01..ac09、ac11 共 10 条用例）。
///
/// 来源：测试阶段备好的验收用例模块（上一轮遗留的交付项：契约 `owned_paths` 只允许
/// `src/main.rs`，不新增 `tests/` 文件，模块当时无处可落，导致 11 条验收标准逐条「无法对账」）。
/// 交付时相对原件只做两处必要修正，其余（断言内容、用例名、契约签名）逐字未改：
/// - ac11 的基线 sha 由 `dd610645…` 改为本变更真基线 `bb9c824…`：原值不在本仓，
///   `git diff` 会报 bad object，该条会走「跳过」分支静默不生效（等于假绿）；
/// - 去掉模块尾部残留的 `// scratch tweak` 半成品注释。
// ---------------- 验收测试（change-20261028-2026-10-28-4） ----------------
#[cfg(test)]
mod acceptance_tests {
    use super::resolve_runtime_root;
    use std::path::PathBuf;

    /// src/main.rs 的**生产段**（首个 `#[cfg(test)]` 之前）——测试模块自身的文本不参与断言，
    /// 免得断言字符串把自己匹配上。
    fn production_source() -> &'static str {
        let src = include_str!("main.rs");
        src.split("#[cfg(test)]").next().unwrap_or(src)
    }


    // ---------------- 验收测试（change-20261028-2026-10-28-4） ----------------
    // 黑盒立场：断言只钉「对外可观察的结果」——解析返回值、错误文案、以及 src/main.rs
    // 文本层面的口径漂移；不钉实现步骤数 / 内部调用次序（纯内部重构不该让它们变红）。
    // 每条用例的注释写明「换成哪种错实现会红」——不会红的测试是装饰品。

    /// 验收 1：`DEVFORGE_HOME = "/x/devforge"`、`HOME = None` → 解析成功并返回
    /// `/x/devforge`（不再因缺 HOME 返回错误）。
    /// 错实现 A（无条件先读 HOME 并 `?` 退进程）→ 这条红（HOME 闭包被调用）。
    #[test]
    fn ac01_devforge_home_alone_resolves_without_home() {
        let mut home_read = false;
        let root = resolve_runtime_root(Some("/x/devforge"), || {
            home_read = true;
            Some("/home/u".to_string())
        })
        .expect("DEVFORGE_HOME 已设时不得因缺 HOME 失败");
        assert_eq!(
            root,
            PathBuf::from("/x/devforge"),
            "DEVFORGE_HOME 已设时运行根就是它本身"
        );
        assert!(!home_read, "DEVFORGE_HOME 已设时不得读取 HOME");
    }

    /// 验收 2：`DEVFORGE_HOME = "/x/devforge"`、`HOME = Some("/home/u")` → 返回
    /// `/x/devforge`（HOME 不影响结果）。
    /// 错实现 B（优先 HOME）→ 这条红。
    #[test]
    fn ac02_devforge_home_wins_over_home() {
        let root = resolve_runtime_root(Some("/x/devforge"), || Some("/home/u".to_string()))
            .expect("DEVFORGE_HOME 与 HOME 同时存在时必须解析成功");
        assert_eq!(
            root,
            PathBuf::from("/x/devforge"),
            "两个变量同时存在时 DEVFORGE_HOME 优先"
        );
    }

    /// 验收 3：`DEVFORGE_HOME = None`、`HOME = Some("/home/u")` → 返回 `/home/u/.devforge`。
    /// 错实现 C（只认 DEVFORGE_HOME、忽略 HOME）→ 这条红（解析失败）。
    #[test]
    fn ac03_home_only_resolves_to_home_dot_devforge() {
        let root = resolve_runtime_root(None, || Some("/home/u".to_string()))
            .expect("仅设 HOME 时必须解析成功");
        assert_eq!(
            root,
            PathBuf::from("/home/u").join(".devforge"),
            "未设 DEVFORGE_HOME 时回落 $HOME/.devforge"
        );
    }

    /// 验收 4：`DEVFORGE_HOME = Some("")`（空串）与 `Some("  ")`（纯空白）、
    /// `HOME = Some("/home/u")` → 返回 `/home/u/.devforge`（空串/空白视为未设置）。
    /// 错实现 D（空串按 var_os 口径视作已设）→ 这条红。
    #[test]
    fn ac04_blank_devforge_home_falls_back_to_home() {
        for blank in ["", "  ", "\t", "\n  "] {
            let root = resolve_runtime_root(Some(blank), || Some("/home/u".to_string()))
                .expect("DEVFORGE_HOME 只有空白时必须回落 HOME 而不是失败");
            assert_eq!(
                root,
                PathBuf::from("/home/u").join(".devforge"),
                "DEVFORGE_HOME={blank:?} 视作未设，应回落 $HOME/.devforge"
            );
        }
    }

    /// 验收 5：`DEVFORGE_HOME = Some("  ")`、`HOME = None` → 返回错误，错误文案同时包含
    /// `DEVFORGE_HOME` 与 `HOME` 两个字面名（双未设 fail fast，且给出两条出路）。
    /// 错实现 E（文案只点 HOME）→ 这条红。
    #[test]
    fn ac05_blank_devforge_home_without_home_fails_naming_both_vars() {
        let err = resolve_runtime_root(Some("  "), || None)
            .expect_err("DEVFORGE_HOME 只有空白且无 HOME 时必须 fail fast");
        let msg = err.to_string();
        assert!(
            msg.contains("DEVFORGE_HOME"),
            "错误文案须点名 DEVFORGE_HOME：{msg}"
        );
        assert!(msg.contains("HOME"), "错误文案须点名 HOME：{msg}");
    }

    /// 验收 6：`DEVFORGE_HOME = None`、`HOME = None` → 返回错误，错误文案同时包含
    /// `DEVFORGE_HOME` 与 `HOME` 两个字面名。
    /// 错实现 E（文案只点 HOME）→ 这条红。
    #[test]
    fn ac06_both_missing_fails_naming_both_vars() {
        let err = resolve_runtime_root(None, || None)
            .expect_err("DEVFORGE_HOME 与 HOME 都没有时必须 fail fast");
        let msg = err.to_string();
        assert!(
            msg.contains("DEVFORGE_HOME"),
            "错误文案须点名 DEVFORGE_HOME：{msg}"
        );
        assert!(msg.contains("HOME"), "错误文案须点名 HOME：{msg}");
    }

    /// 验收 7：`resolve_runtime_root(...)` 返回的根是 `main` 里所有 `root.join(...)` 落点
    /// （logs / manifest.json / profiles / plans / sessions / store.json）的唯一来源——
    /// 不存在第二处 HOME 推导。
    /// 错实现 F（在 main 里另起一条 HOME/.devforge 推导）→ 这条红。
    #[test]
    fn ac07_all_landing_points_derive_from_one_root() {
        let prod = production_source();
        for landing in [
            "logs",
            "manifest.json",
            "profiles",
            "plans",
            "sessions",
            "store.json",
        ] {
            assert!(
                prod.contains(&format!(".join(\"{landing}\")")),
                "落点 {landing} 必须由运行根 join 派生（不得另起推导）"
            );
        }
        let home_reads = prod.matches("env::var(\"HOME\")").count();
        assert!(
            home_reads <= 1,
            "生产段里读 HOME 的地方只能有一处（兜底分支），实测 {home_reads} 处"
        );
    }

    /// 验收 8：`src/main.rs` 的根解析路径中不再存在「无条件 `std::env::var("HOME")?`」形态
    /// 的调用（HOME 读取只发生在 DEVFORGE_HOME 判空之后的兜底分支）。
    /// 错实现 A（无条件读 HOME + `?` 退进程）→ 这条红。
    #[test]
    fn ac08_no_unconditional_home_read_in_root_resolution() {
        let prod = production_source();
        for stale in [
            "env::var(\"HOME\").context(",
            "env::var(\"HOME\")?",
            "env::var(\"HOME\").ok().context(",
        ] {
            assert!(
                !prod.contains(stale),
                "生产段仍存在无条件读 HOME 的形态：{stale}"
            );
        }
        // HOME 的读取不得发生在「先绑定 HOME 再解析根」的形态里（惰性闭包除外）：
        // 旧口径 `let home = std::env::var("HOME")...` 先行 = 无条件依赖 HOME。
        for line in prod.lines() {
            if !line.contains("env::var(\"HOME\")") {
                continue;
            }
            if line.contains("||") {
                continue; // 惰性闭包（兜底分支）——正确形态
            }
            assert!(
                !line.contains("let "),
                "HOME 被提前绑定到变量（无条件依赖 HOME）：{}",
                line.trim()
            );
        }
        let devforge_home_at = prod.find("env::var(\"DEVFORGE_HOME\")");
        let home_at = prod.find("env::var(\"HOME\")");
        if let (Some(d), Some(h)) = (devforge_home_at, home_at) {
            assert!(
                d < h,
                "DEVFORGE_HOME 的读取必须排在 HOME 读取之前（HOME 只在兜底分支被读）"
            );
        }
    }

    /// 验收 9：根解析口径注释与观测根注释都表述为「DEVFORGE_HOME 优先；HOME 仅在
    /// DEVFORGE_HOME 未设时兜底；双未设 fail fast」，无「HOME 必需」或
    /// 「DEVFORGE_HOME 只改观测根」表述。
    /// 错实现 G（注释沿用旧口径）→ 这条红。
    #[test]
    fn ac09_root_resolution_comments_match_the_contract() {
        let prod = production_source();
        for stale in ["HOME 必需", "只改观测根", "只改观测/日志根", "DEVFORGE_HOME 只能改"] {
            assert!(
                !prod.contains(stale),
                "注释仍带旧口径表述：{stale}"
            );
        }
        let contract_comment = prod.lines().any(|l| {
            let t = l.trim_start();
            t.starts_with("//") && t.contains("DEVFORGE_HOME") && t.contains("HOME")
        });
        assert!(
            contract_comment,
            "口径注释须有一行同时点名 DEVFORGE_HOME 与 HOME（DEVFORGE_HOME 优先 / HOME 兜底 / 双未设 fail fast）"
        );
    }

    /// 验收 11（2026-10-29 监工后验重划范围）：根解析特性不得泄漏到 main.rs 之外——
    /// src/ 下除 main.rs 外的任何源文件出现 resolve_runtime_root 即为越界，这条红。
    /// （原断言「相对基线 bb9c824 的改动只有 main.rs 一行」只在 SOP 工作区里有意义：
    ///   合入 master 后任何后续提交都会使其永久变红，且会毒化后续每轮自修工作区。）
    #[test]
    fn ac11_diff_scope_is_only_main_rs() {
        let mut offenders = Vec::new();
        let mut stack = vec![std::path::PathBuf::from("src")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs")
                    && p.file_name().is_some_and(|n| n != "main.rs")
                {
                    let Ok(text) = std::fs::read_to_string(&p) else {
                        continue;
                    };
                    if text.contains("resolve_runtime_root") {
                        offenders.push(p.display().to_string());
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "根解析实现泄漏到 main.rs 之外：{offenders:?}"
        );
    }
}


// ---------------- 验收测试·测试阶段补强（change-20261028-2026-10-28-4） ----------------
// 本模块只含测试代码（`#[cfg(test)]`），不触碰任何生产逻辑。
// 追加在文件末尾：既有用例与计划/对账表里引用的行号不受影响。
#[cfg(test)]
mod acceptance_tests_extra {
    /// src/main.rs 的**生产段**（首个 `#[cfg(test)]` 之前）——测试模块自身的文本不参与断言，
    /// 免得断言字符串把自己匹配上。
    fn production_source() -> &'static str {
        let src = include_str!("main.rs");
        src.split("#[cfg(test)]").next().unwrap_or(src)
    }

    /// 验收 9（加严）：口径注释必须**两处都在**表述契约——运行根注释块与观测根注释块
    /// 各自点名 `DEVFORGE_HOME` 与 `HOME`，且都不带旧口径措辞。
    /// 既有 ac09 只要求「整份文件里有任意一行注释同时点名两个变量」：把观测根注释改回
    /// 旧的「与日志目录同根，`DEVFORGE_HOME` 可覆盖（Q4）」，ac09 仍绿 → 这条红。
    #[test]
    fn ac09b_both_root_and_observability_comments_state_the_contract() {
        let prod = production_source();
        let lines: Vec<&str> = prod.lines().collect();

        // 取某行之前紧邻的连续注释块（自下而上收拢 `//` 开头的行）
        let comment_block_before = |needle: &str| -> String {
            let idx = lines
                .iter()
                .position(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("生产段里找不到 `{needle}` 这一行"));
            let mut block: Vec<&str> = Vec::new();
            for l in lines[..idx].iter().rev() {
                if l.trim_start().starts_with("//") {
                    block.push(l);
                } else {
                    break;
                }
            }
            block.reverse();
            block.join("\n")
        };

        let root_block = comment_block_before("let root = resolve_runtime_root(");
        let obs_block = comment_block_before("let obs_root = devforge::observability::root()");

        for (label, block) in [
            ("运行根注释", root_block.as_str()),
            ("观测根注释", obs_block.as_str()),
        ] {
            assert!(!block.trim().is_empty(), "{label} 缺失（注释被删？）");
            assert!(
                block.contains("DEVFORGE_HOME") && block.contains("HOME"),
                "{label} 必须同时点名 DEVFORGE_HOME 与 HOME（口径：DEVFORGE_HOME 优先、HOME 仅在未设时兜底）：\n{block}"
            );
        }
        assert!(
            root_block.contains("fail fast") || root_block.contains("兜底"),
            "运行根注释须写明双未设 fail fast / HOME 兜底：\n{root_block}"
        );
        for stale in [
            "HOME 必需",
            "只改观测根",
            "只改观测/日志根",
            "与日志目录同根",
            "可覆盖",
        ] {
            assert!(
                !root_block.contains(stale) && !obs_block.contains(stale),
                "注释块仍带旧口径表述 `{stale}`：\n{root_block}\n---\n{obs_block}"
            );
        }
    }

    /// 验收 10：`cargo test` 汇总里出现新增根解析单测的用例名
    /// （= 新单测真的被编译进被执行的测试二进制，不会「写了不跑」）。
    /// 错实现形态：新单测落在未参与编译的模块里 / 用例名被改掉 / 只写了断言没注册
    /// → 这条红。
    #[test]
    fn ac10_new_root_resolution_tests_are_registered_in_the_test_binary() {
        let exe = std::env::current_exe().expect("当前测试二进制路径");
        let out = std::process::Command::new(&exe)
            .arg("--list")
            .output()
            .expect("列举测试二进制内的用例（--list）");
        assert!(
            out.status.success(),
            "--list 退出码 {:?}（stderr: {}）",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        let listing = String::from_utf8_lossy(&out.stdout).to_string();
        for name in [
            "tests::devforge_home_alone_resolves_without_reading_home",
            "tests::devforge_home_wins_over_home",
            "tests::home_is_the_fallback_root",
            "tests::blank_devforge_home_falls_back_to_home",
            "tests::missing_both_fails_fast_naming_both_vars",
            "acceptance_tests::ac01_devforge_home_alone_resolves_without_home",
            "acceptance_tests::ac02_devforge_home_wins_over_home",
            "acceptance_tests::ac03_home_only_resolves_to_home_dot_devforge",
            "acceptance_tests::ac04_blank_devforge_home_falls_back_to_home",
            "acceptance_tests::ac05_blank_devforge_home_without_home_fails_naming_both_vars",
            "acceptance_tests::ac06_both_missing_fails_naming_both_vars",
            "acceptance_tests::ac07_all_landing_points_derive_from_one_root",
            "acceptance_tests::ac08_no_unconditional_home_read_in_root_resolution",
            "acceptance_tests::ac09_root_resolution_comments_match_the_contract",
            "acceptance_tests::ac11_diff_scope_is_only_main_rs",
        ] {
            assert!(
                listing.contains(name),
                "测试清单里必须出现用例 `{name}`：新增根解析单测必须全部被编译执行（cargo test 汇总里出现其用例名）"
            );
        }
    }
}
