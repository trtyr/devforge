# Roadmap — P003 API 接口体系

## Done

- [x] (T001) 形态拍板：devforge = 服务器非 CLI，API 是唯一能力出口；四能力域识别（2026-10-16）

## Next

- [ ] (T002) 协议与风格选型：REST vs JSON-RPC over WebSocket、事件推送机制（SSE/WS）、与流式中继需求的匹配（open-questions Q001/Q002）
- [ ] (T003) 接口清单设计：五能力域逐端点定义（编排控制/产物管理/交互中继/可观测/配置管理）——含项目/任务 CRUD、任务产物归集查询、节点 trace 下钻
- [ ] (T004) 规范：错误码体系、接口版本化策略、鉴权方式（open-questions Q003）
- [ ] (T005) 服务器骨架落地：与 P001 T006 合流（axum/actix 类，选型归 P001 T010 架构选型一起定）
- [ ] (T006) 前端对接文档：接口文档产出（OpenAPI 或等价物）

## Deferred

- 前端本体（另一回事，不在 devforge 仓）
