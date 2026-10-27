# 文档审查报告 — unit-src-27（第 25 轮）

- 单元：`unit-src-27` → `src/config/render.rs`；文档 `docs/contexts/src/config/render.rs.md`（218 行，frontmatter `type: module` / `unit: unit-src-27`）
- 引擎审计（确定性，采信）：覆盖 4/4、缺失 0；doc-lint EXIT=0（0 失败 / 2 提示，提示位置 `:190`、`:195`）
- 输入边界（DR-12）：未打开 `src/config/render.rs`；事实以单元文档、`_meta/facts/unit-src-27.md`、`_meta/gaps.md`、`_meta/state.json`、plan tree、引擎审计与本轮实跑输出为准

## 1. 覆盖与 findings 处置

- 覆盖 4/4 采信。文档「公开成员」表承接 `Renderer` / `render_agent` / `render_all`，「内部条目」表承接 4 个私有符号；22 个测试逐条列表，覆盖口径与 `_meta/state.json` 的 26 项（module×1 + symbol×3 + behavior×22）一致。
- 工作区无 `testing/findings/`，kickoff 未附本单元 doc-side findings → 待处置清单 N/A。
- `gaps.md` 本单元 4 条（auth.json 无清理、两次 render_all 无互斥、渲染非事务且 skills 只增不减、在途重构瞬时打断编译）均在「核心口径 / 证据与缺口」显式承接，无漏项。

## 2. 完整性与 N/A

- 模块三件（职责 / 公开成员 / 依赖）齐备，「不负责」划出 installer、HTTP、事务边界。
- 公开类与两个公开函数按契约节齐备（职责/构造/成员/生命周期/用法；摘要/语义/参数/返回/异常/示例）；私有符号满足「摘要+行为语义」。
- 落盘产物表给出字段、写入条件、生命周期、迁移注意，索引写 `N/A`；数据模型「关系」由「与幂等的关系」列承担。
- Runbook 七件套均有落点：环境（edition 2024 ⇒ Rust 1.85+、`RUSTUP_TOOLCHAIN=stable`）、依赖（`cargo fetch`、`tempfile`、离线开关）、配置项（N/A，给了理由与两处编译期常量）、启停+健康检查（N/A，库内模块，并指到 `PUT /config/manifest` 后台线程）、种子数据（N/A，内存清单）、测试运行（完整命令块）、公开接口清单（N/A，Rust API 已列）。所有 N/A 均带理由，未见省略。
- 未发现超出契约的缺口；lint 2 条「未完成标记」提示实为用例名 `coding_packages_carry_todo_and_lens` 的小写 `todo`，契约 §六明确禁止项扫描只记提示、不判失败，且文档 `:195` 已就地说明。

## 3. 真实性与置信度

- 所有 🟢 断言均给出 `src/config/render.rs:line` 或可重放命令；私有条目 `runtime_settings/:15`、`MAX_RENDER_CONCURRENCY/:27`、`recorded_packages/:277`、`copy_dir/:296` 均有行号。实体存在性采信 lint（0 失败）。
- 内部自洽：测试总数 22 ↔ 用例表 22 行 ↔ 实跑 22；`15` 个 profile ↔ boot 清单 `agents=15`；四件套 `4` ↔ boot 清单 `base_packages=4`；3 个测试计数（22 / 47 / 48）互相一致；链接与 7 个锚点均可解析。
- 置信度诚实：auth 无清理、并发无互斥、渲染非事务、编译窗口四条均以 🟡 + 依据 + gaps 回指，未把推断冒充 🟢。

## 4. 可跑（本轮实跑）

- 测试命令照 Runbook 原样执行（`TMPDIR=/var/tmp/r25boot/tmp`、`RUSTUP_HOME=/home/trtyr/.rustup`、`RUSTUP_TOOLCHAIN=stable`、`CARGO_NET_OFFLINE=true`、仓外空 `CARGO_TARGET_DIR=/var/tmp/r25boot/target`）：`cargo test --lib config::render` → `22 passed; 0 failed; 0 ignored; 576 filtered out`，EXIT=0，与文档逐字吻合。
- 交叉证据：`cargo test --lib config::` → `47 passed; 0 failed; 1 ignored; 550 filtered out`，EXIT=0，与文档一致。
- 当前 HEAD `0f219cb`（文档证据 HEAD `184abb8`），`git diff 184abb8..HEAD -- src/config/render.rs src/config/installer.rs src/config/mod.rs Cargo.toml` 为空，故测试值可直接沿用。
- 宿主级 boot（文档把启停标 N/A，属库内模块；此步为补充证据，非文档 Runbook 要求）：隔离 `HOME=/var/tmp/r25boot/home`、`DEVFORGE_HOME=$HOME/.devforge`、`DEVFORGE_ADDR=127.0.0.1:47531`、token；日志两行「devforge 启动」+「API 就绪」；带 token `200`（52970 字节）、无/错 token `401`；落盘 `manifest.json` 56391 字节 + `logs/`；种子 `base_packages=4 / providers=1 / model_pool=3 / agents=15`；kill 后端口释放。
- 结论：单元 Runbook 的可执行部分从零可复现，未发现阻断点。

## 5. plan tree 与第三方可测

- P002「清单为真源 / profile 为渲染产物 / 模板层写死 + 模型层可变 / PUT 触发全量重渲染 / Key 明文」与文档上位口径一致，无冲突；P002 五类配置在本模块均有落盘产物对应。
- 第三方可据文档写出：`Renderer` 构造与两个方法的参数/返回/错误、7 类产物形状与写入条件、幂等三条件、并发顺序保证、22 个行为用例的断言面、测试运行与隔离方式。测试面充分。

## 6. 非阻塞建议

1. lint 的 2 条提示分别命中用例名行 `:190` 与说明行 `:195`。说明行再次写全名导致第二次命中；可改为「该用例名含待办工具包英文拼写」之类的指代，把提示降到 1 条（不改用例名）。
2. 文档与 `_meta/facts/unit-src-27.md` 的实现行区间存在漂移（如 auth 文档 `:138-144` / facts `:136-141`，WARN `:148-154` / `:143-151`，mcp `:160-165`、`:166-195` / `:157-161`、`:155-193`，handlers 调用 `:680` / `:682`）。测试行号则完全一致。建议用一次 grep 全量重核后刷新 facts，避免两份文档长期各说各话。
3. 「不负责：不算该装哪些包」与行为语义「组包」措辞可收紧为「不决定包集合，只做 `base_packages + extra_packages` 拼接，真源在 manifest」，消除字面张力。
4. 证据 HEAD 仍写 `184abb8`；本轮已在 `0f219cb` 复现同值，建议证据块补一句「`0f219cb` 复跑同值」。
5. 建议为落盘产物表补一行「关系：各自独立文件，唯一跨文件耦合是 `settings.packages` ↔ `npm/package.json` 的幂等判据」，把数据模型「关系」节显式化。

## 结论

覆盖 4/4、lint 0 失败（2 条提示按契约不判失败）采信；findings 无遗漏；必填节与 N/A 合规；🟢 均可回指；Runbook 测试命令本轮从零复现（22/0/0/576，EXIT=0），宿主 boot 正常。判 **PASS**。
