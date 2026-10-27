# B3-A 兼容副本与生产恢复准入 — 2026-10-27

**结论：B3-A PARTIAL / BLOCKED；B3-B NOT_RUN，不放行生产。**
用户本轮正式授权执行当前 TASK；其 B3-B 仍要求预演通过后的独立生产批准。
本轮没有修改生产服务、在线数据库、原 Registry、监控配置、旧 Broker key ACL，
没有创建 Batch、领取任务或重放历史 Action。PLAN/TASK 执行期只读。

基线 HEAD `044965571b7f8a0e3fc03c2365fb44ef5d79d483`，新分支
`codex/b3-recovery-20261027`。本轮没有产品源码或正式脚本修改；临时工具和原始
输出留在忽略目录 `target/b3-recovery-20261027/`。生产 Core 不可用，且当前
TASK 保护生产任务，因此使用 Git 证据提交作仓库账本，没有编造 task/ack ID。

## 制品与已有验收

实现 commit `ea9783268e33e546572fe346335056e28bc4d435`。本轮通过 GitHub
连接器读回 [Windows run 40377126693](https://github.com/lieyanbang/coord/actions/runs/40377126693)
及 job `121763119437`：全部步骤 success，包括真实 LocalService/专用 Runner、
签名发布、崩溃 uncertain、cleanup、Relay/MCP/Python 及打包。
[构建 artifact 12677086008](https://github.com/lieyanbang/coord/actions/runs/40377126693/artifacts/12677086008)
未过期，manifest 和运行 SHA 相符。原下载和内外 ZIP 校验见
[F 阶段报告](PHASE_R5_PS51_FIXTURE_20261027.md)，不重新调查 F0/F1。

本轮重新校验本地留存的部署包、三个 EXE 和监控脚本；上传到隔离目录后再次核验
包及实际运行 Core。SHA-256：

| 对象 | SHA-256 |
| --- | --- |
| 部署 ZIP | `c4edd881f2f52da702389fc37615c2eb1350d96728fa6bec82378e0064ba49a3` |
| Core | `943959b12ce75f8de716320e9fe15e72e3ea2911dbf096bedc6dc4bcfc22f3bc` |
| Runner | `72b46d25722842a30afd6cd3db32c9df5e15ae28fd0a9cf9a2ddb7a14dcb4baa` |
| 发布器 | `a6679db9064f10bfa74008458853db069ce5b79f018a3b6685710546e13c4879` |
| 监控脚本 | `efcee606cf5cb2483f0985b7e939fa661bf13b4035fb81a164a81b4cb7af6a25` |

## 新鲜现场与数据库副本

Windows 初次观察 `2026-10-26T21:39:13Z`，最终观察 `21:55:04Z`（布里斯班为
2026-10-27）。两次均为 Core/Runner Stopped、7777/7779 无监听；Servy 和
CoordRelayAgent Running，PID 仍为 3224/5892。Core 身份 LocalService，Runner
仍为既有 `coord_action_g7`，未更改。最终真实 SYSTEM monitor 最近运行
`21:54:54Z`，`LastTaskResult=1`，状态文件 FAIL；未宣称生产恢复。

Linux `21:39:54Z` 只读核验：Relay、Feathers、OpenResty active，NRestarts=0，
Sidecar HTTP 200、READY/connected、pending=0。没有更改该主机。

通过 SQLite Online Backup API 从只读源连接获取一致副本，不直接复制在线 DB/WAL。
新副本及既有 `g7-20261026T124419Z-0380fb72` 备份均 quick_check=ok、八张表
行数匹配、2,949,120 bytes，备份 SHA-256 均为
`ceea40ca4d25b24a62a5d096e84489a16299c728664d94142320faa5cf53c4c3`。
备份、密钥和含受保护配置的文件没有下载或提交；仅取回不含行内容的审计摘要。

预演位于 Arasaka 独立目录 `C:\coord-b3-20261027-a`，RPC/Broker 使用
127.0.0.1:17777/17779。Registry 保留原 profile 名称、版本及事件结构，但将
执行路径、参数和工作目录重定向至副本范围；没有使用生产发布目标。
测试独立管理凭据及签名密钥仅在该隔离目录生成。

候选迁移后 integrity_check=ok，新增 task_attempts 124 行及 runner_nonces。
八张原有表均用迁移前相同列集合逐表比对行数和规范化指纹，兼容启动、签名/重启
及回退后全部一致；新增列和派生表单列，不把新增 schema 误算为旧数据变更。
源 DB schema cookie 始终 32；备份副本 cookie 为 1，迁移副本为 8，cookie 不作
产品版本号。原任务 attempt、receipt、checkpoint、结果和 Action 均未被重写。

生产最终只读结果与初次 database 审计逐值一致：正式 Batch 121 项，
105 completed/16 pending；105 receipts、106 progress、145 progress receipts；
两个旧 Canary 及正式 Batch 的受保护指纹一致。全局 Action 仍为
38 failed/83 succeeded，未增加 attempt、重试或副作用。旧 Broker key 只核对 ACL，
未打开、导出或哈希其内容；前后 ACL 完全一致。

## 预演结果与阻塞

| Gate | 结果和证据边界 |
| --- | --- |
| 真实 LocalService 兼容 Core | PASS：SID S-1-5-19；Scheduler/DB READY、整体 DEGRADED、Broker UNAVAILABLE |
| 独立管理认证 | PASS：认证 tasks/list（skip_reclaim=true）成功，未认证请求 HTTP 401 |
| Core 密钥隔离 | PASS：真实 LocalService 可读隔离公钥、不能读隔离私钥；Core 未配置生产旧密钥内容读取 |
| 签名 Broker/Core | PASS：Broker READY；没有真实 Runner 心跳时 Runner UNAVAILABLE、整体 DEGRADED |
| 目标机签名和 nonce | PASS：有效签名 200；错 SID、过期、篡改、Bearer 替代及重复 nonce 均 401；Core 重启 3342 ms 后，同一未过期签名仍 401，新签名 200 |
| 目标机专用 Runner | **BLOCKED**：临时标准用户的真实 Runner 启动未通过，见下文 |
| Broker 故障隔离 | PASS：缺失公钥使 Broker/Runner SECURITY_ERROR、整体 DEGRADED；认证 Scheduler RPC 持续可用 |
| 兼容 Core 回退 | PASS：在同一已迁移副本恢复 external_runner=null；Broker UNAVAILABLE、RPC 可用，原有表指纹一致 |
| 崩溃 uncertain / fence / 发布闭环 | 复用相同实现的 Hosted PASS；本轮没有在生产副本创建动作，不宣称 Arasaka 独立 Runner 已验收 |
| 生产完整 READY / SYSTEM 两次成功 | NOT_RUN：B3-B 未获阶段批准，且准入仍阻塞 |

签名/nonce 请求由隔离操作员测试签名器发送，**不能替代专用 Runner 身份证据**。
其最后有效请求可令副本 health 的 Runner 分项显示 READY；这仅证明协议接受了
签名，不能据此放行生产。

三个临时标准账户依次用于有区别的启动试验，均已删除：

- Windows PowerShell 5.1 的跨身份 Process.ExitCode 为 null，没有 Runner 输出，
  不能当作 native exit 0；改用 PowerShell 7 取得实际退出码。
- SSH 会话下 Start-Process -Credential 实际返回 `-1073741502`
  （`0xC0000142` / DLL initialization failed），未出现 Rust 身份日志。
- 改用独立 Password/Limited Scheduled Task 后，在有界等待结束时仍为
  Ready / `267011`（尚未运行），没有 Runner 输出；没有据此断言其登录或 ACL 根因。

这证明目标机的隔离身份启动链路尚未验收，不能证明候选 Runner 有源码缺陷，也
不能从 Hosted PASS 推导 Arasaka PASS。本轮停止主机登录环境调查，不改系统登录
策略、全局权限、现有 Runner 服务账户或其他服务。B3-A 因此 PARTIAL/BLOCKED。

临时助手的其他失败原文也保留：旧远端备份助手不支持 --verify-only（改用隔离
上传的当前助手）；最初 LocalService 启动器未设置进程级 ExecutionPolicy Bypass
且无输出（对齐已有 Hosted 启动器后通过，未更改全局策略）；Node fetch failed
（改用直接 loopback HTTP 后完成协议测试）；第一次清理的 icacls 删除授权失败
（对精确隔离目录重建 Administrators/SYSTEM DACL 并逐项验证后通过）。

最终清理 PASS：临时账户、相关用户配置文件、任务、密钥、进程、17777/17779
监听均为零；保留的副本、服务导出和原始证据仅 Administrators/SYSTEM 可访问。
在线 DB、生产配置、生产服务状态及旧 Broker ACL 均保持。

## 恢复路径及停止决定

本轮只证明候选兼容 Core 可在已迁移副本恢复认证 Scheduler，Broker 明确降级。
没有验证旧 G7 在迁移后安全运行；已知它在现行旧密钥 ACL 下不能启动，因此
**恢复旧 EXE 不是可用回滚**。有新 accepted 结果时应保留当前 DB，使用同一候选的
兼容配置恢复或后续匹配 forward-fix；禁止用旧备份覆盖新结果。

本轮只读导出确认两个 Servy 实例 MaxRestartAttempts=3、RecoveryAction=2、
RecoveryOnCleanExit=false；没有重置计数。导出没有提供已用次数，生产恢复前仍需
确认实际恢复预算。现有生产发布器 hash pin 与文件一致，值为
`cac5496ed1dea50f837f28403feb62515de1ba1459ea2557abe6097ec07bec55`，不同于新包的
发布器；不能直接覆盖后留下不匹配 pin，也不应为本次签名迁移擅改原 profile 白名单。
既有 Servy 快照校验工具在两个已知路径均不存在，完整服务配置恢复演练未完成。

后续必须先解决并通过专用 Runner 的目标机启动/低权限 Gate，完成服务配置恢复
前提，才能形成可执行的 B3-B 变更单。届时应：刷新受保护基线及一致备份；先停止
Runner，部署匹配 Core 与保留管理认证的兼容 Registry，确认 DEGRADED 稳定；再
本机创建生产专用密钥、核对准确 ACL/服务身份并启用签名 Broker/Runner；失败时
停 Runner、恢复兼容 Registry 和同一 Core，保留现有 DB。当前**不得执行**该生产
序列；还需要 TASK 明确要求的独立生产批准。SYSTEM 两次新鲜成功、真实服务 Runner
与完整生产一致性验收均保留 NOT_RUN。B4 和正式 Phase H 没有启动。

## 可复核材料

结构化摘要：[PHASE_R5_B3_EVIDENCE_20261027.json](PHASE_R5_B3_EVIDENCE_20261027.json)。
本轮按 AGENTS 提交门槛运行一次 `scripts/verify.ps1 -Stage r5-b3-a`；命令、退出码
和日志索引见 `stage-r5-b3-a-checks.json`。fmt、locked clippy、locked tests、release
build 全部 exit 0；88 Rust tests passed、8 intentionally ignored，无可执行文件缺失
跳过。产品源码未变，因此复用上述相同实现的
Hosted run，不触发新的全量云端运行；本次提交仅归档证据。
容量专项、Linux/macOS Core 和真实 Web 均 NOT_RUN，没有追加这些范围的 PASS。

本地忽略目录保存 artifact-check.json、copy-verify.json、compat-retry.json、
signed-first-failure.json、signed-credential-failure.json、signed.json、
core-recovery.json、各阶段数据库摘要、final-windows.jsonl、final-summary.json、
清理结果及原始 stderr；Arasaka 隔离目录保留副本和受保护服务导出。

## B3-A1 追加 — 2026-10-27 00:37–00:56 UTC

**B3-A1：PARTIAL；真实低权限 Runner 功能闭环 PASS，临时 profile 清理未闭环。**
本轮唯一 ACTIVE 范围来自更新后的 TASK。源码、生产服务和生产凭据均未改动；
没有重跑已通过的副本迁移、Core/Broker 解耦、签名/nonce/重启测试或完整 CI。
原始失败记录、前面的 B3-A BLOCKED 和同实现 Hosted PASS 继续保留。

### 两个启动值已经分层定位

1. **`267011 / 0x41303` 是 Task 状态，不是 Runner 退出码。**
   历史 Security record `261979`、`261980`（2026-10-26 21:47:19.968Z、
   21:47:20.030Z）均为 event 4625、LogonType=4、Status=`0xC000015B`。
   Task 在批处理登录阶段被拒绝，没有创建 Runner。
   [Microsoft 的 4625 状态说明](https://learn.microsoft.com/en-us/previous-versions/windows/it-pro/windows-10/security/threat-protection/auditing/event-4625)
   与此相符。TaskScheduler/Operational 当时及本轮均未启用；没有打开全局审计，
   不声称取得不存在的历史 Task Instance/进程创建事件。
2. **`0xC0000142` 来自 Session 0 的继承桌面启动环境，发生在 Rust 之前。**
   历史 4624 record `261939` 已证明交互式登录成功。本轮对同一 disposable SID、
   同一原生 `cmd.exe → whoami.exe`、同一命令和创建标志做唯一变量为桌面的对照：
   继承 SSH 桌面 PID 5216，00:46:06.026–06.416Z，退出 `0xC0000142`；
   新建专属非交互 window station/desktop 后 PID 1496，06.424–06.602Z，退出 0。
   仅对新建对象授予该 SID/Administrators/SYSTEM 权限，没有修改已有桌面 DACL。
   这与 [CreateProcessWithLogonW 的桌面权限契约](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createprocesswithlogonw)
   一致；不是候选 Runner 的 DLL 或 Rust 业务代码故障证据。

### 最小隔离修复及真实 Runner 验收

只对本轮新建账户 `coordA164a92cf1`，SID
`S-1-5-21-3592388187-2353680085-815129118-1008`，临时添加一项
`SeBatchLogonRight`。原授权列表仅为 `S-1-5-32-544/551/559`，拒绝列表为空；
添加期间精确等于原列表加该新 SID。没有修改已有账户、GPO、安全模板或全局桌面。
退出后按 [LSA 账户权限 API](https://learn.microsoft.com/en-us/windows/win32/secmgmt/managing-account-permissions)
撤销该项，最终完整授权/拒绝列表均恢复一致。

真实任务采用 Password / Limited，在独立目录 `C:\coord-b3a1-20261027-a`、
端口 18777/18779、新空测试 DB 中运行；没有载入生产 DB 或正式 Batch。
复用原 CI 匹配的三个 EXE，哈希仍为前文列出的值。Core 实际 PID 8648、SID
S-1-5-19；Runner 日志报告本轮专用 SID、`privileged=false`。Core 可读隔离公钥，
不能读私钥；标准 Runner 可加载匹配私钥，不能读 DB/独立管理凭据、不能写
Registry/二进制/Windows 目录，管理 RPC 缺凭据及错误凭据均 401。

| 新鲜目标机检查 | Task Instance / Native PID | Task / Native exit |
| --- | --- | --- |
| 原生 whoami | `9A0A4010-67BE-4C41-9947-A7A19140A44D` / 1212 | 0 / 0 |
| Runner identity | `14AFB648-FD4B-4987-BBA5-AFEE82B1E0C4` / 1188 | 0 / 0 |
| Runner ACL | `2A6D0690-0044-4E1C-B5EA-50251C2B64F8` / 8568 | 0 / 0 |
| 实际签名连接 | `EFBD18B0-9ED0-4CF7-9CC1-1E06C602D587` / 2820 | 0 / 0 |
| task 发布 | `F83A2D12-ADA9-4D38-B270-31EB5B8290B9` / 5224 | 0 / 0 |
| batch index 发布 | `892F691B-8BCC-44ED-8AE3-5DB364735958` / 10036 | 0 / 0 |
| 错误 fence 负例 | `B157AB2D-9B97-49C8-B63E-73E8A37F925B` / 3076 | 0 / 0 |
| 完成后再次轮询 | `7502D3CA-7087-4ACB-887E-D2AE13F59A97` / 3100 | 0 / 0 |
| 错误 Runner SID 负例 | `37A56346-2E45-4EFA-A786-D5D11638976A` / 2024 | 1 / 1（预期拒绝） |

每项保留实际 wrapper/native PID、WindowsIdentity SID、UTC 开始/结束、分离
stdout/stderr、真实 ExitCode 和任务结果；没有将 null 当 0。新 Security 记录中
有本 SID 的 27 条 LogonType=4 成功登录，另有两条原生对照的 Type=2 成功登录；
这些登录记录包含任务注册/凭据验证，不能把 27 条登录算作 27 次 Runner 执行。
例如 identity 对应 records 262446/262449/262452，task 发布对应
262473/262476/262479。Task COM 返回各次真实 Instance GUID。CIM 的 LastRunTime
在不同请求中异常返回相同时间，原文保留但不用于运行时间结论；时间依据各次请求、
wrapper 记录及 Security 事件。没有补造未开启审计的 4688/TaskScheduler 事件。

独立空 DB 中的一项合成任务触发两条 Action；两条均 succeeded、attempt=1、
exit=0，生成 00.json、receipt、index 和 review_queue 四项隔离产物。后续轮询
未重放；由**相同低权限任务身份**发送的签名错误 fence 请求，authorize 返回
200/null，finish 返回 409，终态及 attempt 保持。这里没有用管理员签名器替代
Runner。nonce/重启负例复用相同实现的上一轮目标机证据，没有重跑。

### 清理阻塞与停止线

功能检查全部完成，但原生对照工具使用了 `LOGON_WITH_PROFILE`，本轮新增 profile
`C:\Users\coordA164a92cf1` 仍显示 Loaded=true，HKU 对应 SID 和 Classes hive
仍挂载。这属于**本轮隔离夹具的清理未闭环**，不能将 B3-A1 标为 PASS。
00:56:04Z 的最新核验：账户、任务、18777/18779 监听均为 0；该账户无存活进程，
全部临时私钥/公钥/管理密钥已删除，测试目录逐项恢复 Administrators/SYSTEM only。

已经尝试的有区别清理路径原文均保留：普通管理员 `reg unload` 为 Access denied；
仅在当前清理进程临时启用已持有的 Backup/Restore privilege 后，原生卸载两个
精确临时 hive 仍均返回 Win32 5，进程权限随后恢复；
[DeleteProfileW](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-deleteprofilew)
对精确本轮 SID/path 返回 Win32 87。没有强制卸载、删除 ProfileList 伪造成功、
重启 User Profile Service/Task Scheduler、操作其他 profile 或切换 SYSTEM 执行。

TASK 的停止线明确要求需要 SYSTEM/全局政策等处理时另行授权，因此停止进一步
主机修改。已准备但**未注册、未执行**的一次性 SYSTEM 清理候选：
`target/b3-a1-runner-20261027/system-cleanup-candidate.ps1`；远端同名文件在测试根目录。
它固定校验本轮 SID/path、无账户/活动进程，仅尝试标准 profile 删除 API 并记录结果；
没有强制卸载、服务重启、全局策略或生产操作。若获独立批准，仍失败则继续保留
PARTIAL，不扩大范围。该授权不包含 B3-B。

### B3-B 剩余准入与证据

- 本轮临时 profile 清理需闭环；B3-A1 目前 **PARTIAL**，真实低权限 Runner
  启动/签名/隔离 Action/退出部分 **PASS**，未回退为“Runner 源码不明崩溃”。
- Servy 完整配置副本恢复、实际已用重启预算、旧 publisher pin 与候选部署的
  对齐方案仍未验收；因本轮清理停止线没有继续创建新服务/身份来扩大试验。
- 生产新鲜一致备份、准确服务参数/ACL/生产专用密钥、实际服务 Runner 和 SYSTEM
  两次成功仍须在独立获批的 B3-B 完成；没有恢复生产、B4 或正式 Phase H。
- 最终只读生产 DB 审计与前轮受保护基线逐值一致；正式 Batch 105 completed /
  16 pending，历史 Action 38 failed / 83 succeeded。生产 Core/Runner 仍 Stopped，
  Servy/Relay PID 和身份不变；没有监听 7777/7779，没有读取/哈希旧 Broker key。

新结构化摘要见 [PHASE_R5_B3_A1_EVIDENCE_20261027.json](PHASE_R5_B3_A1_EVIDENCE_20261027.json)。
原始历史与新事件、九项任务 XML、PID/身份/退出记录、流日志、合成输出及清理失败
在 `target/b3-a1-runner-20261027/`；远端副本仍限 Administrators/SYSTEM。
本轮没有产品源码或正式脚本修改，复用 `stage-r5-b3-a-checks.json` 的本地构建门槛
及 Fork run 40377126693，不触发新的无代码变化全量 CI；本轮新增验证均为上述
最小目标机启动、登录、任务及隔离动作测试。TASK/PLAN 和既有 Git 成果保持。

## B3-A1 清理续轮 — 2026-10-27 01:13–01:29 UTC

**B3-A1：PASS。** 前轮真实低权限 Runner 的启动、身份/ACL、签名连接、两项隔离
Action、fence 负例和退出证据保持有效；本轮完成最后的临时 profile 清理。
本结论只适用于 B3-A1；**B3-A 整体仍 PARTIAL，B3-B 尚未具备完整准入且未授权执行**。
用户本轮明确授权“清理继续任务”。全部清理只针对本轮新增的 SID
`S-1-5-21-3592388187-2353680085-815129118-1008` 和
`C:\Users\coordA164a92cf1`，没有修改生产服务、在线配置、凭据或全局策略。

### 清理调用修正与实际结果

| SYSTEM Task Instance / 实际 PID | UTC 开始–结束 | 实际操作与结果 |
| --- | --- | --- |
| `80F05E13-87CA-4D2D-9E41-BDD3DA4D6259` / 1408 | 01:13:50.390–52.444 | 执行原审阅候选，DeleteProfileW 返回 87，profile 仍挂载 |
| `65FDD894-D09B-47D3-917A-8B456951D1D6` / 9880 | 01:15:41.728–41.855 | 标准非强制卸载 SID/Classes，均返回 Win32 5 |
| `333008AD-A0F6-41DD-9730-17848CE7C037` / 5964 | 01:21:37.660–37.759 | 修正 NULL 传参后标准删除返回 Win32 32，确认共享冲突仍在 |
| `734507DD-84D3-45B3-9F28-7F8AB3C86A04` / 4540 | 01:25:14.662–14.976 | 两个精确 hive 卸载 NTSTATUS 均 0；标准删除返回 0；清理 PASS |

四次 Task 的 LastTaskResult 均为 0；前面三次操作仍失败，不能以包装进程退出 0
替代清理成功。每次均记录真实 SYSTEM SID `S-1-5-18`、Task COM Instance、PID、
UTC 和独立操作结果，执行后删除相应一次性任务。

先前 Win32 87 是本轮清理包装脚本的传参缺陷：PowerShell 将 `$null` 转换成 C#
字符串参数时传入空字符串。最小无副作用探针及目标机探针均证实该转换；
[DeleteProfileW 的契约](https://learn.microsoft.com/en-us/windows/win32/api/userenv/nf-userenv-deleteprofilew)
要求第三参数为真正 NULL。改由 C# 内部传入 literal null 后返回 32，才取得真实
共享冲突证据；不能继续把 87 当作 Windows profile 删除层的最终错误。

按全部进程的 `GetOwnerSid` 核对该 SID 无存活进程，避免删除账户后名称无法解析的
漏检。启动器宿主已退出；User Profile Service/Operational records 282/284/285/
287/288 保留本轮加载路径。hivelist 将两个精确 SID 挂载映射到上述临时目录的
NTUSER.DAT 和 UsrClass.dat。加载事件宿主 PID 1700 同时承载 ProfSvc、Schedule
等多个系统服务；没有终止该进程或重启这些服务，也不据此断言已找到具体持锁句柄。

在新的清理授权内，最后采用一次性 SYSTEM 帮助程序，只对硬编码的这两个 native
路径调用 `NtUnloadKey2(REG_FORCE_UNLOAD=1)`，再调用标准 DeleteProfileW。
接口/标志按 [PHNT 原始声明](https://github.com/winsiderss/phnt/blob/master/ntregapi.h)
核对；它仅用于本轮 disposable 夹具收尾，没有进入产品或正式部署脚本。
执行前再次校验 SID、目录、账户不存在、无该 SID 进程及 hive backing paths；
只在本进程临时启用已持有的 Backup/Restore privilege 并恢复。两个卸载成功后
标准删除成功，未手工删除 ProfileList 或目录来伪造清理。前后其他 hive 的完整
映射逐值一致。原有失败记录及三次失败尝试原文均保留。

01:26:40Z 最终只读核对：该账户、profile、目录、两个 hive、相关任务、存活进程
及 18777/18779 监听全部为零；临时私钥/公钥/独立管理密钥仍不存在；保留的夹具
和证据逐项限 Administrators/SYSTEM。生产 Core/Runner 仍 Stopped；Servy/Relay
保持 PID 3224/5892 及原身份，7777/7779 无监听。生产数据库的完整安全摘要与
00:56:04Z 基线逐值一致：正式 Batch 105 completed / 16 pending、两个旧 Canary、
receipt/progress 指纹及历史 Action 38 failed / 83 succeeded 均未改变。

### B3-B 剩余准入

补查发现 Servy 校验器实际在
`C:\ProgramData\coord\backup\g7-20261026T124419Z-0380fb72\servy-snapshot-check.exe`。
因此前文“两个已知路径不存在”不能扩展解释为主机没有校验器。本轮用该工具将
既有 Servy 备份只读恢复为**新的离线副本**，再对照原 inventory 校验：两步 exit 0，
quick_check/integrity_check 均 ok、schema 10、11 条完整服务记录一致，**离线快照
副本验证 PASS**。没有把备份写回在线 Servy.db。

完整 Servy/SCM 配置恢复仍 **NOT_RUN**。当前 CLI 10.2.0 的 import help 仅列出
config/path/install/quiet，没有提供隔离数据库选项；没有为试验向共享 Servy 数据库
导入或更改真实服务。离线记录一致不等于完整服务恢复验收。下一阶段仍需：

- 在可隔离的 Servy/SCM 环境证明完整配置恢复；确认实际已用重启次数和可用预算。
  既有导出的 MaxRestartAttempts=3 是配置上限，不能当作剩余预算。
- 固定 publisher 对齐方案并完成隔离验收。现有文件匹配旧 pin `cac5496e…`，
  新包 publisher 为 `a6679db9…`；本轮没有替换正式发布器或改 profile pin。
  优先评估保留旧 publisher 与原 pin，仅在相应兼容性证据成立后形成部署清单。
- 获得独立 B3-B 生产批准，再刷新一致备份、准确服务身份/ACL及生产签名密钥，
  完成真实服务 Runner、SYSTEM 两次监控和生产一致性验收。

到此停止 B3-A1；没有部署生产、恢复正式 Batch、创建 Canary 或进入 B4。

结构化清理证据见
[PHASE_R5_B3_A1_CLEANUP_20261027.json](PHASE_R5_B3_A1_CLEANUP_20261027.json)。
原始记录、四个 Task XML、一次性清理源码及离线 Servy 校验摘要保存在
`target/b3-a1-cleanup-20261027/`，远端对应证据仍在受保护夹具目录中。
本轮只提交证据文档；产品源码和正式脚本与 `ea9783268e33e546572fe346335056e28bc4d435`
一致。复用 `stage-r5-b3-a-checks.json`（fmt/clippy/tests/release 全部 exit 0，
88 passed / 8 intentionally ignored）及 Fork
[Actions 40377126693](https://github.com/lieyanbang/coord/actions/runs/40377126693)
（本轮只读回查 job 121763119437 及各步骤仍 success），不触发无代码变化的全量 CI。
TASK/PLAN、原有 `.gitignore` 修改、未跟踪文件和 Git 历史保持。
