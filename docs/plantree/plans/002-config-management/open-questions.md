# Open Questions — P002

## Q001 · API Key 的存储与安全边界？

Key 要进 auth.json（pi 读取），但 devforge 清单里怎么存？

- 明文进清单（简单，清单文件需 chmod 600）
- 清单只存引用，Key 存系统钥匙串/独立密钥文件，渲染时注入

倾向：MVP 明文 + 600 权限（与 pi 本机 auth.json 同一安全水位），后续可换钥匙串。待拍板。

## Q002 · 「自动更新」的生效时机？

变更同步到 profile 后：正在运行的 Agent 不受影响（spawn 时注入的配置已定型），下次 spawn 生效——这个默认是否符合预期？还是有场景需要触达运行中的 Agent？

## Q003 · 公共层与差异层的继承关系？

必装四件套是全 Agent 公共的。清单 schema 要不要支持「公共基座 + Agent 差异覆盖」（类似 profile 继承）？还是每个 Agent 清单完整平铺（重复但直观）？

倾向：基座 + 覆盖（改一处全 Agent 生效，符合「必装」的语义）。待拍板。

## Q004 · 子 Agent 的 MCP 需求？

规划阶段的两个 Agent 需要哪些 MCP（如 engram）？MCP 配置是每 Agent 独立还是也走公共基座？——依赖 P001 实际运行后的需求观察，先留白。
