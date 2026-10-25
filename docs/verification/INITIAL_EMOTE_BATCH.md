# 正式初步分类 Batch 创建验收

2026-10-25。**创建及授权验证 PASS；分类尚未启动。** 最新用户指令授权
正式 121 项 Batch，取代 G7 的“只允许 Canary”历史范围；TASK.md 未修改。
没有领取、提交、分类 Agent、正式结果或完成动作执行。

| 项目 | 实际结果 |
| --- | --- |
| Batch UUID | `90ce2ca7-184e-4c3d-9d87-56386db25419` |
| 名称 | Emote image categorization v1 |
| 任务 | 121，`group-00`–`group-120`，唯一键无重复 |
| 覆盖 | 1,206 张唯一图片；12 项重复映射已验证，不重复入队 |
| 状态 | pending=121，claimed=0，submitted=0，全部 attempt_no/attempt_count=0 |
| 真实 MCP 状态 | 六个 worker 的 `active_claims` 均为 0 |
| 原始 API `active` | **121**：定义为非终态任务，包含 pending，不能写成 0 |
| 重试 / 预算 | 最多 3 次，单 attempt 3,600 秒，要求进度 |
| 单次租约 | 原生及 MCP 硬上限 600 秒；任务输入指定 requested_lease_seconds=600 |
| Profile | `emote-publisher-v1` v1，固定原生发布器、参数和 SHA-256 |
| 正式结果 / 动作事件 | 0 / 0，符合未开工要求 |
| 绑定 | 一个已批准 ChatGPT OAuth client，仅此 Batch，worker 0000–0005 |
| 开始 UTC | `2026-10-25T07:58:44.865Z` |
| 到期 UTC | `2026-10-25T19:58:44.865Z`，严格 43,200 秒 |
| Brisbane 到期 | `2026-10-26 05:58:44.865 +10:00` |

BatchConfig 没有默认租约字段；省略 lease_seconds 时核心仍默认 300 秒。
后续经用户另行授权领取时，显式传 `lease_seconds:600` 才取得请求的 600 秒；
续租同样显式传入，受新鲜进度序号和 attempt 预算限制。本轮没有实际租约。

## 输入、权限和发布配置

完整运行 `scripts/phase_g/prepare_initial_tasks.py --source-root D:/ChatGPT_Emote_Work`，
逐文件核对 1,218 个原文件、1,206 个任务副本和重复指向；再确认 1,206 个
不同图片哈希、跨组 ID 唯一。原 A0–A5、task/121、task/122 空目录保持原状。
打包拒绝重解析点、符号链接、路径逃逸和重复 ZIP 条目，未改原文件。

部署根：`C:\ProgramData\coord\g7\initial\business`。包含 1,206 张任务图片、
464 个 refs 文件、3 份清单索引，共 1,673 个输入文件。传输后及最终收尾均
逐文件核验 SHA-256。ZIP 为 58,549,722 bytes，SHA-256：
`815bdbe8380776f5397015090e818e6b8b541befe55a5c7c405472d3134d8adb`。
图片和 ZIP 不提交到 Git。

实际 Runner 服务 SID `S-1-5-21-3592388187-2353680085-815129118-1004`，
非管理员、不同于 daemon LocalService。实际服务令牌运行原生 ACL probe：
125 个目录代表文件可读、不可写；全部 1,673 个文件 ACL 逐项检查，只有
批准主体且 Runner/LocalService 无输入写权限。results、.receipts、
review_queue 可写；数据库、管理 key、签名 key 不可读；管理 RPC 对缺凭据
和 broker key 均 401。未扩权账户、SCM 权限或六个已有私人工作区 ACL。

专用 Profile 复制已验收配置，保留 Job Object、10s 超时、64KiB 输出上限、
幂等及最多两次动作尝试。两个事件均固定执行原生发布器，加固定参数
`--root C:\ProgramData\coord\g7\initial\business`。发布器 hash 为
`cac5496ed1dea50f837f28403feb62515de1ba1459ea2557abe6097ec07bec55`。
没有替换三个 EXE、Rust/Relay 实现、管理/broker key 或签名材料。

## 创建和授权验证

管理员先创建空 Batch 取得真正 UUID，再绑定，再添加任务，避免假造 UUID；
空 Batch 尚无可领取任务。时间在 Windows 服务器实际启用时以 UTC 现算。
Windows Relay、Linux Sidecar 和 CoordMCP 的新 binding 逐字段相同。
Linux 两组件仍加载 `/etc/coord-relay/identity.json`。原三个 binding、
legacy allowlist、旧 Canary 到期时间和 Access 策略均保留。

