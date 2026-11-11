# 权益清单契约，第 1 版

付款之后该开通什么，由 Mooring 来问门户：Mooring 定时拉取一份清单，按清单开通或续期。门户不连接 Mooring，也不知道 Mooring 在哪里。

- 实现：`crates/core/src/payment/feed.rs`（文档的写法）、`crates/store/src/entitlements.rs`（清单和游标）、`crates/web/src/machines.rs`（接口）
- 金样：`contract/fixtures/entitlements-v1/`（`pilot.json` 六条记录，三种都有，`empty.json` 没有新记录时的回答）。金样为了好读带着缩进和换行，门户发出的是去掉这些空白的同一份文档
- 金样测试：`crates/core/tests/contract/payments.rs`
- Mooring 一侧的拉取程序还没有写；第 5 节是它要做的事

## 1. 清单是什么

清单是一本只增不改的流水账。每条记录有一个只增的编号 `seq`；Mooring 记住自己处理到了哪一条（游标），下次只问那之后的。记录有三种：

- `grant`：一张订单已经付款，并且通过了门户的全部核对（`docs/payments.md` 第 2.2 节），请开通。每张订单只有一条。
- `refund`：一张已经开通的订单的付款被退了一部分或全部。
- `dispute`：一张已经开通的订单的持卡人向银行提出了争议。

每条记录都带着订单的全部内容，不需要参照别的记录就能读懂。

## 2. 请求与回答

```
GET /api/mooring/entitlements?after=<编号>
Authorization: Bearer <令牌>
```

- `after`：Mooring 已经处理完的最后一条记录的编号；第一次写 `0`。必须给出。
- 令牌：运营者在门户主机上用 `mooring-portal pull-token` 生成，64 个小写十六进制字符，只显示一次；门户只存它的摘要。令牌放在请求头里，不放进地址。

| 状态 | 含义 | Mooring 应该 |
|---|---|---|
| 200 | 正文是第 3 节的文档 | 处理记录，保存 `next` |
| 400 | 请求不对（没有 `after`，或它不是非负整数） | 当作自己的错误报告，不重试 |
| 401 | 没有令牌、令牌不对，或门户没有设置令牌 | 报告给运营者，不重试 |
| 404 | 门户没有配置套餐文件，不卖东西 | 报告给运营者 |
| 409 | 游标比清单发出过的最大编号还大：这不是 Mooring 上次取的那本清单（例如门户的数据库被换成了旧备份） | **停止拉取并报告给运营者**，不要把游标改小重来 |
| 429 | 请求太频繁，`Retry-After` 是该等的秒数 | 等够时间再来 |
| 5xx | 门户出错 | 稍后重试，游标不变 |

失败时的正文是一两行纯文本，不是 JSON。每次回答都带 `Cache-Control: no-store`。

## 3. 文档

一个 UTF-8 JSON 对象。**每个键都会出现**，不适用的值写 `null`。

| 键 | 类型 | 含义 |
|---|---|---|
| `format` | 字符串，恒为 `"mooring-portal-entitlements"` | 防止把别的 JSON 误当清单 |
| `version` | 整数，恒为 `1` | 契约版本 |
| `generated_at` | Unix 秒 | 门户生成这份文档的时刻 |
| `livemode` | 布尔 | 这些订单是不是用真钱付的。`false` 是 Stripe 的测试模式。取自门户数据库记下的模式：一个数据库只装一种模式的付款，所以同一本清单里的记录都是同一种 |
| `after` | 整数 | 请求里的游标 |
| `entries` | 数组，最多 200 项 | `seq` 大于 `after` 的记录，按 `seq` 从小到大 |
| `next` | 整数 | 下次请求该用的游标：最后一条记录的 `seq`；没有记录时等于 `after` |
| `more` | 布尔 | 清单里是否还有 `next` 之后的记录 |

每条记录：

| 键 | 类型 | 含义 |
|---|---|---|
| `seq` | 正整数 | 记录的编号。只增，不重复使用；中间可能有空缺（见第 6 节） |
| `kind` | `"grant"`、`"refund"`、`"dispute"` | 记录的种类 |
| `order` | 字符串 | 订单编号：20 个小写 Crockford base32 字符 |
| `user` | 字符串 | Mooring 用户编号：下单时账号绑定的用户，开通时账号仍然绑定着他 |
| `plan` | 字符串 | 套餐编号，运营者在门户的套餐文件里起的 |
| `plan_kind` | `"month"`、`"month_auto"`、`"traffic"` | 套餐种类 |
| `days` | 正整数 | `month`、`month_auto`：买到的服务天数；`traffic`：流量的有效天数 |
| `traffic_bytes` | 正整数 | `month`、`month_auto`：这一期的流量配额；`traffic`：额外流量。字节，十进制（1 GB 是 1000000000） |
| `amount` | 正整数 | 订单的价格，以币种的"分"计 |
| `currency` | 字符串 | 币种，小写三个字母 |
| `paid_at` | Unix 秒 | 付款的时刻（Stripe 生成付款通知的时刻） |
| `renewal_of` | 字符串或 `null` | 自动续费生成的订单：开始这个订阅的那张订单的编号；其余为 `null` |
| `amount_refunded` | 整数或 `null` | 只在 `refund` 记录里有值：这笔付款**累计**已经退了多少，单位同 `amount` |
| `refunded_in_full` | 布尔或 `null` | 只在 `refund` 记录里有值：Stripe 是否说这笔付款已经全额退款。以它为准，不要拿 `amount_refunded` 和 `amount` 比：运营者放行过的订单，实付金额可能和套餐价格不同 |
| `amount_disputed` | 整数或 `null` | 只在 `dispute` 记录里有值：争议的金额 |
| `recorded_at` | Unix 秒 | 门户写下这条记录的时刻 |

