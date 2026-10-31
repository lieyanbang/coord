# Coord 自定义任务与管理指南

适用实现：长期入口授权版本（logical 策略 `version: 3`，兼容旧 version 1/2）。本指南依据实际源码；旧临时验收策略不是日常任务管理流程。

对应源码分支：`codex/trusted-entry-20261031`。检查源码或重新部署前核对分支，避免用旧工作树覆盖已部署的授权修正。

## 1. 先区分管理者与 Worker

| 角色 | 接口 | 能做什么 |
| --- | --- | --- |
| 可信管理者 | Core 的 loopback HTTP JSON-RPC，POST `/` | 创建 Batch、添加任务、查看进度、重试失败、取消 |
| 远程 Worker | `Coord Worker` MCP 连接器 | 在授权 Batch 内列出、领取、保存进度、完成、失败、释放、恢复 |
| 部署管理员 | 受保护的身份策略与服务配置 | 为真实 OAuth 客户端授权 Batch、Worker 和 scopes |

**远程 Worker 工具不包含创建 Batch／添加任务。** 只有连接器时，不要猜测 `coord_create_task` 等不存在的工具；把任务定义交给具备管理端权限的操作者。

本地受信环境可以直接使用 Core 或 stdio MCP，不强制使用 OAuth／Relay。原始 RPC 没有租户身份认证；必须限制在 loopback／受信访问边界，不能将其当作公网管理 API。

## 2. 当前部署边界

Arasaka Core 为 `http://127.0.0.1:7777/`，公网 MCP 为 `https://lieyanbang.com/app/CoordMCP/`。任务池的全局冻结已解除，普通创建流程不涉及冻结／解冻。正式 Emote 发布 Action 仍有独立保护，不得借普通任务授权触发发布。

当前 ChatGPT Web 客户端通过长期入口白名单访问共享任务池，无需为新 Batch 或新 Worker 单独续绑。实际部署前后仍应验证已加载的策略和真实连接器；OAuth 失效时走正常认证续期，不改变任务池授权。

## 3. 最快上手：本地创建一个通用 Batch

使用包含上述实现的 `coord.exe`。以下示例**只针对新建的本地练习库**，不连接生产数据库。

### 启动独立 Core

在 PowerShell 窗口 A 中，设置实际二进制绝对路径，再启动：

```powershell
$CoordBin = 'D:\project\coord-generic-workers-20261031\target\ci-artifact\coord.exe'
$Lab = Join-Path $env:TEMP ('coord-guide-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Lab | Out-Null
& $CoordBin serve --addr 127.0.0.1:17888 --db (Join-Path $Lab 'tasks.db')
```

先确认端口空闲；被占用时换一个端口，不要停止其他人的服务。二进制路径是本机示例，其他机器应替换为已验证构建。练习完成后在窗口 A 按 Ctrl+C 停止自己的 Core；保留数据库可继续练习。

### 创建 Batch 并添加任务

在窗口 B 执行。将任务拆成可独立完成、可验证的工作单元；`payload` 放输入、目标、限制和验收要求，不放凭据。

