/**
 * path-guard · 配置发现与加载（P012 topic 08）。
 *
 * 发现顺序（取第一个命中的文件）——R12-P109/P108 修正：
 *   1. <agent-dir>/.pi/pathguard.json（PI_CODING_AGENT_DIR，devforge 的 profile 目录
 *      ——渲染端产出，且 deny_write 提权防护覆盖该目录，agent 不可改）；
 *   2. <cwd>/.pi/pathguard.json（降位为后备：生产 cwd = plans/<项目>/<任务>/ 由
 *      agent 读写，旧口径排在 profile 之前构成「本轮写、下轮生效」的跨 run 自改
 *      边界路径；本仓清单尚未启用 path_permissions，顺序调整无行为回归）。
 *
 * 旧口径的首候选 DEVFORGE_PATHGUARD（自称「spawn 显式注入，最强约定」）删除：
 * 全仓无任何注入点（process.rs 只注入 PI_CODING_AGENT_DIR/DEVFORGE_MANAGED/
 * DEVFORGE_ROLE/DEVFORGE_WORKSPACE_ROOT），死路径（P108）。
 *
 * 判定口径（topic 08 §配置发现）：
 *   - 两处都无                → 未启用（老 profile 零影响，handler 放行一切）；
 *   - 文件存在但解析/校验失败 → fail-closed：所有 write/edit 一律 block（read 放行）。
 */

import * as fs from "node:fs";
import * as path from "node:path";

/** 与 devforge `PathPermissions`（src/config/manifest.rs）逐字段对齐。 */
export interface PathGuardConfig {
  read: string[];
  write: string[];
  deny_read: string[];
  deny_write: string[];
}

export type LoadedConfig =
  | { enabled: false }
  | { enabled: true; corrupted: true }
  | { enabled: true; corrupted: false; config: PathGuardConfig };

/** 按发现顺序列出候选配置文件路径（全不存在 = 空数组）。 */
export function candidatePaths(env: NodeJS.ProcessEnv = process.env): string[] {
  const candidates: string[] = [];
  // R12-P109：profile 候选在前（渲染端产出 + 提权防护覆盖，agent 不可改）；
  // cwd 候选降位为后备——生产 cwd 归 agent 读写，排前即跨 run 自改边界。
  const agentDir = env.PI_CODING_AGENT_DIR;
  if (agentDir) {
    candidates.push(path.join(agentDir, ".pi", "pathguard.json"));
  }
  candidates.push(path.join(process.cwd(), ".pi", "pathguard.json"));
  return candidates;
}

/** 加载配置：三处都无 = 未启用；第一个存在的文件解析失败 = fail-closed。 */
export function loadConfig(env: NodeJS.ProcessEnv = process.env): LoadedConfig {
  for (const file of candidatePaths(env)) {
    let raw: string;
    try {
      raw = fs.readFileSync(file, "utf8");
    } catch {
      continue; // 该候选不存在 → 看下一个
    }
    return parseConfig(raw);
  }
  return { enabled: false };
}

/** 解析并校验单个配置文件文本（导出供测试）。 */
export function parseConfig(raw: string): LoadedConfig {
  let v: unknown;
  try {
    v = JSON.parse(raw);
  } catch {
    return { enabled: true, corrupted: true };
  }
  if (typeof v !== "object" || v === null) {
    return { enabled: true, corrupted: true };
  }
  const o = v as Record<string, unknown>;
  const strArray = (x: unknown): string[] | null =>
    Array.isArray(x) && x.every((e) => typeof e === "string") ? (x as string[]) : null;
  const read = strArray(o["read"]);
  const write = strArray(o["write"]);
  const denyRead = strArray(o["deny_read"]);
  const denyWrite = strArray(o["deny_write"]);
  if (read === null || write === null || denyRead === null || denyWrite === null) {
    return { enabled: true, corrupted: true };
  }
  return {
    enabled: true,
    corrupted: false,
    config: { read, write, deny_read: denyRead, deny_write: denyWrite },
  };
}
