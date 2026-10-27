# 文档审查报告 — unit-src-29（第 26 轮）

- 单元：`unit-src-29` → `src/config/skill_assets.rs`；文档 `docs/contexts/src/config/skill_assets.rs.md`（191 行，frontmatter `type: module` / `unit: unit-src-29`）
- 引擎审计（确定性，采信不重算）：要求 6 条 / 已覆盖 6 条 / 缺失 0；doc-lint EXIT=0（0 失败 / 2 提示，两条提示均在 `render.rs.md:190`、`:195`，不在本单元）
- 输入边界（DR-12）：不打开源码推事实；事实以单元文档、`_meta/facts/unit-src-29.md`、`_meta/gaps.md`、`_meta/state.json`、`inventory.json`、plan tree、引擎审计与本轮实跑为准。仅为验收文档自带的两条「可重放命令」执行了 `grep -c 'include_str!'`、`grep -rn builtin_skill_files`，未阅读实现。

## 1. 覆盖与 findings 处置

- 覆盖 6/6 采信。`inventory.json` 里本单元 12 条（module×1 + symbol×5 + behavior×6）均有落点：模块自报家门 + 公开成员表（3 常量 + 2 函数）+「资产清单」+ 6 例测试表；`status` 仍为 `uncovered` 是缺少标记工具写入，文档侧无缺口（`state.json` 已注明）。
- 工作区无 `testing/findings/`，kickoff 未附本单元 doc-side findings → doc-side 待处置清单 `N/A`。
- `gaps.md` 本单元 4 条（资产目录与登记无机械对账 / python 目录 21 个缓存产物 / pi 加载契约不可重放 / vendor 保真不可验证）在「核心口径」与「证据与缺口」逐条承接，置信度标注与 gaps 一致（🟡/🔴），无漏项。

## 2. 完整性与 N/A

