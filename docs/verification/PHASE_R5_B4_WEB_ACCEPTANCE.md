# R5 B4 — 真实 ChatGPT Web 验收

2026-10-27，证据截至 **11:42:44 UTC**。**B4 PASS；服务 READY；正式业务 FROZEN。**
用户在上一轮 BLOCKED 后明确授权修复 RelayAgent 最小凭据访问并继续。
本轮完成真实 OAuth 重连/refresh、工具发现、两条既有聊天的六项操作、刷新恢复、
fencing、幂等、独立对账和收尾撤权；没有解冻正式业务或进入 B5。

## 实际改动与范围

- 保留 CoordRelayAgent 的 LocalService 身份，仅给两个文件增加非继承 Read：
  C:/ProgramData/coord/g7/relay-private/identity.json 与 identity.key。
  原 ACE、owner 和父目录 ACL 均精确保留。初始给旧 management.key 增加的 Read
  已撤回，恢复其原 ACL；诊断未读取/复制/哈希签名密钥或旧 Broker key 内容。
- Agent config.json 的 coordAuthKeyFile 从旧 G7 路径改为当前 Core Registry 所指的
  C:/ProgramData/coord/g7/r5-daemon-private-20261027/management.key。
  新 key 的既有 ACL 未修改，其他配置字段原样；诊断没有输出凭据。
- 同步现有已验证 R5 MCP/Relay 入口，共 Windows 两文件、Linux 五文件；
  agent.mjs、server.mjs、Core、Runner、三套 publisher profile/pin 未更新。
  仅受控重载 CoordRelayAgent、coord-relay.service、feathersjsapp.service；
  OpenResty PID 109204、Core PID 9844、Runner PID 4616、Servy PID 3224 保持。
- 仓库产品改动限 relay/coord-mcp/：oauth.cjs 在 token 入口复用已有审计设施，
  严格只记录 grant_type 白名单、固定路由、时间和响应状态，排除请求头、其他
  请求内容与凭据；oauth-access.test.cjs 增加泄漏负例。index.cjs 更正 claim
  说明：返回任务的 id 应作为后续 task_id，避免模型误以为领取结果缺字段。
- 新建下述四项固定文本任务；action_profile=null、validator=null、actions=[]。
  没有调用 synthetic publisher 或 Runner Action，实际关联 Action 为 0。
  结果保存在此 Batch 的 SQLite receipt/result；诊断输出限本轮私有目录。

## 真实 Web、OAuth 与六工具

