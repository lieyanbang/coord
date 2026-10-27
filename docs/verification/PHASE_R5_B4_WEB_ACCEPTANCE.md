# R5 B4 — Real ChatGPT Web acceptance

2026-10-27，重启 Codex 后续验。**B4 BLOCKED**：浏览器已恢复，正常 OAuth
重连和候选工具目录可见；RelayAgent 在获授权的重载后启动失败，恢复原代码后仍失败。
已停止写入验收。本轮生产合成 `batch_id = null`，两聊天新增消息/任务调用均为 0。
**原业务 FROZEN；B3 历史 PASS 保留，但当前整套服务不能称 PRODUCTION READY。**

## 当前阻塞与恢复边界

`CoordRelayAgent` 当前 SCM 身份为 LocalService（与本轮起点相同），
而实际引用的以下文件及其父目录 DACL 只授权 Administrators、SYSTEM、旧用户
SID 尾号 1001，没有 LocalService 的读取权限：

- `C:\ProgramData\coord\g7\relay-private\identity.json`
- `C:\ProgramData\coord\g7\relay-private\identity.key`
- `C:\ProgramData\coord\g7\daemon-private\management.key`

启动顺序先加载 identity policy。09:32:09Z 起 Agent 仅记录
`fatal / startup_failed`，日志未输出原生错误细节；09:34:29Z 恢复原版两个模块并
重启后仍同样失败。策略结构用 fixture key 验证通过，未读取真实签名密钥。
服务在恢复尝试后自行进入 Stopped，09:39:08Z 没有 Agent Node 进程；Sidecar
HTTP 503、`DEGRADED / agentConnected=false / pending=0`。
这是有直接 DACL 证据的启动权限不兼容；日志本身不能被写成已捕获 EACCES。

未修改这些 ACL、服务身份或凭据，也未借其他账户启动生产代理。修复需单独授权
精确的 LocalService 访问边界，或决定专用代理身份；现有 TASK 只授权 B4 隔离
配置与受控重载，AGENTS 的凭据/安全边界要求不允许据此自行扩大权限。
本轮重载暴露了该问题并导致 Relay 从连接状态退化；回退代码未恢复连接，不能
用 Core 健康或旧 B3 报告掩盖这一当前限制。

## 实际执行与 Gate

| 项目 | 结果 | 证据与边界 |
| --- | --- | --- |
| 新鲜 B3 起点 | PASS | 08:57:05Z 四组件/SYSTEM READY；08:58:56Z Sidecar 200/READY/connected |
| 用户浏览器 | PASS | Edge 正常登录；只打开两条指定既有聊天及 Coord 设置，未发送新聊天消息 |
| 正常 OAuth 重连 | PASS，UI 流程 | Coord 账户菜单“重新连接”→自有 authorize 页面→Authorize→返回已连接 Primary 账户；无认证响应拦截 |
| 实际工具目录 | PASS，可见性 | 正常“刷新工具”后，页面显示 Write11/Read5；六个名称 claim/progress/complete/release/resume/status 均可见，未当作成功调用 |
| OAuth refresh | NOT_VERIFIED | 没有真实业务工具调用与脱敏 refresh 记录的关联证据；未人为修改 token 期限 |
| 冻结例外隔离 Gate | PASS，仅副本 | 一致副本上运行真实 Core，4 项文本任务、无 Action profile/validator；历史业务写负例均拒绝，最后恢复原 24 个定义 |
| 精确生产 B4 binding/Batch | NOT_RUN | 未建立 binding、未改任何旧期限、未创建生产 Batch 或部署冻结例外 |
| 两聊天写入、六项操作、刷新恢复 | NOT_RUN | A/0000、B/0001 未收到 B4 测试指令；无真实 Web receipt、续租或恢复 PASS |
| 生产业务保护 | PASS | 起止受保护数据与护栏指纹完全相同；SQLite integrity_check=ok |
| 结束服务健康 | BLOCKED | Core/DB/Broker/Runner、SYSTEM READY；RelayAgent Stopped，Sidecar DEGRADED |

