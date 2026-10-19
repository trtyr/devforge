# T000b · doc-lint 骨架（结构检查）证据

- 日期：2026-10-19
- 实现：`src/docgen/lint.rs`（引擎侧纯函数）+ `devforge doc-lint [dir]` 子命令（`src/main.rs`）
- 覆盖检查：必填节（D014 分级表，含 N/A 规则语义）/ 链接与锚点 / Runbook 七件套 / 禁写内容（TODO、超长代码块、粘贴日志）
- 未含：实体存在性、同名异写（需 inventory，t4 接入）

## 一、通过样本

```
$ devforge doc-lint /tmp/.../dl-good
doc-lint: 通过（0 违规）
EXIT=0
```

样本内容：`type: entry` 的条目文档，六节齐全。

## 二、不通过样本（三种违例同时命中）

```
$ devforge doc-lint /tmp/.../dl-bad
dl-bad/10-runbook.md:1 [runbook] Runbook 七件套缺：依赖安装 / 配置项 / 启停与健康检查 / 种子数据 / 测试运行 / 公开接口清单
dl-bad/entry-bad.md:1 [required-section] 条目类型 Entry 缺必填节：输入 / 输出 / 错误 / 示例（不适用须写 `N/A` 并给理由，省略不算）
dl-bad/entry-bad.md:10 [link] 死链：missing.md（解析为 .../dl-bad/missing.md）
doc-lint: 3 个失败 / 0 个提示 → 不通过
EXIT=1
```

验证点：
- 每条违例都有 `文件:行号` + 类别 + 具体缺什么
- 退出码可判：0 通过 / 1 不通过

## 三、单测覆盖（12 条）

干净文档通过 / 缺节 FAIL 带行号 / N/A 节只有存在才算 / 死链与锚点 / Runbook 七件套 / TODO 只提示不 FAIL / 超长代码块提示 / 粘贴日志提示 / frontmatter 损坏不崩 / 内部符号只提示 / summary 计数。

## 四、门禁

`cargo test` 200 通过 / 0 失败；`cargo clippy --all-targets` 0 警告。
