---
type: module
module_path: src/config/mod.rs
unit: unit-src-26
description: 配置模块的门面——声明 installer/manifest/render/skill_assets 四个子模块并转发 8 个公开名字
---

# src/config/mod.rs

crate::config 的门面（13 行）：四个 `pub mod` 声明 + 三条 `pub use` 转发。本文件**没有任何实现**——所有行为都在子模块里；它的作用是给「配置子系统」一个稳定入口，并决定哪些名字从 config 模块面直接可见。

上位口径（文件头注释 `src/config/mod.rs:1`-4）：配置即代码——**清单为真源、profile 目录是渲染产物**；模板层写死（提示词/必装四件套/skills/MCP/Key），模型层可变（模型池 + Agent 引用）。

## 职责

**负责**：
- 声明配置子系统的四个子模块：`installer`（`:6`）、`manifest`（`:7`）、`render`（`:8`）、`skill_assets`（`:9`）。
- 定义转发面：8 个名字（`build_install_command`、`PackageInstaller`、`PiInstallBridge`、`AgentManifest`、`ConfigManifest`、`ModelEntry`、`ProviderDef`、`Renderer`）从 config 模块面直接可见（`:11`-13`）。

**不负责**：不实现任何配置逻辑（渲染、装包、清单、skill 文本分别在子模块）；不做子模块之间的类型转换或再导出裁剪之外的加工；**不转发 `skill_assets` 的任何符号**（该模块只被 `render` 内部使用，`src/config/render.rs:10`）；不转发 installer 的 `FakeInstaller` 与 manifest 的常量/方法（消费者走子模块路径，见下）。

## 公开成员

四个子模块（各自的符号面与行为见其单元文档，本文不重复）：

| 成员 | 类型 | 文档 |
|---|---|---|
| `installer` | 子模块（`src/config/installer.rs`，unit-src-24） | [installer](#installer) |
| `manifest` | 子模块（`src/config/manifest.rs`，unit-src-25） | [manifest](#manifest) |
| `render` | 子模块（`src/config/render.rs`，unit-src-27） | [render](#render) |
| `skill_assets` | 子模块（`src/config/skill_assets.rs`，unit-src-29） | [skill_assets](#skill_assets) |

经本文件转发的 8 个名字（`src/config/mod.rs:11`-13`）：

| 名字 | 来源 | 定义处 |
|---|---|---|
| `build_install_command` | `installer` | `src/config/installer.rs:12` |
| `PackageInstaller` | `installer` | `src/config/installer.rs:47` |
| `PiInstallBridge` | `installer` | `src/config/installer.rs:52` |
| `AgentManifest` | `manifest` | `src/config/manifest.rs:122` |
| `ConfigManifest` | `manifest` | `src/config/manifest.rs:145` |
| `ModelEntry` | `manifest` | `src/config/manifest.rs:96` |
| `ProviderDef` | `manifest` | `src/config/manifest.rs:111` |
| `Renderer` | `render` | `src/config/render.rs:29` |

> **行号稳定性**：`src/config/manifest.rs` 的 ARC-R4「提示词外置」已收官（13/13 常量改为 `include_str!`），文件定型在 948 行，四个类型现落在 `:96`（`ModelEntry`）/`:111`（`ProviderDef`）/`:122`（`AgentManifest`）/`:145`（`ConfigManifest`）。历史上该文件随提示词外置批次从 1504 → 1159 → 948 行，四个类型的行号整体上移过两轮——若再整批外置/回填提示词，这里会再次漂移。复核方式：`grep -n '^pub struct \(ModelEntry\|ProviderDef\|AgentManifest\|ConfigManifest\)' src/config/manifest.rs`。`src/config/installer.rs`、`src/config/render.rs`、`src/config/skill_assets.rs` 不在该重构范围内，行号稳定。

## 依赖

- 上游 / 消费方（**经 config 模块面**）：
  - `src/main.rs:158` 调 `ConfigManifest` 的 `load`、`:160` `builtin`、`:161` `save`——启动装配清单。
  - `src/server/mod.rs:27` `use crate::config::ConfigManifest`（`ServerState` 的 manifest 字段）。
  - `src/server/handlers.rs:14` `use crate::config::ConfigManifest`；`:675` `crate::config::Renderer { … }`（渲染线程体）。
  - `src/server/tests/mod.rs:11`、`tests/f6_observability/fixture.rs:27` 也只用 `ConfigManifest`。
