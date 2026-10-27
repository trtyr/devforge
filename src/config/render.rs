//! Profile 渲染器（P002）：清单（真源）→ profile 目录（渲染产物，可随时重建）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::config::installer::PackageInstaller;
use crate::config::manifest::ConfigManifest;
use crate::config::skill_assets::builtin_skill_files;
use crate::error::{Context as _, DevforgeError, Result};

/// 运行时生存配置（P002 模板层写死；P2-ISSUE-04 修复：中转抖动的重试生存配置、
/// 流挂死快判、reasoning 预算等此前缺失，导致同一中转上真实环境存活而 profile 一次即死）。
fn runtime_settings() -> serde_json::Value {
    serde_json::json!({
        "retry": { "maxRetries": 3, "baseDelayMs": 5000 },
        "httpIdleTimeoutMs": 60000,
        "defaultThinkingLevel": "high",
        "hideThinkingBlock": true,
        "compaction": { "enabled": false, "reserveTokens": 16384, "keepRecentTokens": 20000 },
    })
}

/// 渲染并发度上限（CAL-19-RENDER：八 profile 串行装包实测 >20 分钟，
/// 并发四路为实测保守值——再高会跟 npm 缓存/网络限流撞车）。
const MAX_RENDER_CONCURRENCY: usize = 4;

/// 渲染就绪标记文件名（profile 目录内）。内容 = 清单指纹，不是空文件。
pub const RENDER_READY_MARKER: &str = ".devforge-rendered";

/// 清单指纹：provider + model + 包列表（顺序稳定）→ FNV-1a 十六进制。
///
/// 自审 minor（115）：只需要「两次渲染是不是同一份清单」这一位信息，不引新依赖。
pub fn render_fingerprint(provider: &str, model_id: &str, packages: &[String]) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    fn feed(hash: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *hash ^= *b as u64;
            *hash = hash.wrapping_mul(0x100000001b3);
        }
        *hash ^= 0xff;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
    feed(&mut hash, provider.as_bytes());
    feed(&mut hash, model_id.as_bytes());
    for p in packages {
        feed(&mut hash, p.as_bytes());
    }
    format!("{hash:016x}")
}

/// profile 的渲染指纹（`None` = 没有完成标记：未渲染，或旧版本渲的半成品）。
pub fn rendered_fingerprint(profile_dir: &Path) -> Option<String> {
    std::fs::read_to_string(profile_dir.join(RENDER_READY_MARKER)).ok()
}

pub struct Renderer<'a> {
    pub manifest: &'a ConfigManifest,
    pub installer: &'a dyn PackageInstaller,
    /// skills 内容源目录（含 `<skill-name>/` 子目录）。None = 只建占位目录。
    pub skills_source: Option<&'a Path>,
}

