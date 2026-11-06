# 门户快照契约，第 1 版

门户与 Mooring 之间只有这一份数据通道：Mooring 把每个用户"门户要显示的那几项"导出成一个 JSON 文件，运营者在门户主机上用 `mooring-portal import` 导入。门户不依赖 Mooring 的任何 crate，也不读取 `state.json`、`usage/users.json`、密钥库或 `ops-local/`。

- 实现：`crates/core/src/snapshot/`（解析、校验、编码）
- 金样：`contract/fixtures/v1/`（`valid/` 合法文档，`invalid/` 每条规则一个反例，`stored/` 门户落库的单用户文档）
- 金样测试：`crates/core/tests/contract/`

## 1. 不含什么

文档里没有用户姓名、Mac 上的本地标签、公开别名、凭据、short id、节点、地址或 IP。

用户以 **Mooring 用户编号**（`UserId`，10 个小写 RFC 4648 base32 字符）标识，而不是公开别名：《Mooring 使用约定》第 1 版第 4 条承诺"你的名字和别名只保存在运营者的电脑上，不会发送到服务器"，而用户编号本来就出现在服务器的记录里。别名格式（如 `k7qd-m2xa`）会被当作非法用户编号拒绝（金样 `invalid/alias-as-user.json`）。

## 2. 文档

一个 UTF-8 JSON 对象，最大 1 MiB。**每个键都必须出现**，可空的值写 `null`；出现未列出的键即拒绝。

| 键 | 类型 | 含义 |
|---|---|---|
| `format` | 字符串，恒为 `"mooring-portal-snapshot"` | 防止把别的 JSON（例如 `state.json`）误当快照导入 |
| `version` | 整数，恒为 `1` | 契约版本；门户只读它认识的版本 |
| `generated_at` | Unix 秒 | Mooring 生成快照的时刻 |
| `cycle` | 对象 `{"start": 日期, "next_start": 日期}` | 用量所属的 SIM 计费周期，UTC 日历：从 `start` 的 UTC 零点（含）到 `next_start` 的 UTC 零点（不含），即 Mooring 的 `BillingCycle` |
| `counted_through` | Unix 秒或 `null` | 用量统计截至的时刻（不含）；`null` 表示 Mooring 当时没有用量报告 |
| `users` | 数组，至少一项 | 每用户记录，见下表 |

日期写作 `YYYY-MM-DD`；所有时刻与日期都限于 1970-01-01 至 9999-12-31。

每用户记录：

| 键 | 类型 | 含义 |
|---|---|---|
| `user` | 字符串 | Mooring 用户编号 |
| `status` | `"active"`、`"revoked"` 或 `{"suspended": {"reason": 原因, "since": Unix 秒}}` | Mooring 的 `UserStatus`；`revoke_pending` 与 `revoked` 都写作 `"revoked"`（两者对用户而言都是"不再提供服务"） |
| `cycle_served_bytes` | 整数或 `null` | 本周期内为该用户转发的字节数（双向、所有节点），即 `UserUsage::cycle_served_bytes`；Mooring 无法确定时为 `null` |
| `reality_downlink_counted_at_close` | 布尔 | Mooring 的 `UsageAccuracy`：用户仍被服务时为 `true`，表示 REALITY 下行要等连接关闭才计入，数字可能滞后 |
| `quota` | 对象或 `null` | 有效配额（用户自己的，或 Mooring 计算出的自动配额，即 `effective_quota`）：`{"cycle_served_bytes": 正整数, "warn_percent": 1–99, "action": "warn" 或 "suspend"}`；`null` 表示没有个人配额 |
| `expires_at` | Unix 秒或 `null` | 访问到期时刻（`UserPolicy::expires_at`）；`null` 表示不到期 |

暂停原因 `reason`：`operator`、`quota_exceeded`、`expired`、`abuse_review`。

## 3. 跨字段规则

违反任何一条，整份文档被拒绝（门户报出具体原因）：

1. `cycle.start` 早于 `cycle.next_start`。
2. 周期开始不晚于 `generated_at`。
3. `counted_through` 非空时，落在周期之内（两端都含），且不晚于 `generated_at`。
4. `counted_through` 为 `null` 时，所有用户的 `cycle_served_bytes` 都必须为 `null`：没有报告就没有数字。
5. 暂停时刻 `since` 不晚于 `generated_at`。
6. 用户不重复，至少一个。

