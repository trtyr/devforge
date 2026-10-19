# T001/T002/T003/T003b/T003c/T003d · 清单抽取与三道闭包 证据

- 日期：2026-10-19
- 实现：`src/docgen/inventory.rs`（引擎侧，启发式 v1）+ CLI `devforge doc-inventory <repo> [--out <dir>]`

## 一、清单内容

真实仓（devforge 自身）实测：

```
清单：531 条目（module 84 / symbol 444 / entry 3），单元水位 3
写入：<out>/inventory.json
```

条目字段：`id / kind / path / line / canonical_name / aliases / status / unit_id`，
meta 记录 `root / codegraph_indexed / unsupported_languages / unit_seq`。

抽取来源（v1 启发式，D004 约定 codegraph 为增强层）：
- module：按扩展名白名单扫源文件（跳过 node_modules/target/dist/build/venv/docs/隐藏目录）
- symbol：按语言行首特征抽**公开**符号（Python `def/class` 非 `_` 开头、TS/JS `export …`、Go 首字母大写 `func`、Rust `pub fn/struct/enum/trait/const/mod`）
- entry：文件名/目录约定（`main.py`、`main.rs`、`lib.rs`、`index.ts`、`cmd/`、`bin/` …）
- 不支持符号抽取的语言（java/cs/php/rb/c/cpp/kt/swift/scala/dart）→ `unsupported_languages` 缺口登记，不假装支持

## 二、三道闭包（D002）

| 闭包 | 判据 | 本次实测 |
|---|---|---|
| 文件覆盖 | 磁盘上的源文件必须都在清单里 | 0 项 |
| 符号覆盖 | 磁盘上抽得出的公开符号必须都在清单里 | 0 项 |
| 入口可达 | 从入口文件沿 import/use/mod 边 BFS，未覆盖即报 | **47 项，全部是 vendored skill 模板**（`assets/skills/python-best-practices/assets/templates/…`，合法的不可达资产）；`src/` 下 0 项 |

## 三、反向验证（合同要求）

单测 `file_coverage_closure_catches_missing_module`：构建清单后**删掉 `b.py` 的 module 条目**，`check_closures` 必须报 `file-coverage b.py`。
单测 `symbol_coverage_closure_catches_missing_symbol`：删掉符号 `zzz`，必须报 `symbol-coverage`。

入口可达性同样有两条：`entry_reachability_follows_python_imports`（可达 → 无发现）、`unreachable_file_is_reported`（孤儿文件 → 有发现）。

## 四、编号预分配（D020）

`assign_units` 按顶层目录分组，`unit-<slug>-<NN>`；单测 `unit_numbering_is_stable_across_runs` 断言**连续两次调用结果完全一致**（不漂移），且 symbol/entry 条目继承所属文件的单元号。

## 五、门禁

`cargo test` **214 通过 / 0 失败**；`cargo clippy --all-targets` **0 警告**。

## 六、已知限制（v1，写清楚不隐瞒）

1. 无 tree-sitter：符号抽取是行首启发式，多行签名/装饰器/宏生成符号会漏（漏了会被"符号覆盖"闭包抓出来，不会静默）
2. 依赖边是 import/use/mod 字面解析：动态 import、字符串拼路径、反射不可见（表现为"不可达"发现，需人工判或多语言增强）
3. codegraph 索引只作为 `meta.codegraph_indexed` 记录，本版未消费其图（t4 起按需接入）