impl<'a> Renderer<'a> {
    /// 渲染单个 Agent 的 profile 到 `profiles_root/<agent-name>/`。
    pub fn render_agent(&self, profiles_root: &Path, agent_name: &str) -> Result<PathBuf> {
        let agent = self.manifest.get_agent(agent_name)?;
        let model = self.manifest.resolve_model(agent)?;

        let dir = profiles_root.join(&agent.name);
        fs::create_dir_all(dir.join("skills")).ctx("创建 profile 骨架目录")?;

        // 幂等跳过的前置读取（CAL-19-RENDER）：settings.json 马上会被重写，
        // 已记录的 packages 必须先抓下来，否则无法判断「已装齐」而重复装包。
        let previous = recorded_packages(&dir);

        // AGENTS.md（模板层：角色提示词）
        fs::write(dir.join("AGENTS.md"), &agent.role_prompt)
            .ctx(format!("写 {} 的 AGENTS.md", agent.name))?;

        // settings.json（模型层解析 + 运行时生存配置；packages 由 installer 双记录追加）
        let mut settings = runtime_settings();
        settings["defaultProvider"] = serde_json::Value::String(model.provider.clone());
        settings["defaultModel"] = serde_json::Value::String(model.model_id.clone());
        // 思考档位覆盖（性能剖析 2026-10-19）：审查 Agent 降 medium——review 每轮 11-22s、
        // turns 45-71，输出 token 占大头，高思考直接变成墙钟
        if let Some(level) = &agent.thinking_level {
            settings["defaultThinkingLevel"] = serde_json::Value::String(level.clone());
        }
        fs::write(
            dir.join("settings.json"),
            serde_json::to_string_pretty(&settings).ctx("序列化 settings")?,
        )
        .ctx(format!("写 {} 的 settings.json", agent.name))?;

        // pathguard.json（P012 topic 07）：工具级路径权限，pi 侧 path-guard 插件拦截
        // read/write/edit；deny-first + realpath 语义在插件侧。None = 不生成（老清单兼容）。
        // 提权防护：profile 目录与 guard 配置自身恒追加在 deny_write（插件不可改自己的边界）。
        if let Some(pp) = &agent.path_permissions {
            let mut guard = pp.clone();
            let profile_deny = format!("{}/**", dir.to_string_lossy());
            if !guard.deny_write.contains(&profile_deny) {
                guard.deny_write.push(profile_deny);
            }
            let pg_dir = dir.join(".pi");
            fs::create_dir_all(&pg_dir).ctx("创建 .pi 目录")?;
            fs::write(
                pg_dir.join("pathguard.json"),
                serde_json::to_string_pretty(&guard).ctx("序列化 pathguard")?,
            )
            .ctx(format!("写 {} 的 pathguard.json", agent.name))?;
            // 插件分发：内嵌 TS（include_str!，非源码部署可用）写到 agent-dir extensions/，
            // pi 自动加载（topic 08 §形态与分发）。None 的 profile 不分发（零痕迹）。
            for (rel, content) in crate::config::extension_assets::PATH_GUARD_FILES {
                let target = dir.join("extensions").join("path-guard").join(rel);
                fs::create_dir_all(target.parent().expect("插件路径有父目录"))
                    .ctx("创建插件目录")?;
                fs::write(&target, content).ctx(format!("分发 path-guard/{rel}"))?;
            }
        }

        // models.json（P2-ISSUE-03 修复：provider 定义 + 模型窗口/输出预算）
        let provider = self.manifest.resolve_provider(agent)?;
        let models: Vec<serde_json::Value> = self
            .manifest
            .model_pool
            .iter()
            .filter(|m| m.provider == provider.name)
            .map(|m| {
                serde_json::json!({
                    "id": m.model_id,
                    "name": m.model_id,
                    "contextWindow": m.context_window,
                    "maxTokens": m.max_tokens,
                })
            })
            .collect();
        let models_json = serde_json::json!({
            "providers": {
                provider.name.clone(): {
                    "api": provider.api,
                    "baseUrl": provider.base_url,
                    "models": models,
                }
            }
        });
        fs::write(
            dir.join("models.json"),
            serde_json::to_string_pretty(&models_json).ctx("序列化 models")?,
        )
        .ctx(format!("写 {} 的 models.json", agent.name))?;

        // skills 分发：有源复制；无源用内置资产；两者皆无才建占位（P2-ISSUE-05 修复：
        // 占位目录曾导致 Agent 全盘扫描找规范，单次工具执行 10-30 分钟）
        for skill in &agent.skills {
            let target = dir.join("skills").join(skill);
            match self.skills_source {
                Some(src) => {
                    let from = src.join(skill);
                    copy_dir(&from, &target).ctx(format!(
                        "复制 skill {skill}: {} → {}",
                        from.display(),
                        target.display()
                    ))?;
                }
                None => match builtin_skill_files(skill) {
                    Some(files) => {
                        fs::create_dir_all(&target).ctx("建 skill 目录")?;
                        for (name, body) in files {
                            let f = target.join(name);
                            if let Some(parent) = f.parent() {
                                fs::create_dir_all(parent).ctx("建 skill 子目录")?;
                            }
                            fs::write(&f, body)
                                .ctx(format!("写内置 skill {skill}/{name}"))?;
                            // 自审 major：内嵌资产不带可执行位——ci.sh 这类脚手架
                            // 落盘 0644，skill 自述的 ./ci.sh 唯一门禁入口必然
                            // Permission denied。脚本类文件补 0755。
                            #[cfg(unix)]
                            if name.ends_with(".sh") {
                                use std::os::unix::fs::PermissionsExt;
                                fs::set_permissions(&f, fs::Permissions::from_mode(0o755))
                                    .ctx(format!("补可执行位 {skill}/{name}"))?;
                            }
                        }
                    }
                    None => {
                        fs::create_dir_all(&target).ctx("建 skill 占位目录")?;
                        fs::write(
                            target.join("README.md"),
                            format!("# {skill}\n\n占位目录——无内置资产且未提供 skills 源。\n"),
                        )
                        .ctx("写 skill 占位 README")?;
                    }
                },
            }
        }

        // auth.json——明文 Key，provider 键控形状（P002 decisions/001 明文不设防；
        // P2-ISSUE-01 修复：pi 期望 {"<provider>": {"type": "api_key", "key": ...}}）
        if let Some(key) = &self.manifest.api_key {
            fs::write(
                dir.join("auth.json"),
                serde_json::json!({ (provider.name.clone()): { "type": "api_key", "key": key } })
                    .to_string(),
            )
            .ctx(format!("写 {} 的 auth.json", agent.name))?;
        } else {
            // DR-1（2026-10-23 真实仓首跑）：无 key 的 profile 模型调用必败（No API key），
            // 此前无任何提示。渲染期当场喊出来。
            tracing::warn!(
                component = "config",
                event = "profile.render_without_api_key",
                agent = %agent.name,
                provider = %provider.name,
                "manifest 未带 api_key：该 profile 无凭据，模型调用将失败（No API key found）"
            );
        }

        // mcp.json（P005 T003：pi 原生 schema，<agent-dir>/mcp.json 全项目可用；
        // 空 = 不生成该文件，规划双 Agent 不受影响；但若旧渲染留过文件（如 DR-12：
        // doc-review 从挂码图到清空的迁移），必须清掉，否则「空」语义形同虚设）
        if agent.mcp.is_empty() {
            let mcp_path = dir.join("mcp.json");
            if mcp_path.exists() {
                fs::remove_file(&mcp_path)
                    .ctx(format!("清 {} 的旧 mcp.json（清单已置空）", agent.name))?;
            }
        } else {
            let mut servers = serde_json::Map::new();
            for m in &agent.mcp {
                let mut entry = serde_json::Map::new();
                entry.insert("command".into(), serde_json::Value::String(m.command.clone()));
                entry.insert(
                    "args".into(),
                    serde_json::Value::Array(
                        m.args.iter().cloned().map(serde_json::Value::String).collect(),
                    ),
                );
                if !m.env.is_empty() {
                    entry.insert(
                        "env".into(),
                        serde_json::Value::Object(
                            m.env.iter()
                                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                                .collect(),
                        ),
                    );
                }
                servers.insert(m.name.clone(), serde_json::Value::Object(entry));
            }
            let mcp_json = serde_json::json!({ "mcpServers": servers });
            fs::write(
                dir.join("mcp.json"),
                serde_json::to_string_pretty(&mcp_json).ctx("序列化 mcp")?,
            )
            .ctx(format!("写 {} 的 mcp.json", agent.name))?;
        }

        // 包安装（必装四件套 + 差异化包）——双记录由 installer 负责
        let mut packages = self.manifest.base_packages.clone();
        packages.extend(agent.extra_packages.iter().cloned());
        // 自审 major：撤包方向的对账——manifest 移除的包若只在 settings 抹掉、
        // npm 依赖里残留，双记录自此自相矛盾（后续「已装齐」判定也建立在脏记录上）。
        prune_npm_dependencies(&dir, &packages)?;
        // 幂等跳过（CAL-19-RENDER）：已装齐且双记录完整则不重装，只回写 settings 记录。
        // 判定标准刻意保守——npm/package.json 缺失、或清单新增了包，一律重装全量。
        let installed = packages.iter().all(|p| previous.contains(p));
        let dual_record = dir.join("npm").join("package.json").exists();
        if installed && dual_record && !previous.is_empty() {
            settings["packages"] = serde_json::Value::Array(
                packages.iter().cloned().map(serde_json::Value::String).collect(),
            );
            fs::write(
                dir.join("settings.json"),
                serde_json::to_string_pretty(&settings).ctx("序列化 settings")?,
            )
            .ctx(format!("回写 {} 的 settings.json", agent.name))?;
            tracing::info!(
                component = "renderer",
                agent = %agent.name,
                packages = packages.len(),
                profile = %dir.display(),
                "profile 复用（包已装齐，跳过安装）"
            );
            return Ok(dir);
        }
        self.installer.install(&dir, &packages)?;

        // 自审 minor（115）：**完成标记最后写**——就绪判据不能是「settings.json 存在」，
        // 那个文件在装包**之前**就写了，闸门中途看到它就会放行一个半成品 profile（包还没装）。
        // 标记内含清单指纹（provider/model/packages），也能区分「profile 是上一版清单渲的」。
        let fingerprint = render_fingerprint(&model.provider, &model.model_id, &packages);
        fs::write(dir.join(RENDER_READY_MARKER), &fingerprint)
            .ctx(format!("写 {} 的渲染完成标记", agent.name))?;

        tracing::info!(
            component = "renderer",
            agent = %agent.name,
            packages = packages.len(),
            profile = %dir.display(),
            "profile 渲染完成"
        );
        Ok(dir)
    }

