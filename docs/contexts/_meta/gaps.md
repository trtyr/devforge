# 缺口记录 — unit-src-01（src/change/boundary.rs）

## GAP

- `replan_instruction` 文本中的「三张表」以及「影响面」「风险」小节，指代 plan 文档的既定章节结构；该结构定义在 `src/change/plan_audit.rs` 相关条目中（不属于本单元），此处不做展开，交叉引用即可。影响面：文档对「补进白名单」的落点描述依赖外部 plan 结构知识。

# 缺口记录 — unit-src-24（src/config/installer.rs）

## GAP

- 🟡→🔴 `pi install` 的「双记录」形状（`settings.json` 的 `packages` 数组 + `npm/package.json` 的 `dependencies`）只在本仓注释与 `FakeInstaller` 里被声明/模拟，`pi` 本体不在仓内，无法重放验证。影响面：若 pi 改变双记录落点或形状，`PiInstallBridge` 仍能「装成功」，而渲染层的幂等判据（`src/config/render.rs:277` `recorded_packages`、`:203` 判 `npm/package.json` 存在）会误判 → 每次重装或错误跳过；仓内 5 个单测全绿也发现不了。
- 🟡 `PiInstallBridge::install`（`src/config/installer.rs:57`）用 `Command::output()` 且**没有超时**：`pi install` 若卡在网络/交互提示上，该调用不会返回，`render_all` 的 `std::thread::scope` 会一直等 join（`src/config/render.rs:248`），`put_manifest` 的后台线程（`src/server/handlers.rs:639`）也就永远不发 `config.rendered` / `config.render_failed`。影响面：文档无法给出「渲染最长耗时」这条运维口径；外部只能靠进程级超时或重启，仓内没有任何熔断点。
- 🟡 `FakeInstaller::install` 在 profile 里已有**形状不符**的 JSON 时走 `expect` panic（`"settings 应为对象"` `src/config/installer.rs:85`、`"packages 应为数组"` `:89`、`"package.json 应为对象"` `:106`、`"dependencies 应为对象"` `:111`），不是 `Err`。影响面：真实 bridge 不会触发该路径，但任何把 `FakeInstaller` 用作注入桥的场景（服务测试 `src/server/tests/mod.rs:134`、e2e `tests/f6_observability/fixture.rs:31`）遇到畸形 profile 会以 panic 终止后台线程，用户侧只看到「渲染线程异常终止」（`src/server/handlers.rs:689`）——错误信息里没有原始 JSON 形状线索。
- 🔴 `PI_CODING_AGENT_DIR` 与包标识符的 `npm:` 前缀语义同样属于 pi 侧契约（本仓只负责透传），仓内无任何模块校验其有效性。影响面：pi 若改环境变量名/前缀规则，`PiInstallBridge` 会静默装到错误 profile 或执行失败，文档无法先行判定。

# 缺口记录 — unit-src-25（src/config/manifest.rs）

## GAP