- 模块三件齐备：职责（含「不负责」四项：不写盘、不决定装哪些 skill、无版本/校验、不复制外部源）、公开成员（5 项逐个指向条目）、依赖（上游 render、零 crate 依赖、编译期资产、许可登记）。
- 常量条目按配置项口径齐备：名称/类型、默认值/取值、是否必填、影响、示例（3 条均为 `builtin_skill_files(...)` 的具体返回）。
- 两个公开函数六节齐备：摘要 / 行为语义（纯查表、幂等、转发关系）/ 参数（大小写敏感）/ 返回 / 异常（N/A 带理由）/ 示例。
- 私有符号（40 个元组）给了汇总行 + 名单，满足「摘要 + 行为语义」。
- Runbook 七件套逐件有落点：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN=stable`）、依赖（`cargo fetch`、离线、构建期 43 文件前置）、配置项（N/A + 理由）、启停+健康检查（N/A + 理由 + 指向 render 产物）、种子数据（N/A + 理由）、测试运行（完整命令块 + 预期 6/0）、公开接口清单（N/A + 5 个名字并说明不经 `config` 门面转发）。所有 N/A 均带理由，无「省略即不适用」。

## 3. 真实性与置信度

本轮机械复核（实体存在性）全部命中：

- `assets/skills/` 7 个目录、文件数 1/1/1/1/1/2/57，与文档「7 个目录、43 处 include_str!」及分档条数 1/1/1/1/1/2/36 一致；`grep -c 'include_str!'` → 43，与「可重放」预期相符。
- python 36 项清单与磁盘（剔除缓存后）**逐行完全一致**（`diff` 空，36/36）；缓存产物 21 = `.mypy_cache` 18 + `.ruff_cache` 3，与文档一致（facts 写 19+2 属 facts 侧漂移）。
- 正文特征断言可离线复核：rust/go 关键串各 1、coding-discipline `**反例**` 12 与 `**证据形态**` 12、来源三串（cursor/plugins/pstack、MIT、Lauren Tan）齐、doc-contract 含 `name: doc-contract`/`N/A`/Runbook、plan-tree 含「目录约定」、7 份 SKILL.md 均以 `---` 开头。
- `docs/plantree/plans/005-coding-stage/evidence/skills-sources.md` 存在；`src/config/mod.rs` 的 `:9` 声明与「不在 `:11`-13 转发」与 `mod.rs.md` 表述完全一致；`render.rs:10`/`:112` 的消费点与 `render.rs.md` 一致；`_ => None` 在 `:116`。
- 置信度诚实：4 条 🟡 均给了依据且不做可重放承诺（pi 加载契约、vendor 保真、无机械对账），未把推断写成 🟢；🟢 条条带 `path:line`，可回指。

## 4. 可跑（本轮从零 boot）

- 依赖：`cargo fetch`（离线、仓外空 `CARGO_TARGET_DIR=/var/tmp/r26boot/target`）EXIT=0。
- 测试（照 Runbook 命令原样）：`cargo test --lib config::skill_assets` → `6 passed; 0 failed; 0 ignored; 592 filtered out`，EXIT=0，与文档预期逐字吻合；交叉 `cargo test --lib config::` → `47 passed; 0 failed; 1 ignored; 550 filtered out`，EXIT=0，与「47 passed; 1 ignored」一致。6 个用例名与文档测试表一一对应。
- 宿主 boot（本单元 Runbook 把启停/健康/种子标 N/A，此步为补充证据）：`cargo build --bin devforge` 从零 20.35s；隔离 `HOME=/var/tmp/r26boot/home`、`DEVFORGE_HOME=…/.devforge`、`DEVFORGE_ADDR=127.0.0.1:47532`、token；日志两行「devforge 启动」+「API 就绪 addr=…」；`GET /api/v1/config/manifest` 带 token `200`（52970 字节，与 `mod.rs.md` 记录一致）、缺 token `401`；落盘 `logs/` + `manifest.json`；种子 `agents=15 / model_pool=3 / providers=1 / base_packages=4`；按 pid kill 后请求返回 `000`（端口释放）。
- 结论：该单元 Runbook 的可执行部分从零可复现，无阻断点。

## 5. plan tree 与第三方可测

- P002（skills 属模板层写死、渲染为复制或占位）与 P005 T003（四语言 skill vendor 入库、来源登记、编译期嵌入全量分发）均与文档口径一致，无冲突；「全量分发」与 21 个缓存不入二进制之间的落差已在 gaps/文档显式披露，不算文档与 plan 冲突。
- 第三方可据文档写出：3 个常量名的返回形状、5 个 skill 的条数与相对路径、36 项 python 清单、未收录 → `None` → 占位 README 的降级链、6 个用例的断言面、测试运行与隔离方式。测试面充分。

## 6. 非阻塞建议（不回注 doc-fix 修订）

1. `:41` 的「全仓无其它引用（`grep -rn builtin_skill_files` 只命中这两处）」与实际不符：实跑命中 8 行：定义 `skill_assets.rs:18`、测试调用 `:126`/`:137`/`:150`/`:156`/`:177`、生产调用 `render.rs:10`/`:112`。建议改为「生产侧仅 `render.rs` 两处；本文件测试内 5 处」。
2. 行号区间反引号成对错位 6 处（`:18`、`:37`、`:44`、`:48`、`:53`、`:189`）：写法 `:29`-71`` 会让区间后半段与后续正文被吞进行内代码，渲染时行号段断裂。建议统一为 `` `:29`-`:71` `` 或 `:29`–`:71`。
3. `:162` 「契约六要素」实际列了 7 项（frontmatter/name/必填/N/A/反模式/path:line/Runbook）；改为「七要素」或删一项，与 `:135` 用例锚点对齐。
4. `_meta/facts/unit-src-29.md` 把 21 个缓存写成 `.mypy_cache` 19 + `.ruff_cache` 2（磁盘为 18+3，文档正确）；建议下次维护刷新 facts，避免两份材料各说各话。
5. `:44`/`:53` 以 `src/config/skill_assets.rs:27`-28` 指代 vendor 注释，与 gaps 的 `:27`-28` 同源；上面第 2 条修完即可。
6. 「doc-contract 四处共用」与「5 个 Agent 持有」并列（`:74` 条目）易被读成矛盾，建议加半句「4 个角色 / 5 个 profile（`doc-agent` 额外持有，测试只断言 4 个）」。

## 结论

覆盖 6/6、lint 0 失败（2 条提示在别的单元文档，按契约不判失败）采信；findings 无遗漏；必填节与 N/A 合规；🟢 全部可回指，实体逐条落地；Runbook 命令本轮从零复现（6/0/0，EXIT=0），宿主 boot 健康检查与种子数据正常。判 **PASS**。
