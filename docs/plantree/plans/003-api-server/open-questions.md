# Open Questions — P003

## Q001 · 协议选型：REST、WebSocket 还是混合？

交互中继（Agent 对话流 + ask-user-question UI 事件）是重流式场景；编排控制是请求-响应场景；可观测是订阅推送场景。

候选：
- REST（控制面）+ WebSocket（事件流/中继面）混合
- 纯 WebSocket JSON-RPC（单一通道，客户端实现简单）
- gRPC（性能好但前端对接不如 WS 顺手）

倾向：混合（REST 管控制、WS 管流式与订阅）——各取所长，前端生态成熟。待 T002 细化定案。

## Q002 · 流式中继的具体形态？

Plan Agent 对话流（text_delta）、ask-user-question 的 UI 请求、可观测事件流——是共用一条 WS 连接多路复用，还是分通道？消息信封怎么设计（run-id / node-id / 类型）？与 T011 运行图模型对齐后一并定。

## Q003 · 鉴权与安全边界？

服务器监听范围（localhost only？）、要不要 token 鉴权、未来多用户可能性。测试环境从简（用户对安全的态度见 P002 decisions/001），但接口设计的鉴权预留位要留。

## Q004 · API 版本化策略？

URL 前缀（/api/v1/）vs header 协商？前端未上线前接口还会变，版本化从第一天带还是首个前端版本冻结时引入？倾向：路径前缀从第一天带，成本近零。
