# Codex 官方对齐 TODO

更新时间：2026-03-16

## 目标

把 `CodexManager` 持续补齐到更接近官方 `codex-rs/app-server + core + login` 的协议与运行时行为。

这份 TODO 只记录：

- 已经完成的对齐项
- 还没对齐的差距
- 建议的实施顺序
- 每项的最小验收标准

不记录与本轮无关的桌面 UI 视觉问题，也不把已经按产品策略主动偏离的行为混进“遗漏”。

## 当前已完成

### 网关与上游请求形状

- [x] `/responses/compact` 上游路径和 compact 头部语义已对齐
- [x] 关键头透传已补齐
  - `session_id`
  - `x-client-request-id`
  - `x-openai-subagent`
  - `x-codex-turn-state`
- [x] `Originator / User-Agent / Residency` 已改为动态运行时配置
- [x] 流式 `/responses` 已支持 `zstd` 请求体压缩

### 登录与账号基础能力

- [x] ChatGPT OAuth 登录闭环可用
- [x] 授权码换 token 可用
- [x] `chatgptAuthTokens` 注入与 refresh 可用
- [x] `planType` 已统一到官方枚举语义
  - `free / go / plus / pro / team / business / enterprise / edu / unknown`
- [x] `account/rateLimits/read` 已有基础兼容入口

### 轻量 app-server compat 方法

- [x] `initialized`
- [x] `experimentalFeature/list`
- [x] `collaborationMode/list`
- [x] `thread/start`
- [x] `thread/resume`
- [x] `thread/fork`
- [x] `thread/read`
- [x] `thread/name/set`
- [x] `thread/compact/start`
- [x] `thread/realtime/start`
- [x] `thread/realtime/appendAudio`
- [x] `thread/realtime/appendText`
- [x] `thread/realtime/stop`
- [x] `turn/start`
- [x] `turn/steer`
- [x] `turn/interrupt`
- [x] `skills/list`
- [x] `skills/config/write`
- [x] `skills/remote/list`
- [x] `skills/remote/export`
- [x] `plugin/list`
- [x] `plugin/install`
- [x] `plugin/uninstall`
- [x] `app/list`
- [x] `model/list`
- [x] `config/read`
- [x] `config/value/write`
- [x] `config/batchWrite`
- [x] `configRequirements/read`
- [x] `config/mcpServer/reload`
- [x] `mcpServerStatus/list`
- [x] `mcpServer/oauth/login`
- [x] `externalAgentConfig/detect`
- [x] `externalAgentConfig/import`
- [x] `review/start`

说明：

- 上述方法都已经不再返回 `unknown_method`
- 其中一部分只是“稳定入口 + 明确错误 / 空结果”，还不是真工作流

### 连接级初始化

- [x] 已补最小连接级 `initialize -> initialized` 状态机
- [x] 带连接标识时，初始化前 / 确认前的其它方法会被拒绝
- [x] 重复 `initialize` 会返回 `Already initialized`
- [x] session 已保存：
  - `clientInfo`
  - `capabilities.experimentalApi`
  - `capabilities.optOutNotificationMethods`

说明：

- 当前仍依赖 HTTP 头 `X-CodexManager-Rpc-Connection-Id`
- 当前已新增 `GET /rpc` WebSocket 入口，支持同连接 request / notification / server notification
- 但这仍不是官方 stdio / 单 transport 全路径语义的完整复刻
- 已新增 `/rpc/events` SSE 持久连接入口，可把连接标识回传给客户端

## 当前未对齐

## P0：必须先补的底座

### 1. 持久双向 JSON-RPC 传输层

状态：

- [~] 部分完成

差距：

- 当前已新增 `/rpc/events` SSE 持久连接入口和连接注册表
- 但业务 RPC 仍是一次一请求的 HTTP POST
- 官方 app-server 是持久双向 JSON-RPC 连接
- 官方支持 stdio / websocket 传输，并能在同一连接上双向收发

最小实施建议：

1. 把当前 `SSE + HTTP POST` 过渡方案升级为真正的单连接双向 transport
2. 连接内维护 session
3. 支持服务端主动下发 notification
4. 复用已有 `RpcRequestContext`

最小验收标准：

- [x] 服务端可建立持久连接并回传连接标识
- [x] 连接关闭后 session 自动清理
- [x] 客户端在单连接上连续发送 `initialize -> initialized -> 其它方法`
- [x] 服务端在同一 WebSocket 连接上双向收发
- [ ] 去掉对 `SSE + HTTP POST` 过渡路径的依赖，统一到单 transport 语义

