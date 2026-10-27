# R5 B5 — 历史 Completion Action 对账与恢复准入

2026-10-27；现场核验窗口 **12:45–12:55 UTC / 22:45–22:55 AEST**。
**B5 总体 BLOCKED，按 TASK 停止；服务 READY，正式业务 FROZEN。**

| 阶段 | 真实状态 | 结果 |
| --- | --- | --- |
| B5-A | PASS | 全部 38 条逐项对账、旧 publisher 复现、字段根因及当前 ACL 审计完成 |
| B5-B | PARTIAL / BLOCKED | 一致副本和故障复现完成；无符合不可变 result 与原 publisher 契约的恢复候选，部署 Gate 未通过 |
| B5-C | BLOCKED / NOT_RUN | 恢复 0；生产 Canary 尝试 0；没有修改任何历史 Action 或发布文件 |
| B5-D | NOT_RUN | B5-C 未 PASS；未解冻、未恢复 OAuth binding、未发送六 Agent 提示词 |

用户本轮已批准 A/B/C 及 C PASS 后自动进入 D；停止来自实际硬 Gate，
并非缺少重复阶段授权。没有降低 Gate、补写虚构复核内容或重新分类已完成任务。

## 任务与 Action 的独立事实

唯一正式 Batch：`90ce2ca7-184e-4c3d-9d87-56386db25419`。
管理 RPC `batches/status`、`scheduler/trusted_status`、`actions/status` 与可信
SQLite 只读查询一致；Action 查询返回 105 条，`truncated=false`。

- 任务：121 项，**105 completed / 16 pending / 0 claimed / 0 submitted / 0 failed**。
- 正式 Action：**67 succeeded / 38 failed**；pending/running/uncertain 均为 0。
- 旧 G7 两 Batch：10/4 completed，Action 11/5 succeeded；B4 Batch 仍为
  4 completed、0 Action。历史“83 succeeded”是所有 Batch 合计，正式 Batch 为 67。
- 正式队列 `failure_count=112`、`needs_attention=0` 是已有调度历史；
  不等于 112 个当前失败任务，也没有因此执行 `batches/retry_failed`。
- 38 条任务的原 result、event_id、attempt、当前提交 receipt/fence 在主机内逐项吻合；
  每条有 1 个 complete receipt，38 条均保留 completed。
- 67 个成功 Action 的结果 JSON 与数据库 result 全部一致，67 个发布 receipt 的
  task/batch/agent/attempt 与结果文件 SHA-256 全部匹配。
- 正式 results 共 134 个文件，恰为上述 67 对结果/receipt；38 个失败对象的
  76 个预期文件全部缺失，没有部分发布对或冲突文件。
  `index.json` 与 `review_queue/candidates.json` 尚不存在，Batch 尚未完成。

固定 manifest 的 **1,673** 个输入文件逐个存在且 SHA-256 匹配，无 reparse 文件。
未导出私人图片、result 原文、receipt fence、密钥或凭据派生值。

## B5-A 根因与 A–E 分类

38 条历史记录均为 `emote-publisher-v1` / version 1 / `on_task_completed`，
`attempt_count=2`、`max_attempts=2`、`exit_code=1`、`output_bytes=55`，
`last_error=nonzero_exit_retryable`。原始失败发生于 2026-10-25
08:40:07–09:23:24 UTC；各行创建/更新时间保存在脱敏证据 JSON。

使用现场原可执行文件，核验其原 pin：

```text
cac5496ed1dea50f837f28403feb62515de1ba1459ea2557abe6097ec07bec55
```

在线 SQLite backup API 取得一致副本后，仅在受限诊断根目录复制 refs 并建立独立
results 目录。逐项从副本确认 accepted receipt、原 task/result/event/fence 后构造
离线故障复现输入；生产 DB 只读，未将事件重新放入生产 outbox。
原 publisher 对全部 38 个失败输入均返回 **exit 1 / stderr 55 bytes**：