聊天引用：
[A / 0000](https://chatgpt.com/c/6adc81f8-d6fc-83ec-a347-1f5e22910047)、
[B / 0001](https://chatgpt.com/c/6adc82c9-4b6c-83ec-b273-477e77ade133)。
它们是同一 OAuth owner 下的逻辑标签；未声称独立对话身份隔离。
Connector 为 `asdk_app_6adda574a9248191931fb576668f28a2`，入口
`https://lieyanbang.com/app/CoordMCP/`。保留原有批准设置，未扩大权限。
没有访问 OAuth store、浏览器存储、token、client secret、cookie 或签名密钥内容。

## 入口同步、回退与测试

部署前发现 Linux 入口仍为旧 G7 十工具版本。候选来自现有源码提交
`ea9783268e33e546572fe346335056e28bc4d435`，与本地源码逐文件匹配；仓库产品源码
没有新修改。09:32Z 仅更新 Windows Agent 的 `identity.cjs / protocol.mjs`，
Linux MCP 的 `index.cjs / oauth.cjs`、共享 `identity.cjs` 和 Sidecar 的
`identity.cjs / protocol.mjs`，合计 7 个文件；`agent.mjs / server.mjs` 原本相同。
事前备份精确代码及 policy，原文件 ACL/owner/mode 保持；仅重载 CoordRelayAgent、
coord-relay.service、feathersjsapp.service。未重载 OpenResty、Core、Runner 或 Servy。

Agent 启动失败后，09:34:29Z 回退 Windows 两文件，09:36:41Z 回退 Linux 五文件；
读回原始字节及 Windows ACL、Linux owner/mode 均一致，policy 精确未变。
正常发生的 OAuth 流未回滚。ChatGPT 随后刷新工具并整页重载，最终目录恢复
Write7/Read3；中途仍显示 16 项的旧页面快照另留，未把缓存当作回退失败或继续写入。

本轮已完成的定向验证（退出码均 0，复用结果，没有为留日志而重跑）：

```text
node.exe --test --test-name-pattern='trusted tool schemas|trusted RPCs|trusted owner OAuth' relay/coord-mcp/coord-mcp.test.cjs
3 passed / 0 failed，918.9411 ms
node.exe --test --test-name-pattern='logical worker input|one authenticated owner|logical worker, agent|signed logical requests' relay/logical-workers.test.mjs
4 passed / 0 failed，181.0574 ms
node --check：候选四种文件均 exit 0
```

09:24:43.809–09:24:44.575Z 的副本 Gate 使用 Core PID 2860、loopback 20777。
bootstrap 只改变 1 个 INSERT 护栏，获得真实副本 Batch ID 后 scope 改 13 个定义；
始终有 24 个护栏，所有 DELETE 与 completion_actions 护栏保持原定义。
正式任务、旧 Canary、receipt 删除、历史 Action 修改均被拒绝；进度/续租、resume、
release、旧 attempt 拒绝、完成、同结果幂等重放与变更结果冲突均有真实 Core 结果。
副本共 4 项，1 completed / 3 pending、0 active claims、0 Actions；原 8 张表所有
原列（含 fence）在 SQLite 内比较均 changed_or_missing=0。
结束恢复原 SQL，确认新 claim 被拒绝，所有自有 Core 已退出、临时管理 key 已删除。
副本 ID 仅存在隔离证据，不能给浏览器当生产 ID。

[Fork CI 40377126693](https://github.com/lieyanbang/coord/actions/runs/40377126693)
本轮经 GitHub Connector 复核 `completed / success`，head 即上述候选 SHA。
没有新 Rust 改动或新产品源码提交，因此未重跑全套 Rust/容量/B3 或无变化完整 CI。
定向测试、隔离 Gate 和旧 CI 均不能替代尚未进行的真实双会话验收。

## 结束保护与证据

09:37:11.193Z 结束审计：Core PID 9844（LocalService）、Runner PID 4616（原专用
SID 尾号 1004）、Servy PID 3224 均未变；7777/7779 仍为原 loopback。
四组件 READY，SYSTEM monitor Ready、LastTaskResult=0、失败计数 0。
结束审计 exit 1 正确保留，原因是 RelayAgent 已 Stopped；没有将其写为全项 PASS。
Linux 09:39:08Z 三服务 active/running、NRestarts=0，OpenResty PID 109204 未变，
但 Sidecar 503/DEGRADED；进程 running 不等于链路可用。

受保护数据起止精确一致：正式 Batch 121 项（105 completed / 16 pending），两旧
Canary 10/4 completed，历史 Action 38 failed / 83 succeeded；没有 claim/replay。
原 24 个生产护栏始终未修改，SQL SHA256：
`678a8a1227dda09cbf98e43b90f348e8b242b2ce66e94cdfec1c23e8ee48dd3a`。
旧 Broker key 仅核对 ACL 元数据，未读取/复制/哈希其内容。
没有生产 B4 授权可撤销，四个原 binding 仍过期，三套 publisher profile/pin 原样保留。

本轮原始证据在忽略目录 `target/b4-web-resume-20261027/`：

- `before.* / after.* / preservation-summary.json`：生产起止、失败退出码和保护比较；
- `coord-reconnected.png`、`discovered-tools-16.*`、`rollback-tools-final.*`：正常 UI 与真实目录；
- `isolated-gate-v4.*`、`b4-freeze.py`：已通过的副本 Gate 及精确恢复算法；
- `payload/source-manifest.json`、`reused-validation.json`：来源、哈希、定向测试控制台转录与 CI；
- `windows-entry-deployment.json / windows-entry-rollback.json / rollback-audit.json`、
  `linux-entry-deployment.json / linux-entry-rollback.json / linux-after.json`：部署、回退、当前故障与 ACL 元数据；
- `relay-startup-readonly.json / policy-structure.json`：真实配置引用与 fixture-key 结构检查；
- `baseline.json / git-preservation.json`：原有 3991 项工作区状态和只读合同摘要。

远端 `C:\coord-b4-web-resume-20261027-a` 的副本/备份限 Administrators/SYSTEM；
Linux 备份 `/var/backups/coord-b4-web-resume-20261027` 权限 0700，暂存目录同为 0700。
没有新增常驻服务、账户、计划任务或持续测试端口；私有 DB/策略备份未下载或提交。
只保留必要诊断和可恢复备份，不以旧 DB 覆盖生产数据。

历史失败保留：上一轮三个浏览器初始化入口均报 kernel assets / os error 3，记录在
`target/b4-web-20261027/browser-runtime-failures.json`，重启后已恢复。上一轮首个
只读复合断言失败、随后两次只读通过的原始证据和提交 `d49236e7` 仍保留。
本轮副本 v1/v2/v3 分别因 Registry 版本、空 Registry、旧 Batch profile pin 不全
启动失败，证据留存，后改用已测试的离线 B3 fixture；没有反复触碰生产。
一次准备夹具的组合命令在执行前被自动审查以 `blocked by policy` 拒绝，未给细项；
未重试读取现场 publisher/Registry 的做法，改用既有离线 fixture 后 Gate PASS。

## 后续准入

先单独授权并恢复 RelayAgent 的最小凭据访问/服务身份兼容，使真实 Sidecar READY；
保留所有现有权限，不默认扩大目录继承或切换为 SYSTEM。之后恢复已验证的 R5
入口，再沿本报告继续 B4：精确 client/owner、真实新 Batch、时限绑定、生产冻结
例外与独立回读、双聊天六项操作和中断恢复、真实 OAuth refresh。未完成的 Gate
维持 NOT_RUN/NOT_VERIFIED。TASK/PLAN 本轮只读，仓库交付仅此报告，证据提交
使用 `[skip ci]` 并只推用户 Fork 非发布分支；原脏文件、未跟踪成果和历史保留。
**不解冻正式 16 项、不重放历史 38 条失败 Action、不运行旧 Canary、不进入 B5。**
