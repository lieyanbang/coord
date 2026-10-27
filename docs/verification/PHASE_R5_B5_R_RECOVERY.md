# B5-R：证据补充与版本化恢复

2026-10-27；执行分支 `codex/b5-r-recovery-20261027`。
**进行中：R1 PASS（保留冲突），R2 本地 PASS / Fork CI 待验；R3/R4/B5-D NOT_RUN。**
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
Fork CI 和匹配产物待验；当前不宣称生产 Gate PASS。

## R3 / R4 / B5-D

尚未执行生产 ACL 修复、签名低权限恢复 Canary 或六会话任务分派。
正式 16 项 pending 和原 38 条失败 Action 不作无差别重放。
仅 R3/R4 的全部适用真实 Gate 通过后继续 B5-D。