```text
Error: invalid character or incomplete review evidence
```

成功对照为 group 73，Action `b0dd254e-1aea-4d23-b9fb-09f4e0931736`，
event `2a2ff2d1-149a-40f5-aa14-6b40d4b2ce93`，
task `046df6fa-0389-45f5-b1e9-3407f5d6b9f4`；同一旧 publisher、同一隔离
refs 环境下 exit 0，并仅在隔离目录生成匹配的两个文件。
这证明旧 publisher 可运行；**没有把管理员离线执行当作签名 Broker 或低权限 Runner 验收**。

进一步对照原数据与 `src/core/emote.rs:195`：38 份结果中共 **196 个**角色
`confidence < 0.6`，而 `notes.trim()` 仅 **7–19 个 Unicode 字符**，低于
20 字符的既有复核证据门槛。该错误组的其他 character 条件逐项检查均通过。
Python 的现有结果验证工具也有同一最低长度要求
（`scripts/phase_g/validate_result.py:216`）；未为发布器放宽要求。

接受与发布校验不一致解释了“任务 completed、Action failed”：正式 Batch 的
内置 validator 配置只要求 `schema_version/group_id/agent_id/attempt_no/
source_image_ids/characters/uncertain` 顶层键，`equals=null`。
`src/core/validator.rs` 的 JsonValidator 不检查嵌套角色证据；后置原生
publisher 才执行上述语义检查。证据不足不能由自动填字、提高 confidence 或
重新领取 completed 任务来修复。未检查图片以重新判断身份，未声称分类本身错误。

| 分类（仅这 38 条） | 数量 | 判定 |
| --- | ---: | --- |
| A 已有完整匹配副作用 | 0 | 67 条正常成功产物另行核对，不混入失败分母 |
| B 确认缺失且原契约下可幂等补写 | 0 | 无通过原 publisher 的 failed 结果 |
| C 冲突或不确定 | 0 | 预期输出均缺失；没有 uncertain Action |
| D 仅修复环境或实现即可恢复 | 0 | 初审 D=38；字段与旧 binary 复现后全部转为 E |
| E 当前不可变结果/既有发布契约下不可恢复 | 38 | 每条至少一个低置信度复核 notes 过短；不代表永久不可处理 |

## 38 条逐项清单

下表每行共同具有：task=completed、Action=failed、profile/version/kind 和
旧 2/2 次失败信息如上、receipt/event/result/fence 核对通过、两个输出均缺失、
旧 binary 隔离复现 exit 1 / 55 bytes、分类 E。
“不足数”是该结果中低置信度 notes 少于 20 字符的角色数量。
全部精确字段及原始时间保存在
[PHASE_R5_B5_EVIDENCE_20261027.json](PHASE_R5_B5_EVIDENCE_20261027.json)。