所有时刻都在 1970-01-01 至 9999-12-31 之间。

## 4. 每种记录的意思

- **`grant`**：按 `plan_kind` 开通。
  - `month`、`month_auto`：给 `user` 延长 `days` 天的服务，这一期的流量配额是 `traffic_bytes`。建议从"现有到期时刻与现在两者中较晚的"往后延，这样提前续费不吃亏。`month_auto` 每续一期，就有一张新的订单和一条新的 `grant`（`renewal_of` 指向第一张）。
  - `traffic`：给 `user` 加 `traffic_bytes` 的额外流量，`days` 天内有效。
- **`refund`**：`order` 的付款累计已经退了 `amount_refunded`（`amount` 是订单的全价），`refunded_in_full` 说这是不是全额。同一张订单可以有多条：累计金额变大时追加一条；Stripe 说已经全额退款、而清单里这张订单还没有 `refunded_in_full` 为 `true` 的记录时，也追加一条。退款都是运营者在 Stripe 后台做的：按《使用约定》，部分退款多半是补偿停机，服务照常；`refunded_in_full` 为 `true` 时应当撤销这张订单带来的时长或流量。门户只报告事实，怎么处理由 Mooring 的规则决定。
- **`dispute`**：`order` 的持卡人向银行提出了争议，金额 `amount_disputed`。每张订单至多一条。建议暂停该用户并通知运营者。

`refund` 和 `dispute` 只会出现在同一张订单的 `grant` 之后（`seq` 更大）。一张订单在开通之前就被全额退款或被争议的，清单里什么也不会有。

## 5. Mooring 一侧要做的

1. **保存游标**，和开通的结果写在同一次落盘里：先处理 `entries`，成功后把游标设为 `next`。`more` 为 `true` 时立即再取。
2. **按订单编号只开通一次**。游标丢失后从 `0` 重取，或者门户重发了已经给过的记录，都不能重复开通：`grant` 以 `order` 去重，`refund` 以 `order`、`amount_refunded` 和 `refunded_in_full` 去重，`dispute` 以 `order` 去重。
3. **`livemode` 为 `false` 的文档不开通**，除非运营者明确在演练。测试模式里任何人都能用 Stripe 的测试卡号"付款"。运营者演练完会清掉演练的记录再收真钱（门户的 `discard-test-payments`）：清掉的记录的 `seq` 不会再用，所以演练时取过清单的 Mooring 接着用自己的游标就行，不用归零。
4. **不认识的 `user` 不开通**，报告给运营者。门户只保证这个编号是账号下单时和开通时绑定的那个，不保证 Mooring 里还有这个用户。
5. **`format`、`version` 不对，或某个键缺失、多出、类型不对，整份拒绝**，游标不动，报告给运营者。
6. **409 时停下**（第 2 节）。
7. **建议：开通前向 Stripe 核对。**清单是从门户取的，门户被攻破时，攻击者能写出假的 `grant`。Mooring 可以用一把只读的 Stripe 受限密钥，按订单编号（它就是 Stripe 结账记录上的 `client_reference_id`）查到这笔付款，确认它已付、金额和币种相符，再开通。这把密钥只放在运营者的 Mac 上，不放门户。自动续费生成的订单没有结账记录，要按订阅的账单核对。做了这一步，门户就只是提示，不是付款的唯一凭证。
8. 拉取的间隔由 Mooring 定。门户按来源限流：每个来源每小时 600 次。

## 6. 门户的保证

- `seq` 严格递增，发出过的编号不会再用，已经写下的记录不会改。
- 一条 `grant` 只在订单通过了 `docs/payments.md` 第 2.2 节的全部核对之后才写下。
- 门户记着 Mooring 出示过的最大游标，把它当作"Mooring 已经有了这之前的全部记录"。运营者删除一个账号时，这个账号的订单和记录随之删除，但门户会先确认它们都已经在这个游标之前（`delete-account` 否则拒绝）。所以 Mooring 正常拉取时不会漏掉记录；从 `0` 重新拉取时，已删除账号的记录不在了，`seq` 出现空缺，这是正常的。
- 例外是封禁（`ban`）：它不等 Mooring，被封账号还没被取走的记录会直接消失。这正是想要的结果。
- 游标比发出过的最大编号还大时答 409，不会答"没有新记录"。
- 一条 `grant` 写下之后不会被收回：之后订单的付款有什么变化，只会在它后面追加 `refund` 或 `dispute`。
- 文档里的 `livemode` 是数据库记下的模式，不随门户这一次启动用的套餐文件变。

## 7. 版本演进

- 增加、删除或改变任何键或取值，都是新版本（`version: 2`），并在本目录新增规范与金样。
- 门户只写它声明的版本；Mooring 拒绝它不认识的版本。
