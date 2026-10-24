# 05 · 沙箱化 Spawn（Sandboxed Spawn）

> 前置硬需求：worktree 隔离文件但不隔离权限——change agent 有 bash，能摸宿主凭证/网络/其他仓。
> 并行 = 每 agent 爆炸半径相乘，所以沙箱化是底座的安全地基，不是可选项。

## 选型评估（2026-10-24，用户拍板前调研）

### 要不要沙箱？

**要，但时机在 M1 之后。** 现阶段单任务串行、agent 跑自家 SOP，风险低；真正的需求是并行化之后：
N 个 agent 各持 bash 无人值守跑通宵，爆炸半径相乘，一个失控命令可以同时砸掉所有 worktree。
不做沙箱的并行底座 = 赌每个 agent 都不犯错。

### 候选全景（pi 原生四条 + 外部库一条）

| 方案 | 归属 | 文件面 | 凭证面 | 网络面 | 资源面 | 程序化 RPC | 判定 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| **Plain Docker** | pi 原生文档一等公民 | ✅ 挂载即硬隔离 | ✅ env 白名单 | ⚠️ bridge 全通 | ✅ cgroup | ✅ stdin/stdout 原样 | **主方案** |
| Docker Sandboxes (sbx) | pi 原生（Docker 官方） | ✅ | ✅✅ 凭证代理 | ✅ | ✅ | ❌ CLI 工作流，非长驻 RPC 形状 | 弃 |
| OpenShell (NVIDIA) | pi 原生（外部门产品） | ✅ | ✅ | ✅✅ 策略化 | ✅ | ⚠️ gateway 依赖重 | 二期远程时代再评 |
| Gondolin (pi 作者自家 microVM) | pi 扩展（宿主 pi + 工具路由进 VM） | ✅ | ❌ **文档自认不是凭证边界**（VM 内继承宿主 env） | ⚠️ | ✅ | ⚠️ 每 agent 一 QEMU，并发不友好 | 弃 |
| **srt (Anthropic sandbox-runtime)** | 外部库（pi 官方扩展示例） | ⚠️ **只包 bash 进程树**，read/write/edit 内置工具不经它 | ❌ 不管 env | ✅✅ 域名白名单 + DNS 解析地址检查 + metadata 防护 | ❌ | ✅ 进程内扩展，零协议 | **网络层补充（二期可选）** |

### 关键事实（决定结论的三条）

1. **srt 只罩 bash**：pi 官方扩展示例（examples/extensions/sandbox/index.ts）只重写了
   bash 工具 + user_bash；read/write/edit 是 pi 内置 Node fs 实现，不经 srt。而 devforge
   需要的文件面隔离必须罩住全部写路径——srt 单独当主体会漏。
2. **Docker 一层罩三面**：文件（只挂 worktree rw）+ 凭证（env 白名单 + 不透宿主 env）+
   资源（cgroup），且是 devforge spawn 层一行 `docker run` 的事，引擎协议零改动。
   弱项只有网络白名单（bridge 全通）。
3. **互补而非互斥**：srt 的强项（allow-only 域名白名单、防 DNS 逃逸、防云 metadata）
   恰好是 Docker 的弱项；二期若需要「容器内再收网络」，在镜像里装 srt 包 bash 层（嵌套）
   即可，不必换主体。Gondolin/sbx/OpenShell 形状与服务器编排不合，直接出局。

### 结论

**主体 = pi 原生 Plain Docker；srt = 二期可选的网络白名单补充层；不引外部插件当主体。**
另外顺势整理工具面：真正的沙箱对象只有持 bash 的 agent（change coding）；doc-review
（AF-3 后仅 read/write）无任意命令执行面，容器化对它的意义只剩文件面收敛；doc-agent
若 AF-5 交卷工具化落地且无 bash 正当用途，可顺势摘 bash、退出沙箱名单。

## 本地执行摩擦与解法（2026-10-24 实测盘点，用户问：本地跑代码用 Docker 影响吗）

**本机条件（实测）**：docker 是 WSL2 内原生 dockerd（Ubuntu 26.04，overlayfs）非 Docker Desktop——
项目在 WSL2 ext4（/home/trtyr/code-work），bind mount 是同文件系统绑定，**无 9p 共享的灾难性能**；
容器内出站可达 tailnet（实测 100.64.0.1:8008 通，bridge 出站经宿主路由进 tailnet）。
性能预期：cargo test 宿主 3.7s → 容器 5-8s，agent 验证回路秒级差异，可接受。

**四条真实摩擦与解法**：

1. **工具链进镜像**：agent 要 cargo test，镜像必须预装 rust/node+pi/git。按项目类型分层预建
   （base=node+pi+git → rust 层 → python 层…）。**离线机坑**：本机有过离线 profile 渲染卡死前科，
   镜像必须提前在线构建缓存，不指望容器内现装；pi 升级 = 重建 base 镜像（自动化进 setup）。
2. **依赖缓存不重下**：cargo registry / npm cache 挂 named volume（容器销毁缓存留）；
   cargo target/ 在 worktree 里天然保留。
3. **git worktree 路径一致性（最阴）**：worktree 的 .git 是指回宿主主仓的绝对路径指针文件，
   容器挂到 /workspace 会断链。解法：**容器内外同绝对路径挂载**
   （`-v /home/trtyr/code-work/devforge:/home/trtyr/code-work/devforge`），指针继续有效。
