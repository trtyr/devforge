# Roadmap — P003 API 接口体系

## Done

- [x] (T001) 形态拍板：devforge = 服务器非 CLI，API 是唯一能力出口；四能力域识别（2026-10-16）
- [x] (T002) 协议与风格选型定案：REST + SSE 混合（decisions/001），四 OQ 全结案（2026-10-16）
- [x] (T003+T004) 接口清单与规范落地（src/server/）：五能力域端点（编排控制/产物管理/交互中继/可观测/配置管理）、统一 JSON 信封、鉴权预留位、/api/v1 前缀——74 单测全绿（oneshot 内存态 + SSE 推送 + UI 中继回路 + 鉴权 401）（2026-10-16）
- [x] (T005) 服务器骨架：axum 常驻 + PlanningRpcEngine 生产引擎接线（src/server/mod.rs + main.rs）

## Next

- [ ] (T006) 前端对接文档：接口文档产出（OpenAPI 或等价物）——待前端启动时

## Deferred

- 前端本体（另一回事，不在 devforge 仓）
