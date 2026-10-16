# Open Questions — P003

## Q001 · 协议选型：REST、WebSocket 还是混合？【已解决 2026-10-16】

**结论：REST + SSE 混合**（decisions/001）——REST 管控制/查询/中继输入，SSE 管事件推送。WS 不引入：编排事件是单向流，SSE 更简单。

## Q002 · 流式中继的具体形态？【已解决 2026-10-16】

**结论：单一 SSE 通道 + 统一 JSON 信封**（decisions/002）：`{"type": "...", ...}` 一行一事件；`ui.request` 事件带 id/question/options，应答走 `POST /ui/{id}/answer`。与 T011 落盘格式对齐。

## Q003 · 鉴权与安全边界？【已解决 2026-10-16】

**结论：默认 127.0.0.1 + token 预留位**（decisions/003）——token 未配置即放行，配置即强校验 401。多用户远期。

## Q004 · API 版本化策略？【已解决 2026-10-16】

**结论：`/api/v1/` 路径前缀从第一天**（decisions/004）。