```powershell
$CoordUrl = 'http://127.0.0.1:17888/'
function Invoke-CoordRpc {
    param([string]$Method, [hashtable]$Params)
    $body = @{
        jsonrpc = '2.0'
        id = [guid]::NewGuid().ToString()
        method = $Method
        params = $Params
    } | ConvertTo-Json -Depth 30 -Compress
    $reply = Invoke-RestMethod -Uri $CoordUrl -Method Post `
        -ContentType 'application/json; charset=utf-8' `
        -Body ([Text.Encoding]::UTF8.GetBytes($body))
    if ($null -ne $reply.error) {
        throw ($reply.error | ConvertTo-Json -Depth 10 -Compress)
    }
    return $reply.result
}

$batch = Invoke-CoordRpc 'batches/create' @{
    name = 'custom-doc-review'
    description = '独立检查两份说明文档，不修改源码，不触发发布'
    config = @{
        max_attempts = 3
        require_progress = $true
        agent_lease_budget_seconds = 3600
    }
}
$BatchId = $batch.id
$items = Invoke-CoordRpc 'batches/add_tasks' @{
    batch_id = $BatchId
    tasks = @(
        @{
            key = 'review-install-v1'
            name = '检查安装说明'
            payload = @{
                input = '由管理者提供所有执行端可访问的安装说明路径或链接'
                instruction = '核对命令与前置依赖；只读，不执行生产部署'
                acceptance = '返回问题清单、证据位置和未验证项目'
            }
        }
        @{
            key = 'review-api-v1'
            name = '检查 API 示例'
            payload = @{
                input = '由管理者提供所有执行端可访问的 API 文档路径或链接'
                instruction = '核对参数和示例；只在隔离环境验证'
                acceptance = '返回核对结论、执行命令及实际结果'
            }
        }
    )
}
$items.tasks | Select-Object id, batch_id, task_key, name, state
Invoke-CoordRpc 'batches/status' @{ batch_id = $BatchId }
Invoke-CoordRpc 'tasks/list' @{ batch_id = $BatchId; limit = 100 }
```

预期为两个属于新 Batch 的 `pending` 任务；真实 ID 以返回值为准。示例材料只是占位说明，正式分发前必须换成实际可访问的输入，Coord 不会替 Worker 同步文件。

`batches/add_tasks` 每次最多 1000 项；`key` 为 Batch 内唯一的 1–256 UTF-8 字节标识。同一 key、名称和 payload 原样重试返回原任务；更改内容会冲突并回滚该次添加。`batches/create` **没有名称去重或通用请求去重**：创建响应不确定时先对账，不要换请求 ID 盲目再建，也不要把名称当作唯一 ID。管理者应保存真实 Batch ID 和任务清单。

普通自定义 Batch 不配置 `validator`／`action_profile`，默认不进行 Emote 校验，也不执行发布 Action。添加验证器后，`submitted` 不等于最终接受；必须等待实际判定。`ack`、`knowledge`、`decision` 有创建即完成的公告语义，不能拿来替代待领取的普通工作。

## 4. 远程入口授权：初次接入配置

授权的单位是**已认证的 MCP 客户端入口**，不是任意互联网用户，也不是逐个 Worker。管理员初次接入时配置一次，之后新 Batch 自动可访问；业务授权长期有效，直到显式撤销或调整范围。不为 Worker 注册密钥，不设置 24 小时业务到期时间。

现有策略路径为 Linux `/etc/coord-relay/identity.json`、Windows `C:\\ProgramData\\coord\\g7\\relay-private\\identity.json`；管理员还应核实 facade 实际加载路径。新策略的完整结构如下，路径与客户端哈希须替换为实际值：

```json
{
  "version": 3,
  "mode": "logical",
  "enabled": true,
  "signing_key_file": "<本机已有签名密钥绝对路径>",
  "legacy_client_sha256": [],
  "bindings": [{
    "id": "chatgpt-web",
    "client_sha256": "<已验证 OAuth clientId 的 SHA-256，小写 64 位十六进制>",
    "batch_id": "*",
    "worker_ids": ["*"],
    "scopes": ["coord:read", "coord:claim", "coord:progress", "coord:submit", "coord:failure", "coord:release"]
  }]
}
```

- `batch_id:"*"` 授予整个共享任务池（含今后新建 Batch）；也可为其他入口显式限制单个 Batch。每次 Worker 调用仍指定真实 Batch UUID，列表不会退化为全局列表。
- `worker_ids:["*"]` 接受任意符合格式的稳定标签；同客户端不配置重叠、冲突的授权。scopes 限制可用操作，不授予 Batch 管理或发布权限。
- version 3 不接受 `not_before`／`expires_at` 或单个 Worker 白名单。旧 version 1/2 的短期策略保留兼容，不是正常共享入口的推荐配置。
- 从已验证 OAuth 客户端登记或上下文确定真实 clientId，不能使用 Worker 自报的身份、access token 或 token 哈希替代。OAuth token 到期刷新仍然必要，但不要求重新配置业务绑定。
- facade、Sidecar 和 Windows Agent 校验兼容策略与签名；保留已有密钥、ACL、有效兼容消费者，不把秘密写入 Git 或任务。
- 首次部署或主动修改／撤销授权时受控重载实际加载策略的服务；**正常新建 Batch 和新增 Worker 不改配置、不重载服务**。
- 一次 RPC 的短期签名防止传输重放；真正的临时执行归属由任务租约、进度预算、attempt fencing 决定，两者都不是入口业务授权续期。
- 初次接入后用真实连接器调用 `coord_status`／受限列表验证；正常任务创建只需管理端创建、回读，再交给 Worker 执行。