add_tasks 后先精确读回键/payload，再用相同内容再次添加，验证幂等。
只有一个正式 Batch。管理员读回和只读 SQLite 验证 121 个唯一键、1,206 个
不同输入 ID、零领取/尝试/动作。

真实已连接 Coord MCP 的六次 agent_status 全部成功，六个 worker 都看到
pending=121、active_claims=0。另用生产 MCP 工具处理器、实际注册且未过期的
OAuth 上下文、生产策略和签名检查领取授权：**claim 在发送前拦截，没有
任何 claim RPC 到生产队列**。Windows 实际模块独立验证六组签名/scope/batch。
未授权 worker、client、其他 batch、精确到期均拒绝。未声称真实领取生命周期
或六条 ChatGPT 分类对话验收。

最初验证器未找到未过期的 access-token 元数据，随即停止生产变更并只读
诊断。正常连接器只读调用自动刷新 OAuth 后，六次真实查询及验证器均通过。
没有编辑 OAuth store、制造 token 或放宽授权。

## 生产回归与本地门

- 在线备份中所有 22 个旧任务、3 个 Batch、19 条 receipt、6 条 progress、
  6 条 progress receipt、6 条续租 receipt、16 个动作逐字段保留。包含原
  8 项任务、operator Canary 10/10 和 browser Canary 4/4。
- 安装的三 EXE 保持 matching G7 CI 哈希。管理 RPC/broker 仅 loopback，
  当前 Runner 非管理员；SYSTEM monitor exit=0、认证 READY 新鲜。
- 283 项非所属服务身份和启动方式未变；七项受保护应用服务 state/PID
  未变。OS 按需 AppXSvc 自行从 Running 变为 Stopped，单独记录。
- Linux 备份中 13 个非策略配置/模块逐文件未变；三个相关服务 active、
  Sidecar READY。13011/13012 使用 Tailscale 上的原始 TCP，实际可达；六次
  两跳 RPC 成功。八项公开路由使用浏览器 User-Agent 时与原状态完全一致。
- `scripts/verify.ps1 -Stage initial-emote` 的 fmt、locked clippy/tests/release
  四门 exit 0；Python 8 项测试 PASS，19 个 Python 和 3 个新增 PS helper
  语法检查 PASS；diff/秘密字段检查通过。无实现修改，复用 G7 CI 产物，
  没有重建云端 artifact 或部署本机 build。

没有重启整机、OpenResty 或无关应用。只按需重启 coord、Runner、Relay、
Sidecar/Feathers。G7 逻辑标签可被同 owner 冒用、无后台唤醒、F3-R PARTIAL
及真实分类质量未验收的边界均保留。

证据：`stage-initial-emote-checks.json` 和四份日志、`initial-emote-*.json`。
逐文件 hash 见 `initial-emote-deployment-manifest.json`。探针错误及纠正理由
见 `initial-emote-verification-notes.json`，包括 Python 版本/API 和 import
调用问题、过期 OAuth、AppXSvc、错误的 HTTP/loopback 假设及 User-Agent 差异。
无安全或完整性检查被绕过；验证开始后没有继续生产变更。
本轮使用管理员运维通道，没有领取 shared ledger 或分类任务；独立 Git
分支和部署证据为执行记录，不伪造任务/ack ID。

## 备份与回滚记录

Windows：`C:\ProgramData\coord\g7\private\initial-20261025T075241Z-1651573f`。
包含 SQLite Online Backup API 一致 DB（含已提交 WAL）、quick_check=ok、
三 EXE、原策略/registry、三个 Servy export、完整服务/health 基线。
继承既有 admin/SYSTEM 私有目录 ACL，Runner 不可读。

Linux：`/var/backups/coord-initial-20261025T075307Z-18128120`，root-only。
tar 已读回验证，含原策略、相关模块、配置和 unit/drop-in。两处备份均在
该主机生产变更前完成。未手工修改或恢复活跃 OAuth store。

最终只读回滚条件检查通过：正式任务全部未领取、零动作、零发布结果。
没有执行故障回滚；临时 Runner 探针参数按计划恢复后装载正式 profile。
若撤销，先在三处**保留正式 Batch 的 binding，但使其 expires_at 过期**，
reload 并验证拒绝新请求。直接删除 binding 可能使 legacy 误当成未保护批次，
因此禁止直接恢复旧策略或关闭身份策略。再读回任务/动作/receipt；仅当没有
已领取/未决动作时恢复原 registry，按需重启所属服务。保留输入、Batch、
任务及备份作为审计记录；不覆盖 live DB、不删除接受结果、不恢复旧 OAuth
grants、不自动重放 uncertain。开始处理后必须先逐项核对，不能套用创建前
回滚步骤。12 小时到期后 fail closed，无自动续期。