同一用户既有会话：
[A / worker 0000](https://chatgpt.com/c/6adc81f8-d6fc-83ec-a347-1f5e22910047)、
[B / worker 0001](https://chatgpt.com/c/6adc82c9-4b6c-83ec-b273-477e77ade133)。
通过可见 UI 选择已连接 Coord，发送本轮最少测试指令；未操作其他聊天/插件。
worker_id 是同一 OAuth owner 的标签，不是独立对话安全身份。

正常 UI 重新连接 → 自有 authorize 页面 → Authorize → 回到 Primary 账户。
仅从可见授权 URL 提取公开 client_id 做精确绑定；未读取 OAuth store、
浏览器存储、cookie、token、client secret 或拦截认证响应。
正常 OAuth 连接保留，原有批准设置未扩大。

网页刷新工具后实际显示 **Write11 / Read5**，包含 coord_claim / progress /
complete / release / resume / status；最终页面明确显示修正后的 id 说明。
两聊天的真实调用均经 MCP → Sidecar → Windows Agent → Core，独立 Relay
日志共 **37** 次：claim 5、progress 5、resume 6、release 1、status 7、
get_task 6、complete 7（其中 5 次成功，含一次精确重放；2 次预期 rpc_error）。

**真实 refresh PASS**：无凭据审计记录在 11:21:20.853、11:28:23.778、
11:33:28.374 UTC 均为 grant_type=refresh_token / HTTP 200 / finish；
分别与随后 B 首次周期、A 重分配、B 最后周期的真实 Relay 请求时序关联。
11:41:59.154Z 收尾只读请求还发生一次正常 refresh，业务绑定已过期，请求被拒绝。
没有修改 token 期限或以合成 HTTP 客户端替代 Web 验收。

## 新 Batch 与独立结果

真实生产 Batch：**538fef02-5e14-4ece-aaa3-48c417fcd6b8**
（coord-b4-web-20261027-0857，11:13:37Z 创建）。

| task key | 真实 task ID | worker / attempt | 最终结果 |
| --- | --- | --- | --- |
| b4-text-01 | 00d3dace-00db-4c5a-ae00-4f44e0959ba1 | A / 1 | completed，marker 1 |
| b4-text-02 | d5c8d2b0-080b-4875-9fd4-f948f2b214c7 | B / 1 | completed，marker 2 |
| b4-text-03 | d44b2a91-f963-4abd-ab29-19d5bf13189c | B / 1 released → A / 2 | completed，marker 3 |
| b4-text-04 | 2d3fc1a1-a110-4c44-8289-4ccf160c7f21 | B / 1 | completed，marker 4 |

各 result 均为 COORD_B4_TEXT_V1 与任务 payload.marker 原文。
管理 RPC、SQLite 和 Action 状态独立确认：
**4 completed / 0 pending / 0 claimed / 0 active claims / 0 failed /
0 needs_attention / 0 Actions**；4 条完成 receipt、5 条 progress receipt、
5 条 attempt（4 completed、1 released）。无新 ACK。
两聊天各完成两项真实 claim→progress→complete。

| Gate | 实际证据 |
| --- | --- |
| progress 与续租 | A attempt 1 原租约至 11:28:14.846Z，progress 后至 11:31:35.295Z；sequence/checkpoint/receipt 持久化。简化 progress 原子续租，未冒称另行调用 legacy extend；legacy extend receipt 为 0 |
| 浏览器刷新恢复 | A checkpoint 写入后独立回读，真实整页 reload；新轮先 resume，返回同 task/attempt 1 与 B4-A-SAVED-CHECKPOINT-20261027，再完成和精确 get_task；未再次 claim |
| release / 重分配 | B 的 task 03 attempt 1 保留 B4-RELEASE-CHECKPOINT 并 released；resume 返回 released。随后 A 领取 attempt 2，持久化新 checkpoint；业务 failure_count 仍 0 |
| 旧 attempt fencing | 11:30:35.894Z，B 对旧 attempt 1 仅提交一次，被 Core rpc_error 拒绝。提交前后独立任务读回完全相同：仍由 A 持有 attempt 2，result=null |
| 精确重复完成 | B task 02 首次完成并 get_task 确认后，同结果再提交返回 ok=true / replayed=true；修改 marker 的重复提交被 -32009 拒绝，原结果未变；完成 receipt 总数仍仅 4 |
| 权限边界 | Web 错误 worker 0002、正式 Batch 的只读请求均拒绝；正常新 Batch status 成功。其他 client 的拒绝使用无真实密钥的策略 fixture 验证，未创建其他 OAuth 账户；loopback MCP 无 OAuth 为 401，公开无认证请求被 Cloudflare 403 拒绝，两者分别记录 |
| 结果不确定处理 | 刷新后部分历史工具回复显示“无法加载此回复”；先独立管理/SQLite 对账，再由新轮 resume 取得权威状态，无盲重放 |

恢复仅证明用户发起下一轮后的续接，没有声称 ChatGPT 自主后台唤醒。

## 冻结、撤权与服务收尾

复用上轮一致副本 Gate：原有业务写负例拒绝、受限合成任务可写、精确恢复原 SQL；
没有重跑已通过的迁移/Core-Broker/签名/nonce 生产验收。
本轮生产只部署同一已测试算法：bootstrap 改 1 个定义，取得真实 Batch/任务 ID
后锁定仅四项的例外（13 个定义）；始终有 24 个护栏，所有 DELETE 和 Action
护栏保持原义。生产对正式任务、旧 Canary、旧 receipt、历史 Action 的四项
负例均拒绝，全部探针事务回滚。原 8 张表的所有原列（含 fence）在 SQLite 内
比较均 changed_or_missing=0，未输出任何 fence/credential 值或哈希。

11:36:56–57Z 恢复原 **24** 个 SQL 定义；精确 SHA256：
678a8a1227dda09cbf98e43b90f348e8b242b2ce66e94cdfec1c23e8ee48dd3a。
SQLite schema_version 从 65 到 145，是获授权触发器 DDL 的计数变化；
未将其当作业务列变更，也未人为回写此计数。integrity_check=ok。
accepted 四项及 receipt 保留，没有用旧数据库回滚。

精确 B4 client + Batch + workers 0000/0001 的 scopes 仅为
coord:read/claim/progress/submit/release，原定窗口 11:14–15:14 UTC。
收尾实际到期：Windows 11:37:22.133Z，MCP/Sidecar 11:37:22.835Z；
三跳均已重载，五个 binding 全部过期，原四项绑定和 legacy allowlist 未改。
撤权后 A 只调用一次 status，返回 INVALID_ARGUMENT，未重试；UI 不细分错误原因，
结合实际过期配置、重载与未转发至 Relay 的事实核对收尾，未写成 OAuth 401。

最终保护/健康证据：
- 正式 Batch 仍 121 项：105 completed / 16 pending；两旧 Canary 仍 10/4 completed；
  历史 Action 仍 38 failed / 83 succeeded；受保护业务指纹起止一致。
- Core/DB/Broker/Runner 全部 READY；SYSTEM monitor Ready、LastTaskResult=0。
  Core/Runner 原 PID、SID、7777/7779 loopback 不变。
- Agent 真实 Node PID 3772，父服务 PID 3672，SID S-1-5-19；
  仅两项授权 Read 差异，其余原 ACL 和父目录精确一致。
- Linux Relay/Feathers/OpenResty active/running、NRestarts=0；
  11:42:44Z Sidecar HTTP 200 / READY / agentConnected=true / pending=0。
  无测试常驻服务、账户、计划任务或 20777/20778 监听。

## 测试、CI、Git 与证据

~~~text
scripts/verify.ps1 -Stage b4-oauth-audit
fmt / locked clippy / locked tests / release build：全部 exit 0
Rust：88 passed，8 intentional ignored；未将 ignored capacity 算作容量证据
npm test --prefix relay/coord-mcp：19 passed / 0 failed
token lifecycle audit 定向负例：1 passed / 0 failed
claim 说明修正后的 trusted schemas / RPCs / owner OAuth：3 passed / 0 failed
~~~

[Fork CI 40494154171](https://github.com/lieyanbang/coord/actions/runs/40494154171)
在 9b693e0bfd8b8b2744200f2d8ffa732bba89281e 上 success（无凭据 OAuth 审计）。
真实 Web 暴露字段说明问题后，
[最终 Fork CI 40495362795](https://github.com/lieyanbang/coord/actions/runs/40495362795)
在 04f91afc7cb9d41db46a64586a6974122e187f6e 上 success，匹配最终 MCP 源码。
第二候选复用输入未变的本地 Rust Gate，新增说明做定向验证；两个云验收均由实际
源代码候选触发，没有重触发无代码变化的完整 CI。Core/Runner 沿用 B3 已验二进制。

原始脱敏证据在忽略目录 target/b4-web-acl-20261027/：
- before.*、after.*、preservation-summary-v2.json、final-windows-v2.json、
  final-linux.json：保护、ACL/身份、三跳到期、健康和全部 Relay 请求 ID/状态。
- batch-created.json、各 readback、terminal-acceptance-summary.json、
  final-accepted-data.json：真实 ID、原八表逐列比较、attempt/checkpoint/receipt/result。
- production-denials.json、freeze-restored.json、两端 policy-bound/expired.json：
  冻结负例、精确恢复与限定期限。
- 各 a/b-*-ui.txt、corrected-tools-final.*、batch-completed-final.png：
  实际网页工具、刷新恢复、负例、幂等和 4/4 完成截图。
- stage-b4-oauth-audit-*、coord-mcp-tests.log、claim-description-tests.log、
  ci-final.json：命令、退出码、原始本地测试及匹配 CI。
- baseline.json、git-preservation.json：原有 3991 项脏/未跟踪状态及只读文件摘要。

远端数据库副本、原始策略/ACL/配置备份只保存在本轮私有目录：
Arasaka C:/coord-b4-web-acl-20261027-a（Administrators/SYSTEM），
Linux /var/backups/coord-b4-web-acl-20261027 与暂存目录（0700）。
私有 DB、策略备份、密钥和原始认证数据未下载或提交。
本轮无需撤销其他临时权限；必要服务修复与已验入口保留。

历史失败可追溯：d49236e7 的浏览器运行时不可用、0dd5650 的 Relay 启动 BLOCKED、
上轮三个副本夹具失败及自动审查拒绝均保留于历史和原始目录。
本轮首次 claim 因说明不符停止，已修正；配置修复包装的数组筛选失败在改动引用后
停止，随后精确对账、撤回闲置权限并完成重载。结束探针一次因绝对路径筛选漏掉
相对 agent.mjs，另一次因把 schema_version 计入全对象比较而失败；原结果保留，
分别用真实 SCM 父子 PID 和原八表全列/护栏 SQL 证明，无生产回退被掩盖。

## 剩余边界

B4 必需 Gate 无剩余阻塞。ChatGPT 历史工具回复在刷新后仍可能显示
“无法加载此回复”；本轮证明独立对账与新轮 resume 可恢复，没有修复 ChatGPT
自身的显示持久化。两条聊天共享可信 owner，未提供不可冒用的会话隔离。
本轮仅文本 fixture，无外部 publisher 动作或正式分类验收。

TASK.md、docs/PLAN.md 和原有 .gitignore 未改，原未跟踪成果及 Git 历史保留。
工作分支 codex/b4-web-20261027，仅推用户 Fork，最终报告提交使用 [skip ci]。
**停止于 B4；不解冻剩余 16 项、不重放历史 38 条失败 Action、不运行旧 Canary，
不自动部署其他生产组件、不进入 B5。后续正式业务必须有新任务和独立授权。**
