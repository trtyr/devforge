/**
 * path-guard · pi 扩展本体（P012 topic 08）。
 *
 * 定位：devforge 系统的内置插件——devforge 渲染 profile 时把本目录分发到
 * `<profile>/extensions/path-guard/`（pi 的 agent-dir extensions，自动加载），
 * 配置 `.pi/pathguard.json` 由 devforge 从 `AgentManifest.path_permissions` 渲染。
 *
 * 拦截面：read / write / edit 三个工具（bash 不拦——持 bash agent 的边界由
 * worktree + 契约终校验承担，见 topic 08 §边界）。deny-first：deny_read/deny_write
 * 永远赢，deny_read 命中连带堵写。fail-closed：配置损坏堵所有写（读放行）。
 * 遥测：block 的 reason 恒带 `[path-guard] ` 前缀——devforge 客户端据此转
 * boundary.violation 事件（RPC 流 tool_execution_end isError + 前缀识别）。
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import * as fs from "node:fs";
import * as path from "node:path";
import { covers } from "./match";
import { loadConfig, type LoadedConfig, type PathGuardConfig } from "./config";

export const DENIED_PREFIX = "[path-guard] ";

/** 写面工具：deny_write / write 白名单的判定对象。 */
const WRITE_TOOLS = new Set(["write", "edit"]);

/** realpath 解析：目标不存在（write 新文件）→ 父目录 realpath + 文件名拼接。 */
function resolveReal(p: string): string {
  try {
    return fs.realpathSync(p);
  } catch {
    const parent = path.dirname(p);
    try {
      return path.join(fs.realpathSync(parent), path.basename(p));
    } catch {
      return p;
    }
  }
}

/** 提权双保险：profile 目录（agent-dir）恒追加在 deny_write（渲染端已加，运行时再自检）。 */
function ensureEscalationGuard(cfg: PathGuardConfig, env: NodeJS.ProcessEnv): PathGuardConfig {
  const agentDir = env.PI_CODING_AGENT_DIR;
  if (!agentDir) {
    return cfg;
  }
  const guard = `${agentDir}/**`;
  if (cfg.deny_write.includes(guard)) {
    return cfg;
  }
  return { ...cfg, deny_write: [...cfg.deny_write, guard] };
}

export default function (pi: ExtensionAPI) {
  const loaded: LoadedConfig = loadConfig();
  if (!loaded.enabled) {
    // 未启用：老 profile 零影响，不注册任何 handler。
    return;
  }
  let cfg: PathGuardConfig | null = loaded.corrupted ? null : ensureEscalationGuard(loaded.config, process.env);
  let warned = false;

  pi.on("tool_call", async (event, ctx) => {
    const tool = event.toolName;
    if (tool !== "read" && tool !== "write" && tool !== "edit") {
      return undefined;
    }
    if (loaded.corrupted || cfg === null) {
      // fail-closed：配置损坏堵所有写（读放行），只提醒一次。
      if (WRITE_TOOLS.has(tool)) {
        if (!warned) {
          warned = true;
          if (ctx.hasUI) {
            void ctx.ui.notify("[path-guard] 配置损坏：所有写操作已阻断", "warning");
          }
        }
        return { block: true, reason: `${DENIED_PREFIX}配置损坏，写操作 fail-closed` };
      }
      return undefined;
    }

    const raw = (event.input as Record<string, unknown>)["path"];
    if (typeof raw !== "string" || raw.length === 0) {
      return undefined;
    }
    const p = resolveReal(raw);

    // deny-first：deny_read 命中连带堵写；deny_write 命中堵写
    if (WRITE_TOOLS.has(tool) && covers(p, cfg.deny_read)) {
      return { block: true, reason: `${DENIED_PREFIX}${p} 在 deny_read 内（连带堵写）` };
    }
    if (WRITE_TOOLS.has(tool) && covers(p, cfg.deny_write)) {
      return { block: true, reason: `${DENIED_PREFIX}${p} 在 deny_write 内` };
    }
    if (tool === "read" && covers(p, cfg.deny_read)) {
      return { block: true, reason: `${DENIED_PREFIX}${p} 在 deny_read 内` };
    }

    // 白名单语义：列表非空 = 只允许列表内
    if (WRITE_TOOLS.has(tool) && cfg.write.length > 0 && !covers(p, cfg.write)) {
      return {
        block: true,
        reason: `${DENIED_PREFIX}${p} 不在 write 允许列表内（允许：${cfg.write.join(", ")}）`,
      };
    }
    if (tool === "read" && cfg.read.length > 0 && !covers(p, cfg.read)) {
      return { block: true, reason: `${DENIED_PREFIX}${p} 不在 read 允许列表内` };
    }

    return undefined;
  });
}
