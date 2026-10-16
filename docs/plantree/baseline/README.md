# Baseline — DevForge 项目现状

## 项目定位

AI 自动化开发流水线 / 编排平台。把「从 0 到出现一个 MVP」写成剧本（playbook），剧本本质是对 Agent 的编排。

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
- **plan tree 跟系统走**：所有 plan tree 统一存 `~/.devforge/plans/`，不跟目标项目走

## 全局设计哲学（已拍板）

**无硬限制**：不做 token 上限、step 上限、循环次数熔断等任何硬限制，信任模型收敛。适用于全部阶段、全部 Agent。（见 P001 decisions/003）

## 风险热点

- Agent 间消息传递与「保活/死亡」生命周期管理是编排器的核心难点，机制未定（P001 open questions）
