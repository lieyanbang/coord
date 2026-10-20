# coord 最小改造计划：通用 MCP 多 Agent 并行任务调度框架
> 状态：实施中；基线日期：2026-10-20；逐阶段证据见 `verification/`
> 本地仓库：`D:\project\coord`；Fork：`https://github.com/lieyanbang/coord`
> 上游：`https://github.com/DmarshalTU/coord`；基线 HEAD：`44fe390fac64ed4828d479896d7079da272c0152`
> 原则：**保留 coord 架构、以最小必要变更完成通用任务闭环、逐阶段真实验收**。

## 1. 目标与非目标
目标：在 coord 现有 Rust + SQLite WAL + JSON-RPC + stdio MCP 架构上，提供多个独立 Agent 动态领取、执行、提交、超时恢复、可选异步验收与进度统计的能力；不绑定模型或任务类型。
优先保留 `src/core/store.rs`、`src/core/types.rs`、`src/a2a/mod.rs`、`src/mcp/mod.rs`、`src/server.rs`、已有 CLI/TUI、Vault、长轮询和原有公开接口。
不引入 Redis、PostgreSQL、消息总线、DAG、多 Agent 角色编排、多租户平台或 Web Dashboard；不进行无关重构。
原有 coord 使用场景（人工指定 task ID、ack、watch、A2A subset、Vault）应继续可用；涉及租约安全性的旧客户端写接口可以明确返回升级错误，**不得保留绕过令牌的写入路径**。
本计划覆盖：A 质量修复、B 租约可靠性、C 规划剩余功能、D 自动化与故障验收。按门槛分阶段完成，不能以文档或模拟输出代替实测。

## 2. 基线与工程约束
开始前检查 `git status --porcelain`、`git remote -v`、`git rev-parse HEAD`、Rust/Cargo、目标端口与相关进程；保留已有工作与 Git 历史。
阅读 `AGENTS.md`，注意它针对运行中的 coord 会话的操作约定；本仓库自身未运行守护进程时不可伪造已登记任务，须记录 bootstrap 限制。
固定基线提交并留存 `docs/verification/` 下的测试结果；不得重置或覆盖用户的其他修改；提交和推送由执行阶段根据授权边界决定。
基线执行：`cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-features`、`cargo build --release`；可用时运行 `cargo audit`（工具不可用必须记录为未执行）。
首次执行记录构建系统及平台，尤其 Windows；现有 CI 仅覆盖 Ubuntu/macOS，应增加 Windows 检查或记录平台限制。
修复 `Justfile` 的旧名称 `agentd/agentctl`、错误端口/路径与实际二进制 `coord` 不一致问题；同步 README、示例与测试用法，不进行风格性大改。
## 3. Phase A — 代码质量与风险清理（Gate A）
### A1 已确认问题
- `src/core/store.rs::row_to_task`：未知状态降级为 PENDING、无效 JSON 降级为 Null，改为显式错误；旧合法 DB 不应无故失效。
- 修复并增强 `tests/lease.rs`：旧租约完成不能再被允许为合法结果；原测试允许错误结果，需改成强制失败断言。
- 清查数据库事务边界、错误传播和 migration；避免半成功状态、静默覆盖与不可追踪异常。
- 验证 `tasks/list` 长轮询的订阅窗口是否丢唤醒：补充发送事件与开始等待之间的竞争回归测试；如确认问题则最小修复为订阅后复查。
- `src/a2a/mod.rs` 暴露无鉴权 HTTP RPC：默认仅绑定 loopback，禁止误以为公网安全；外网部署需显式受信任反向代理/隧道。评估有限请求体与并发限制以防资源耗尽。
- 查看 `src/vault/mod.rs` 对任务名/Agent ID 的文件名、路径构造与写入行为，排除路径穿越、覆盖及异常输入；有证据才修改。
- 检查关键 `unwrap/expect`、SQLite 锁等待与失败日志；修复可触发的可靠性缺陷，不开展全局消除 unwrap 的重构。
### A2 工程检查
- 对照 Cargo.lock 和依赖声明检查安全公告、许可证、过期风险；不得为追求版本最新而无条件升级依赖。
- 保持严格 CI：fmt/clippy/test，并补充 Windows 工作流（具备条件时）。
- 保持 MIT 原始许可证及版权声明；在 Fork 的改造文档里明确来源与差异。
Gate A：原版功能回归通过、无高风险未处理静默损坏；新增质量测试可复现缺陷并验证修复；所有跳过/未验证项目明确登记。

