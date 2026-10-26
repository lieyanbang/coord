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
