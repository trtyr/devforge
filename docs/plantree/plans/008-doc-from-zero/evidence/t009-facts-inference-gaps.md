# T009/T010/T011 · 勘察 / 解释 / 缺口转问题 证据

- 日期：2026-10-19
- 改动：`src/config/manifest.rs`（DOC_AGENT_PROMPT 三阶段流水）、`src/docgen/lint.rs`（置信度诚实性 + 缺口登记检查）

## 一、三阶段写进提示词（事实 / 推断分离）

```
1. 勘察 → _meta/facts/<unit>.md：只记“有什么”，严禁写“为什么”
2. 解释 → 每条断言标 🟢/🟡/🔴
     🟢 CONFIRMED 同行必须给 path:line 或可重放命令
     🟡 INFERRED 写清依据
     🔴 GAP 一律写进 _meta/gaps.md（一条一行含影响面），不得静默省略
3. 生成 → 按 doc-contract 写 docs/contexts/；不适用写 N/A；跨模块只建引用
```

## 二、机械检查（doc-lint 新增 `confidence` 类）

| 规则 | 判定 |
|---|---|
| 行内出现 🟢（或 CONFIRMED），同行无 `path:line` / 路径 / 可重放命令 | **FAIL**（"最严重类"） |
| 文档树里出现 🔴，但 `gaps.md`（或 `_meta/gaps.md`）不存在 | **FAIL**（缺口必须登记，不许静默） |
| 作用域 | 只对声明 `type:` 的契约条目文档生效（与实体检查同口径，避免设计文档噪音） |

可重放命令按前缀白名单识别：`devforge/python/pytest/cargo/npm/pnpm/curl/git/bash/make/go/node/docker/psql/sqlite3/redis-cli`。

## 三、单测（5 条新增）

- `green_mark_with_path_line_passes`
- `green_mark_with_replayable_command_passes`
- `green_mark_without_evidence_fails`（断言行号指向标记行）
- `red_gap_without_gaps_file_fails`
- `red_gap_with_gaps_file_passes`

## 四、门禁

`cargo test` **243 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。
