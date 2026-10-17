# 四语言 skills vendor 来源登记（P005 T003）

入库纪律（decisions/006）：外部搜索 → 下载 → 原样入库，不经手重写。入库日期 2026-10-18。

| 本仓 skill 名 | 来源（原样 vendor） | 上游 | 许可证 | 版本基线 | 内容 |
|---------------|---------------------|------|--------|----------|------|
| `rust-best-practices` | [rust-strict/SKILL.md](https://github.com/0xMassi/claude-skills/tree/main/rust-strict) | 0xMassi/claude-skills | MIT | Rust 1.95 / edition 2024 | 安全与严格规则：unsafe 审计、unwrap 禁令、错误层级、并发安全、edition 2024 迁移、发布 profile |
| `go-best-practices` | [go-strict/SKILL.md](https://github.com/0xMassi/claude-skills/tree/main/go-strict) | 0xMassi/claude-skills | MIT | Go 1.26 | 错误包装 %w、internal/pkg 分层、并发生命周期、中间件校验、log/slog、os.Root、range-over-func |
| `ts-js-best-practices` | [typescript-strict/SKILL.md](https://github.com/0xMassi/claude-skills/tree/main/typescript-strict) + [javascript-strict/SKILL.md](https://github.com/0xMassi/claude-skills/tree/main/javascript-strict)（原样双文件打包为一个 skill 目录） | 0xMassi/claude-skills | MIT | TS 6.0/7.0、Node 24 LTS | no-any/no-as、穷尽联合、satisfies；AbortController、streams、stdlib 优先 |
| `python-best-practices` | [python-project-standard/](https://github.com/VoldemortGin/AI-Coding-Skill-Bible/tree/main/python-project-standard)（全目录原样：SKILL.md + references/standard.md + scripts/ + assets/templates/） | VoldemortGin/AI-Coding-Skill-Bible | MIT | ruff + mypy 严格门 | 工程章程：严格静态门、provider 接缝、module-per-domain、脚手架与一致性验收脚本 |

## 分发机制

- 仓库真源：`assets/skills/<skill>/`（原样文件）
- 生产分发：`src/config/skill_assets.rs` 编译期 `include_str!` 全量嵌入（`builtin_language_files`），
  skills_source 未提供时照常完整分发（P2-ISSUE-05 修复路径的延伸）
- 嵌套目录分发：渲染器 builtin 分支逐文件建父目录（T003 修复）

## 未入选与理由

- VoldemortGin/AI-Coding-Skill-Bible 的 rust-project-standard（0xMassi rust-strict 已覆盖且更聚焦严格规则；避免双标准冲突）
- ComposioHQ/travisvn awesome-claude-skills 榜单（是目录不是 skill 本体）
- 语义检索类（opencode-codebase-index / pi-smritikosh）：Deferred，见 P005 roadmap