## 4. Phase B — 租约正确性与故障恢复（Gate B，最高优先级）
租约不只是 `claimed_by`；每次成功领取必须产生新的不透明随机 `lease_token` 和递增 `attempt_no`（可先在 tasks 中存储；独立 TaskAttempt 历史视审计需求添加，避免无必要的大迁移）。
最小数据库增量：`lease_token`、`attempt_no`、`attempt_count`、`max_attempts`、`available_at`；保留现有 `claimed_at/lease_until`。
领取：单条条件 UPDATE 或短事务保证 PENDING→CLAIMED 原子化；生成新令牌；只允许尚可执行、未取消且未耗尽重试的任务领取。
续租：必须匹配 task ID + agent ID + 当前 token + CLAIMED + `lease_until > now`；使用数据库当前读取的时间语义；失效后拒绝“复活”，续租后期限不能回退。
完成/失败：必须匹配当前令牌、领取者及有效租约；检查幂等重复提交（相同请求识别为已接受，冲突结果不能覆盖）；完成与可选 ack 的写入仍在同一事务。
回收：过期 CLAIMED→PENDING/FAILED，清理 token；重试次数达到上限即终止；同一时刻最多一个有效租约。过期 Agent 的提交和续期一律被拒绝。
禁用旧版 `tasks/complete` 等缺少 token 的绕过路径；CLI、HTTP、MCP 同步兼容性更新并提供迁移说明。
若客户端崩溃后外部副作用已发生，系统只能保证调度状态的至少一次语义；不承诺外部代码、文件修改恰好一次。任务需使用业务幂等键或隔离工作区。
Gate B：双 Agent 竞争、A 过期 B 领取后 A 续租/提交、同一 agent_id 再领取旧 token、取消与提交竞态、断电/重启重领、重复提交全部有自动化回归；不允许以单线程模拟替代多连接/多进程验证。
## 5. Phase C — 通用任务批次与动态领取（Gate C）
**最小新增模型**：`batches` 表（id/name/description/config/created_at/status）；tasks 表增 `batch_id`、`ready_at`（如复用 available_at 则不要重复字段）、`error`、`completed_at`。原有单任务允许 batch_id=NULL，不破坏现存数据。
管理端：`create_batch`、`add_tasks`（事务内批量插入，支持用户指定幂等任务键）、`get_batch_status`、`retry_failed_tasks`、`cancel_batch`；初期 CLI 或 JSON-RPC 足够，无需全在 MCP 上暴露。
Agent MCP：`claim_task`（可选 batch_id，原子找到下一可领任务，空队列返回明确 empty）、`submit_result`、`report_failure`、`extend_lease`、`get_task`、`get_progress`。
避免 read-list → choose → claim 的高竞争开销：由存储层事务内选取并条件更新；为状态、ready_at、batch_id 建适当索引。SQLite 单写者是已知约束，先测量再优化。
生命周期保留旧状态兼容映射：PENDING、CLAIMED/RUNNING、SUBMITTED（开启验收时）、COMPLETED、FAILED、CANCELLED；对外新接口使用统一状态，同时为原有接口提供清晰映射。
任务输入、结果均为通用 JSON；模板批量生成放在 CLI/管理层实现，避免把任务类型写死到调度器。
执行失败/租约超时：有界重试、指数退避（含上限，必要时加抖动）、最后进入 FAILED；执行尝试与验收尝试分别计数。
取消批次须阻断后续领取与新提交、支持重复取消；批次结束条件：无 PENDING/CLAIMED/SUBMITTED，全部任务终态；已取消任务也计入终态。
Gate C：100 个独立任务、动态加入退出 Agent、异常回收、限制次数、进度准确、取消与重试幂等及新老入口回归通过。

