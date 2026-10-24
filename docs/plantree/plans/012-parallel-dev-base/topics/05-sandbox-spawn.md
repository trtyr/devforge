# 05 · 沙箱化 Spawn（Sandboxed Spawn）

> 前置硬需求：worktree 隔离文件但不隔离权限——change agent 有 bash，能摸宿主凭证/网络/其他仓。
> 并行 = 每 agent 爆炸半径相乘，所以沙箱化是底座的安全地基，不是可选项。

## pi 容器化能力（已查证 containerization.md）

| 方案 | 隔离面 | 凭证 | 评估 |
| --- | --- | --- | --- |
| Plain Docker | pi 整进程 + 内置工具 + `!` 命令 + 扩展 | 凭证以 env 传入容器 | **M2 选它**：最简单、依赖仅 docker |
| Docker Sandboxes | 同上 + provider 凭证留宿主由代理替换 | 最小暴露 | 二期升级 |
| OpenShell | 本地/远程沙箱，策略化文件/进程/网络/凭证 | 策略控制 | 远程 worker 时代再上 |
| Gondolin 扩展 | 宿主 pi + 工具执行进 microVM | 凭证留宿主 | 需宿主接口适配，备选 |

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