| group | Action ID | event ID | task ID | 不足数 | notes 字符数 |
| --- | --- | --- | --- | ---: | --- |
| 01 | `5b698cfa-a7d3-4baf-83f9-f471d120fb22` | `89aaac35-f18d-44c6-ac0a-400c701d3d4b` | `6c4ef684-19ca-44d7-aab2-76b4eecd45c8` | 7 | 9–19 |
| 04 | `76b8830c-bcf3-4267-b003-50577e46ca99` | `2ed26098-9383-4ed2-b0ba-fa1c001638a0` | `5e18774a-1a0e-4649-876e-1fe5b93f8b5f` | 6 | 11–19 |
| 06 | `9e00bcc6-7509-4251-b94c-96b40e74316a` | `1553119b-1374-429d-b9a3-cb3d089fa6b8` | `52fe89e0-e822-46f5-aca5-0ce0c83deb07` | 9 | 10–19 |
| 07 | `711ec9be-8359-4184-ab72-8da45109cfec` | `b3a574de-7a20-433e-9862-e520ed79b52a` | `1336c492-ea2a-4737-a9b4-7793ef9c2820` | 1 | 19–19 |
| 11 | `9a927170-07b3-4320-b276-b05852632b66` | `8c83ce8e-7688-42db-8122-8441fdeb8b61` | `85276579-af48-4337-8803-63596775380a` | 1 | 17–17 |
| 13 | `0a4c992c-1ec8-4bac-a0cf-393ab746334f` | `d3dda879-88e3-47fd-95a3-855174d3a415` | `32d42337-2916-4658-b15f-996c88bfbdc2` | 6 | 7–14 |
| 14 | `acce7a56-a7a0-4d83-9019-e8130a4228fe` | `45a3c4e0-2297-4765-859d-3cb99d4e00f7` | `36000db1-f41c-45ae-93d4-6a1eb56e3b1f` | 1 | 10–10 |
| 16 | `f46c4dbf-c8c9-48f9-b54e-b427f24d81df` | `edea24f7-0411-4b9a-928c-e697d7d9f216` | `05be8952-5d51-4792-b1a5-e5e66480cf06` | 2 | 14–18 |
| 25 | `702c73a5-0df0-4dcf-a6fb-a432aa5860f5` | `88e31e37-5c7e-4ec7-a5f7-bd998548063e` | `1d14f34f-1af1-4963-a413-db28d81f78c9` | 5 | 8–17 |
| 27 | `8b760a1d-1213-4622-8acf-4ffa686b3adb` | `a8fa691b-d29a-43f6-bf63-6658c3c6e976` | `5d31c109-7a40-4dbc-903a-7e81fad572f0` | 8 | 18–18 |
| 33 | `33f38960-4789-4294-8ec2-25962b06cec2` | `89962320-849d-4a65-8ace-efd017a35066` | `bce27c0f-3c34-48eb-ae06-47b7a0ccd7d8` | 4 | 12–18 |
| 35 | `0fef3a2d-4c1d-4f83-b16b-6d85b54e9e78` | `3df277b6-6752-40ff-bcfa-579d9fa4c3a1` | `3d1c12a9-0481-4fd5-9a5d-7def27f84220` | 6 | 10–18 |
| 37 | `5d867770-388f-49ec-bc98-f35ae5421991` | `a04c1568-1fc6-4bfc-9fcf-29c2398f447d` | `1839062f-926a-4142-8c54-caa12e2af4bf` | 3 | 12–17 |
| 38 | `99faee41-e564-4fec-a4e8-bc053f115e64` | `44f8ce09-a0c3-4441-8454-2b6133db02cc` | `66d39658-fec9-4ba2-bbba-c67fb07f4bcc` | 5 | 11–18 |
| 41 | `e23bae44-c278-4112-a454-503fe4b639f5` | `ed980998-de68-4c7e-887e-f558696dff56` | `15aa65f4-acea-479b-8c6c-ac613cab9aa3` | 3 | 14–16 |
| 44 | `b49323ef-f29b-453e-8712-8c5802dd7420` | `ae362214-82cc-4f31-b7dd-41fc7d170bcc` | `b981e0f2-3820-4cc0-8f63-8935011b7ee4` | 5 | 10–17 |
| 46 | `c00c0e9f-48bd-417a-b338-883a5e7f363a` | `b717699c-1128-4352-9490-17a2d22f3100` | `83ef7376-3ded-4bce-853c-ba4e7a3ff60a` | 6 | 10–17 |
| 50 | `d7618ad3-6b9e-406b-8bab-93eb778aab3e` | `7e9ee2fb-a9d1-4853-a9bb-f4ae6690e9bb` | `930ad818-e57a-48e6-b783-b8b144b1172a` | 1 | 15–15 |
| 51 | `105f8155-e653-4eba-bd3e-8afa274f709e` | `f92c3d8f-0d0d-4ab2-aec0-3810182da41d` | `b6644ae2-e25a-4cb5-997f-a00a67da1443` | 10 | 8–18 |
| 54 | `02e1c807-e693-492a-85fc-f0ede325a2a0` | `20640e90-d0a5-42b9-8cf7-ef1eda10d93f` | `1cf664c0-3b25-4791-a55d-b73ead64a3ec` | 1 | 14–14 |
| 56 | `7322c021-5650-4d71-bd95-9f185476ff23` | `bad3954a-7069-488d-a5e1-501dff21a794` | `4fb48d5c-7b37-4d96-bc6c-13b16f22b857` | 6 | 10–19 |
| 60 | `12123244-e8bd-461e-ac01-e6984ed536cf` | `b8a5735f-753b-4d07-b1cf-9d84391a2392` | `a99b1b8a-6a47-4bc4-8186-29c2ce24d06b` | 5 | 19–19 |
| 63 | `16713f4e-0627-48ef-844b-2fc8dcd54d8c` | `40287c56-98cf-4a61-9225-cf3ea1eafd6b` | `911c7d81-3689-40fe-a17c-d4e44ead19f5` | 7 | 8–15 |
| 65 | `f5ca49f4-af63-43a5-b6a3-02bcf12edc18` | `ddf66571-10e6-4c12-9ebb-3c32522b0fa6` | `57771d84-8716-47bb-b0d5-0e6304db6515` | 6 | 10–19 |
| 67 | `f17ed907-9654-4eb8-9013-8be73604abe9` | `2b476ffe-4539-4e3c-891c-27ba6f119c4a` | `4ec36f4b-c831-4f69-b8e4-12a81fe94285` | 7 | 16–16 |
| 68 | `4f33ec3d-5da8-4f9e-9455-7f82c278a84c` | `e6dc885e-d742-47d7-b54e-0e96426f73b8` | `17e6edd2-fce7-41e1-8a07-2a207e4ca772` | 3 | 13–19 |
| 69 | `af31c120-d924-4504-ba83-a95be6b470ad` | `d890078d-b86a-4bfe-b839-2176bd517659` | `22f86218-21ad-4545-a4c8-099f96da8583` | 4 | 14–16 |
| 72 | `dac9bfe0-8ca6-4920-99b8-2fa88c29725e` | `a5d41cd7-9853-457e-aaf2-65ed0773172d` | `20aca05b-fb4c-4185-9420-c49f7fafcc86` | 4 | 11–19 |
| 75 | `d5e2b1bb-c8d1-420b-a1ad-c6b338dd321a` | `3fbca759-8ec6-4093-a405-09b845149bb5` | `3668199d-3128-4682-b586-ed96f1aa07ed` | 7 | 11–15 |
| 76 | `3f24e54b-6ec1-46cf-b290-b83db59e4e1c` | `bcf70507-a687-4b2f-868d-b1e62f171361` | `881eb2c0-fafd-4d8d-be30-519685b1cc10` | 5 | 13–18 |
| 87 | `7e21b688-6a92-4812-ab47-e5cdc8076e1e` | `33fa6075-53eb-44a6-95fe-e7f26220b128` | `37f759a6-f6d6-4335-8224-885ab2c039e3` | 5 | 11–16 |
| 88 | `34b0cf3a-4154-43a4-9648-2352cfd986b2` | `4bc02173-8768-4fd0-a566-b8b3f4779129` | `af03c398-f985-4a49-b18a-5c4d7284a8e2` | 9 | 8–19 |
| 92 | `c38fa5c8-921f-41fc-8a96-a5424e6c225d` | `499b08e7-a2c4-4867-80ae-88f66d8cecba` | `8e89911c-f0df-4ba4-b0ff-ab91f1004244` | 9 | 7–19 |
| 95 | `63b98222-94fe-490c-9028-c4ba523d758b` | `fc466734-b47c-4be8-9dd5-03c7df343122` | `9616343d-56a3-45d7-ae32-fca793d538b6` | 6 | 9–16 |
| 106 | `9d5c0351-4e5c-462a-a524-6173bd4e7c4b` | `b8faef3b-b706-4454-b75d-b185a510d1da` | `bbc4425d-f3e9-41ba-a745-830331838203` | 6 | 17–17 |
| 107 | `6f528042-d483-44b3-b838-45932112f0de` | `7c83f281-7c47-4688-859f-2eaeea233936` | `793e282f-b79d-4444-9582-8b8451cfe3da` | 7 | 11–19 |
| 113 | `e3d5c794-89ea-46f7-add8-0298d61e3ab9` | `dca7d8fc-dd7a-41db-a2b5-1f3bdb5c6db5` | `2c577c6f-963f-4b07-8bb6-e10c610b782f` | 2 | 12–14 |
| 114 | `d4b4fbb8-63b4-440c-88f8-e34a35865b8d` | `ca9a3729-8985-4aab-8c34-a8c32398cee8` | `af3e6165-6995-40fc-a2be-81b06fe15e3c` | 8 | 16–16 |

