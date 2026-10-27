# 文档审查报告 — unit-src-24（第 21 轮）

- 单元：`unit-src-24` → `src/config/installer.rs`；文档 `docs/contexts/src/config/installer.rs.md`（165 行）
- 引擎审计（确定性，采信不重算）：要求 5 条 / 已覆盖 5 条 / 缺失 0；doc-lint 通过（0 违规）
- 输入边界（DR-12）：不打开源码；事实以 `_meta/facts/unit-src-24.md`、`_meta/gaps.md`、`_meta/glossary.md`、plan tree、引擎审计与实跑输出为准

## 1. findings 处置完整性

工作区无 `testing/findings/`，kickoff 未附本单元 doc-side finding。`_meta/gaps.md` 本单元段现登记 4 条：①双记录形状属 pi 侧契约（🔴）、②`PiInstallBridge::install` 无超时（🟡）、③`FakeInstaller` 畸形 profile 走 `expect` panic（🟡）、④`PI_CODING_AGENT_DIR` / `npm:` 前缀语义（🔴）。文档处置：①在 `:53` 以 🟡 INFERRED + 三点依据 + 指向 gaps 披露，未冒充事实；③在「异常」段（`:127`）逐字列出四处 panic 文案与触发条件（只在既有 profile 形状不对时）；②④未在文中展开（属「文档给不出的运维口径」，非漏写行为）。无漏项，可判 N/A–pass。

## 2. 覆盖对账（5/5 采信）

要求集合 = module ×1 + 公开符号 ×4（`build_install_command:12`、`PackageInstaller:47`、`PiInstallBridge:52`、`FakeInstaller:71`）。「公开成员」表 4 行全部链到独立小节，锚点 `#build_install_command` / `#packageinstaller` / `#piinstallbridge` / `#fakeinstaller` / `#失败诊断面私有-install_failure_error` 与标题 slug 逐字一致；私有条目 1 行亦给落点。5 例 behavior 由「测试覆盖」表逐例承接 `:125/:135/:174/:183/:193`，与 facts 全同。无悬空条目。

## 3. 完整性 / 必填节 / N-A