- 走**子模块路径**（不经本文件转发面）的消费方：`src/main.rs:201`、`src/server/mod.rs:26`、`src/server/handlers.rs:699`、`src/server/tests/mod.rs:134`、`tests/f6_observability/fixture.rs:31` 用 installer 子模块路径（`PackageInstaller` / `PiInstallBridge` / `FakeInstaller`）；`tests/change_ask_ring.rs`、`tests/change_review_protocol/main.rs:25` 用 manifest 子模块路径。
- 向下依赖：本文件不 `use` 任何东西（零依赖）；子模块依赖方向单向——`render → installer`（`src/config/render.rs:8`）、`render → manifest`（`:9`）、`render → skill_assets`（`:10`），`installer` / `manifest` / `skill_assets` 互不引用。

## 核心口径

- **转发面只转 8 个名字**：1 个 trait + 1 个结构体（`Renderer`）+ 1 个结构体族（清单四类型）+ 1 个纯函数 + 1 个零尺寸真桥结构体。🟢 CONFIRMED 逐条定位——`src/config/mod.rs:11` 转 `build_install_command`（定义 `src/config/installer.rs:12`）、`PackageInstaller`（`:47`）、`PiInstallBridge`（`:52`）；`:12` 转 `ModelEntry`（`src/config/manifest.rs:96`）、`ProviderDef`（`:111`）、`AgentManifest`（`:122`）、`ConfigManifest`（`:145`）；`:13` 转 `Renderer`（`src/config/render.rs:29`）。新增配置符号默认走子模块路径这一「约定」是 🟡 INFERRED（依据：现有 3 条 `pub use` 的取舍无书面依据，见 [_meta/gaps.md](../../_meta/gaps.md)）。
- **`skill_assets` 不转发**：它被 `render` 直接 `use`（🟢 `src/config/render.rs:10`：`use crate::config::skill_assets::builtin_skill_files;`）；转发面只有三条 `pub use`，不含它——🟢 CONFIRMED，可重放：`grep -n '^pub use' src/config/mod.rs` → 只输出 `:11`、`:12`、`:13` 三行，无 `skill_assets`。
- **`FakeInstaller` 不在转发面**：🟢 CONFIRMED `FakeInstaller` 定义在 `src/config/installer.rs:71`，而 `src/config/mod.rs:11` 只列 `build_install_command` / `PackageInstaller` / `PiInstallBridge`；测试一律走子模块路径（`src/server/handlers.rs:699`、`src/server/tests/mod.rs:134`、`tests/f6_observability/fixture.rs:31`），故测试桥不进生产可见面。
- **本文件零逻辑**：🟢 CONFIRMED 行构成——13 行 = 4 行注释（`src/config/mod.rs:1`-4`）+ 4 行 `pub mod`（`:6`-9`）+ 3 行 `pub use`（`:11`-13`）+ 2 行空行；文件内无 `fn` / `impl` / `const` / `static`。「改它不改变运行时行为、只改变可导入名」是 🟡 INFERRED（依据：无任何可执行条目，全部是对子模块条目的引用）。
- **编译期耦合**：`skill_assets` 的常量是 `render` 的编译期依赖（`src/config/render.rs:10` 直接 `use`），所以新增/改名 skill 资产必须与 render 同一次提交落地。🟡 **INFERRED**（依据：`use crate::config::skill_assets::builtin_skill_files` 是编译期符号引用，名字不一致即编译失败——这一点由编译器保证，仓内无独立文档声明）。

---

### installer

- 职责：包安装桥接（`pi install` 一把梭 + 假桥模拟双记录）。符号面与行为见 [installer.rs](./installer.rs.md)（unit-src-24）——本文只给摘要，不重复。
- 公开成员：`build_install_command`（`src/config/installer.rs:12`）、`PackageInstaller`（`:47`）、`PiInstallBridge`（`:52`）、`FakeInstaller`（`:71`，**不在转发面**）。
- 依赖：被 `render` 使用（`src/config/render.rs:8`、`:222` 调 `install`）；经本文件转发 3 个名字（`src/config/mod.rs:11`）。

### manifest

- 职责：配置清单真源（必装包/差异化包/MCP/13 份提示词/模型池 + 内置 15 Agent）。符号面与行为见 [manifest.rs](./manifest.rs.md)（unit-src-25）——本文只给摘要，不重复。
- 公开成员：5 个包常量、`McpServer` + `codegraph_mcp` / `context7_mcp`、13 份提示词常量、`ModelEntry` / `ProviderDef` / `AgentManifest` / `ConfigManifest`、`builtin` / `resolve_model` / `resolve_provider` / `get_agent` / `save` / `load`。本文件只转发其中 4 个类型（`src/config/mod.rs:12`）。
- 依赖：被 `render` 使用（`src/config/render.rs:9`）；被 `main` / `server` / `handlers` 经转发面使用（`ConfigManifest`）。

### render

- 职责：清单 → profile 目录的渲染器（AGENTS.md / settings / mcp.json / skills / 装包 / models.json / auth.json）。符号面与行为见 [render.rs](./render.rs.md)（unit-src-27）——本文只给摘要，不重复。
- 公开成员：`Renderer`（`src/config/render.rs:29`，本文件唯一转发的 render 名字）、`render_agent`（`:38`）、`render_all`（`:239`）。
- 依赖：依赖 `installer` / `manifest` / `skill_assets` 三个兄弟模块（`src/config/render.rs:8`-10）；被 `src/server/handlers.rs:675` 的渲染线程体使用。

### skill_assets

- 职责：内嵌 skill 文本资产（skill 内容编译进二进制）。符号面与行为见 [skill_assets.rs](./skill_assets.rs.md)（unit-src-29）——本文只给摘要，不重复。
- 公开成员：`PLAN_TREE_SKILL_MD`（`src/config/skill_assets.rs:7`）、`DOC_CONTRACT_SKILL_MD`（`:10`）、`CODING_DISCIPLINE_SKILL_MD`（`:13`）、`builtin_skill_files`（`:18`）、`builtin_language_files`（`:74`）——前三个都是 `include_str!` 内嵌的 skill 正文。
- 依赖：只被 `render` 使用（`src/config/render.rs:10`）；**本文件不转发它**，故 config 模块面上看不到任何 skill 资产符号。

## 测试覆盖

本文件无 `#[cfg(test)]`、无 `#[test]`（`src/config/mod.rs` 全文 13 行）。模块树的测试归属（实测按模块过滤计数）：installer 5 例、manifest 15 例（含 1 个 `#[ignore]` 生成器）、render 22 例、skill_assets 6 例——合计 48 项（47 passed + 1 ignored），各自见 [installer.rs](./installer.rs.md) / [manifest.rs](./manifest.rs.md) / [render.rs](./render.rs.md)（skill_assets 属 unit-src-29）。