### 2. 通知队列与通知分发

状态：

- [~] 部分完成

差距：

- 官方大量能力依赖 notification
- 当前已具备连接注册、广播和 `optOutNotificationMethods` 过滤
- 但还没有离线队列、重放、恢复和更细粒度的 per-connection state

需要先补的通知：

- `account/login/completed`
- `account/updated`
- `account/rateLimits/updated`
- `skills/changed`

最小验收标准：

- [x] 已初始化连接可以注册到通知总线
- [x] 服务端可按连接过滤 opt-out 方法
- [x] 以上 4 类通知至少能从后端事件触发并广播
- [ ] 支持离线队列 / 恢复 / 重放

### 3. 官方初始化语义补全

状态：

- [ ] 部分完成

当前已有：

- 最小握手 gate
- session 元数据保存

还缺：

- 真正的“每连接生命周期”
- 初始化失败 / 连接重用边界处理
- 与持久连接天然绑定，而不是靠 HTTP 头补连接标识

最小验收标准：

- 持久连接关闭后 session 不残留
- 不同连接之间 session 不串
- 未初始化连接上的请求统一错误语义

## P1：补成“能被官方客户端协议驱动”的核心能力

### 4. Thread 生命周期

状态：

- [~] 部分完成

待补方法：

- `thread/start`
- `thread/resume`
- `thread/fork`
- `thread/read`
- `thread/name/set`
- `thread/compact/start`

最小验收标准：

- [x] 能创建 / 恢复 / 分叉线程
- [x] 能返回线程对象
- [x] 能发出 `thread/started` / `thread/name/updated` / `thread/status/changed`
- [~] `thread/compact/start` 已接入最小真实 compaction 生命周期，仍未接远端 `/responses/compact` 历史替换工作流
- [ ] `thread/read` 持久化语义、`notLoaded` 状态和 rollout 文件对齐

### 5. Turn 生命周期

状态：

- [~] 部分完成

待补方法：

- `turn/start`
- `turn/steer`
- `turn/interrupt`

最小验收标准：

- [x] `turn/start` 能返回 turn 对象
- [x] 能产出 `turn/started` / `turn/completed`
- [x] `turn/interrupt` 真正能终止运行中的 turn
- [ ] turn 接入真实 Codex 执行器，而不是 skeleton runtime
- [ ] `turn/steer` 接入真实 in-flight turn 输入追加语义

### 6. Item 事件流

状态：

- [ ] 未完成

待补通知 / 事件：

- [x] `item/started`
- [x] `item/completed`
- [x] `item/agentMessage/delta`
- [x] `thread/tokenUsage/updated`
- 工具调用 / 命令执行 / reasoning 相关 item

最小验收标准：

- [x] turn 运行中能下发基础 item 生命周期事件
- [ ] 客户端可以按官方方式重建消息和工具输出

### 7. Review 真工作流

状态：

- [ ] 只有兼容入口

当前：

- `review/start` 入口已存在
- 但只返回“尚未接入真实工作流”

最小验收标准：

- `review/start` 真正创建 review turn
- 能返回 `reviewThreadId`
- 能产生 review 相关 item / turn 通知

## P1：更贴近官方 runtime 的网关能力

### 8. Responses WebSocket / prewarm / reuse

状态：

- [ ] 未完成

差距：

- 当前只有 HTTP/SSE 路径
- 官方 `core/client.rs` 有 websocket transport、prewarm、reuse、回退逻辑

最小验收标准：

- 有 websocket 主通道或兼容实现
- 有 `prewarm` 行为
- 有连接复用 / 状态复用能力
- 出错时仍能回退到当前 HTTP 路径

### 9. Realtime 流

状态：

- [ ] 未完成

待补：

- `thread/realtime/start`
- `thread/realtime/appendAudio`
- `thread/realtime/appendText`
- `thread/realtime/stop`

最小验收标准：

- 能建立 realtime 会话
- 能持续追加音频 / 文本
- 能正常 stop 并产出状态更新

### 10. 审批流

状态：

- [ ] 未完成

待补：

- 服务端主动 approval request
- 客户端响应 approval

最小验收标准：

- 命令 / 网络 / 权限审批能通过 notification 下发
- 客户端回复后，turn 能继续运行

