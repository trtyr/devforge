---
type: module
module_path: src/config/skill_assets.rs
unit: unit-src-30
description: 内置 skill 资产——7 个 skill 的正文在编译期用 include_str! 嵌入，按 skill 名返回「相对路径 → 内容」清单
---

# src/config/skill_assets.rs

配置渲染的**内容兜底源**（P2-ISSUE-05）：渲染器在 `skills_source` 未提供时，从这里拿 skill 真正的正文写进 `<profile>/skills/<name>/`，而不是只建占位 `README.md`。文件里没有任何运行时逻辑——全部内容在**编译期**由 `include_str!` 嵌进二进制，两个公开函数只做「skill 名 → 文件清单」的查表。

模块面：`src/config/mod.rs:9` 声明 `pub mod skill_assets;`，但**不在** `src/config/mod.rs:11`-13 的三条 `pub use` 里——config 模块面看不到本文件任何名字，唯一消费者 `render` 走子模块全路径导入（`src/config/render.rs:10`）。

## 职责

**负责**：
- 内嵌 3 个自有 skill 的正文常量：`PLAN_TREE_SKILL_MD`（`:7`）、`DOC_CONTRACT_SKILL_MD`（`:10`）、`CODING_DISCIPLINE_SKILL_MD`（`:13`-14`）。
- 内嵌 4 个语言 skill 的资产：40 个私有 `(&str, &str)` 元组（`:29`-71`）——路径 → 正文，覆盖 `rust-best-practices`（1 文件）、`go-best-practices`（1）、`ts-js-best-practices`（2）、`python-best-practices`（36）。
- 查表：`builtin_skill_files`（`:18`）是渲染器用的统一入口；`builtin_language_files`（`:74`）是语言 skill 子表，`builtin_skill_files` 的 `other` 分支原样转给它（`:23`）。

**不负责**：不写文件、不建目录（落盘在 `src/config/render.rs:101`-134）；不判断该给哪个 Agent 装哪些 skill（那是 `AgentManifest` 的 skills 字段的事，见 [manifest.rs.md](./manifest.rs.md)）；不做 skill 版本管理或校验（无 schema、无哈希）；不复制外部源目录（`skills_source` 分支走 `render` 的 `copy_dir`）。

## 公开成员

