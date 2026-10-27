# B5-R：证据补充与版本化恢复

2026-10-28（现场 UTC 仍为10-27）；执行分支 `codex/b5-r-recovery-20261027`。
**R1/R2/R3 PASS；R4安全Gate PASS、历史恢复PARTIAL（31恢复/7保留）；B5-D准备中。**
本报告不改变 [旧 B5 BLOCKED 历史](PHASE_R5_B5_ACTION_RECOVERY.md)。
TASK.md、docs/PLAN.md、用户原工作区成果保持只读/原样。

## R1：真实视觉补证

复用旧 38 Action / 196 条短备注映射。三个受限 reviewer 实际查看原图和引用
参考图，独立记录绑定原 batch/task/action/event/result/character hash 的补证。
主 Agent 重新核验完整覆盖、原结果 hash、实际文件 hash、落盘编码及审核状态。

| 条目 | 数量 |
| --- | ---: |
| APPROVED | 183 |
| CONFLICT | 12 |
| NEEDS_REVIEW | 1 |
| 完整批准组 | 31 |
| 保留阻塞组 | 7 |

阻塞组为 **01、27、37、46、65、76、92**。前六组存在原识别或描述/图片映射
冲突，92 的闭眼原图不能核实原结果声称的眼色。原结果不改写，整组不发布。
局部批准的角色不使冲突组通过。

review-1 初版 14 份文件受到 PS5 ASCII 管道损坏，中文变为问号；保留原文件，
明确作废。其 62 条实际复核备注以 ASCII 转义 JSON 写入独立 `supplements-v2`，
逐条精确读回（3277 汉字、0 问号）；另外两片各有 3810 / 4175 汉字，0 问号/
替代字符，语义读回通过。没有用长度通过掩盖编码损坏。

原图、原 accepted result、完整补证、原始事件和备份仅保存在 Arasaka 受限目录
`C:\coord-b5r-recovery-20261027-a`。本地只保留脱敏 ID、状态、hash 及计数。
`publication-supplements` 仅收录 31 份完整批准文件的逐字节副本；复制不会创建
正式 Action，也不授予 pending 任务权限。

13:18Z 生产基线及补证核验时仍为 105 completed / 16 pending，67 succeeded /
38 failed；旧 failed 均保留 2 次尝试，76 份旧缺失输出仍不存在。未解冻业务。

## R2：独立 publisher v2 与提交前校验

新增 `coord-emote-action-v2`，保留生产 v1 和三个既有 profile/pin。
v2 发布原 accepted result；有效补证只参与内存语义校验，另写 `.reviews` 和
version 2 receipt，记录原事件、新发布事件、结果和补证 hash。不改 confidence。
无补证、未批准、绑定不符、图像/refs 变化、短备注及编码占位均拒绝。
整套输出全相同才可幂等确认；冲突或部分落盘停止，对账后再处理。
批量索引可验证旧 v1 receipt 和 v2 独立证据。

profile 的 `submission_validation: "emote-v2"` 使未来提交在原子接受前执行相同
嵌套语义规则；失败不改变 claim、fence、failure budget 或 receipt，可在当前
有效 claim 下纠正。Core 不获得业务图像访问权；文件存在/containment 仍由
publisher 在真实 Runner 身份下检查。旧 profile 不启用该前置规则。

管理端新增 `actions/recover`，只允许 registry 内指定 batch/source profile/
Action ID/原结果 hash/补证 hash，且限时有效。独立 outbox/event 与
`action_recoveries` 唯一关联旧 Action，原 failed/attempt/error 不变；恢复执行
最多一次，超时保持 uncertain，不自动重试。Broker 在真实 accepted receipt
和当前结果之上再次核验恢复权限。对同一原 Action 的不确定恢复不创建新事件。

