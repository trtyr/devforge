# D001 · F2 根因定论：pi-lens 安装失败 = /tmp ENOSPC 被上游吞错伪装，非上游缺陷

- 日期：2026-10-18
- 状态：已定论（本机实证）
- 关联：P005 e2e 缺陷 F2-PI-LENS-NPM-FAIL（tests/e2e/TEST_REPORT.md v3 §四）

## 现象（昨晚 e2e 记录）

`pi install npm:pi-lens` 失败，根因报错：

```
Failed to move @ast-grep/cli binaries into place.
```

连锁：npm install 整包失败 → pi install 失败 → devforge PiInstallBridge 报错 → render_all 中断 → 后续 profile 不渲染。

## 根因链（三环）

1. **e2e HOME 建在 /tmp**：common.py 用 `tempfile.mkdtemp(prefix="devforge-e2e-")`，安装目标
   （`<HOME>/.devforge/profiles/<agent>/npm/node_modules`）全在 /tmp tmpfs 上。
2. **昨晚 /tmp 被塞满**：历届 e2e 家目录与会话累积，04:30 清理前 **0 字节可用**（清理后回血 3.8G）。
   03:28 前后 test_11 的安装恰好跑在现场已满/将满的 tmpfs 上。
3. **上游吞错放大**：@ast-grep/cli 的 postinstall.js 把 52M 原生二进制 link/copy 进位，
   `installBinary` 的 linkSync 与 copyFileSync 双双 `catch (_)`——**真实 errno（ENOSPC）被丢弃**，
   只剩谜语 "Failed to move @ast-grep/cli binaries into place."，看似上游缺陷实为磁盘满。

## 本机实证（2026-10-18）

| 实验 | 结果 |
|------|------|
| 干净目录裸装 `npm install @ast-grep/cli`（/tmp 3.8G 余量） | ✅ 5s 成功（cli + cli-linux-x64-gnu 两包就位） |
| 真实路径 `PI_CODING_AGENT_DIR=<profile> pi install npm:pi-lens` | ✅ 6s 成功，"Installed npm:pi-lens"（15 包） |
| ENOSPC 铁证：1M tmpfs 挂载后装同包 | ❌ 复现装挂——`TAR_ENTRY_ERROR ENOSPC` 后报同族谜语变体 "Failed to locate @ast-grep/cli native binary."（extraction 路径；昨晚的 "Failed to move" 是 move/copy 路径的姊妹变体——上游两处 catch 吞错所致） |

postinstall 源码要点（node_modules/@ast-grep/cli/postinstall.js）：`detectPackageName()` → 平台包
`@ast-grep/cli-linux-x64-gnu` → `resolveBinaryDir()` 找到 52M 二进制 → `installBinary()`:
`fs.linkSync` 失败回落 `fs.copyFileSync`，**两层 catch 均丢弃 errno** → 统一谜语报错。

## 结论与可行安装路径

- **F2 非上游缺陷、非环境顽固问题**：/tmp 有余量时，裸装与真实 pi install 路径均秒级成功。
- **可行安装路径（已验证）**：保证安装目标盘余量 ≥ ~200M（52M 二进制 + npm 提取峰值）后正常
  `pi install npm:pi-lens` 即可。
- **运维防线（下轮 e2e 校准点）**：e2e HOME 宜迁至 /var/tmp（根分区 880G），或在 campaign 启动时
  预检 /tmp 余量；本轮不改 e2e 脚本（coding 阶段纪律）。
- **产品侧硬化（本轮落地）**：PiInstallBridge 失败时只报 `pi install {pkg} 失败（exit: N）`，
  stderr 全丢——同族排障以后还会抓瞎。本轮给安装失败错误附带 stderr 尾部摘要（带单测），
  下次"为什么装不上"一眼可见。
