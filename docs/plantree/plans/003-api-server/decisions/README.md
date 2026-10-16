# Decisions — P003

稳定决策记录。只记「选了什么 + 为什么 + 后果」。

---

## 001 · 协议定案：REST（控制）+ SSE（事件推送）

- **决策**：编排控制/产物查询/配置管理/中继输入走 REST 请求-响应；事件推送（run 生命周期、ui.request、领域事件）走 SSE 单向流；路径前缀 `/api/v1` 从第一天
- **为什么**（用户拍板框架下的简化）：原倾向 REST+WS 混合，落地时发现编排事件是单向流——SSE 比 WS 简单得多（无握手/心跳/双向状态机），前端 EventSource 原生支持，符合「尽可能简单」哲学；WS 不引入
- **后果**：① 无 WS 依赖；② 中继输出（Agent 文本流）走 SSE `message` 事件；③ 用户输入走 REST POST——双向通过「SSE 下行 + POST 上行」组合完成

## 002 · SSE 信封：一行 JSON 一事件

- **决策**：SSE `data` 为单行 JSON：`{"type": "...", ...字段}`。已知 type：`run.started` / `run.finished` / `run.error` / `project.created` / `task.created` / `ui.request`（含 id/question/options）
- **为什么**：单一信封让前端一个解析器通吃；type 字段即路由键
- **后果**：T011 可观测数据层直接复用该信封格式落盘；新增事件类型只加 type 不改协议

## 003 · 鉴权：localhost 绑定 + token 预留位

- **决策**：默认绑定 `127.0.0.1:7888`（`DEVFORGE_ADDR` 可覆盖）；token 预留位——`token: None` 时放行，`Some(t)` 时要求 `x-devforge-token` 头精确匹配否则 401
- **为什么**（用户对安全的一贯口径 P002 decisions/001）：测试环境从简，但接口设计留好扩展缝
- **后果**：多用户/远程部署是远期议题，协议面已留位置

## 004 · 版本化：`/api/v1/` 路径前缀从第一天

- **决策**：全部端点挂 `/api/v1/` 前缀
- **为什么**：成本近零，前端上线后免迁移
- **后果**：破坏性变更走 v2 并行，v1 不动