- 模块三件齐备：职责（负责 3 条 + 不负责 3 条，显式排除「算包、幂等跳过、解析输出语义」）/ 公开成员 / 依赖（消费链、向下依赖、外部 `pi`）。
- 公开函数六件齐备（摘要/行为语义/参数/返回/异常/示例）；`异常：N/A` 给了理由（不执行、不失败）。
- trait 与两个结构体：职责 / 构造或初始化 / 公开成员 / 生命周期 / 典型用法五件齐备，另附错误语义。
- 私有 `install_failure_error` 摘要 + 行为语义齐备（内部符号档位）。
- 配置表 7 列齐（名称/类型/默认值/必填/取值范围/影响/示例）；`MAX_STDERR_BYTES` 以「N/A（编译期常量，不可配）」声明，合规。
- 数据模型表五件齐（字段/索引/生命周期/关系/迁移注意）；唯一小瑕：「可空」未逐行显式声明（见 notes）。
- Runbook：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN=stable`）、依赖获取（`cargo fetch` + `tempfile` `Cargo.toml:27`）、测试运行（命令 + 预期输出）、隔离（tempdir、不 spawn/不联网、别在单测用真桥）在册；配置项 / 启停与健康检查 / 种子数据 / 公开接口清单四项 N/A 且逐项给理由（库内模块，HTTP 入口归 `src/server`），与前轮同类单元口径一致。
- 无 TODO/TBD、无整段日志粘贴、无死链。

## 4. 真实性 / 置信度诚实性

- 5 处 🟢 全部可回指：`:12`（`build_install_command`）、`:56`-64（`PiInstallBridge::install`）、`:74`-117（`FakeInstaller::install`）、`:23`（`install_failure_error`）、`:155`（实跑命令 + EXIT=0，属「可重放命令」档）。
- 唯一 🟡（`:53`）给了三点依据并指回 gaps，诚实。
- 与 facts 逐项核对一致：符号行 12/47/52/70-71/74；`MAX_STDERR_BYTES` `:24`；`expect` 四处 `:85/:89/:106/:111`；测试 5 行号；辅助 `:161`-171；消费点 `render.rs:198-199/200-221/203/222/239/244/248/263/277/318/323-326/431/440`、`server/mod.rs:131`、`main.rs:201`、`handlers.rs:618/639/675-680/687/699/731`、`server/tests.rs:132`、`tests/f6_observability.rs:282`。
- 第 19 轮 FAIL 点（第二记录「解析失败 → 骨架」口径）已消解：现文档三档并列（缺文件→骨架 `:101`-103 / 可解析→文件内容 / 不可解析→`{}` 无 name·private `:98`-100），与重写后 facts（`else` 在 `:102`、`unwrap_or(json!({}))` 在 `:100`）一致，且 `:129` 补了第三方可写的判别用例。`doc/contexts/src/config/mod.rs.md:92`「installer 5 例」与本文一致。

## 5. plan tree 一致性

`002-config-management/roadmap.md:7`（T003：安装桥接 = pi install 命令构造 + FakeInstaller 模拟双记录、单测不依赖网络）与 `README.md:11`（API 插件 → 双记录）逐条对应；`006-coding-hardening/roadmap.md:23`（500B 保尾截断 + 单测 ×3）与失败诊断面一致。无冲突。

## 6. 内部自洽

三档起始态在配置表（`:50`）与 FakeInstaller 行为语义（`:117`-125）表述一致；「不负责幂等」↔ 追加语义（push 不 dedupe / map 覆盖）；`PackageInstaller` 的 `Send + Sync` 要求 ↔ `render_all` 的 4 worker 共享 `&dyn`；「非事务」披露与 `rendered.push(res?)` 取首错一致。未发现行为矛盾（`:19` 「不解析…输出」措辞略紧，见 notes）。

## 7. 可跑（照 Runbook 从零 boot）

- 环境：`TMPDIR=/var/tmp/r21t/tmp`（`/tmp` 已 100% 满）、`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`、全新空 `CARGO_TARGET_DIR=/var/tmp/r21t/target`（从零编译）。
- 依赖获取：`cargo fetch`（`CARGO_NET_OFFLINE=true`）EXIT=0。
- 测试：`cargo test --lib config::installer` → 编译 33.67s，`running 5 tests` 全 ok，`test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 593 filtered out; finished in 0.00s`，EXIT=0。5 个用例名与文档测试表逐名一致；`filtered out` 数与文档预期输出完全吻合（编译耗时 44.18s→33.67s 属机器抖动）。
- 启动 / 健康检查 / 种子数据 / 配置项：库内模块无独立进程，N/A 理由成立。

## 8. 第三方可测标准

四档判别（命令构造的程序名/环境/参数顺序、双记录落盘、stderr 保尾/空 stderr/UTF-8 边界、骨架 vs 空对象）、错误模板逐字形状、panic 面文案、临时目录隔离与「不要真跑 `pi`」的禁忌，足以让第三方在不看源码的前提下写出等价测试。

## 结论

覆盖 5/5、doc-lint 0 采信；findings 无遗漏、plan tree 无冲突、必填节与 N-A 合规、🟢 全可回指；Runbook 从零 `cargo fetch` EXIT=0 + 5/5 EXIT=0 且输出与文档逐字吻合。**PASS**。

## 非阻塞建议

1. `:50` 的两处行号括注 inline code 断裂（``（`:101`-103`）``、``（`:98`-100`，…）``），建议规整为 ``（`:101`-103`）``；doc-lint 未报，但复制到别处会坏。
2. 数据模型表「可空」列未显式声明：建议补「非空（键缺失时由写入方补默认）」一句。
3. `:19` 「不解析 `pi` 自己的退出语义之外的输出（只看退出状态是否成功）」与失败诊断面读 stderr 尾部略有张力，建议改为「不解释 stderr 语义，只把尾部原样带进错误」。
4. `:155` 的编译耗时 44.18s 为机器相关值，建议标注「机器相关」或删除，避免第三方误判基线。
5. gaps 的 ②（无超时）与 ④（env/前缀属 pi 契约）可在 `PiInstallBridge` 生命周期段补一句「无超时，渲染耗时无上界，见 `_meta/gaps.md`」，使运维口径显式化。