## 6. Phase D — 可选异步验收与 Worker Loop（Gate D）
按最小原则先定义一个与 MCP 解耦的 `Validator` 接口：`validate(task, result, config) -> PASS/FAIL/ERROR`；PASS→COMPLETED，FAIL→重试/FAILED，ERROR→独立有界验收重试或记录不可验收，不错误消耗执行预算。
第一版实现内置 JSON 结构检查及简单规则验证；外部命令验证器只能在显式可信配置下运行，必须设置工作目录、超时、输出上限与权限边界。LLM Judge、自定义插件后续可选，不强制成为依赖。
开启验收时 `submit_result` 立即提交并转入 SUBMITTED；后台独立消费验收队列，执行 Agent 无需等待 Judge 完成便可领取下一个任务。
未开启验收时：提交成功直接 COMPLETED，只代表已提交执行结果，不暗示业务正确。
提供最小 Worker Loop / 接入模板：领取→执行适配器→续租→结果提交→继续领取；正常退出、空队列等待、断线退避、租约丢失后的中止提交必须处理。
区分“现有 IDE Agent 可调用 MCP”与“运行器可持续自动执行”；不承诺所有原生 MCP 客户端能够无人值守循环。Worker 执行实际逻辑由 Agent Runtime/可插拔适配器提供，框架不强制启动子 Agent。
Gate D：异步验收期间执行队列继续流动、PASS/FAIL/ERROR 分流正确、Worker 退出/崩溃恢复、验收开关两种模式、外部执行超时控制均经实际 E2E 验证。

## 7. Phase E — 自动验收与性能回归（最终 Gate）
每阶段提交前执行 `cargo fmt --all -- --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test --all-features`、`cargo build --release`，结果存档到 `docs/verification/`。
正式 E2E 使用真实 coord daemon + 多个独立进程/连接 + MCP 调用；分别测试 100、1000、10000 个任务，记录吞吐量、claim P50/P95、空闲等待、SQLite busy 次数、内存峰值。
必须执行可重复的故障注入：并发抢占、断开连接、过期续租、旧结果提交、重复调用、进程强杀、daemon 重启、数据库迁移与取消竞态。每项记录命令、输入、预期、实测和退出码。
性能只作为容量数据，不预先捏造每分钟任务数；如果不合格先剖析锁竞争再局部优化。必须证明任务无丢失、没有双有效租约、终态一致。
校验迁移：使用未改造的真实数据库副本升级，验证任务、Agent、ack、Vault 仍可读取；验证回退方案，不在原数据库上做不可逆试验。
结果统一标记 PASS/FAIL/BLOCKED/NOT_RUN；所有关键 Gate 实测 PASS 才可宣布 MVP 验收完成，不接受以“理论可行”代替。
## 8. 任务执行顺序与交付
顺序：基线（只读及现有测试）→ A 质量修复 → B 租约 → C 批次与调度 → D Worker/可选验收 → E 全面验证。每一阶段独立提交和保留证据，前一 Gate 失败不得跳过进入后一阶段。
优先改已有源码，新增文件/模块必须能够解释为何不可复用。对于 JSON-RPC/MCP，明确接口字段、错误码与幂等性，保留现有 watcher/ack 语义。
计划输出：`docs/PLAN.md`（本文件）；执行时另建 `docs/verification/` 下日志与最终报告，必要时建立接口规范文档，避免大量重叠规划文件。
历史边界：上述规划建立轮次只允许更新文档。2026-10-20 当前用户与 TASK.md 已授权按阶段实现、独立提交并向用户 Fork 的非发布分支推送以验证 Actions；不启动影响其他服务的 daemon。
完成标准：上述功能实际可用、全部 P0/P1 相关回归门槛 PASS，MCP 多 Agent 自动任务闭环 E2E PASS；原 coord 核心能力无回归，未通过项目有明确记录与处置。

## 9. 执行进度

### 当前平台契约（用户 2026-10-20 后续指示）

Windows 为主要且默认支持的平台。Linux/macOS 不默认支持，不在默认 CI 或
Release 构建矩阵中；其构建与运行均标记 NOT_RUN，不宣称支持。此指示覆盖
TASK.md 中原三平台 CI/Gate E 的对应要求；TASK.md 保持只读。所有阶段门槛以
Windows 本机及 Windows Actions 的真实测试、构建与产物为准。

| Stage | 状态 | 证据 / 依赖 |
| --- | --- | --- |
| 0 | PASS | `572771a`；Windows Actions 39428331459 success；产物已下载并运行 |
| A | PASS | 本机四项 Gate PASS；4 个缺陷先红后绿；质量/服务边界/长轮询测试；Stage B 租约缺陷明确待修复 |
| B | PASS | token/attempt、不可复活/不可缩短、原子 receipt+ack；多连接取消竞态和真实重启/旧客户端拒绝通过 |
| C | TODO | 依赖 Gate B |
| D | TODO | 依赖 Gate C |
| E | TODO | 依赖 Gate D；要求真实 Windows Actions 与容量证据 |