| 成员 | 类别 | 文档 |
|---|---|---|
| `PLAN_TREE_SKILL_MD` | 常量 | [PLAN_TREE_SKILL_MD](#plan_tree_skill_md) |
| `DOC_CONTRACT_SKILL_MD` | 常量 | [DOC_CONTRACT_SKILL_MD](#doc_contract_skill_md) |
| `CODING_DISCIPLINE_SKILL_MD` | 常量 | [CODING_DISCIPLINE_SKILL_MD](#coding_discipline_skill_md) |
| `builtin_skill_files` | 函数 | [builtin_skill_files](#builtin_skill_files) |
| `builtin_language_files` | 函数 | [builtin_language_files](#builtin_language_files) |

### 内部条目（不在模块面）

| 条目 | 行 | 说明 |
|---|---|---|
| 40 个 `(&str, &str)` 私有常量 | `src/config/skill_assets.rs:29`-71` | 每个 =（相对 skill 目录的路径，`include_str!` 正文）；命名规则 `<SKILLNAME>_<序号>`（如 `PYTHONBESTPRACTICES_17`），见 [资产清单](#资产清单分发给-profile-的内容) |

## 依赖

- **上游（谁调用）**：只有 `src/config/render.rs`——`:10` `use crate::config::skill_assets::builtin_skill_files;`，`:112` 在 `skills_source = None` 分支调用。全仓无其它引用（`grep -rn builtin_skill_files` 只命中这两处）。
- **下游**：本文件不 `use` crate 内任何模块，只用标准库类型（`Option`、`Vec`、`&'static str`）；`include_str!` 是编译器内建宏。
- **外部（编译期）**：`assets/skills/` 下 7 个目录共 43 处 `include_str!`——文件缺失或路径写错即**编译失败**，不会运行时降级。
- **来源 / 许可登记**：`docs/plantree/plans/005-coding-stage/evidence/skills-sources.md`（版本说明见 `src/config/skill_assets.rs:27`-28` 注释；文件存在）。

## 核心口径（解释层，逐条带置信度）

- 🟢 **内容在编译期进二进制**：43 处 `include_str!`，全部是 `pub` 常量或私有元组；运行时不读 `assets/`。可重放：`grep -c 'include_str!' src/config/skill_assets.rs` → 43。🟢 CONFIRMED `src/config/skill_assets.rs:7`、`:10`、`:13`-14`、`:29`-71`
- 🟢 **查表失败的语义是「回落占位」，不是报错**：`builtin_skill_files` 未命中 → `builtin_language_files` → `None`；渲染侧拿到 `None` 建占位目录 + `README.md`（`src/config/render.rs:124`-130）。所以清单里写错 skill 名不会让渲染失败，只会得到一个空壳 skill。🟢 CONFIRMED `src/config/skill_assets.rs:18`-24、`src/config/render.rs:112`-130
- 🟢 **两个函数的关系是「转发 + 子表」**：`builtin_skill_files` 先匹配 3 个自有 skill，其余**全部**转给 `builtin_language_files`；因此语言 skill 的增删只需改子表，但自有 skill 名若与语言 skill 重名，会命中自有分支（当前无重名）。🟢 CONFIRMED `src/config/skill_assets.rs:23`
- 🟢 **返回的路径是「相对 skill 目录」而不是仓库路径**：渲染器把它拼到 `<profile>/skills/<skill>/<相对路径>` 并逐段建父目录（`src/config/render.rs:116`-121）。所以 assets/templates/src_pkg/… 这类带子目录的条目会在 profile 里还原成同构目录树。🟢 CONFIRMED `src/config/skill_assets.rs:37`-70、`src/config/render.rs:116`-121
- 🟡 **pi 的 skill 加载契约（frontmatter 必需）**：用例要求正文以 `---` 开头、含 name 字段、`coding-discipline` 的 description 非空，并显式禁止 disable-model-invocation: true（`:154`-173）。依据是测试注释与断言本身（「缺 description → skill 根本不加载」「该开关会把它排除出系统提示的技能清单」）；pi 本体不在仓内，无法重放加载行为。🟡 INFERRED
- 🟡 **「vendor 原样嵌入」**：注释 `:27`-28` 声明四语言 skill 是外部 vendor 原样入库，源代码与许可证登记在 P005 evidence 文件里。仓库内能验证的是「文件在 + 关键串在」（`:213`-228`），无法验证与上游版本逐字一致。🟡 INFERRED
- 🔴 **资产目录与代码登记没有机械对账**：没有任何用例遍历 `assets/skills/` 与 `include_str!` 清单做双向比对（现有用例只抽查若干文件名/关键串）；往资产目录加文件若忘了登记，不会变红。影响面见 `_meta/gaps.md`。

## 资产清单（分发给 profile 的内容）

| skill 名 | 条数 | 相对路径 | 正文特征（测试断言） |
|---|---|---|---|
| `plan-tree` | 1 | SKILL.md | 以 `---` 开头、含 `# plan-tree`、「目录约定」 |
| `doc-contract` | 1 | SKILL.md | 含 `name: doc-contract`、必填节规则、`N/A` 规则、反模式表、path:line、Runbook 七件套 |
| `coding-discipline` | 1 | SKILL.md | `name: coding-discipline`、12 条纪律锚点、`- **反例**：`×12、`- **证据形态**：`×12、来源 cursor/plugins/pstack / `MIT` / `Lauren Tan` |
| `rust-best-practices` | 1 | SKILL.md | 含 `Rust Strict Standard` |
| `go-best-practices` | 1 | SKILL.md | 含 `Go Strict Standard` |
| `ts-js-best-practices` | 2 | SKILL.md、javascript-strict.md | 正文以 `---` 开头（rust 项有断言；ts 只断言条数） |
| `python-best-practices` | 36 | 见下 | 含 SKILL.md、references/standard.md、`scripts/` 前缀项 |

`python-best-practices` 的 36 项（`src/config/skill_assets.rs:36`-71`；下表按目录分组展示，与源码里常量序号顺序不同，清单内容已逐项校对一致）：

```text
SKILL.md
ci.sh
references/standard.md
scripts/check_appendix_sync.py
scripts/check_conformance.py
scripts/scaffold.py
assets/templates/.env.example
assets/templates/.gitignore
assets/templates/.project-root
assets/templates/CLAUDE.md
assets/templates/Makefile
assets/templates/README.md
assets/templates/ci.sh
assets/templates/configs/settings.yaml
assets/templates/docs/adr/0001-record-architecture-decisions.md
assets/templates/pyproject.toml
assets/templates/scripts/check_drift.py
assets/templates/src_pkg/__init__.py
assets/templates/src_pkg/adapters/__init__.py
assets/templates/src_pkg/adapters/mock.py
assets/templates/src_pkg/adapters/openai_llm.py
assets/templates/src_pkg/cli.py
assets/templates/src_pkg/core/__init__.py
assets/templates/src_pkg/core/logging.py
assets/templates/src_pkg/core/prompts.py
assets/templates/src_pkg/core/settings.py
assets/templates/src_pkg/ports/__init__.py
assets/templates/src_pkg/ports/exceptions.py
assets/templates/src_pkg/ports/factory.py
assets/templates/src_pkg/ports/protocols.py
assets/templates/src_pkg/prompts/rag/answer.md
assets/templates/src_pkg/schemas/__init__.py
assets/templates/tests/conftest.py
assets/templates/tests/test_conformance.py
assets/templates/tests/test_settings.py
assets/templates/tests/test_smoke.py
```

- **生命周期**：只读。写盘由渲染器负责（每次渲染覆盖写；清单移除某 skill 时旧目录不删，见 [render.rs.md](./render.rs.md)）；没有任何清理或卸载路径。
- **索引 / 唯一约束**：`N/A`（内存查表 + 编译期常量，无存储层）。
- **迁移注意**：无版本字段、无哈希；替换某个 skill 的正文 = 改 `assets/skills/` 下的文件后重编译（二进制内嵌内容随之变化）。**磁盘上多出未登记文件不会被分发**（例如 `assets/skills/python-best-practices/` 里现存的 21 个缓存产物（.mypy_cache 18 个 + .ruff_cache 3 个））——见 `_meta/gaps.md`。

---

### PLAN_TREE_SKILL_MD

- 名称 / 类型：`pub const PLAN_TREE_SKILL_MD: &str`（🟢 CONFIRMED `src/config/skill_assets.rs:7`）
- 默认值 / 取值范围：由 include_str! 指向 assets/skills/plan-tree/SKILL.md，编译期嵌入，取值固定为仓库内该文件全文；正文以 `---` 开头（frontmatter）、含 `# plan-tree` 与「目录约定」。
- 是否必填：常量无「必填」语义；是否分发取决于清单里有没有 skill 名 `plan-tree`（`builtin_skill_files` 命中后由渲染器写入 profile）。
- 影响：改它 = 改所有拿到 `plan-tree` 的 Agent 读到的规范正文；`plan_tree_skill_has_frontmatter_and_body`（`:125`）会把 frontmatter / 标题 / 「目录约定」三处钉住。
- 示例：`builtin_skill_files("plan-tree")` → `Some(vec![("SKILL.md", PLAN_TREE_SKILL_MD)])`（`:20`）。

### DOC_CONTRACT_SKILL_MD

- 名称 / 类型：`pub const DOC_CONTRACT_SKILL_MD: &str`（🟢 CONFIRMED `src/config/skill_assets.rs:10`）
- 默认值 / 取值范围：`assets/skills/doc-contract/SKILL.md` 全文；含 `name: doc-contract`、必填节规则、`N/A` 规则、反模式表、path:line 真实性要求、Runbook 七件套。
- 是否必填：常量无「必填」语义；`doc-contract` 这个名字被 5 个 Agent 持有（见 [manifest.rs.md](./manifest.rs.md) 的供给矩阵），但持有者集合的测试只断言 4 个。
- 影响：四处共用同一份契约（coding 写文档 / 审查 / doc-fix / doc-from-zero），改它等于同时改这些 Agent 的文档判据；`doc_contract_skill_carries_frontmatter_templates_and_rules`（`:135`）逐个锚点钉死。
- 示例：`builtin_skill_files("doc-contract")` → `Some(vec![("SKILL.md", DOC_CONTRACT_SKILL_MD)])`（`:21`）。

### CODING_DISCIPLINE_SKILL_MD

- 名称 / 类型：`pub const CODING_DISCIPLINE_SKILL_MD: &str`（🟢 CONFIRMED `src/config/skill_assets.rs:13`-14`）
- 默认值 / 取值范围：`assets/skills/coding-discipline/SKILL.md` 全文；12 条纪律（行为断言 / 先证后报 / 减法优先 / 最小 diff / 根因 / 边界收口 / 类型纪律 / 读者负担 / 上下文守护 / 不阻塞人类 / 可验证单元 / 幂等），每条带「反例」与「证据形态」；含来源与 MIT 归属登记。
- 是否必填：常量无「必填」语义；清单里 4 个 coding 系 Agent 持有该 skill（`coding-agent` / `coding-review` / `change-coding-agent` / `change-coding-review`）。
- 影响：改正文可能撞两条用例——`:154`（frontmatter 可加载性：`description` 非空、不得含 disable-model-invocation: true）与 `:176`（12 条目 + 反例/证据形态各 ≥12 + 来源三串）。
- 示例：`builtin_skill_files("coding-discipline")` → `Some(vec![("SKILL.md", CODING_DISCIPLINE_SKILL_MD)])`（`:22`）。

### builtin_skill_files

- **摘要**：按 skill 名返回内置文件清单（相对路径 → 正文）；未收录返回 `None`，由调用方回落占位行为。（🟢 CONFIRMED `src/config/skill_assets.rs:18`）
- **行为语义**：纯查表、无 IO、无副作用、幂等（同输入同输出）。先匹配 3 个自有 skill，`other` 分支转给 `builtin_language_files`（`:23`）——因此语言 skill 名也走同一入口。返回的 `&'static str` 直接来自二进制内嵌常量。
- **参数**：`skill: &str`——skill 名（也即 `<profile>/skills/` 下的目录名）；大小写敏感、必须逐字（`"plan-tree"` 而非 `"plan_tree"`）。
- **返回**：`Option<Vec<(&'static str, &'static str)>>`——`Some` 时每项为（相对 skill 目录的路径，文件正文）；`None` 表示未收录。
- **异常**：`N/A`（不返回 `Result`，未命中即 `None`）。
- **示例**：`assert!(builtin_skill_files("ghost").is_none())`（用例 `:149`）；`builtin_skill_files("coding-discipline")` 的清单恰有 1 项 SKILL.md（用例 `:154`）。

### builtin_language_files

- **摘要**：四语言 skill 的子表——skill 名 → 文件清单，未收录返回 `None`。（🟢 CONFIRMED `src/config/skill_assets.rs:74`）
- **行为语义**：纯 `match` 查表；`rust` 1 项、`go` 1 项、`ts-js` 2 项、`python` 36 项，其余 `_ => None`（`:116`）。无 IO、无副作用；每次调用新建 `Vec`（元素是 `Copy` 的 `&'static str` 对，开销可忽略）。
- **参数**：`skill: &str`——语言 skill 名（`"rust-best-practices"` / `"go-best-practices"` / `"ts-js-best-practices"` / `"python-best-practices"`）。
- **返回**：`Option<Vec<(&'static str, &'static str)>>`。
- **异常**：`N/A`（未命中即 `None`）。
- **示例**：`let rust = builtin_language_files("rust-best-practices").unwrap(); assert_eq!(rust.len(), 1); assert!(rust[0].1.contains("Rust Strict Standard"))`（用例 `:213`-228`）。

## 测试覆盖（`#[cfg(test)]` `:120` 起，6 例，全绿）

| 测试 | 覆盖口径 | 位置 |
|---|---|---|
| `plan_tree_skill_has_frontmatter_and_body` | SKILL.md 存在、frontmatter、标题与「目录约定」 | `src/config/skill_assets.rs:125` |
| `doc_contract_skill_carries_frontmatter_templates_and_rules` | 契约六要素（frontmatter/name/必填/N/A/反模式/path:line/Runbook） | `src/config/skill_assets.rs:135` |
| `unknown_skill_returns_none` | 未知 skill → `None`（走语言子表后仍 `None`） | `src/config/skill_assets.rs:149` |
| `coding_discipline_skill_is_registered_with_pi_loadable_frontmatter` | pi 可加载性：frontmatter、`description` 非空、禁用开关 | `src/config/skill_assets.rs:154` |
| `coding_discipline_skill_carries_twelve_entries_with_counterexamples_and_evidence` | 12 条目 + 反例/证据形态计数 + 来源许可 | `src/config/skill_assets.rs:176` |
| `language_skills_vendor_coverage` | 四语言条数与关键串 + python 子目录覆盖 | `src/config/skill_assets.rs:213` |

## 测试运行与环境（Runbook 分片）

- **环境要求**：`edition = "2024"`（`Cargo.toml:4`）⇒ Rust 1.85+；rustup 无默认 toolchain 时显式 `RUSTUP_TOOLCHAIN=stable`。无平台特定代码。
- **依赖安装**：`cargo fetch`（首次需网络）；本模块测试不需要 `tempfile`，也不读写临时目录。离线用 CARGO_NET_OFFLINE=true。
- **构建期依赖**：`assets/skills/` 下的 7 个目录共 43 个被 `include_str!` 引用的文件——缺任一即编译失败。这是本模块唯一的「环境」前置条件。
- **配置项**：`N/A` — 无运行时可配键；「配置」就是源码里的常量表（`MAX` 之类的开关不存在）。
- **启停 + 健康检查**：`N/A` — 库内纯函数模块，无进程/端口/健康端点；运行期表现通过 `render` 的产物观察。
- **种子数据**：`N/A` — 输入是编译期常量，无外部种子文件。
- **测试运行**：

  ```bash
  RUSTUP_HOME=$HOME/.rustup RUSTUP_TOOLCHAIN=stable \
  CARGO_TARGET_DIR=/var/tmp/u30-target TMPDIR=/var/tmp/u30-tmp \
  CARGO_NET_OFFLINE=true cargo test --lib config::skill_assets
  ```

  预期：`6 passed; 0 failed; 0 ignored`。本轮实测（在 `git archive HEAD` 导出的干净树 `/var/tmp/u30-head` 上跑，因为写作时主工作区有在途 `src/integration/` 改动、整包编译不过）：`6 passed; 0 failed; 0 ignored; 606 filtered out`，EXIT=0。

  > 若主工作区编译不过，先 `git archive HEAD | tar -x -C <临时目录>` 再在该树跑——本模块的断言只依赖仓内资产与常量，干净树同样成立（`filtered out` 随仓内测试总数变化，passed 数不受影响）。整棵 config 树的用例见 [mod.rs.md](./mod.rs.md)：本轮实测 `cargo test --lib config` → `54 passed; 0 failed; 1 ignored`（`mod.rs.md` 里的 47/48 是更早快照，随 config 各模块增删而变）。
- **公开接口清单**：`N/A` — 无 HTTP/CLI 端点；Rust 面即上表 5 个名字，且不经 `config` 模块面转发（`src/config/mod.rs:11`-13`），调用方必须写 `crate::config::skill_assets::…`。

## 证据与缺口

- 结构事实逐行复核：3 个 `pub` 常量（`:7`/`:10`/`:13`）、40 个私有元组（`:29`-71`）、2 个公开函数（`:18`/`:74`）、测试模块（`:120`-229）、6 个用例首行（`:125`/`:135`/`:149`/`:154`/`:176`/`:213`）。
- 唯一消费者：`src/config/render.rs:10`、`:112`。
- 本轮实跑（干净树 `/var/tmp/u30-head`，由 HEAD 提交的 git archive 导出）：`cargo test --lib config::skill_assets` → 6 passed / 0 failed / 606 filtered out / EXIT=0；`cargo test --lib config` → 54 passed / 0 failed / 1 ignored / EXIT=0。
- 资产对账（本轮脚本）：`include_str!` 引用 43 个文件、`assets/skills/` 实有 64 个——差值 21 全在 python-best-practices 目录下的 .mypy_cache（18 个）与 .ruff_cache（3 个），引用侧无一个缺失。
- 开放问题记在 `_meta/gaps.md`（本单元：资产目录与登记无机械对账、python 资产目录含 21 个缓存产物、pi 加载契约不可重放、vendor 保真无法验证）。