## 测试运行与环境

- 因为本文件无测试，验证方式用**整个 config 模块树**：
  ```bash
  TMPDIR=<宽裕临时目录> RUSTUP_HOME=<rustup 根> RUSTUP_TOOLCHAIN=stable \
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=<仓外独立空目录> \
  cargo test --lib config::
  ```
  实测输出（本轮实跑，机器相关：从零编译 37.64s，EXIT=0）：`test result: ok. 47 passed; 0 failed; 1 ignored; 0 measured; 550 filtered out; finished in 0.16s`（`1 ignored` = manifest 里的 `dump_change_agents_fixture` 生成器；`filtered out` 随仓内测试总数变化）。
- **环境前提**：edition 2024（`Cargo.toml:4`）⇒ Rust 1.85+ 且 toolchain 可解析（rustup 无默认 toolchain 时显式 `RUSTUP_TOOLCHAIN=stable`）。**依赖获取**：`cargo fetch`（首次需网络；本模块树测试用 `tempfile`，`Cargo.toml:27`）。
- **隔离性**：installer 与 render 的用例用 `tempdir`，不联网、不开端口；manifest 的 `manifest_json_roundtrip` 同理。唯一有副作用的是被 `#[ignore]` 的夹具生成器（会写 `tests/e2e/fixtures/change_agents.json`）。
### 从零 boot（端到端取证）

