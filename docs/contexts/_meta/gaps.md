# 缺口与未覆盖登记（🔴 GAP 台账 · D013）

规则：任何在文档里标 🔴 的结论必须在此登记一条（含影响面）；缺一条即视为静默缺口。
本文件按单元追加，勿覆盖既有小节。

## unit-src-01（`src/config/installer.rs` / `src/config/manifest.rs`）

| # | 缺口 | 影响面 | 现状 / 可解法 |
|---|---|---|---|
| 1 | 本单元的 Rust 单测未在本机执行——仓库是 Rust 工程（有 `Cargo.toml`），但本机 `rustup toolchain list` 输出为 `no installed toolchains`，无法编译运行 `cargo test`。 | 文档中关于单测断言的描述只到「源码写了什么断言」这一层，不能声称「断言当前为真」。 | 可解法：装好 toolchain 后跑 `cargo test config::` 复核；本轮所有测试相关结论均以单测代码行号为证据。 |
| 2 | `pi install` 的真实双记录未实测（本机已装 pi，但真实装包会联网取包并改 pi 配置目录，本轮未执行）。 | 「双记录」的真实形状来自 pi 的行为，devforge 侧只做模拟（`FakeInstaller`）；真桥与假桥写出的记录是否逐字一致，本仓代码里看不出来。 | 依据只到 pi 官方文档层面：个人安装写进配置目录的 `settings.json`（`/home/trtyr/.local/lib/node_modules/@earendil-works/pi-coding-agent/docs/packages.md:19`）；可解法：在装有 pi 的机器上跑一次真桥，比对 `<profile>/settings.json` 与 `<profile>/npm/package.json` 的实际内容。 |
| 3 | 清单里的 7 段提示词常量篇幅大且写死在代码里，其**设计依据**只以注释里的决策编号（P001/P005/P007/P008）出现，正文不含出处。 | 第三方只靠本文档无法知道每条纪律为什么存在、改动要连带动哪份 plan。 | 已在本单元文档给出「每段提示词管什么 + 落点行号」与仓库内决策记录路径 `docs/plantree/plans/002-config-management/decisions/README.md`（引用了决策编号，未逐条复述其结论）。 |
| 4 | `ConfigManifest` 反序列化时缺字段的容错边界（如落盘清单少了 `api_key` 或某个 Agent 条目）在本仓无单测覆盖。 | 外部手改 `<home>/.devforge/manifest.json` 后启动失败时的错误信息，代码里只能看到 serde 的通用报错；「新增 `Option` 字段是否真的向后兼容」也缺实测。 | 可解法：补一条「缺字段加载」单测；本轮按 serde 派生语义推断（`Option` 字段缺失按 `None` 处理、其余字段缺失报 missing field），已在文档中标 🟡。 |
| 5 | 两个内置 MCP server（`codegraph` 与 `npx @upstash/context7-mcp`）的连通性、版本与鉴权未实测。 | 本单元只负责把 server 写进 `<agent-dir>/mcp.json`，不负责拉起；「mcp.json 渲染正确」不等于「MCP 可用」。 | 可解法：在渲染出的 profile 里跑一次 MCP 握手；本轮只核对命令与参数的字面配置（`src/config/manifest.rs:54`、`src/config/manifest.rs:64`）。 |