## P2：扩展能力与非核心目录能力

### 11. Skills 远端目录真接入

状态：

- [ ] 只有空列表 / 明确错误

待补：

- `skills/remote/list`
- `skills/remote/export`

最小验收标准：

- 远程技能可列出
- 可导入到本地 skills 目录
- 导入后能反映到 `skills/list`

### 12. Plugin / App / MCP 状态真接入

状态：

- [ ] 只有空壳 / 明确错误

待补：

- `plugin/list`
- `plugin/install`
- `plugin/uninstall`
- `app/list`
- `mcpServerStatus/list`
- `mcpServer/oauth/login`

最小验收标准：

- 不是固定空结果
- 能接到真实插件 / app / mcp 状态源
- install / uninstall / oauth login 有实际工作流

### 13. External Agent Config 真迁移

状态：

- [ ] 只有空结果

待补：

- `externalAgentConfig/detect`
- `externalAgentConfig/import`

最小验收标准：

- 能检测迁移项
- 能执行导入
- 导入后有可见结果

### 14. Config 分层语义补全

状态：

- [ ] 只有 CodexManager 自身设置子集

还缺：

- 更完整的 `config.toml` 语义
- project layer
- managed layer
- requirements / policy 层的真实合并

最小验收标准：

- 不只是映射 app settings
- 至少具备 user / project 两层
- `config/read` 能返回更真实的 layer/origin 结果

### 15. Model 目录语义补全

状态：

- [ ] 基础列表已完成

还缺：

- 更完整的官方模型目录
- `upgrade`
- `upgradeInfo`
- `availabilityNux`
- hidden 模型策略

最小验收标准：

- `includeHidden` 有实际效果
- upgrade 元数据不再固定 `null`

## 非协议但仍建议补齐

### 16. 登录错误体验继续对齐

状态：

- [ ] 部分完成

当前已有：

- token endpoint 错误体解析已增强

还缺：

- 更完整的人类可读提示
- URL / issuer 脱敏日志
- entitlement / plan type 更友好反馈

## 建议实施顺序

建议不要再按“方法名数量”推进，而是按依赖顺序推进：

1. 持久双向 JSON-RPC 传输层
2. 通知总线和通知分发
3. `account/* updated`、`skills/changed`
4. `thread/*`
5. `turn/*`
6. `item/*`
7. `review/start` 真工作流
8. `responses websocket / prewarm / reuse`
9. `thread/realtime/*`
10. Plugin / App / MCP / Skills 远端真实接入
11. Config 分层与 model 目录语义补全

## 建议切分方式

### 阶段 A：连接与通知底座

- [ ] 持久连接 transport
- [ ] 通知队列
- [ ] connection registry
- [ ] opt-out 过滤
- [ ] `account/*` / `skills/changed` 通知

### 阶段 B：对话运行时

- [ ] `thread/*`
- [ ] `turn/*`
- [ ] `item/*`
- [ ] approval flow
- [ ] `review/start`

### 阶段 C：上游传输增强

- [ ] websocket / prewarm / reuse
- [ ] realtime

### 阶段 D：扩展生态

- [ ] skills remote
- [ ] plugin/app/mcp
- [ ] external agent migration
- [ ] config layering
- [ ] model metadata

## 参考文件

- 对齐说明：
  - [20260316145500000_codex源码对齐差异说明.md](../report/20260316145500000_codex%E6%BA%90%E7%A0%81%E5%AF%B9%E9%BD%90%E5%B7%AE%E5%BC%82%E8%AF%B4%E6%98%8E.md)

- 当前关键实现：
  - [codex_compat.rs](../../crates/service/src/rpc_dispatch/codex_compat.rs)
  - [mod.rs](../../crates/service/src/rpc_dispatch/mod.rs)
  - [rpc_endpoint.rs](../../crates/service/src/http/rpc_endpoint.rs)
  - [proxy.rs](../../crates/service/src/gateway/upstream/proxy.rs)
  - [transport.rs](../../crates/service/src/gateway/upstream/attempt_flow/transport.rs)

- 上游参考：
  - `D:\MyComputer\own\GPTTeam相关\codex\codex-rs\app-server\README.md`
  - `D:\MyComputer\own\GPTTeam相关\codex\codex-rs\core\src\client.rs`
  - `D:\MyComputer\own\GPTTeam相关\codex\codex-rs\login\src\server.rs`