本文件是进程启动的必经门面：`src/main.rs:157`-161 经本文件转发的 `ConfigManifest` 装配清单（有 `<root>/manifest.json` 就 `load`，否则 `builtin()` + `save`），`src/main.rs:201` 经 installer 子模块路径把装包桥注入状态机。因此「进程能不能起来、清单能不能落盘」就是本单元的运行验证。

以下命令本轮实跑（`$D=/var/tmp/dfz-src26d`，端口取随机高位、运行根全隔离、进程按 PID 停）：

**第一段：装依赖 → 配置 → 启动 → 就绪 → 健康检查 → 种子数据**

```bash
D=/var/tmp/dfz-src26d
mkdir -p $D/home $D/tmp

# 0) 依赖 + 构建（在仓库根执行；仓外独立 target，不污染仓库）
#    RUSTUP_HOME 填本机 rustup 根（本机 = /home/trtyr/.rustup；服务账号的 HOME 可能不同，
#    所以不要图省事写成 $HOME/.rustup，会撞上"missing manifest in toolchain"）
TMPDIR=$D/tmp RUSTUP_HOME=/home/trtyr/.rustup RUSTUP_TOOLCHAIN=stable \
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=$D/target cargo build
# → Finished `dev` profile … in 29.89s（从零；增量重跑 <1s；机器相关）

# 1) 配置：隔离运行根（HOME 决定清单根；DEVFORGE_HOME 只改观测/日志根）
export HOME=$D/home
export DEVFORGE_HOME=$D/home/.devforge
export DEVFORGE_ADDR=127.0.0.1:47531
export DEVFORGE_TOKEN=boot-probe-token

# 2) 启动，pid 落 pidfile（停止只按 PID，禁用 pkill/killall 宽匹配）
$D/target/debug/devforge > $D/server.log 2>&1 &
echo $! > $D/server.pid

# 3) 等就绪（最多 20s；进程起监听有毫秒级延迟，直接 curl 会拿到 000）
for i in $(seq 1 40); do
  code=$(curl -s -o /dev/null -w '%{http_code}' -H "x-devforge-token: $DEVFORGE_TOKEN" \
    http://127.0.0.1:47531/api/v1/config/manifest)
  [ "$code" = "200" ] && break
  sleep 0.5
done

# 4) 健康检查：就绪 = 清单接口 200 + 日志行「devforge API 就绪」
curl -s -o $D/manifest.json -w '%{http_code}\n' -H "x-devforge-token: $DEVFORGE_TOKEN" \
  http://127.0.0.1:47531/api/v1/config/manifest          # → 200
curl -s -o /dev/null -w '%{http_code}\n' \
  http://127.0.0.1:47531/api/v1/config/manifest          # → 401（缺 token）

# 5) 种子数据：首次 boot 自动落盘内置清单
python3 -c 'import json,sys;d=json.load(open(sys.argv[1]));print(len(d["agents"]),len(d["model_pool"]),len(d["providers"]),len(d["base_packages"]))' "$D/manifest.json"
# → 15 3 1 4（15 Agent / 3 模型 / 1 provider / 四件套）
ls $D/home/.devforge     # → logs  manifest.json（profile 目录启动时不建）
```

**第二段：停止 → 二次启动（走 load 分支）→ 再停止**

```bash
D=/var/tmp/dfz-src26d                      # 与第一段同一个运行根
export DEVFORGE_TOKEN=boot-probe-token     # 只用于健康检查的鉴权头

# 6) 停止（只按 PID）
kill $(cat $D/server.pid)

# 7) 二次启动走 load 分支：清单文件 mtime/size 不变
$D/target/debug/devforge > $D/server2.log 2>&1 &
echo $! > $D/server.pid
curl -s -o /dev/null -w '%{http_code}\n' -H "x-devforge-token: $DEVFORGE_TOKEN" \
  http://127.0.0.1:47531/api/v1/config/manifest          # → 200
stat -c '%Y %s' $D/home/.devforge/manifest.json          # 与步骤 5 前记下的值相同
kill $(cat $D/server.pid)
```

