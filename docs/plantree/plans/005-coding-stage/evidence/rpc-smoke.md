# T003 RPC 单点冒烟记录（2026-10-18）

环境：手工 profile（settings.json 带 rpiv-todo+pi-lens 包声明；mcp.json 带 codegraph+context7）
+ `PI_CODING_AGENT_DIR` 注入 + `pi --mode rpc`（75s 存活窗口，EOF 收场）。

结果：
- 进程零 stderr（无插件加载错误、无 MCP 配置报错），退出码 0
- RPC 通道应答正常（对空行回 parse 错误响应——协议层活着）
- 局限：工具级可见性（todo/lens 工具是否注册、MCP 是否连上）未验——需要真会话 prompt，
  归 T008 e2e（用户醒来拍板）。本冒烟只证「profile 形状合法、加载无副作用」。
