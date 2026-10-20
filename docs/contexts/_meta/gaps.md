# 缺口与未覆盖登记（🔴 GAP 台账 · D013）

规则：任何在文档里标 🔴 的结论必须在此登记一条（含影响面）；缺一条即视为静默缺口。
本文件按单元追加，勿覆盖既有小节。

## unit-src-01（`src/config/installer.rs` / `src/config/manifest.rs`）

| # | 缺口 | 影响面 | 现状 / 可解法 |
|---|---|---|---|
| 1 | 本单元的 Rust 单测未在本机执行——仓库是 Rust 工程（有 `Cargo.toml`），但本机 `rustup toolchain list` 输出为 `no installed toolchains`，无法编译运行 `cargo test`。 | 文档中关于单测断言的描述只到「源码写了什么断言」这一层，不能声称「断言当前为真」。 | 可解法：装好 toolchain 后跑 `cargo test --lib config::` 复核；本轮所有测试相关结论均以单测代码行号为证据。 |
| 2 | `pi` 命令行程序本机未安装，`PiInstallBridge` 的真机行为（`pi install` 的真实退出码、stderr 形态、双记录的真实落盘位置）未实测。 | 「双记录」的真实形状来自 pi 的行为，devforge 侧只做模拟（`FakeInstaller`）；真桥与假桥的记录是否完全一致，代码里看不出来。 | 可解法：在装有 pi 的机器上跑一次真桥，比对 `<profile>/settings.json` 与 `<profile>/npm/package.json` 的实际内容。 |
| 3 | 清单里的提示词常量（`CODING_AGENT_PROMPT` 等 7 个）篇幅大且写死在代码里，其**设计依据**只以注释里的决策编号（P001/P005/P007/P008）出现，正文不含出处。 | 第三方只靠本文档无法知道每条纪律为什么存在、改动要连带动哪份 plan。 | 已在本单元文档给出「小节标题清单 + 仓库内决策记录路径」；决策原文见 `docs/plantree/plans/008-doc-from-zero/decisions/README.md` 与 `docs/plantree/plans/002-config-management/decisions/README.md`（引用了决策编号，但未逐条复述其结论）。 |
| 4 | `ConfigManifest` 反序列化时缺字段的容错边界（如落盘清单少了 `api_key` 或某个 Agent 条目）在本仓无单测覆盖。 | 外部手改 `<home>/.devforge/manifest.json` 后启动失败时的错误信息，代码里只能看到 serde 的通用报错。 | 可解法：补一条「缺字段加载」单测；本轮按 serde 派生语义推断（`Option` 字段缺失按 `None` 处理，其余字段缺失报 missing field），已在文档中标 🟡。 |
