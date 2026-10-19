# T000b（续）· 实体存在性与同名异写 证据

- 日期：2026-10-19
- 实现：`src/docgen/lint.rs` 的 `EntityIndex` + `check_entities`；CLI 新增 `--repo <root>`
- 真相源：**源码语料**（不是清单符号表）——符号表只收公开符号且靠启发式，拿它当唯一真相会把合法内部标识符误报成幽灵

## 一、检查规则

| 对象 | 判定 |
|---|---|
| 行内代码里的**路径**（含 `/` 或已知扩展名） | 从文档目录逐级向上解析，找不到 → FAIL「幽灵路径」 |
| **`path:line`** 引用 | 文件必须存在；行号超出文件行数 → FAIL |
| 行内代码里的**标识符**（`create_note()`、`NoteStore`…） | 在源码语料中找不到 → FAIL「幽灵实体」 |
| 命中清单登记的 **alias** | 只提示「同名异写」，不 FAIL（D011：终审判） |

**作用域**：实体检查只对**声明了 `type:` 的契约条目文档**生效。
理由：总览/Runbook/plan 设计文档天然会引用"将来要创建的东西"（`.codegraph/`、`gaps.md`），对它们硬查实体只会制造噪音。这一条是被真实文档探针逼出来的（见下）。

## 二、幽灵样本（带行号）

```
$ devforge doc-lint <tmp>/t4-ghost --repo .
ghost.md:8  [entity] 幽灵实体：`ghost_func_zzz` 在代码中找不到
ghost.md:14 [entity] 幽灵路径：src/nowhere.py 在代码中不存在
doc-lint: 2 个失败 / 0 个提示 → 不通过
```

## 三、零误报（真实文档探针，两次迭代）

| 轮次 | 真实 plan 文档结果 | 处理 |
|---|---|---|
| v1（只按语料查全部行内代码） | **85 个失败**——协议块 `<stage-complete>N</stage-complete>`、`N/A`、plan 概念 `phase3`/`doc_level`、未来产物 `gaps.md` 全被判幽灵 | 收窄：占位符跳过 + 作用域限定 `type:` 条目 |
| v2（当前） | **0 个失败 / 4 个提示**（提示全是契约表里自己写的 `TODO` 字样） | ✅ |

结论：**规则收窄后零误报**，且收窄没有牺牲检出——幽灵样本仍然 100% 被抓。

## 四、单测（6 条新增）

`real_entities_pass_with_zero_false_positives` / `ghost_symbol_is_reported_with_line`（行号指向引用处）/ `ghost_path_is_reported` / `out_of_range_line_reference_is_reported` / `alias_usage_is_warning_not_failure` / `fenced_code_is_not_entity_checked`。

## 五、门禁

`cargo test` **220 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。

## 六、已知限制

1. 只查**行内代码**（backtick）里的引用；纯散文提到的实体不查（那是人工审查面）
2. 含空格的行内代码（命令行）整体跳过——不做命令解析
3. 符号存在性是"语料里出现过这个 token"，不是"它在某个作用域可见"；同名遮蔽类问题留给审查