## 4. 门户如何解读

- **未知不是零**：`cycle_served_bytes` 为 `null` 时显示"无法获取"，绝不显示 0；已知的 0 显示为 0 B。
- **配额档位**与 Mooring 的 `QuotaState::of` 一致：用量 ≥ 配额为"已达配额"；`用量 × 100 ≥ 配额 × warn_percent` 为"接近配额"；否则"正常"；用量未知时"无法判断"，绝不当作"正常"。已用百分比向下取整，超额时超过 100%，进度条封顶在 100%。
- **到期**按门户自己的时钟计算剩余整天数（向上取整，同 Mooring 的 `days_until`）；到期时刻不晚于此刻即"已到期"。
- **新鲜度**：页面写明"数据截至"`generated_at`；超过 48 小时提示数据可能过时。
- 页面时间一律换算为北京时间（UTC+8）并注明。

## 5. 门户如何导入

`mooring-portal --db <文件> import <快照文件>`：

- 只保留**已绑定**到门户账号的用户的记录；其余用户只计数报告（"not kept: N user(s)…"），不落库。门户不需要的人的数据，门户就不保存。
- 已绑定但快照里没有的用户，逐个报告为警告；他们的页面保留上一次的记录。
- `generated_at` 比门户主机时钟晚 300 秒以上：拒绝（两台机器有一台时钟错了）。
- 某个用户已存的记录比这份快照新：整份拒绝，什么也不写（同一时刻的快照可重复导入）。
- 导入在一个事务里完成：要么全部写入，要么什么也不写。
- 解绑或删除账号时，对应记录随之删除。

## 6. 存储格式

门户把每个已绑定用户的部分存成**只含这一个用户的第 1 版文档**（紧凑 JSON），读出时用同一套规则重新校验；校验不过即报"数据库内容损坏"（500 错误页），不会显示成"暂无订阅"。样例：`contract/fixtures/v1/stored/`。

## 7. 版本演进

- 增加、删除或改变任何键或取值，都是新版本（`version: 2`），并在本目录新增规范与金样。
- 门户拒绝它不认识的版本，并在错误里写明版本号。
- 若门户里已存有旧版本记录，新版本发布时须同时提供迁移。

## 8. Mooring 一侧的导出器（尚未实现）

导出器属于 Mooring 仓库（例如 `mooringctl portal export`），按本契约定稿、运营者同意后在 Mooring 单独提交。建议做法：

| 字段 | 来源 |
|---|---|
| `generated_at` | 导出时刻 |
| `cycle` | 有用量报告时取报告的 `UsagePeriods::cycle`，否则取导出时刻所在周期（`BillingCycle::of_instant`） |
| `counted_through` | 报告的 `UsagePeriods::through`；没有报告时为 `null` |
| `user` | `User::id` |
| `status` | `UserStatus`：`Active` → `active`；`Suspended { reason, since }` → 同名原因（`QuotaExceeded` 不带周期日期）；`RevokePending`、`Revoked` → `revoked` |
| `cycle_served_bytes` | `UserUsage::cycle_served_bytes()`；无报告或无法确定时为 `null` |
| `reality_downlink_counted_at_close` | `UsageAccuracy::of(user)` |
| `quota` | `effective_quota(...)` 的结果 |
| `expires_at` | `UserPolicy::expires_at` |

- 只导出运营者指定的用户（例如 `--user <id>` 可重复），默认不导出全部，以免把未绑定用户的数据带上联网主机。
- 输出文件以 0600 创建，导入后删除。
- 导出器应有自己的金样测试，与本目录的 `valid/` 文档逐字对照。

## 9. 与《使用约定》的关系

门户会在一台联网主机上保存已绑定用户的编号、状态、本期用量、配额和到期时间，以及门户账号的邮箱和密码摘要。这是《Mooring 使用约定》第 1 版第 4 条没有列出的新存放位置。**在为真实用户绑定账号之前，须修订使用约定（写明门户保存什么、存多久、在哪里），并请用户重新确认。**