本地定向测试覆盖独立恢复与历史保留、绑定/状态/长度/源文件负例、可纠正提交、
过期授权、混合版本索引与 provenance 冲突。
`pwsh -NoProfile -File scripts/verify.ps1 -Stage b5-r2` 全部 exit 0：fmt、locked
clippy、93 passed / 8 intentionally ignored、locked release build。
命令/退出码/日志见 `stage-b5-r2-checks.json` 及同名前缀日志。
首轮 [Fork CI 40514410347](https://github.com/lieyanbang/coord/actions/runs/40514410347)
在 `5a1373464c7dabf6f64e7d58a41c70e4c9590c7d` 的 fmt/clippy/93 tests/release/
health monitor、真实低权限发布/索引/崩溃恢复均通过，但最终夹具清理失败：晚发现的
Core 子进程 Stop-Process 后立即查询，尚未退出。artifact `12740005582` 的 summary
保留该 FAIL，未把它当作整轮通过。为此分支补上同样的有界退出等待；完整本地
`-Stage b5-r2-ci-cleanup` 四项再次 exit 0。最终 [Fork CI 40516227702](https://github.com/lieyanbang/coord/actions/runs/40516227702) 在 c303c3f897ffa54aecce62bb1130e1d9ab9fae1e 全部success；artifact 12738804725的manifest、四个binary hash在本地和Arasaka均匹配。

## R3：真实 Runner 令牌和最小 ACL

隔离正例/反例/撤销均通过。管理调用方不能打开服务进程令牌，保留两次拒绝；
一次性 SYSTEM 任务取得**现有 Runner PID 4616** 的令牌，以线程 impersonation
执行全部 I/O。核验 SID 尾号 1004、没有 Administrators；没有密码或私钥读取。
输入读取、输出创建和清理通过；输入写入、受限文件打开被拒绝；撤销后再次拒绝，
原 SDDL 精确恢复。该证据证明 OS ACL，R4 仍另需真实签名 Broker/Runner 发布。

生产先备份全部精确目标 ACL。首次方案被旧输出写保护探针拒绝：134 份旧发布
文件 owner 本来就是当前 Runner，新增 CREATOR OWNER 继承会恢复其数据写权限。
完整撤回后，原 ACL mismatch=0，输出 content changes=0；未强行继续。
修正版对旧发布文件增加 Runner Write/Delete deny，保留原 ACE、owner、继承边界；
输入/refs 1670 个必要文件只加 Read/Execute，输出目录允许创建，CREATOR OWNER
只用于其新建文件的写入和清理；31 份合格 supplement 只读。

13:53Z 前生产实际令牌验证：results、.receipts、.reviews、review_queue 均可创建/
清理自己的临时标记；输入/refs 可读，输入和旧输出不能写；Broker/管理凭据与 DB
不能打开。敏感 ACL 不变，134 份旧输出字节不变，全部原 ACE/owner/继承边界保留。
一次性任务已撤销。没有更改数据库、registry、密钥权限或六 worker 绑定。

## R4：真实签名恢复与收尾

隔离一致副本通过：真实LocalService Core + 现有Runner原token（SID尾号1004、非管理员）+ 新隔离签名密钥，Broker/原生v2发布07一次succeeded。独立event/receipt/review绑定原accepted result；重复恢复请求返回同一事件。排除组46、原task/failed Action/receipt/其他batch负例均拒绝。原8表全部原列unchanged，134份旧文件逐字节不变。恢复24个定义、撤销父目录临时ACL、封存隔离keys，进程/监听0。

启动失败完整保留：首次无日志退出，未取得原生退出码；采用已验证专属非交互桌面后Core启动。第二次真实Runner identity成功，但PS5返回空ExitCode，且cleanup遇到已退出包装器的空CommandLine；独立清理后改用PS7并检查空值，随后完整闭环PASS。没有将操作脚本错误归因于业务源码或抹掉失败证据。

同版Core + Broker禁用/原profile的forward recovery已在产生新恢复事件的副本实测：认证Scheduler可读、未认证401、DEGRADED，原accepted数据和新输出保留。生产上线前另做SQLite在线一致备份；没有用旧DB覆盖当前库。

生产07 Canary：Action 3e6377a7-a6d0-453b-8251-101696431971，新event a0828737-4d33-4d16-b2bf-90fbce99c8fb，真实服务Runner一次成功。其后30组每批5条恢复并独立读回。首次扩大allowlist被脚本状态检查拒绝（护栏已恢复，authority记录仍为旧07定义），未创建额外Action。改为仅接受精确原24定义或精确本轮已知定义，先在副本验证31项切换/负例/恢复，再继续；未允许未知SQL。

最终原38条failed/2次尝试全部保留，新增31条独立恢复Action和31个event，全部succeeded/attempt1/exit0；正式Action总计98 succeeded/38 failed。任务仍105 completed/16 pending，完成receipt未增加。31组原result与published JSON相同，补证独立保存；旧67个result和134份输出字节不变。7组01/27/37/46/65/76/92没有创建恢复事件。

原24个触发器精确恢复，hash 678a8a1227dda09cbf98e43b90f348e8b242b2ce66e94cdfec1c23e8ee48dd3a，integrity_check=ok。v2 profile保留前置校验，临时recovery policy删除；撤权后恢复请求拒绝。三个原profile与v1 binary/pin逐项未变。服务恢复策略及monitor调度恢复；Core PID5660/LocalService、Runner PID3680/SID1004。14:16:51、14:16:54 UTC两次真实SYSTEM monitor均exit0/READY/failed_checks=[]。

38条精确映射、新旧Action/event、各批ID、hash、护栏和SYSTEM证据见 [生产脱敏证据](PHASE_R5_B5_R_PRODUCTION_20261028.json)。原图、业务原文、完整补证、数据库、配置备份和密钥仅留受限远端。

## B5-D：准入检查与身份兼容修复

通过正常浏览器UI核对原项目六会话worker0000–0005；尚未派发正式任务、开放worker绑定。原会话较早canary范围由本轮正式授权取代；不跨worker、不手写results、实际看图/refs和不确定提交先对账等边界保留。

新鲜副本只圈定16 pending，准备将batch切换v2/profile version2/单次非自动重试Action策略；validator和其他config保留。SQL正反例及恢复通过，但简化claim被拒绝：trusted工具传四位worker，Core直接保存owner，而正式schema/publisher要求emote前缀。这还会造成提交身份不匹配。生产保持冻结，没有让真实会话承担缺陷。

修复仅对启用emote-v2前置校验的批次把四位标签规范化为emote-XXXX，覆盖claim/progress/release/resume/complete；generic及历史B4文字批次保留原标签。针对测试已通过：简化领取/进度/释放/重分配、错误worker拒绝、可纠正前置拒绝、精确完成重放、原生v2发布和generic兼容。完整本地 `scripts/verify.ps1 -Stage b5-d-identity` 四项exit0（94 passed / 8 intentionally ignored）；Fork CI及副本重验待完成，通过前不正式派发。
