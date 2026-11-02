/**
 * path-guard · match 单测（node --test match.test.ts 运行）。
 *
 * 样例集 = test-cases.json，与 Rust 侧（src/domain/contract.rs 的样例集测试）共用同一份，
 * 保证双端 glob 语义逐例同判。修改任何一端语义都必须让这份样例集在两端全绿。
 */

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { pathCoveredBy } from "./match.ts";

const cases: [string, string, boolean][] = JSON.parse(
  readFileSync(new URL("./test-cases.json", import.meta.url), "utf8"),
);

for (const [path, pattern, expected] of cases) {
  test(`pathCoveredBy(${JSON.stringify(path)}, ${JSON.stringify(pattern)}) === ${expected}`, () => {
    assert.equal(pathCoveredBy(path, pattern), expected);
  });
}

/// R11-P105：仓根相对化（纯函数）——绝对路径相对化后再比相对 glob 才命中；
/// 仓外路径与无基准（root=null）返回 null（绝对形态只被绝对 glob 命中）。
import { relativize } from "./match.ts";

test("relativize: absolute path inside root becomes repo-relative", () => {
  assert.equal(relativize("/repo/src/a.rs", "/repo"), "src/a.rs");
  assert.equal(relativize("/repo", "/repo"), ".");
});

test("relativize: outside-root path and null root yield null", () => {
  assert.equal(relativize("/profiles/x/settings.json", "/repo"), null);
  assert.equal(relativize("/repo/src/a.rs", null), null);
});
