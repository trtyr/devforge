# 事故记录 · 测试套件自指递归（fork bomb）

- 日期：2026-10-19
- 触发：test_19 全链测试（`bg_cy74yhhe`，e2e home `/tmp/devforge-e2e-x9xajbsc`）
- 现象：跑到 4h49m 仍未结束；进程表里 `python -m pytest -q` 有 52 个同时存活，且**每 ~21 秒新增一层**，形成两条无限增长的父子链
- 处置：人工停止（`bg_kill` + 清理残留进程）。4h49m 的运行作废

## 根因

测试 Agent 在 S5 写了「门禁测试」`tests/e2e/test_s5_ui.py`：

```python
GATES = [
    ("ruff check", [...]), ("ruff format --check", [...]),
    ("pytest", ["python", "-m", "pytest", "-q"]),   # ← 这里
]
def test_gate_command_exits_zero(...): _run(command, timeout=600)
def test_all_five_gates_together(...):  # 同样跑 pytest -q
def test_pytest_suite_reports_no_failures(...):  # 同样跑 pytest -q
```

`pytest -q` 在仓库根运行 → 收集全部测试 → **包含本文件自己** → 命中同一个用例 → 再跑一次 `pytest -q`……每层约 21 秒，无穷递归。

放大因素：
1. 每层 `subprocess.run(timeout=600)`，一层失败要等 10 分钟；外层 `timeout 1500` 只杀直接子进程，孙进程链会继续
2. 每次递归都真起一套服务（`web.main`），进程与端口一起泄漏
3. 测试输出到 pytest 捕获缓冲，**运行期间日志为空**（`test_19` 也是末尾才落盘），外部完全看不到异常

## 为什么审查没拦住

- review-6（20:44）审的是 **S4**，S5 还没写；S5 是之后才产出的
- 当时的审查面里没有「测试自身可靠性」这一项——只看覆盖、矩阵、黑盒纪律、todo 实物
- 「跑一次门禁命令」在人类工程常识里是合理的，问题只在**参数写法**（不带路径 + cwd 在根），属于语义层面的自指，不是一眼可见的错

## 修复

见 commit（TEST_AGENT_PROMPT / TEST_REVIEW_PROMPT）：

- 测试 Agent 新增「测试自身的可靠性」7 条硬约束：禁止在套件内运行整套测试（只允许 `--collect-only` / 显式 `--ignore` 自身 / 确认不含本套件的脚本）、每个外部命令短超时、失败最多重跑 1 次、进程组回收、临时端口与 `tmp_path`、禁止假绿（必须报 passed/failed/skipped 三数 + 反向验证）、禁止交互式/无界等待命令、报告整套耗时
- 测试审查新增**一票否决**审查面：测试里存在自指跑整套测试 → 直接 FAIL；并在审查纪律里写明「发现挂死风险先停跑，不要等它超时（每等一层就多长一层）」

## 教训（写进体系，不只写在这里）

1. **「验证命令能跑」是最容易写出自指的地方**——凡是在测试里执行测试命令，必须显式证明它不会重新进入自己
2. **无超时的 subprocess 是长跑杀手**：单层 600s × 无限层；短超时是硬要求
3. **运行期日志为空 = 排障盲区**：`subprocess`/pytest 捕获会把进度吞掉；长任务要留增量输出
4. **审查面要覆盖"工具自身的失效模式"**，不只覆盖被测系统的行为