## B5-B / C 的停止依据

一致备份 integrity_check=ok；38 条根因复现与 1 条成功对照均完成。
但恢复必须同时保持原 result/receipt/fence 和原三个 publisher profile/pin；
目前没有一条 failed Action 可以在这些条件下通过原发布器。
因此没有最低风险 Canary，B5-B 部署准入未通过，B5-C **BLOCKED / NOT_RUN**。

另有独立的当前生产 ACL 阻塞：1,673 个输入文件的 Allow ACE 均仅见
Administrators、SYSTEM 和原本机用户 SID 尾号 1001；业务根及 results/receipts/
review_queue 也未给当前 Runner SID 尾号 1004 相应权限。
SCM/原生进程确认 Runner 为 `coord_action_g7` / SID 尾号 1004，组成员仅 Users；
该 SID/组不在这些 Allow ACE 中。这里只读核验 ACL 和组成员，未启动该身份的
生产读写探针，亦未扩大权限。**组件 health READY 并不证明正式业务输出可写。**
元数据不能将历史 55-byte 语义失败归因为 ACL；两者分开记录。

| B5-B/C Gate | 状态 |
| --- | --- |
| 一致 DB 副本、原 pin、逐项 receipt/result/fence、缺失输出核验 | PASS |
| 原 publisher 逐条复现与成功对照 | PASS（离线管理员诊断范围） |
| 可恢复 failed Action 候选 / 最低风险 Canary | BLOCKED：0 个 |
| 独立低权限身份的签名/ACL 恢复闭环 | NOT_RUN；当前 ACL 元数据存在阻塞 |
| 缺失/匹配/冲突/uncertain/签名失败的完整恢复接口正反例 | NOT_RUN：没有安全恢复候选或接口变更 |
| 精确 action allowlist、限时触发器例外、原子撤销 | NOT_RUN：无生产恢复必要，不实施 DDL |
| 生产 Canary、分组恢复、专用恢复 receipt | NOT_RUN；0 次、0 条 |