    /// 渲染清单中全部 Agent。
    ///
    /// 并发执行（CAL-19-RENDER）：八 profile 串行 `pi install` 实测 >20 分钟，
    /// 各 profile 目录相互独立（文件写入 + 装包均落在自己目录），可安全并行。
    /// 返回值顺序与 `manifest.agents` 一致；任一路失败时按清单顺序取首个错误上抛。
    pub fn render_all(&self, profiles_root: &Path) -> Result<Vec<PathBuf>> {
        let agents = &self.manifest.agents;
        if agents.is_empty() {
            return Ok(Vec::new());
        }
        let workers = MAX_RENDER_CONCURRENCY.min(agents.len()).max(1);
        let next = AtomicUsize::new(0);
        let slots: Vec<Mutex<Option<Result<PathBuf>>>> =
            (0..agents.len()).map(|_| Mutex::new(None)).collect();
        std::thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= agents.len() {
                        break;
                    }
                    let res = self.render_agent(profiles_root, &agents[i].name);
                    *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(res);
                });
            }
        });
        let mut rendered = Vec::with_capacity(agents.len());
        for slot in slots {
            // 锁中毒口径统一：into_inner 拿 PoisonError 也恢复读取（渲染结果一致性不受 poison 影响）
            match slot.into_inner().unwrap_or_else(|e| e.into_inner()) {
                Some(res) => rendered.push(res?),
                None => {
                    return Err(DevforgeError::System(
                        "渲染槽位未填充（并发调度异常）".to_string(),
                    ))
                }
            }
        }
        Ok(rendered)
    }
}

