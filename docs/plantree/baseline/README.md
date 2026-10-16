# Baseline — DevForge 项目现状

## 项目定位

AI 自动化开发流水线——**服务器形态的编排平台**（非 CLI，2026-10-16 拍板，接口体系见 P003）。把「从 0 到出现一个 MVP」写成剧本（playbook），剧本本质是对 Agent 的编排。

## 五阶段流水线构想

1. **规划阶段** — Plan Agent 与用户沟通产出 plan tree，Plan Review Agent 检查语义一致性（P001 的范围）
2. **开发阶段** — 未设计
3. **测试阶段** — 未设计
4. **验收阶段** — 未设计
5. **部署阶段** — 未设计

当前只有规划阶段进入设计（见 P001），其余阶段 ideas 先行。

## 代码现状

- Rust 仓库，edition 2024，仅 `Cargo.toml` + `src/main.rs`（初始模板），git 零 commit
- 无任何代码模块 → module map / runtime flows / storage / test gates：Unknown, needs inventory（有代码后回填）

## 全局技术底座（已拍板）

- **复用 PI Agent 作为底座**：devforge 不自写 Agent runtime，负责 spawn pi 子进程 + 注入独立配置 + 管生命周期
- **每类 Agent 独立配置**：绝不跟全局配置走。机制已验证——pi 的 `PI_CODING_AGENT_DIR` 环境变量可覆写配置目录（默认 `~/.pi/agent`），每类 Agent 一个独立 profile 目录（各自的 AGENTS.md / skills / settings）
- **Agent 通道用 RPC 模式**：`pi --mode rpc` 长驻子进程 + JSONL 控制（decisions/005）
- **plan tree 跟系统走**：所有 plan tree 统一存 `~/.devforge/plans/`，不跟目标项目走
- **Agent 配置跟系统走**：所有 Agent 的 profile 统一在 `~/.devforge/profiles/`，每 Agent 一个文件夹，系统级注册（decisions/006）

## 基础架构三原则

所有模块必须遵守，详见 [architecture-principles.md](architecture-principles.md)：完整错误处理 · 完整结构化日志 · AI Friendly 架构 · 全链路可观测 · Agent 完全解耦 · 厚 Agent 薄编排。P001 验收标准含前两者（不许先跑通再补）。

## 进程身份红线

devforge 只管理自己启动的 pi 进程，按 spawn 台账精确清理，禁止全系统扫进程杀 pi（decisions/007，机制见 P001 orchestrator-design）。

## 全局设计哲学（已拍板）

**无硬限制**：不做 token 上限、step 上限、循环次数熔断等任何硬限制，信任模型收敛。适用于全部阶段、全部 Agent。（见 P001 decisions/003）

## 系统全景（系统 = MVP 流水线 + 功能点）

devforge 是一个系统，不只是规划流水线。按「MVP 流水线内 / 外」划分现状（用户拍板的划分方式，2026-10-16）：

**流水线内（MVP 主线）**：规划阶段的编排流程（P001）——两个 Agent 的状态机、Review 循环、plan tree 产出。

**流水线外的系统能力点**：
- 已立项：配置管理体系（P002）· API 服务面（P003）· 项目/任务/SOP 三层管理（P004）——P004 是系统主要对象模型：三层结构、任务 = SOP 实例、并发为任务层正式需求
- 已有归属：可观测数据层（P001/T011）· 进程身份与台账（P001/decisions/007）· 项目/任务/产物管理（P003 能力域）
- **待定义的功能点（开放）**：系统自身运维管理（服务启停/健康/状态面）……——清单未盘完，见 ideas；用户逐批提出后归位

**未来阶段流水线**（各自立项，不入本清单）：开发 / 测试 / 验收 / 部署。它们将来也是「流水线」，与规划阶段平级，复用横切能力（P002/P003/原则）。

## 风险热点

- Agent 间消息传递与「保活/死亡」生命周期管理是编排器的核心难点，机制未定（P001 open questions）
