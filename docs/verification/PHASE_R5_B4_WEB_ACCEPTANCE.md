# R5 B4 — Real ChatGPT Web acceptance

2026-10-27. **WAITING_USER_BROWSER_ACTION — B4 未完成。**
本轮新鲜只读验证为 PASS；B3 保持 **PRODUCTION READY / BUSINESS FROZEN**。
未创建 B4 合成 Batch，`batch_id = null`，也没有对正式任务或历史 Action 写入。

## 唯一当前阻塞

浏览器控制运行时在执行任何 JavaScript、浏览器 inventory 或页面操作前失败：

```text
failed to write kernel assets: 系统找不到指定的路径。 (os error 3)
```

实际尝试依次为：`mcp__cua_repl.js` 的 `cua.getState()`；通过
`mcp__node_repl__js` 初始化已安装 Computer Use 技能的 `@oai/sky` 备用入口；
重置 CUA kernel 成功后重新调用 `cua.getState()`。三个初始化入口均遇同一错误，
备用入口的 import 未执行。没有截图、页面/登录状态或工具发现的当前证据。
只读进程元数据未观察到现有 Chrome/Edge 的 remote-debugging 标志，未重启浏览器、
修改用户 profile、启用调试端口或修复 Codex 全局安装来绕过阻塞。

依据本轮只读 TASK §2.2，在浏览器能力不可用时停为
`WAITING_USER_BROWSER_ACTION`；HTTP metadata、Codex 自身的 Coord 工具目录和历史
G7 Web 结果均不算本轮真实 ChatGPT Web 验收。

## 本轮 Gate

| Gate | 状态 | 实际证据/边界 |
| --- | --- | --- |
| B3 四组件、真实进程、SYSTEM/Servy | PASS，只读 | 06:57:40Z 与 06:58:55Z 均 READY；未启动/停止/重载服务 |
| Linux/Sidecar、公开 OAuth metadata | PASS，只读 | 06:56:46Z 三服务 active/running、NRestarts=0；Sidecar HTTP 200/READY/connected；两个公开 metadata endpoint 200 |
| 用户浏览器与已安装 Coord Connector | WAITING_USER_BROWSER_ACTION | 初始化前失败；未打开或操作 Settings/Apps，未验证当前登录与 Connector 状态 |
| 真实 OAuth 重连、MCP discovery、精确 client 归属 | NOT_RUN | 无 OAuth UI 操作；未访问 token/refresh-token store、cookie、浏览器存储或私钥 |
| 实际 OAuth refresh | NOT_VERIFIED | 未执行；没有以 metadata 200 或历史 token exchange 冒充 refresh |
| 新合成 Batch、限定冻结例外与 synthetic profile | NOT_RUN | Batch/任务/输出均未创建；原 24 个护栏保持原定义，没有部署例外或改三套 profile |
| 两聊天六项操作、续租/fencing/receipt | NOT_RUN | 两聊天消息数 0，真实 Web 工具调用数 0，没有 claim/progress/complete/release/resume/status 写测试 |
| 同一会话刷新/中断后的恢复 | NOT_RUN | 未进行浏览器会话操作或 HTTP 代理替代测试 |
| 正式 Batch/旧 Canary/历史 Action 保护 | PASS，只读 | 初始数据库采样、复核及结束采样与 B3 受保护指纹一致；触发器 SQL 一致 |

## 生产与保护读回

最终 UTC **2026-10-27T06:58:55.3384941Z**：

- Core PID **9844**，LocalService SID `S-1-5-19`，包装服务 PID 9036；
  Runner PID **4616**，原专用 SID 尾号 **1004**，包装服务 PID 7376。
  Core/DB/Broker/Runner 全部 READY；7777/7779 仍为原 loopback 监听。
- Servy/CoordRelayAgent 保持 PID **3224/5892**、原身份与 Auto；SYSTEM task
  `coord-health-monitor` 处于 Ready、LastTaskResult=0，内容 READY、失败计数 0。
- Linux Relay/Feathers/OpenResty PID **127496/127510/109204**，均 NRestarts=0。
  本轮只读取健康和公开 metadata，没有使用本地 client secret 伪造浏览器请求。