没有使用直接 SQL reset、重新 claim/complete、更改 max_attempts、替换原
publisher、删除文件、覆盖外部输出或抹去失败历史。38 条仍 failed，真正恢复 0。
诊断隔离目录保留受限证据，不构成生产恢复产物。

继续处理需要先确定一个**保留原 receipt/result 且允许补充真实复核证据或明确版本化
发布契约的方案**，并明确相应变更权限；现有合同不允许以凑足字符或跳过校验替代。
此外还须解决正式业务 Runner 的精确 ACL 准入。这里没有自动进入这些新决策工作。

## B5-D 六 Agent 执行成果

**NOT_RUN，新增完成 0，六会话提示词发送 0，临时 worker binding 新增 0。**
B5-C 未 PASS，冻结切换方案未实施/验收，因此没有打开并驱动原有六个 Agent、
恢复其正式领取权限或执行剩余 16 项。不存在等待用户浏览器登录的当前阻塞。
这 16 项仍 pending；105 项旧 completed 未重做，38 条旧 Action 未重放。

## 最终独立核验与保全

- **12:55:32 UTC** 的收尾 SQLite 事务：tasks、agents、batches、task_receipts、
  task_progress、task_progress_receipts、task_progress_extend_receipts、
  completion_actions、task_attempts 共九表，逐列双向 EXCEPT 对一致备份比较，
  removed/changed/added 全为 0。包含所有 Batch 的原 result、receipt 和 fence，
  比较仅输出计数。活动 Runner 的 nonce 表正常轮换，不要求其静止，未修改或导出 nonce。