本地练习数据库不会自动出现在生产连接器里。若需远程测试，应明确配置隔离路由，不偷换生产 Agent 或将裸 RPC 暴露公网。

## 5. Worker 工具与操作流程

每个并行执行者选择稳定且不同的标签，例如 `writer-1`、`writer-2`。允许 `^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$`，普通标签区分大小写；四位十六进制标签及 `emote-XXXX` 历史别名按兼容规则规范化。不要用大小写变体绕过归属，也不要共享一个标签并行干活。

以下是**连接器工具参数**，不是 PowerShell 命令。工具可能有宿主前缀，以实际发现的 schema 为准。`BATCH_UUID`、`TASK_UUID` 和 `attempt_no` 必须取自真实返回值：

| 工具 | 示例参数／用途 |
| --- | --- |
| `coord_status` | `{"batch_id":"BATCH_UUID","worker_id":"writer-1"}` 查看权威批次进度 |
| `coord_tasks_list` | 上述两项加 `"state":"pending","limit":100`，只返回授权 Batch |
| `coord_resume` | 上述两项，读取最近 attempt／检查点，不领取或转移任务 |
| `coord_claim` | 上述两项加 `"lease_seconds":300`，原子领取；同 Worker 的有效领取可恢复 |
| `coord_progress` | `{"batch_id":"BATCH_UUID","worker_id":"writer-1","task_id":"TASK_UUID","attempt_no":1,"checkpoint":"已完成输入核对，下一步检查示例","lease_seconds":300}` |
| `coord_complete` | 同一 Batch、Worker、task_id、attempt_no，加 `"result":{"summary":"实际结论","evidence":["实际证据位置"]}` |
| `coord_fail` | 同一执行归属，加 `"error":"真实业务失败原因"`，消耗失败预算 |
| `coord_release` | 同一执行归属，可加 `"reason":"当前会话结束，检查点已保存"`，不消耗业务失败预算 |
| `coord_get_task` | `{"id":"TASK_UUID","batch_id":"BATCH_UUID","worker_id":"writer-1"}`，只读回查结果 |

`coord_claim` 返回任务使用 `id`；后续 Trusted 写工具参数是 `task_id`。始终保存本次 `attempt_no`，不要硬编码 1 或提交到旧 attempt。Trusted 工具不要求客户端保存租约 token。

正常顺序：status/list → resume 对账 → claim → 实际工作并及时 progress → complete／fail／release → 回读。租约请求为 1–600 秒，进度会按 Batch 规则续租；进度要求模式还受配置的累计租约预算约束。无活动会话时不能假定平台会后台心跳。

超时、连接断开或写响应不确定时，先 resume/get/status 对账，不要改结果盲目重放。已接受的 complete／fail 有精确内容幂等回执；release 不能据此当作同等可重放。租约已过期或 attempt 已换人时，旧执行者不能继续提交；重新领取并基于持久检查点恢复。

claim 为空不表示 Batch 成功：可能存在活跃租约、退避或待验证。查看 `batch_progress`／批次状态中的 `outcome`、`all_succeeded`、`states`；旧字段 `status:"completed"` 只表示队列已终结，也可能包含失败。

## 6. 管理已创建的任务

在第 3 节管理 RPC 环境中使用真实 `$BatchId`：

```powershell
Invoke-CoordRpc 'batches/status' @{ batch_id = $BatchId }
Invoke-CoordRpc 'tasks/list' @{ batch_id = $BatchId; state = 'failed'; limit = 100 }
```

