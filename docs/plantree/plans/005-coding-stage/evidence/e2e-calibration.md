# P005 e2e 校准清单（task-1，2026-10-18，被测版本 db262bf）

逐脚本对照 T002-T006 行为差校准。原则：只改测试脚本，不碰 src/。

## 校准项（改了什么/为什么）

| 脚本 | 变更 | 原因 |
|------|------|------|
| test_02_tasks.py | 未知 SOP 例 `coding`→`ghost`；新增正向断言「coding SOP 任务 == 201」 | P005 后 coding 入 SopRegistry（原「未知 SOP」例已注册，400 变 201）；注册表扩容验证面 |
| test_10_timeout_async_render.py | `config.rendered` 通知的 rendered 计数 `==2` → `==4` | builtin manifest 现渲染四 profile（规划双 + 编码双） |
| test_03_config.py | 默认 Agent 断言 `双 Agent` → `四 Agent`（plan/coding 双对） | 同上：SopRegistry/manifest 扩容 |
| run_all.py | SCRIPTS 补 test_11-15（编码 SOP 五脚本，含 14 号 3600s 长窗口）；`--quick` 跳过清单加 test_14 | 新增功能点的脚本登记（脚本本体在 task-2/3 落地） |

## 核对无需改动项

- test_00/01/04/05/06：触面（项目 CRUD/鉴权/SSE/产物图）与本轮行为差无交集
- test_03_config：profiles 轮询为子集迭代（plan-agent/plan-review），四 profile 扩容不影响
- test_07_rerun_serial：串行守卫/cancel/留档断言均 planning 面，行为不变
- test_08_e2e_planning：全链结构不变（SSE autopilot 应答/树落盘/Settled+Completed）；后台渲染新增两个 coding profile 的 npm 安装不阻塞等待环（只盯规划双 profile 的 package.json 标记）
- test_09_run_control：planning cancel 路径未动
- dialog_responder.py：探测制回填与 SOP 无关，签名/逻辑零改动（usage 输出复核）

## 编译验证

py_compile 全过（test_00-10、dialog_responder、common、run_all、run_light_batch）。
