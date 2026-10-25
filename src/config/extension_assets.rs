//! path-guard 插件资产（P012 topic 08）：编译期内嵌，渲染时写出。
//!
//! 产品化纪律（topic 08 §六）：非源码部署下二进制自带插件全文——**绝不运行时依赖
//! 仓库目录**。pi 侧 jiti 免编译，渲染写出的 `.ts` 文本即可执行。
//! match 的 glob 语义与引擎 `path_covered_by`（src/domain/contract.rs）同构，
//! 样例集 `test-cases.json` 由两端测试共同驱动（见 contract.rs 的样例集测试）。

/// 插件运行件（分发到 `<profile>/extensions/path-guard/` 的相对路径 → 正文）。
/// `match.test.ts` 是开发件，不分发（pi 只加载 index.ts 入口）。
pub const PATH_GUARD_FILES: &[(&str, &str)] = &[
    (
        "index.ts",
        include_str!("../../assets/extensions/path-guard/index.ts"),
    ),
    (
        "match.ts",
        include_str!("../../assets/extensions/path-guard/match.ts"),
    ),
    (
        "config.ts",
        include_str!("../../assets/extensions/path-guard/config.ts"),
    ),
];
