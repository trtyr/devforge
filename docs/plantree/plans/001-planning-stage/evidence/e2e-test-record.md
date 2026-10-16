# Evidence · 端到端测试首轮记录（测试阶段 T009）

- 时间：2026-10-17 00:00 – 02:55
- 执行者：devforge 会话（AI 替用户应答 UI，用户委托）
- 脚本组：`tests/e2e/test_00…test_08`（9 脚本，纯标准库 Python）
- 结果：**76 PASS / 7 FAIL，14 项发现（去重）**

## 结果速览

- 基础 API 面（项目/任务/配置/事件/产物/鉴权放行路径）：**全通**
- 串行守卫 409（同任务+跨任务）：真 pi Running 窗口内验证通过
- 真 pi spawn + 模型对话：经测试侧配置补偿后打通（补偿=复制真实 models.json + 修正 auth.json 形状 + 校准 model_id）
- **端到端未走通**：模型首轮生成 >900s 未完成（增长探针证实极慢生成非死流）→ 无完成确认/无 review/无 plan tree 产物/任务永久 Running

## 问题清单（只记录不修）

14 项，按严重度：high 7（run 卡死无取消、完成确认未运转、plan tree 零产物、慢生成、auth 形状、models.json 缺分发、UI 桥 fire-and-forget 误应答）、medium 4（PUT 同步装包阻塞、半注册项目、token 无配置入口、pi stderr 黑盒）、low 1（错误码分类）、info 1（模型 id/baseUrl 校准结论）。

## 完整报告

- 详见 `tests/e2e/TEST_REPORT.md`（根因三链：配置供给链 / UI 桥接协议 / 可观测运维缺口）
- 逐脚本数据：`tests/e2e/results/*.json`
- 事件时间线：`tests/e2e/results/test_08_timeline.json`

## 对后续阶段的指向

修复顺序建议：配置供给三补 → UI 桥按 method 分流 → run 超时+取消+pi stderr 落日志+SSE 推对话流 → 提示词正文与 plan-tree skill 真实分发 → 端到端复测。
