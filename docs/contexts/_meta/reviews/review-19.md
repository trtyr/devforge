# 文档审查报告 — unit-src-24（第 19 轮）

- 单元：`unit-src-24` → `src/config/installer.rs`；文档 `docs/contexts/src/config/installer.rs.md`（147 行）
- 引擎审计（确定性，采信不重算）：要求 5 条 / 已覆盖 5 条 / 缺失 0；doc-lint 通过（0 违规）
- 输入边界（DR-12）：不打开源码；事实以 `_meta/facts/unit-src-24.md`、`_meta/gaps.md`、`_meta/glossary.md`、plan tree 与引擎审计为准，代码实体存在性由 doc-lint 背书

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 doc-side finding。`_meta/gaps.md` 登记本单元 2 条 GAP（`pi install` 双记录形状不可验证；`PI_CODING_AGENT_DIR` 与 `npm:` 前缀属 pi 侧契约），文档 `:53` 已以 🟡 INFERRED + 指向 gaps 的方式披露，未冒充事实。**无遗漏，N/A**。

## 2. 覆盖对账（5/5 采信）

档位要求集合 = module ×1 + 公开符号 ×4（`build_install_command:12`、`PackageInstaller:47`、`PiInstallBridge:52`、`FakeInstaller:71`）。「公开成员」表 4 行 + 私有条目 1 行，均链到独立小节，`#build_install_command` / `#packageinstaller` / `#piinstallbridge` / `#fakeinstaller` / `#失败诊断面私有-install_failure_error` 锚点可解析；5 条 behavior（测试覆盖表）逐例承接 `:125/:135/:174/:183/:193`。无悬空条目，与 5/5 吻合。

## 3. 完整性 / 必填节 / N-A

- 模块三件：职责（负责 3 条 + 不负责 3 条边界）/ 公开成员 / 依赖（消费链、向下依赖、外部 `pi`）齐备。
- 公开函数 `build_install_command`：摘要 / 行为语义 / 参数 / 返回 / 异常（N/A 带理由「不执行、不失败」）/ 示例 六件齐备。
- `PackageInstaller`（trait）与 `PiInstallBridge`、`FakeInstaller`（结构体）：职责 / 构造或初始化 / 公开成员 / 生命周期 / 典型用法齐备。
- 私有 `install_failure_error`：摘要 + 行为语义齐备（按内部符号档位）。
- 配置表 7 列（名称/类型/默认值/必填/取值范围/影响/示例）齐备；`MAX_STDERR_BYTES` 以「N/A（编译期常量）」声明不适用，合规。
- Runbook：环境、依赖获取、测试运行与隔离在册；配置项 / 启停与健康检查 / 种子数据 / 公开接口清单四项 N/A 且逐项给理由（库内模块，无独立进程；HTTP 入口归 `src/server`），与前轮同类单元口径一致。
- 无 TODO/TBD、无整段日志粘贴、无死链。

## 4. 真实性 / 置信度诚实性（1 处 FAIL）

- 🟢 全部带 `path:line`（`:59`、`:90`、`:108`、`:117` 及 `:139` 实跑命令），无裸绿；🟡 双记录语义明确标 INFERRED 并给依据。
- 与 facts 逐项核对：符号行号 12/47/52/71、私有 `:23`/`:24`、测试 125/135/161-171/174/183/193、消费链 `render.rs:198-199/203/222/239/244/248/277`、`server/mod.rs:1163-1164`、`main.rs:201`、`handlers.rs:618/669/675-680/699/731`、`server/tests.rs:132`、`tests/f6_observability.rs:282` 均对得上。
- **FAIL（`:110` 行为语义与确定性事实矛盾）**：文档写 `FakeInstaller` 第二记录「**缺失** → 骨架 `{"name":"pi-extensions","private":true,"dependencies":{}}`；**存在但不可解析 → `{}`**」。facts 明确为「**缺失或解析失败** → 骨架 `{…}`」。后果：第三方照 `:110` 为「已有坏 JSON 的 `npm/package.json`」写用例，会断言 `{}` 形状（缺 `name`/`private`），实际落盘是骨架 + `dependencies`，用例直接失败；现有 5 例只测缺文件，仓内测试也发现不了这条。且 `:50` 的「默认值」列只覆盖「缺文件」，无法自我纠偏。需把 `:110` 改为「缺失或不可解析 → 骨架」，并同步 `:50` 的默认值口径；若核实后确为 `{}`，则应修正 facts 并重新审计，不可两说并存。

## 5. plan tree 一致性（无冲突）

- P002 `README.md:11`「`pi install` → settings.json `packages` + npm/package.json 双记录」↔ `:10`/`:50`。
- P002 `roadmap.md:7`（T003 渲染器 + FakeInstaller 模拟双记录）↔ 职责/`FakeInstaller` 节。
- P006 `roadmap.md:23`（stderr 尾部 500B 保尾、单测 ×3）↔ `:118`/`:120`/测试表。
- P007 `roadmap.md:33`（4 worker 有界并发、返回值保序、幂等跳过）↔ `:19`/`:36`/`:111`；文档明确把幂等归 `render.rs`、本模块不做，未越界。

## 6. 内部自洽（除上条外通过）

`:19` 的「不负责幂等」与 `:111`、`:50` 互相一致；`:112` 四处 `expect` 文案与 facts 相同；`:139` 的 `5 passed / 593 filtered out` 与实跑一致。非阻塞小瑕：`:47`「默认值 = 不设 / 必填 = 是」措辞略拧（实际是「父环境不必设、本模块无条件写子进程」），后有解释，不阻断；`:55-65`、`:74-117` 等方法行区间取的是函数体内部行，facts 记的是含签名/收尾的 `:54-66`、`:73-118`，不影响回指。

## 7. 可跑（照 Runbook 从零 boot）

- 环境：`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`（本机无默认 toolchain，文档已要求显式指定）、`TMPDIR=/var/tmp/dfz-r19/tmp`（`/tmp` 99% 满）、`CARGO_TARGET_DIR=/var/tmp/dfz-r19/target`（全新空目录）；`rustc/cargo 1.99.0`，与文档一致。
- 依赖：文档原文 `cargo fetch` → **EXIT=0**（0.14s，缓存命中）。
- 测试：`cargo test --lib config::installer` → `running 5 tests`，`test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 593 filtered out`，**EXIT=0**；从零编译 53.61s（文档记 43.17s，机器抖动，输出行未承诺固定耗时）。
- 5 个用例名与测试表逐名一致；隔离性成立（`tempfile` tempdir + 内存 `Output`，不 spawn 子进程、不联网、不读真实 profile）。
- 配置 / 启停与健康检查 / 种子数据：库内模块确无独立进程，N/A 理由成立。

## 8. 第三方可测标准

除 `:110` 的坏 JSON 回退分支外，接口/参数/返回/异常、双记录落盘形状、追加语义、失败诊断模板与 5 例测试描述足以支撑等价测试。该分支修正后即可闭合。

## 结论

覆盖 5/5、lint 0 违规、findings N/A、plan tree 无冲突、必填节/N-A 合规、Runbook 从零实跑 5/5 EXIT=0 且预期输出吻合；但 `:110` 的 `FakeInstaller` 解析失败回退口径与 facts 矛盾，直接误导第三方对「坏 JSON」场景的断言。**FAIL**，回注 doc-fix 修订该一条（连带 `:50` 默认值列）；其余均为非阻塞小瑕。