- `/etc/coord-relay/identity.json` 的 mtime 仍为
  `2026-10-25T07:59:15.841005+00:00`，四个原 binding 均过期；只输出选定的
  ID/batch/worker/scopes/期限元数据，没有读取签名文件、OAuth store 或输出 client digest。
  三跳新 B4 binding 的建立/加载验收尚未执行，未延长任何原期限。
- 正式 Batch `90ce2ca7-184e-4c3d-9d87-56386db25419` 仍 **121 tasks：
  105 completed / 16 pending**，attempt 合计 112、最大 2；两个旧 Canary 分别
  10 completed / 4 completed。原 progress/receipt/action 的安全指纹及派生 attempt
  指纹与 B3 及本轮起止读回完全一致，历史 Action 仍 **38 failed / 83 succeeded**。
- SQLite integrity_check=ok，**24** 个冻结触发器与审阅定义精确一致；SQL SHA256：
  `678a8a1227dda09cbf98e43b90f348e8b242b2ce66e94cdfec1c23e8ee48dd3a`。
  旧 Broker key 仅核对 ACL 元数据，与 B3 相同；内容未读取、复制或哈希。

首个采样脚本在复合保护断言处 exit 1，但首版尚未在断言前保存详细服务快照；
当时的只读数据库及护栏输出和原错误均保留，不能据此确定服务故障或断言失败原因。
将保存顺序移至断言前后，06:57:40Z 复核及 06:58:55Z 结束采样均 exit 0、全项
通过；初始/复核/结束业务与护栏指纹一致。期间没有生产修复或重启，未将首个失败
改写为 PASS。

## 变更、证据与继续点

Git 基线 `49c872c8054ab921b59a19764e097c1463587fda`；新分支
`codex/b4-web-20261027`。仓库仅新增本报告；一次性诊断脚本、工具错误转录和原始
读回位于忽略目录 `target/b4-web-20261027/`：

- `browser-runtime-failures.json`：三个运行时失败及零 UI/聊天/OAuth 操作记录；
- `before.execution.json`、`before.raw.stderr.log`、`before-data.json`、
  `before-freeze.json`：首个未通过的采样及当时已成功的只读子结果；
- `before-recheck.*`、`after.*`、`preservation-summary.json`：真实进程、服务、
  SYSTEM、数据/触发器与起止比较；
- `linux-before.json`、`public-metadata.json`：只读外部健康/策略元数据和公开入口；
- `baseline.json`：本轮开始时 3991 条原有脏/未跟踪状态及 TASK/PLAN/.gitignore 摘要。

Arasaka 的 `C:\coord-b4-web-20261027-a` 仅留本轮诊断源码和结果，限
Administrators/SYSTEM；没有新账户、任务计划、常驻服务、密钥、临时授权或合成
业务对象，因此没有需要撤销的 B4 权限。没有修改 OAuth/identity policy、生产
SQLite、冻结 SQL、registry、profile、Servy/SCM 或 Access，也没有回滚任何正常 OAuth 流。

产品源码/正式脚本未变，沿用 B3 候选的
[Fork CI 40377126693](https://github.com/lieyanbang/coord/actions/runs/40377126693)；
没有新跑 Rust/全量 CI，也不把旧测试视为 B4 PASS。证据提交使用 `[skip ci]`，
仅推送已验证用户 Fork 的非发布分支。TASK/PLAN、原有 .gitignore、未跟踪成果及
Git 历史保持。

**一次性恢复动作**：退出并重新启动 Codex 桌面应用，保留用户已登录的浏览器；
回到本聊天发送“继续 B4”，先确认浏览器控制入口恢复。此操作是针对本轮初始化
故障的恢复尝试，不承诺重启一定修复。若仍报相同错误，需要先恢复该运行时，
不要以私有认证接口或复制浏览器凭据替代。

恢复后沿用本轮 B4 授权，从 B4-0 的真实浏览器检查继续：仅打开 TASK 指定
[会话 A](https://chatgpt.com/c/6adc81f8-d6fc-83ec-a347-1f5e22910047)（0000）与
[会话 B](https://chatgpt.com/c/6adc82c9-4b6c-83ec-b273-477e77ade133)（0001）；
正常 UI 授权/发现通过后，再验证新合成 Batch 的精确冻结例外，最后执行写测试。
当前没有可交给浏览器的 B4 Batch ID。正式 16 项、38 条失败 Action、旧 Canary
继续冻结，不进入 B5。
