/**
 * path-guard · glob 匹配（P012 topic 08）。
 *
 * 语义与 devforge 引擎侧 `path_covered_by`（src/domain/contract.rs）**同构**：
 *   - 精确相等；
 *   - 目录前缀（条目以 `/` 结尾）；
 *   - `/**` 递归（条目以 `/**` 结尾 = 前缀自身或前缀下全部）。
 * 修改匹配语义必须双端同步：样例集 `test-cases.json` 由两端测试共同驱动
 * （TS：`node --test match.test.ts`；Rust：`src/domain/contract.rs` 的样例集测试）。
 */

export type PathPattern = string;

/** 单条目匹配（与 Rust `path_covered_by` 逐分支对齐）。 */
export function pathCoveredBy(path: string, pattern: PathPattern): boolean {
  if (pattern === "**") {
    return true; // 裸 ** = 一切路径（gitignore 语义，渲染端 write:["**"] 默认形态）
  }
  if (pattern.startsWith("**/")) {
    // 中间通配（gitignore 语义）：`**/secrets/**` = 任意层级的 secrets 目录下全部
    const sub = pattern.slice(3);
    if (sub === "**") {
      return true;
    }
    if (sub.endsWith("/**")) {
      const dir = sub.slice(0, -3);
      return path === dir || path.startsWith(`${dir}/`) || path.includes(`/${dir}/`);
    }
    if (sub.endsWith("/")) {
      return path.startsWith(sub) || path.includes(`/${sub}`);
    }
    return path === sub || path.endsWith(`/${sub}`);
  }
  if (pattern.endsWith("/**")) {
    const prefix = pattern.slice(0, -3);
    return path === prefix || path.startsWith(`${prefix}/`);
  }
  if (pattern.endsWith("/")) {
    return path.startsWith(pattern);
  }
  return path === pattern;
}

/** 多条目 any-of（契约 owned_paths 的判定入口）。 */
export function covers(path: string, patterns: PathPattern[]): boolean {
  return patterns.some((p) => pathCoveredBy(path, p));
}