本轮实测结果：

| 步骤 | 预期 | 实测 |
|---|---|---|
| 构建 | 二进制产出、退出码 0 | `Finished dev profile … in 29.89s`（从零，机器相关） |
| 启动日志 | 两行就绪：`devforge 启动`（`src/main.rs:153`）+ `devforge API 就绪 addr=…`（`src/server/http.rs:80`） | 两行都在，addr=`127.0.0.1:47531` |
| 健康检查（带 token） | `200` + 清单 JSON | `200`，响应体 52970 字节（`get_manifest` 返回紧凑 JSON）；落盘清单文件 56391 字节（`save` 走 `to_string_pretty`）——两者**不是同一个数** |
| 健康检查（无 token） | `401` | `401`（`src/server/handlers.rs:82`） |
| 种子数据 | 清单文件 = 15/3/1/4 | `15 3 1 4` |
| 运行根产物 | `<root>/logs` + `<root>/manifest.json`，无 `<profiles_root>/` | 一致（`<profiles_root>/` 要 `PUT /config/manifest` 触发渲染才出现，见 [render.rs](./render.rs.md)） |
| 二次 boot | 清单文件 mtime/size 不变（走了 `load` 而不是 `builtin`+`save`） | 本轮 `1792834435 56391`，重启前后同值（mtime 每轮不同，判据是「前后相等」） |
| 停止 | `kill $(cat pidfile)` 后端口释放、无残留进程 | 终态 `ss -ltnp | grep 47531` 无输出（端口已释放）；二次 boot 能重新绑定同一端口即第一次确已退出 |

复跑记录（HEAD 184abb8，仓外 `mktemp -d` 运行根，端口 47531）：`cargo build` 从零 52.44s、EXIT=0；启动后 200 就绪、无 token 401；GET 响应体 52970 字节 / 落盘 56391 字节；种子 `15 3 1 4`；就绪日志两行命中；mtime 重启前后同值 `1792836825 56391`；两次 `kill` 后端口均无监听（`ss` 计数 0）。

### Runbook 边界（N/A 与理由）

- **本文件自身**：不读任何配置键、无端点、无后台线程 → 配置项 / 公开接口两项在本文件层面为 `N/A`；上面的 boot 流程给的是「本文件参与的进程级」证据。
- **进程级配置项**（由 `main` 读，不在本文件）：`HOME` → 运行根 `<root>` = `$HOME/.devforge`（`src/main.rs:143`-144）；`DEVFORGE_HOME` → 只改观测/日志根（`src/observability/mod.rs:334`；回退 `<home>/.devforge` 在 `:344`-`347`），**不改清单根**；`DEVFORGE_ADDR` → 监听地址，默认 `127.0.0.1:7888`（`src/main.rs:229`-230）；`DEVFORGE_TOKEN` / `--token` → 鉴权（`src/main.rs:215`-218、`src/server/http.rs:25`）。非回环地址不配 token 会拒绝启动（`src/main.rs:235`-241）。
- **健康检查**：没有专用健康端点（`src/server/http.rs:32`-72 的路由表里只有业务/可观测/配置三类路由）；就绪判据用 `GET /api/v1/config/manifest`（`src/server/http.rs:66`-69）返回 200，或看日志行 `devforge API 就绪`（`src/server/http.rs:80`）。
- **种子数据**：首次 boot 生成 `<root>/manifest.json`（`src/main.rs:157`-161，内容 = `manifest` 单元的 `builtin()`：15 Agent / 3 模型 / 1 provider / 四件套）；`<profiles_root>/` 目录由渲染产生，属 [render.rs](./render.rs.md) 的 Runbook。
- **测试运行与隔离**：见上一节；测试不依赖 boot，两者互不干扰。
- **公开接口清单**：本文件只提供导入面（上表 8 个名字）；进程级 HTTP 面是 `GET`/`PUT /api/v1/config/manifest`（`src/server/http.rs:66`-69，鉴权头 `x-devforge-token`，`src/server/handlers.rs:80`-82）。