/// 读取 profile 已记录的 packages（settings.json `packages`）。
/// 任何读取/解析失败都退化为「空集」→ 走全量安装（保守）。
fn recorded_packages(dir: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(dir.join("settings.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    value
        .get("packages")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 撤包对账：把 npm/package.json 的 dependencies 修剪成清单期望集。
/// 清单外残留（已撤包）与缺失（损坏记录）都交给重装路径；无记录则不动。
fn prune_npm_dependencies(dir: &Path, desired: &[String]) -> Result<()> {
    let path = dir.join("npm").join("package.json");
    if !path.exists() {
        return Ok(());
    }
    let raw = fs::read_to_string(&path).ctx("读 npm/package.json")?;
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Ok(());
    };
    let Some(deps) = v.get_mut("dependencies").and_then(|d| d.as_object_mut()) else {
        return Ok(());
    };
    let before = deps.len();
    deps.retain(|k, _| desired.iter().any(|p| p == k));
    let pruned = before - deps.len();
    if pruned == 0 {
        return Ok(());
    }
    fs::write(&path, serde_json::to_string_pretty(&v).ctx("序列化 package.json")?)
        .ctx("写 npm/package.json（撤包对账）")?;
    tracing::info!(
        component = "renderer",
        pruned,
        path = %path.display(),
        "npm 依赖对账：移除清单外的残留包"
    );
    Ok(())
}

/// 递归复制目录。
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to).ctx(format!("建目录 {}", to.display()))?;
    for entry in fs::read_dir(from).ctx(format!("读目录 {}", from.display()))? {
        let entry = entry.ctx("读目录项")?;
        let ty = entry.file_type().ctx("读文件类型")?;
        let target = to.join(entry.file_name());
        if ty.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target).ctx(format!(
                "复制 {} → {}",
                entry.path().display(),
                target.display()
            ))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod render_tests;