- 🟡 `doc_contract_skill_is_shared_by_four_profiles`（`src/config/manifest_tests.rs`）的断言集只含 4 个 Agent（`coding-agent` / `coding-review` / `doc-fix` / `doc-review`），但 `builtin()` 实际有 **5** 个持有 `doc-contract`——`doc-agent`（`src/config/manifest.rs:341`）在断言之外。影响面：把 `doc-agent` 的 `doc-contract` 摘掉不会让任何测试变红，而 doc-agent 的提示词正依赖该 skill 的契约文本 → 该 Agent 的文档产出会静默失去契约约束；文档只能标注「实际 5 个持有 / 断言 4 个」这一差距，无法判断哪个是目标口径。
- 🔴 清单**没有 schema 版本字段**，也没有迁移代码：`ConfigManifest` 的字段集合（`src/config/manifest.rs:145`-156）一旦变化，旧 `<root>/manifest.json` 只能靠 serde 的 `#[serde(default)]` 兜（仅 `mcp` / `thinking_level` / `env` 三个字段带 default，见 `:49`/`:134`/`:139`），其余缺失即 `load`（`:476`-479）报错、运行起不来。影响面：升级后首次启动的兼容性无法从代码判断「能不能续用旧清单」，文档只能提示「删掉旧清单让它重生成」——这条不在代码里，属部署侧约定。
- 🟡 13 份提示词与机械门常量的**字面量耦合**（`PLAN_BLOCKS` / `VERDICT_MUST_ASK` / `VERDICT_SELF` / `VERDICT_ANSWER_FIELD`）只由单测保证（`src/config/manifest.rs:487`-541，标记引用 `:510`-512 与 `:528`-529）；提示词里还有大量未被锚点覆盖的散文约束（如 change-coding 的 `<impact-gap>` 字段格式），改提示词时没有机械检查兜底。影响面：文档只能引用「哪些锚点被钉住」，无法保证「没被钉住的约束仍生效」。
- 🟡 `doc-agent` 的模型与 plan tree 不一致：代码是 `minimax-m3.1-flash`（`src/config/manifest.rs:339`，断言消息称「自 2026-10-20 起」，`:693`-695`），而 P008 D018 写的是 `deepseek-flash`，仓内未见对应变更决策或回写记录（selfhost 记录称 10-21 起因渠道不可用改用 minimax，不在本仓）。影响面：文档只能以代码为准并标出落差；下次按 plan tree 复核会被当成回归，也无法判断「是决策未回写还是实现偏离决策」。
- 🟡 本模块测试**无法在整包不可编译时独立执行**：`src/config/manifest.rs` 的 15 个用例都是 lib 级（`cargo test --lib`），任何兄弟模块的编译错误都会让整轮 EXIT=101 且不进入测试（实测 2026-10-24 审查轮：ARC-R5 在途重构期间 `src/engine/policy.rs` 改名 + `map_intervention` 平移，13 处编译错误）。影响面：文档只能给出「先 `cargo test --lib --no-run` 验编译 / 用 `git archive HEAD` 干净树复跑」的操作口径，给不出「本单元自身红灯」的判据；审查侧若在另一路改动未落地时复现，看到的红与本单元无关。
- 🟡 **行号引用随源码外置整批漂移**（本单元按 HEAD `158b64f`、`src/config/manifest.rs` 948 行重核；该文件自 `19b3717` 未变）：`use` 三行实为 `src/config/manifest.rs:7`/`:9`/`:11`，`builtin()` 的局部提示词 `:180`-`:213`（旧稿记 `:736`-`:768`，那是 1159 行版本），`agents.len()` 断言 `:597`-`599`（旧稿 `:1153`-`:1157`）。影响面：提示词动辄上百行、`\` 续行密集，ARC-R4 把 13 份提示词外置后整个文件从 1504 → 1159 → 948 行，任何「先按段落写死行号再批量改提示词」的流程都会整体错位 → 文档里的 🟢 行号必须与源码同轮重核，不能沿用上一轮。

# 缺口记录 — unit-src-23（src/change/testing_gate.rs）

## GAP

- 🟡 `reconcile_acceptance`（`src/change/testing_gate.rs:77`）**无生产调用点**：本文件外只出现在 `src/change/mod.rs:43` 的转发与单测。运行时对账实际是两步——`session` 用 `parse_acceptance_table` 读表、逐条 `path_refs` 判引用可解析（`src/change/session.rs:536`-546），覆盖率由 `summarize_acceptance` 精确相等汇总（`src/change/report.rs:125`-136，调用点 `src/change/orchestrator.rs:309`、`:372`）。两套判据**宽严不同**：`reconcile_acceptance` 用双向 `contains`（`src/change/testing_gate.rs:85`-87），运行时用 `d == *c`（`src/change/report.rs:128`）。影响面：改 `reconcile_acceptance` 的宽松匹配不会改变任何运行时行为；反之运行时收紧/放宽也不会被它的 3 个单测（`src/change/testing_gate.rs:194` / `:204` / `:211`）发现——文档无法断言「运行时用的是哪一套」，只能并列陈述两处。
- 🟡 `reconcile_acceptance` 的「空 `test_ref`」分支（`src/change/testing_gate.rs:99`-110）**不可达且无测试**：`parse_acceptance_table`（`:66`-71）会先丢弃右半为空的行，因此该分支只能靠手工构造 `AcceptanceMatch` 才走到，12 个用例无一覆盖。影响面：该错误文案（「验收项没有指到测试 path:line」）若被改坏或删除，全套测试仍绿；文档只能照现状陈述，无法给出可重放证据证明该路径仍生效。
- 🟡 「回归优先」是**单向**判定：仅拦「日志红 + 自称通过」（`src/change/testing_gate.rs:34`-40）。反向「日志绿 + 自称不绿」不判矛盾、直接 `Ok(code)`，此时 agent 会白走一次回注轮次（`src/change/orchestrator.rs:333` 起）。影响面：文档只能说明「本门不覆盖该方向」，无法判断这是刻意口径还是遗漏（代码与文件头注释均未提及该方向）。
- 🟡 **回归日志的解析错误文案串到基线口径**：`check_regression_claim` 复用的 `parse_baseline_log` 内部固定传 `BASELINE_LOG` 与标签「基线」（`src/change/coding_gate.rs:104`-106），因此回归日志格式坏时，外层是「回归日志不可解析（要写 `exit_code: N`）」，内层仍是「基线证据缺退出码（evidence/baseline-test.log 里要写 `exit_code: N`）」。影响面：运维照错误文本去找基线证据，而真正坏的是回归日志；`src/change/coding_gate_tests.rs`-563 的用例只断言外层前缀，掩盖不了这层错指。

# 缺口记录 — unit-src-27（src/config/render.rs）

## GAP

- 🟡 `auth.json` **没有清理分支**：只在 `api_key` 为 `Some` 时写（`src/config/render.rs:136`-141），为 `None` 时既不写也不删，只打 `profile.render_without_api_key` WARN（`:143`-151）。而同一函数里 `mcp.json` 有「清空即删旧文件」的对称逻辑（`:157`-161）。影响面：`PUT /config/manifest` 把 `api_key` 置空后，旧 profile 目录里的明文 Key 仍留在 `auth.json` 里并被 pi 继续使用——文档只能写「无清理」，无法判断这是刻意保留（避免重复填 Key）还是遗漏；安全侧需要部署方自行删除该文件。
- 🟡 **两次并发 `render_all` 之间没有互斥**：`render_all` 只在「同一次调用内」用原子计数器保证每个索引只被一个 worker 处理（`src/config/render.rs:244`-259），没有跨调用的文件锁或目录锁；而 `PUT /config/manifest` 每次都起一个新的后台线程（`src/server/handlers.rs:639`），连续两次 PUT 会让两套 worker 同时写同一批 profile 文件。影响面：可能出现「A 写入的 settings.json 被 B 覆盖一半」「A 判定双记录完整时 B 正在写 `npm/package.json`」这类竞态，文档无法给出「渲染是否串行化」的运维口径。
- 🟡 **渲染不是事务，且 `skills/` 只增不减**：7 类文件逐个直接写（无临时文件 + rename，无失败回滚），安装失败时已写文件留在盘上（`src/config/render.rs:222`）；清单里移除某个 skill 后，profile 下旧的 `skills/<name>/` 目录不会被删除（`:104`-131 只做写/复制，无删除分支）。影响面：profile 目录可能与当前清单不一致（多出废弃 skill），而幂等判据只看 `packages` 与 `npm/package.json`（`:202`-203），不会因此触发修复性重渲染——文档只能提示「删掉 profile 目录让它重建」。
- 🟡 **仓内在途重构会瞬时打断编译**：本单元写作中途 `cargo test --lib config::render` 因 `src/sop/doc/mod.rs` 未声明 `pub mod prompts;` 而整体编译失败（12 个 E0432/E0433/E0308 错误，全在 `src/sop/doc/*` 与 `src/engine/doc_wiring.rs`），重构落地后复跑即绿（22 passed）。影响面：说明「文档门禁与代码门禁在重构窗口内会同时红」，本单元测试证据的时效性依赖重构收尾；记录在案以免后续把这类编译错误误判成本模块回归。

# 缺口记录 — unit-src-26（src/config/mod.rs）

## GAP

- 🟡 **config 门面的转发面取舍没有仓内书面依据**：`src/config/mod.rs:11`-13 只转发 8 个名字（`build_install_command` / `PackageInstaller` / `PiInstallBridge` / 清单四类型 / `Renderer`），而 `FakeInstaller`（`src/config/installer.rs:71`）、`skill_assets` 全部符号、manifest 的常量与方法都不转发。P002 的 decisions 目录（`docs/plantree/plans/002-config-management/decisions/`）里没有关于该取舍的记录，代码里也没有注释说明。影响面：文档只能陈述「事实上的转发面 + 单向依赖使它们没被转发」，无法判断「刻意收窄」还是「顺手写成」；后续往里加/减名字时，没有任何机械检查会变红（`config::` 的 47 个测试没有一条断言导入面）。

# 缺口记录 — unit-src-30（src/config/skill_assets.rs）

## GAP

- 🟡 **资产目录与 `include_str!` 登记没有机械对账**：`assets/skills/` 下新增文件若忘了在 `src/config/skill_assets.rs:29`-71` 加常量与清单项，该文件**静默不分发**（渲染器只遍历登记过的路径），6 个用例（`:125`/`:135`/`:149`/`:154`/`:176`/`:213`）只抽查若干文件名与关键串，不会变红；反向（改路径/删文件）由编译器兜住。影响面：文档无法断言「assets 目录内容 = 分发给 profile 的内容」，第三方按目录清单核对会多出未分发文件。
- 🟡 **python 资产目录带 21 个缓存产物**：`assets/skills/python-best-practices/` 下现存 `.mypy_cache/`（18 个文件，含 `.gitignore`、`CACHEDIR.TAG`、`cache.0`-`cache.15`）与 `.ruff_cache/`（3 个），均未被 `include_str!` 引用。影响面：不进入二进制，但随仓库分发、会污染「vendor 原样嵌入」的口径，且任何「目录文件数 = 分发文件数」的直觉核对都会差 21；`language_skills_vendor_coverage`（`src/config/skill_assets_tests.rs`）只验 3 个路径，发现不了。
- 🟡 **pi 的 skill 加载契约不可重放**：`coding_discipline_skill_is_registered_with_pi_loadable_frontmatter`（`src/config/skill_assets_tests.rs`）断言 frontmatter 以 `---` 开头、`description` 值非空、且不得含 `disable-model-invocation: true`，依据只是用例注释（「缺 description → skill 根本不加载」「该开关会排除出系统提示清单」）。`pi` 本体不在仓内，文档只能把这些当契约声明转述，无法给可重放命令证明加载行为。
- 🟡 **「vendor 原样嵌入」无法逐字验证**：注释 `src/config/skill_assets.rs:27`-28` 声明四语言 skill 是外部 vendor 原样入库、来源与许可登记在 `docs/plantree/plans/005-coding-stage/evidence/skills-sources.md`；仓内测试只验 `Rust Strict Standard` / `Go Strict Standard` 等关键串（`:213`-228`）。影响面：文档无法判断当前资产与上游版本是否一致，也无法在升级时判定「哪些改动是 vendor 带来的、哪些是本地改写」。

# 缺口记录 — unit-src-31（src/docgen/audit.rs）

## GAP

- 🟡 **两个同名 `DocLevel` 语义不同**：本文件的 `DocLevel` 有 `Essential/Complete/Detailed` 三档、`parse` 永不失败（未知值回落 `Complete`，`src/docgen/audit.rs:24`-30）；`src/change/gate0.rs:24`-39 的同名枚举是 `None/Essential/Complete`、`parse` 返回 `Result`（未知值报 `Config` 错误）。影响面：文档只能并列陈述两处；任何把门 0 的档位字符串直接喂给本文件（或反之）的代码都会静默得到不同档位——`detailed` 在门 0 会报错、在本文件才是 Detailed，而 `none` 在本文件会变成 Complete（全量要求）。
- 🟡 **非模块条目只做子串匹配，覆盖可能虚高**：`entry_covered` 对 `Symbol`/`Config`/`Data`/`Entry` 只要求任意文档正文 `contains(canonical_name)`（`src/docgen/audit.rs:227`）。影响面：①兄弟模块文档里提一次就算覆盖（「同一实体只在一篇里描述」的口径无人校验，只能靠提示级的 `duplicate_mentions` 兜）；②短名（如 `new`、`get`、`run`）会被任意散文误命中；③名字出现在代码示例/围栏块里也算。文档无法断言「覆盖 100% = 每条都有真落点」，只能说明判据本身。
- 🟡 **Runbook 抽取只认文件名**：`runbook_commands` 用 `file_stem` 是否含 `runbook` 过滤（`src/docgen/audit.rs:263`-271）。影响面：本仓多数模块文档把 Runbook 作为 `## 测试运行与环境` 小节内嵌在模块文档里（例如 `docs/contexts/src/config/render.rs.md`），这类命令块**抽不到**，审查官拿到的「可执行命令块」清单会缺项；除非每个模块另建 `*-runbook.md`。
- 🟡 **重复正文按物理行判定，函数注释称「段落」**：`duplicated_content` 的判据是「归一化后单行 ≥40 字符且跨 ≥2 篇」（`src/docgen/audit.rs:333`、`:361`-367）。影响面：软换行拆成多行的同一段解释抓不到；反之两篇文档里各自一行 40 字以上的**正常共享短句**（如免责声明）会被判重复 → 终审可能误杀。文档只能描述实现口径（行），不能沿用注释里的「段落」表述。
- 🟡 **三个函数签名是 `Result` 但实现不会返回 `Err`**：`duplicate_mentions`（`:298`）、`duplicated_content`（`:333`）、`collect_markdown`（`:231`）都容忍读失败——逐文件 `read_to_string` 失败静默跳过、目录不存在返回空集。影响面：调用方（`src/main.rs:66`-68、`src/sop/doc/wiring.rs:311`/`:321`）用 `if let Ok(...)` 或 `?` 都无法区分「没有重复」与「文件读不了所以没看到重复」；文档无法给出「审计结果不完整」的信号，只能提示排查文件权限。

# 缺口记录 — unit-src-33（src/docgen/impact.rs）

## GAP

- 🟡 **`ChangedFile.kind` 的实际取值比字段注释宽，且 `C`（复制）被归一成 `R`**：字段注释写 `A`/`M`/`D`/`R`（`src/docgen/impact.rs:16`），但 `parse_name_status` 原样保留 `T`（类型变化，`:69`-`:76`）、把 git 的 `C` 也记成 `'R'`（`:78`-`:87`）；`parse_porcelain` 只会产出 `A`/`M`/`D`/`R`。影响面：下游若按注释只匹配四种 kind 会漏掉 `T`；把复制当重命名会让 `old_path` 指向「源文件」而非「旧名字」，`vanished_names` 可能多算一个并未消失的名字（多算方向，与「宁可多算」一致，但语义不精确）。
- 🟡 **`changed_since` 按「新路径」去重，可能覆盖掉 diff 里的 `old_path`**：两条命令的结果按 `f.path` 放进同一个 map（`src/docgen/impact.rs:136`-`:140`），后写的 `status --porcelain` 条目会覆盖同路径的 diff 条目。影响面：已提交的重命名（`R old -> new`）若又在工作区被修改（`M new`），`old_path` 丢失 → `units_touching` / `vanished_names` 少看旧路径 → **漏算**（与文件头「宁可多算」相反）。仓内 6 个用例无一覆盖「已提交改名 + 未提交修改」的组合。
- 🟡 **删除文件的单元只在「整体零命中」时回退旧清单**：`impacted` 先按新清单求 `units`，仅当整体为空才改用旧清单（`src/docgen/impact.rs:302`-`:305`）。影响面：一次变更里既有活文件（新清单有）又有被删文件（新清单无）时，被删文件所属单元不会进入集合，除非它的名字恰好在 `reference_files` 里被引用；文档无法承诺「删除某模块 = 该单元必被重写」。
- 🟡 **单字符符号名被过滤，消失符号可能漏报**：`vanished_names` 只保留 `len() >= 2` 的名字（`src/docgen/impact.rs:192`）。影响面：`x`、`f` 这类短符号的删除不进 notes、不触发引用扫描；代码注释已承认「误命中率太高」，但没有替代判据（例如按类型/文件限定），文档只能标注为已知漏算面。
- 🟡 **汇编页按 unit token 命中的分支与真实 unit id 形态不匹配**：token = unit id 去掉 `unit-` 前缀后把 `-` 换成 `/`（`src/docgen/impact.rs:341`-`:344`），本仓 id 形如 `unit-src-30` → `src/30`，而汇编页写的是模块路径（`src/config/render.rs`）或 id 原文（`unit-src-30`），两者都不含 `src/30`。影响面：`assembly_review` 实际只靠「消失符号名」命中；单元被改而名字未消失时，README/架构页不会被要求复核，与 D016「叶子一变汇编页可能陈旧」的意图存在落差。用例 `:475` 用的是自造的 `core/auth` 形态，掩盖了这一形态差。
- 🟡 **引用扫描只覆盖 23 种源码扩展名**：`is_source_ext`（`src/docgen/impact.rs:285`-`:291`）不含 md/toml/yaml/json/sql 等。影响面：「文档、配置、SQL 里还引用旧名字」不算引用方，汇编页之外的引用漏扫；`SKIP_DIRS` 又把 `docs` 整目录排除，故文档引用永远不参与。文档只能说明白名单，无法给出完整性保证。

# 缺口记录 — unit-src-34（src/docgen/inventory/ 七个文件）

## GAP

- 🟡 **`assets` 被列进扫描排除表，是仓库特定取舍，会变成覆盖盲区**：`SKIP_DIRS` 含 `assets`（`src/docgen/inventory/scanner.rs:156`-`:171`，注释 `:157`-`:159` 说明它是随技能发布的 vendored 模板），而 `closure::check_closures` 的分母也来自同一个 `scan_source_files`（`src/docgen/inventory/closure.rs:10`）。影响面：本仓「文件覆盖闭包」结构上不可能报出 `assets/` 漏登记；若仓库哪天把产品代码放进 `assets/`，清单与闭包会一起看不见它（分母与校验同源，天然无法自证）。
- 🟡 **`public_symbols` 漏 `pub(crate)` / `pub(super)` 与跨行 `pub`**：Rust 分支要求行首 `pub ` 后紧跟 `async`/`unsafe`/`fn`/`struct`/`enum`/`trait`/`const`/`mod`（`src/docgen/inventory/scanner.rs:321`-`:334`），`pub(crate) fn` 的 `rest` 以 `(crate)` 开头 → 不匹配。影响面：这些符号不进清单，符号覆盖闭包（`closure.rs:34`）也就不会要求它们有文档；反向地，条目里少了它们会让「公开面清单」不完整。
- 🟡 **`is_entry_file` 白名单窄且不可配置**：只认 11 个文件名 + `cmd/`、`bin/`、`/bin/` 前后缀（`src/docgen/inventory/scanner.rs:226`-`:246`）。影响面：`wsgi.py`、`asgi.py`、`examples/`、`benches/`、`src/bin/*`（不含 `bin/` 前缀路径时）这类真实入口不会被标 `Entry`，进而入口可达闭包会把从它们出发的文件全判「不可达」（或反之：没有 `Entry` 时整仓每文件报一条）。
- 🟡 **`scan_source_files` 与 `scan_support_files` 的 `Result` 永不 `Err`**：`read_dir` 失败一律 `continue`（`src/docgen/inventory/scanner.rs:181`-`:184`、`src/docgen/inventory/support.rs:155`-`:157`）。影响面：权限不足的目录与「空目录」在建清单时表现相同——清单会静默缺少整棵子树，闭包 ① 也拿同一份扫描结果比对，因此**发现不了**「有目录读不动」。文档无法承诺「清单 = 磁盘全集」。
- 🟡 **`sql_objects` 返回小写名**：匹配前整行 `to_ascii_lowercase()`（`src/docgen/inventory/support.rs:51`），名字从 `lower` 里切（`:58`-`:63`）。影响面：SQL 层条目名与 DDL 原文大小写不一致（`CREATE TABLE Users` → 条目名 `users`），而文档里常按原样写表名；同一张表在「清单覆盖」和「文档正文」两边字符串对不上，覆盖审计按 `contains(canonical_name)` 判定就会误判缺覆盖。
- 🟡 **`package_scripts` 的行号靠字符串搜索，可能指错行**：键的行号 = 全文第一处包含 `"键名"` 的行（`src/docgen/inventory/support.rs:107`-`:111`），找不到给 `1`。影响面：脚本名与依赖名同名（如 `"build"` 同时出现在 `scripts` 与 `dependencies`）时，条目行的 `path:line` 指向依赖块，审查官按指针跳过去会看到无关内容。
- 🟡 **`resolve_imports` 只有 Python 分支有测试，另三条分支的保真度无证据**：可达性用例全部用 Python 夹具（`src/docgen/inventory/tests.rs:103`-`:137`）。影响面：TS/JS、Go、Rust 三条解析规则（`src/docgen/inventory/imports.rs:68`/`:86`/`:99`）若有偏差，只能表现为「入口可达闭包误报不可达」，而仓内没有红用例兜底。附带两处已知不精确：Go 按「末段同名文件」全仓找且遍历序不确定（`imports.rs:159`-`:173`，无排序）；Rust 只认 `mod name;`，不认 `#[path]` 与内联 `mod x { }`（`imports.rs:116`-`:121`）。
- 🟡 **`unit_id` 复用会把一个 chunk 里两种既有单元号静默合并**：每个切片取**第一个**非空 `unit_id` 当整片的号（`src/docgen/inventory/scanner.rs:372`-`:376`）。影响面：手改过 `inventory.json`（或跨版本重排后）同一顶层目录的条目带不同单元号时，重跑 `assign_units` 会把它们统一成其中某一个 → 单元数量与审查范围变化，而 `unit_numbering_is_stable_across_runs`（`tests.rs:48`）只覆盖「同一批再算一次」的幂等，不覆盖这种混合输入。
- 🟡 **`codegraph_indexed` 只是目录存在性**：`root.join(".codegraph").is_dir()`（`src/docgen/inventory/scanner.rs:143`）。影响面：清单元信息不能当作「codegraph 索引已建且新鲜」的证据；审查侧若据此决定跳过某些扫描，会用到陈旧索引。
- 🟡 **`save` 非原子且无并发保护**：先 `create_dir_all` 再 `serde_json::to_string_pretty` 整文件覆写（`src/docgen/inventory/data.rs:149`-`:156`），无临时文件 + rename、无锁。影响面：进程在写中途被 kill 会留下截断的 `inventory.json`，下一次增量模式 `plan_impact` 读到它只会得到 serde 错误（`src/docgen/impact.rs:361`-`:363`），无法区分「文件坏」与「从没生成过」。是否有上层单写者保证，代码里看不出来。
- 🟡 **`.env` 侧与代码侧的键字符白名单不一致**：`env_file_keys` 只收 `[A-Za-z0-9_.]`（`src/docgen/inventory/support.rs:36`-`:40`），`keys_after`（代码侧）还接受 `-`（`:136`）。影响面：同一个带连字符的键，从代码里能抽到、从 `.env.example` 里抽不到（或反之在条目里出现两种写法），清单里会出现同名异写。

# 缺口记录 — unit-src-08（src/change/gate0.rs + src/change/history.rs）

## GAP

- 🟡 **往返历史的表格单元格不做 markdown 转义**：`append_roundtrip` 用文本格式化把 file / reason / evidence 直接拼进 `| … |` 单元格（`src/change/history.rs:34` 至 `:38`），不处理竖线、换行、反引号。影响面：越界理由里带 `|`（例如「A | B 两条路」）会把一行拆成错位的列，人读时列对不上；即便只看三例测试也发现不了（测试用的理由都是「接口在这」，无特殊字符）。文档只能披露该限制，无法承诺表格始终可解析。
- 🟡 **读历史文件失败被静默降级为空串**：`std::fs::read_to_string(&path).unwrap_or_default()`（`src/change/history.rs:29`）把「权限不足 / 文件被占用 / 非法 UTF-8」与「文件真的是空的」合并成同一路径。影响面：读失败时既不补表头也不报错，写回时**覆盖**原内容 → 已有历史可能被悄悄清空，而调用方只看到 `Ok(路径)`。编排层把它当纯追加用（`src/change/orchestrator.rs:246`），没有回读校验。
- 🟡 **同一往返重复调用会重复落行**：`append_roundtrip` 无去重与幂等标记（`src/change/history.rs:33` 至 `:38`）。影响面：崩溃恢复后重放同一往返（状态里的 roundtrips 未持久化成功时）会得到重复行，事后策展无法区分「真的回退了两次」与「记录写了两次」。

# 缺口记录 — unit-src-12（src/change/plan_audit.rs）

## GAP

- 🟡 **`path:line` 只校验路径存在，不校验行号是否在文件范围内**：`audit` 对每条引用只做 `repo.join(&path).exists()`（`src/change/plan_audit.rs:276` 至 `:282`），行号 `u32` 解析后不再使用；而 doc-lint 侧的同类检查会做 `path_line_in_range`。影响面：计划可以写 `src/change/gate0_tests.rs` 通过机械体检，审查官只看「路径存在」就会被误导；文档不能承诺「体检通过 = 引用可信」。
- 🟡 **「像文件路径」的扩展名白名单偏窄（12 种）**：`looks_like_file` 只认 rs/py/ts/tsx/js/go/java/kt/md/toml/json/yaml（`src/change/plan_audit.rs:373` 至 `:384`）。影响面：白名单里写 `scripts/build.sh`、`config/app.yml`、`migrations/001.sql`、`src/lib.h` 时**抽不到** → 该文件不计入白名单（不判存在）也不参与「要改的 ⊆ 白名单」，一次真实的越界改动可能被判干净；反之「白名单为空」也可能因为只写了非白名单扩展名而误报。
- 🟡 **六块齐全用子串包含判断**：`plan.contains(block)`（`src/change/plan_audit.rs:265` 至 `:268`）。影响面：正文任何位置出现「风险」「白名单」等词（例如「本次无风险」写在需求复述里）就算该块齐；不判标题层级、不判顺序、不判块内容为空 → 结构缺失只能靠审查官那条「结构缺失」判据兜。
- 🟡 **`section_lines` 用标题「含子串」切块，且不区分标题层级**：判据是 `line.contains(title)`（`src/change/plan_audit.rs:346` 至 `:356`）。影响面：①「白名单」出现在「风险」小节的标题里会把块提前切走；②小标题（`###`）与一级块（`##`）同等对待，一个 `### 验收标准补充` 会覆盖前面真正的验收块（后出现者胜）→ 抽取到的条款可能不是主表。
- 🟡 **`PATH_MIGRATIONS` 是手工登记，与 doc-lint 的路径解析各自维护**：迁移表只有 7 条（`src/change/plan_audit.rs:28` 至 `:36`），新增仓库手术（再搬一次文件）时若忘了登记，两处口径立刻分叉：计划里写旧路径会被 `audit` 判「不存在」，而写新路径的步提交扫描又对不上。影响面：文档只能声明「这是单点事实源，必须人工维护」，无法给出完整性保证。
- 🟡 **`VerdictAudit` 的 `facts` 实际永不为空，`render_facts` 的空分支不可达**：每次调用必 push 一条统计 fact（`src/change/plan_audit.rs:138` 至 `:142`），歧义块缺失时也先 push 一条再返回（`:100` 至 `:106`）。影响面：`render_facts` 里「（无）」的分支（`src/change/plan_audit.rs:79` 至 `:81`）是死代码，审查指令的「裁决门事实」段永远有内容——读者别把「有内容」当「有问题」。

# 缺口记录 — unit-src-15（src/change/prepare.rs）

## GAP

- 🟡 **`ChangePrep.doc_action` 的 `FullChainRequired` 在返回值里不可达**：`complete` 档在 `docs::apply` 之后立即转成 `Quality` 错误并返回（`src/change/prepare.rs:58` 至 `:62`），所以成功返回的结构体里该字段只可能是 `Skipped` 或 `EssentialDone`。影响面：调用方若照 `DocAction` 的三变体穷举写分支，第三个分支永远走不到（死逻辑）；反过来若按「拿到 `FullChainRequired` 再自己跑文档」设计，永远拿不到信号——必须靠 `Err` 分支识别。
- 🟡 **失败前已有部分副作用，且顺序不可回滚**：`prepare` 先建计划目录（`src/change/prepare.rs:52` 至 `:54`），再跑文档动作（`:57`），再建基线（`:65`），最后建黑板骨架（`:67` 至 `:73`）。影响面：①`complete` 档被拒时，目标仓里已经留下空的 `docs/plantree/changes/<change-id>/` 目录；②`essential` 档写完清单后若基线失败（如 git 不可用），文档清单已在仓内、基线却缺失，重跑会再次写清单；③黑板骨架失败时基线提交已产生，「门 0 失败」并不等于「仓未被改动」。
- 🟡 **`today` 三元组不做范围校验**：`date_segment` 只做 `{:04}{:02}{:02}` 拼接（`src/change/change_id.rs:87` 至 `:89`），`month=13`、`day=32` 都能生成 id 段。影响面：错误日期会静默进入 change-id 与计划目录名（`change-20261332-…`），后续按日期排序/归档的流程拿到的是伪日期；测试里固定 `(2026,10,20)` 也说明该参数是外部注入的，但注入点（`today_parts()`）未见校验。
- 🟡 **`assign_change_id` 返回的黑板 id 目录被丢弃后成为空目录**：`prepare` 用 `_id_dir` 接下第二个返回值（`src/change/prepare.rs:49`），而该目录由 `assign_change_id` 建出（`src/change/change_id.rs:77` 至 `:78`）；仓内所有黑板落点都是 `<blackboard>/evidence`、`<blackboard>/testing/...` 这类直接子路径，没有 `<blackboard>/<change-id>` 的消费点。影响面：每次门 0 都会在黑板里留下一个空 id 目录，事后按目录清点变更容易误判「该 change 有产物」。
- 🟡 **同任务并发调 `prepare` 无锁**：幂等依赖「marker 文件存在 → 复用 id」与「`baseline.txt` 非空 → 复用 sha」两次读判（`src/change/change_id.rs:62` 至 `:73`、`src/change/gate0.rs:73` 至 `:82`），中间没有文件锁或原子创建。影响面：并行触发同一 task 时可能双写 `current`、重复 `git init`/基线提交（后写者覆盖 marker），导致两个 change-id 指向同一计划目录；现有 7 个用例都是串行调用，覆盖不到该竞态。

# 缺口记录 — unit-src-17（src/change/session.rs + src/change/session/session_tests/{mod,plan}.rs）

## GAP

- 🟡 **跑测试有两个环境前提（工具链 / `TMPDIR`），任一不满足都全红且与代码无关**：本机 rustup 无默认工具链，裸跑 `cargo test --lib change::session` 报「could not choose a version of cargo to run」，必须显式 `RUSTUP_HOME=~/.rustup RUSTUP_TOOLCHAIN=stable`（`Cargo.toml` 为 `edition = "2024"` ⇒ Rust 1.85+）；另外 `fixture` 用 `tempfile::tempdir()`（`src/change/session/session_tests/mod.rs:90`）按 `TMPDIR` 建目录，若把 `TMPDIR` 指到不存在的路径，依赖夹具的 32 例全部 panic（`PathError { kind: NotFound }`）。实测：补齐 `RUSTUP_TOOLCHAIN=stable`、`TMPDIR=/var/tmp/u17-tmp`（先 `mkdir -p`）后 `cargo test --lib change::session` → 34 passed; 0 failed; 579 filtered out; EXIT=0，`...::session_tests::plan` → 15 passed; 0 failed; 598 filtered out; EXIT=0。影响面：文档只能把这两个前提写进 Runbook；读者在别的环境复现时看到的红不构成「本模块回归」的证据。
- 🟡 **空箱追补会丢掉原始开工语**：空箱被拒收后把下一条消息整体换成 `ISSUES_NUDGE`（`src/change/session.rs:515`），不再携带 `testing_kickoff` 的正文。影响面：模型在第二轮失去「跑回归 → 写测试 → 对账表」的原始顺序提示，只能凭上一轮记忆补明细；测试只断言消息条数与话术里含「请补齐失败明细后重发」（`src/change/session/session_tests/plan.rs:293`），覆盖不到「上下文丢失导致补错明细」。
- 🟡 **`issues_of` 的解析口径很宽**：只取第一个 `<issues>` 到 `</issues>` 之间（无闭合则到串尾），不识别代码围栏，且任何非空行都算一条明细（只剥行首 `-`/`*` 前缀）——`src/change/session.rs:238` 至 `:252`。影响面：回复里若在围栏代码块中举例 `<issues>`，或把说明性散文写进块内，都会被当作失败明细回注给 coding 轮；引擎侧不校验每条是否带 `path:line`。
- 🟡 **「自证证据覆盖本轮 diff」只是提示词要求，机械门不验**：`coding_kickoff` 要求「覆盖写、不许沿用上一轮残留」（`src/change/session.rs:153`），但门只验文件存在、可解析、退出码为 0（`src/change/coding_gate.rs:135` 至 `:156`）。影响面：沿用上一轮残留的绿色证据只要「覆盖写」过就能过门；文档不能承诺「过自证门 = 证据与本轮 diff 相关」。
- 🟡 **coding / testing 审查判负与 plan 判负的形状不一致**：plan 侧返回 `PlanTurn::Fail{issues}` 交编排层走修复回路（`src/change/session.rs:322` 至 `:327`），coding 与 testing 侧直接抛 `DevforgeError::Quality`，明细只以「；」拼进错误文本（`:488` 至 `:491`、`:558` 至 `:561`）。影响面：上层若想拿结构化 issues 做回注，只能解析字符串；两条路径的错误处理无法共用一套。
- 🟡 **审查轮次只有 plan 段落成字段**：`plan_review_instances` 由 plan 审查写回（`src/change/session.rs:321`），coding / testing 的实例数只随 turn 返回值带出（`:486`、`:556`）。影响面：调用方若只读 `plan_reviews()` 会得到「只有 plan 轮次」的偏口径；报告侧必须从 turn 里取另外两段轮次。
- 🟡 **计划文件「必须落盘」是隐含前提**：`run_plan_with` 在标记齐了之后立刻按路径读计划文件，读不到就判 `Quality`（`src/change/session.rs:298` 至 `:300`），而「agent 是否真把计划写进目标仓该路径」只由提示词约束，仓内无第二处校验。影响面：模型若只把计划贴在回复里，本轮直接判质量失败且没有修复回路（不是 `PlanTurn::Fail`），用户侧看到的是引擎错误而非「计划不合格」。
- 🔴 **空箱计数的语义边界无法从代码判定**：`MAX_EMPTY_ISSUES_NUDGES = 3` 时实际允许「连续 4 次空箱」才失败（`nudges > 3`，`src/change/session.rs:510` 至 `:514`），常量名与生效次数差一；同时 plan / coding 两种路由共用同一计数与同一话术，是否有意为之（例如认为「换路由不算新情况」）在代码与注释里都看不出来。影响面：文档只能如实写「常量 3、实际第 4 次报错」，无法给出设计意图。
- 🟡 **`evidence_exit_code` 与 `parse_baseline_log` 的失败被压平成 `None`**：读不到/解析不了时不区分「文件不存在」「格式坏」「编码坏」（`src/change/session.rs:204` 至 `:207`），红基线里程碑因此可能少一个字段。影响面：事后按 `exit_code` 字段过滤日志时，「没有该字段」既可能是解析失败也可能是文件缺失，无法区分。
- 🟡 **跨模块行号引用是快照，且已漂过一次**：装配层 `src/server/change_engine.rs` 的 `ProdChangeSession {` 与 `fn build_ctx` 在本单元文档首稿（2026-10-24 19:50）时按 `:180` / `:246` 记录，现树（HEAD `afe2c64`）已漂到 `:228` / `:307`（文件 mtime 19:58，晚于首稿）；本轮已按现树改正，但该文件在 P012 M1 工作区改造窗口内仍在变。影响面：`devforge doc-lint` 只验行「存在」不验语义错位（`:180` / `:246` 当时都是合法行），这类漂移只能靠人工复核或按符号名（`ProdChangeSession {` / `fn build_ctx`）定位。

# 缺口记录 — unit-src-19（src/change/session/session_tests/runtime.rs）

## GAP

- 🟡 **路由分支完全跳过证据链**：`testing_route_coding_returns_failures`（`src/change/session/session_tests/runtime.rs:316`）用 `fixture(true)` 却**不写**回归日志与验收对账表，仍能拿到 `TestingTurn::RouteCoding`——因为 `run_testing` 一旦命中 `<route>coding|plan</route>` 就提前返回（`src/change/session.rs:520` 至 `:525`），`check_regression_log` 与对账表解析都在其后。影响面：测试阶段「先声称回归跑了」的纪律对回注路径不成立；agent 可以完全不给回归证据、只发一条路由 + 一条 issues 就触发回注，文档不能承诺「回注必然带证据」。
- 🟡 **超限边界只有「第 4 次」一侧**：两个通道的用例都喂 4 条脚本（`src/change/session/session_tests/runtime.rs:469`、`:521`），断言超限成立；但**没有**「恰好 3 次不超限」的对照用例。影响面：若把上限常量从 3 改成 2（或把 `>` 改成 `>=`），现有用例里只有「4 条必超限」仍会通过——边界语义（3 次仍重试 / 第 4 次才终止）在测试层面没有被钉死，只能靠读 `src/sop/review_gate.rs:16` 与 `:19` 的常量与 `>` 比较。
- 🟡 **里程碑断言绑定 tracing 字段的字面形态**：`coding_gates_and_review_open_are_logged_as_milestones`（`src/change/session/session_tests/runtime.rs:44`）按 `"passed":true`、`"steps":1`、`"role":"change-coding-review"`、`"round":1`、`"target":"devforge::change::session"` 做子串断言，且要求 `"event":` 行过 `assert_milestone_shape`。影响面：字段改名、改序或把 `tracing::info!` 挪到别的模块文件（target 变化）都会让用例变红，即使对外语义没变——读者看到红不能直接推「门坏了」。
- 🟡 **断言工具的跨模块行号随 `src/observability/mod.rs` 在途改动漂移**：`test_support` 模块声明与两个断言函数在本单元文档首稿（2026-10-24 19:37）时是 `:381` / `:423` / `:437`，同文件 19:43 改动后现树为 `:422` / `:464` / `:478`（本轮已按现树改正）。影响面：`doc-lint` 只验行号「存在」，旧行号当时也都合法 → 这类漂移抓不到；读者应按符号名（`pub mod test_support` / `capture_jsonl` / `assert_milestone_shape`）定位，不依赖行号。
- 🟡 **`PLAN_PASS` 常量被三个角色共用**：通过样例常量定义在夹具里叫 `PLAN_PASS`（`src/change/session/session_tests/mod.rs:68`），却同时用于 coding 审查与 test 审查的通过回复（`src/change/session/session_tests/runtime.rs:19`、`:151`、`:303` 等）。影响面：命名暗示「只用于 plan」，实际是通用审查通过样例；文档若照名字解释会误导，且将来若三种角色的通过协议分化，这个常量会被拆开而文档的「同一份协议」说法需要跟着改。
- 🟡 **清单与本文件用例数差一**：`inventory.json` 里本单元登记 18 条 behavior，文件实有 19 个 `#[test]`——首个用例 `coding_runs_gates_and_returns_changed_files`（`src/change/session/session_tests/runtime.rs:7`）未进清单。影响面：按清单审计覆盖会把它当「不存在」，按清单统计「本单元测试数」会少 1；本文档已按文件实况列 19 行，清单未改（无标记工具）。
- 🔴 **空回复与「早退」的区分只在文案里**：`review_gate` 把「空回复」归入实例失败（`src/sop/review_gate.rs:118` 至 `:127`），本文件只断言超限后的错误文案含「基础设施故障」（`src/change/session/session_tests/runtime.rs:521`）。至于「进程早退但输出了一半协议块」算哪条通道，仓内既无用例也无注释可查——无法从测试与代码判断该情形的归属。
