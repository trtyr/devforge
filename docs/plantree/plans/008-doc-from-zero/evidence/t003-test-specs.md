# T003 · 测试即可执行规格

**结论**：清单新增第四类来源——测试里的行为条目（`spec:` 前缀的 `Behavior`）。在 devforge 自身实测 268 条（主要是 `src/**` 里的内联 `#[test]`）。

## 抽什么

| 语言 | 识别 | 为什么这么认 |
| --- | --- | --- |
| Python | `def test_*` / `async def test_*` | pytest 的唯一事实（不再猜 `unittest.TestCase` 的方法） |
| TS/JS | `it("…")` / `test("…")` | 名字是作者写的自然语言，比函数名更像「规格」 |
| Go | `func Test*` | go test 的约定 |
| Rust | `#[test]` 后的 `fn` | **不看文件名**：Rust 测试默认写在源文件内联，按文件卡会漏掉绝大多数 |

每条都带 `path:line`，挂到该文件的单元；`id` 形如 `spec:tests/test_core.py:test_add`。

## 为什么只抽名字、不抽断言

断言是实现细节的镜像：把 `assert x == 1` 抄进文档，代码一改文档必错，且没有增量信息。
文档要的是「这条行为存在、并且可以按这个位置重新验证」——`path:line` 本身就担保了可验证性。

## 档位与成本

`Behavior` 只在 `detailed` 档是必覆盖项（`complete` 默认不要求），因为一个千条测试的仓库
把每条都写成文档既不现实也没人读。要深挖的调用方把 `doc_level` 调到 `detailed` 再要。

## 已知边界（v1）

- 不识别 `skip` / `xfail` / `@pytest.mark`（它们仍是行为，但可信度不同——留给审查判断）；
- TS 只认字面量字符串，模板字符串与变量拼出来的名字抽不到；
- 不抽测试文件里的 fixture / helper（除非名字以 `test` 开头）。