`tasks/get` 使用 `@{ id = $TaskId }` 回读单项；列表最多取 100 条时不要把一页当作全部，批次总量以 status 为准。

仅在所有者明确要求后执行以下写操作，不要把它们当作每次查询的固定步骤：

- `batches/retry_failed`，参数 `@{ batch_id = $BatchId }`：重开可重试失败项，保留历史；预算耗尽时增加一个失败名额，不抹掉既往失败。
- `tasks/cancel`，参数 `@{ id = $TaskId }`：取消指定任务。
- `batches/cancel`，参数 `@{ batch_id = $BatchId }`：取消整个 Batch，撤销活跃执行归属；该 Batch 不再允许添加、领取或重试。

修改任务输入时优先创建有新 key 的新工作项并明确处理旧项，不直接编辑 SQLite。所有取消、重试均需回读实际状态；不要重派已完成的历史任务。

## 7. 常见阻塞

| 现象 | 正确处理 |
| --- | --- |
| HTTP 401 | 检查正常 OAuth 连接／有效期，不复制别人的 token |
| HTTP 403 `identity_binding_missing_or_expired` | 检查真实客户端、OAuth 有效期、入口策略与业务绑定；错误是入口的统一拒绝，不足以单独判定具体原因 |
| `logical_worker_or_batch_denied`／工具参数拒绝 | 对照实际 schema、入口资源范围和规范化 Worker；不要改填其他人的身份尝试越权 |
| 管理 RPC 创建被数据库冻结保护拒绝 | 保留错误并报告管理员；不删保护、不直写生产数据库 |
| 工具中没有“创建任务” | 这是 Worker 接口边界，使用受信管理端 RPC |
| `coord_response_too_large` | 当前远程响应上限为 64 KiB；单项 payload 应保持简短，用路径／链接引用大材料。列表可减小 limit；单项已超限时，减小 limit 也无效。若领取响应超限，先对账，不盲目再领；由任务管理者处理输入大小，不把它误判为权限错误 |
| 任务长时间无法领取 | 核对活跃租约、失败退避、验证状态、取消状态及 `needs_attention`，不要重复建相同任务 |
| `-32001` 冲突 | 可能是旧 attempt、输入 key 冲突或结果不一致；先回读，不替换他人的执行凭据 |

## 8. 可复制给其他 Codex 的启动说明

```text
使用已连接的 Coord Worker。Batch ID：<真实 UUID>；你的稳定 worker_id：<自选合法且不与其他执行者重复的标签>。
目标与输入：<真实材料位置>。允许操作：<明确范围>；禁止操作：<例如生产发布、修改历史结果>。
先发现实际工具 schema，再用 coord_status、coord_tasks_list 和 coord_resume 核对授权与当前归属。
无有效归属时用 coord_claim 自主领取；不要创建重复任务，也不要使用其他 Worker 的标签。
保存任务 id 与实际 attempt_no；实际执行，及时 coord_progress 保存可恢复检查点。
成功用 coord_complete 提交结论及证据；真实业务失败用 coord_fail；中断退出先保存进度再 coord_release。
写响应不确定时先回读对账；旧 attempt 不继续提交。空领取不等于成功，查看权威 Batch outcome。
遇到 401/403、冻结或缺少管理工具，报告确切阻塞；不改身份策略、不绕过平台确认、不触发未授权 Action。
最后报告真实 Batch/Task ID、实际完成情况、证据及未完成事项。
```

## 源码定位

- `src/a2a/mod.rs`：管理 JSON-RPC 与 Trusted 调度参数。
- `src/core/types.rs`：`BatchConfig`、`TaskInput`；`src/core/store.rs`：去重、状态、重试及结果语义。
- `relay/coord-mcp/index.cjs`：公开工具 schema、logical 参数及入口鉴权。
- `relay/identity.cjs`：Worker 规范化、绑定校验、scope 与签名。
- `docs/API.md`：完整协议。`docs/phase-g/LOGICAL_WORKERS.md` 是早期四位 Worker 实验说明，不应用它否定当前通用字符串支持。