4. **模型/MCP 网络**：已实测容器内可达 tailnet；模型 API（new-api 内网/公网）与 engram MCP
   （公网域名）均无障碍。宿主 localhost 服务若 agent 需要访问，用 host-gateway 别名。

**降摩擦的分级沙箱策略（2026-10-24 修正版）**：

防御面拆解（不止单一工具）：
- **bash = 执行面**（最大）：任意命令、外传、装后门、删库。
- **write/edit = 写面 + 提权链**：pi 官方 security.md 明说「working folder 不阻止工具访问
  Pi 进程权限内的其他路径」——持 write 的 agent 能改宿主任意可写文件，包括**改自己的
  settings.json 加回被剥的工具（提权链）**，或改 devforge profiles、其他仓代码。
- **read = 泄露面**：越权读（.ssh/凭证/其他仓）内容进上下文；出口已收窄时（无 bash、
  MCP 受控）风险降级但不为零。
- 官方三连：cwd 不是围栏、project trust 不是安全边界、人工 review 不是边界——
  pi 承认的唯一边界 = OS/虚拟化隔离（整进程容器）。

这正是容器主体 > srt 的根本原因：**容器罩的是整个进程（所有工具一起罩），srt 只罩 bash 树**。

推面按风险序，终点全容器化：
1. 第一批（必须容器）：持 bash 的 change coding agent（工具链镜像）。
2. 第二批：持 write 的全部 agent（doc-agent/doc-review）——用 base 镜像（node+pi+git）
   零额外工具链成本，收掉写面提权链与越权读。
3. 终态：全 agent 容器化；SandboxSpec None（宿主直跑）仅作调试/退化模式。

（原「无 bash 不进容器」的说法作废：write 提权链是实打实的任意写面。）

## pi 容器化能力（已查证 containerization.md）

| 方案 | 隔离面 | 凭证 | 评估 |
| --- | --- | --- | --- |
| Plain Docker | pi 整进程 + 内置工具 + `!` 命令 + 扩展 | 凭证以 env 传入容器 | **M2 选它**：最简单、依赖仅 docker（本机 29.9 在位） |
| Docker Sandboxes | 同上 + provider 凭证留宿主由代理替换 | 最小暴露 | CLI 工作流形状不合 spawn 编排，弃 |
| OpenShell | 本地/远程沙箱，策略化文件/进程/网络/凭证 | 策略控制 | 远程 worker 时代再上 |
| Gondolin 扩展 | 宿主 pi + 工具执行进 microVM | ⚠️ 文档自认**不是凭证边界**（VM 内继承宿主 env） | QEMU 资源开销 + 扩展旁路面，弃 |

## 设计

### 契约面

`AgentManifest.sandbox: Option<SandboxSpec>`：

```jsonc
{
  "kind": "docker",
  "image": "devforge/pi-sandbox:latest",
  "mounts": [
    {"src": "{worktree}", "dst": "/workspace", "mode": "rw"}   // 唯一 rw 面
  ],
  "env_allow": ["API_KEY"],        // 白名单；宿主其余 env 一律不透
  "network": "bridge"              // 模型 API 需出网；后续可加 allowlist 代理
}
```

None = 宿主直跑（现状兼容，逐步收敛为全沙箱）。

### 引擎面

`engine/process.rs build_spawn_command` 按 sandbox 分支生成 `docker run --rm ... pi --mode rpc ...`；
RPC 管道走 stdin/stdout（docker attach 即可），引擎事件协议不变。
镜像 `Dockerfile.pi`：node24-slim + pi + git + ripgrep + ca-certs；rust 工具链按目标仓需要
做分层镜像（doc 任务不需要 cargo 镜像）。

### 红线（来自 containerization.md 的暴露面警告）

- **不挂载宿主 `~/.pi/agent`**（= 交出全部凭证/会话/扩展）——容器内用独立 named volume 或
  只挂该 agent profile 目录（只读）。
- **只 rw 挂 worktree**；models.json/auth.json 由渲染器写入 profile（在 worktree 外、随容器卷注入）。
- env 白名单只透模型 key；宿主其余环境变量（DEVFORGE_*、其他服务凭证）一律不透。

### 运维

- 镜像构建纳入 env-sync/setup；`docker image prune` 纳入清理工作流
- 沙箱内 OOM/退出 → 引擎按基础设施故障重试（现有 DR-8 通道）
- 沙箱内时间/时区与宿主对齐（Asia/Shanghai 舰队基准）

## 分阶段推面

1. doc-review 沙箱化试点（工具面最小：read/write，无 bash 依赖后风险最小）
2. doc-agent → change-coding-agent → 其余
3. 全量沙箱后，`isolation: "none"` 枚举退役

## 验收

- 沙箱内完成一次完整 change 任务（模型调用/工具/git 全正常）
- 宿主 HOME、其他仓路径、宿主 env 在沙箱内不可见（负向测试）
- worktree 改动照常落盘、宿主无泄漏写入
- 二期（若上 srt 嵌套）：白名单外域名（含 DNS 重绑定到禁地址）被拒且 violation 可查