- integrity_check=ok；24 个冻结触发器始终存在且未修改；完整 SQL hash 仍为
  `678a8a1227dda09cbf98e43b90f348e8b242b2ce66e94cdfec1c23e8ee48dd3a`。
- 正式 134 个输出文件起止路径/大小/内容 hash 清单完全相同；成功发布对 67/67 匹配，
  没有新增输出、重复发布或覆盖。三个 profile 的 version、task/batch pin 与 binary 一致。
- Core/DB/Broker/Runner 收尾全部 READY；SYSTEM LastTaskResult=0、READY、连续失败 0；
  Coord/Runner/Servy/RelayAgent 均 Running，Core 与 Broker 只监听批准的 loopback
  7777/7779。Core 原生 SID 为 LocalService，Runner 原生 SID 尾号 1004。
- Linux Relay/Feathers/OpenResty active/running，NRestarts=0；12:52:15 UTC
  Sidecar HTTP 200、READY、agentConnected=true、pending=0。
- Windows、Linux 三跳既有五个 binding 均已过期；没有新增或延长 OAuth/MCP 授权。
  旧 broker.key 只比较 ACL，与 B3 基线精确相同；未读取、复制或哈希该密钥内容。
- 没有生产服务重启、账户/身份变更、新服务/计划任务/常驻诊断进程或临时监听；
  本轮运行的 Python/PowerShell 诊断子进程均已结束。
- Git 从 `f97391d` 新建 `codex/b5-action-recovery-20261027`；原有用户文件、
  未跟踪状态、TASK/PLAN/.gitignore 保留，三文件起止字节 hash 一致。
  正式 daemon 整体冻结，不写开发 heartbeat/task/ack 来突破保护；使用本阶段提交作 ledger，
  没有编造任务/ack ID。urgent/high pending 只读扫描均为空，未发现活动任务重叠。

## 验证、Git 与证据位置

按 AGENTS 的提交门槛执行一次：

```text
pwsh.exe -NoProfile -File scripts/verify.ps1 -Stage b5-action-audit
cargo fmt --all -- --check                            exit 0
cargo clippy --all-targets --all-features --locked -- -D warnings  exit 0
cargo test --all-features --locked                    exit 0 (88 passed / 8 intentionally ignored)
cargo build --release --locked                       exit 0
```

命令、退出码及日志见 `stage-b5-action-audit-{checks.json,fmt.log,clippy.log,test.log,build.log}`。
本轮只有审计文档/脱敏证据，无产品源码或部署脚本改动，因此没有触发新的 GitHub Actions；
上述本地回归不冒充生产恢复验收或容量证据。仅推送已核验用户 Fork
`lieyanbang/coord` 的本阶段非发布分支；不推 upstream，不改历史/tag/release。

原始 DB 副本、含业务内容的离线 event、publisher stderr 和隔离 outputs 仅留在
Arasaka `C:/coord-b5-action-recovery-20261027-a`（Administrators/SYSTEM 专属 ACL），
未下载/提交。脱敏本机材料在忽略目录 `target/b5-action-recovery-20261027/`；
本报告关联 JSON 仅含 ID、状态、错误、计数、长度及非秘密验证元数据。
首次只读探针误用了不存在的 task_attempts.outcome 列，exit 1；修正为当前 schema 后
探针 exit 0，原 stderr 保留。未因该诊断失败重试任何生产动作。

到此停止 B5，保留硬阻塞及全部历史失败；B5-C 与 B5-D 没有成功执行成果。
