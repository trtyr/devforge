# ARC-R7 · 800-1000 档收尾方案（2026-10-24）

> R6 的自然延续：把 800-1000 档最后三件拆到 ≤800，全仓归零 >800。
> 三件结构高度一致（本体 + 尾部 tests mod）——R2 tests 外迁模式直接套用。

## 现状与拆法

| 文件 | 行数 | 结构 | 拆法 |
| --- | --- | --- | --- |
| src/config/manifest.rs | 948 | 常量区 1-95（包/提示词 include_str!/MCP 工厂）+ 类型区 96-175 + impl ConfigManifest 176-481 + tests 482-948 | tests mod → `config/manifest_tests.rs`（467）；本体 481 |
| src/config/render.rs | 829 | runtime_settings + impl Renderer ~40-276 + 自由 fn 277-294 + tests 315-829 | tests mod → `config/render_tests.rs`（515）；本体 314 |
| src/engine/client.rs | 810 | WaitOutcome/RpcClient 类型 + impl 50-423 + Drop 424-431 + tests 432-810 | tests mod → `engine/client_tests.rs`（379）；本体 431 |

三件本体与拆出件全 ≤800 ✓。目录形态不引入（单件外迁，同 R2 的 builtin_tests 先例——
`mod manifest_tests;` 等声明在原文件尾部）。

## 牵连面预判

1. **include_str 路径**：manifest.rs 的 13 个 include_str!（assets/prompts/）留在本体不动 ✓。
2. **tests 内 super::* 链**：拆出件 `use super::*;`（与 R2/R6 同）。
3. **tracing target 断言**：client.rs 测试若有 target 字面量断言需核对（client 的日志
   target 来自生产码不在测试文件，预计无牵连）。
4. **plan_audit 白名单**：三件无历史 change-plan 引用（grep 验证过 c7-1 等只引 retry/server
   tests）——预计零牵连；若有，归一化表加条目。

## 批次（确认后连续推进）

| 批 | 内容 | commit |
| --- | --- | --- |
| 1 | 三件 tests mod 外迁（一次做完，每件独立 commit） | 3 commits |
| 终验 | 全仓 wc -l 复扫零 >800 + 707 测试数对账 + clippy worktree diff + 7889 重启 resume + 台账 R7 Done | 1 commit |

## 验收口径

- 全仓 `wc -l` 复扫：零 >800（R6 后仅剩这三件）；
- 全量 cargo test exit 0，707 测试数持平；
- clippy 与当前基线 worktree 逐条 diff 零差异；
- 7889 新 binary 重启 + acc2 resume 推进证据；
- 台账：本文件收官段 + arch-ai-friendly 台账加 R7 行（Done）。

## R7 收官回写（2026-10-24，状态 Done，操作员执行）

| commit | 内容 |
| --- | --- |
| cd4bc90 | task-1：本方案落档（用户确认门通过后开工） |
| 85d92b3 / b48815c / e596eaa | 批 1：三件 tests 外迁（E0583 子目录路径教训：非 mod.rs 文件的子 mod 找 <stem>/ 子目录——R6 session 同款） |

终态：全仓 wc -l 复扫零 >800（R1..R7 七期拆解收官）；全量 7 靶 exit 0、707 测试数持平；
clippy 与批 1 前基线 worktree 逐条 diff 零差异；验收 pid 3239830 探活 200、acc2 resume 后
doc-agent 活跃（accept-r7.log 11:28 时段）。
