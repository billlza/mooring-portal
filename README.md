# mooring-portal

Mooring 的用户门户，对外品牌为 **泊光 Moorlight**：夜里引船进港停泊的那盏灯。品牌名、标语和释义只写在 `crates/web/src/brand.rs` 一个文件里；crate、二进制和路径都不带品牌名，改名只改这一个文件。

门户是一个服务端渲染的网站，每个页面不开 JavaScript 都能用，供少数受信任的 Mooring 用户查看自己的账户和订阅。唯一的脚本是首页主图上那幅会动的灯塔夜景，只有首页运行它（2026-11-10，`docs/security.md` 第 6 节）：

- 公开页面：首页、订阅说明（导航里叫“套餐”）、下载（CFM 客户端，版本跟随 GitHub 上最新的正式发布，见 `docs/releases.md`）、帮助；
- 账号：注册、登录、退出、修改密码；中英文切换，默认中文；
- 账户页：账号编号、邮箱、注册时间、绑定状态；
- 订阅页：状态、本计费周期用量、配额、到期时间，数据来自 Mooring 导出的快照；以及 CFM 配置：生成订阅链接（CFM 0.5.x「导入链接」，之后「更新」）或下载 JSON 配置文件（「导入文件」）；
- 下单与付款（给门户配了套餐文件时）：订阅页列出套餐，已绑定的用户下单后到 Stripe 的页面上用银行卡付款；门户接收 Stripe 的付款通知，核对后把该开通的订单列进清单，等 Mooring 来取（`docs/payments.md`）；
- 运营者命令：列出账号、按账号编号绑定或解绑 Mooring 用户、导入快照、导入 CFM 配置包、导入 CFM 的最新发布、删除账号、封禁与解除封禁、查看和处理订单与付款、管理 Mooring 的拉取令牌、收真钱之前清掉演练的付款记录。

门户保存运营者导入的 CFM 配置，其中含节点凭据，所以门户主机按存放凭据的机器对待（`docs/security.md` 第 9 节）。付款在 Stripe 的页面上完成：门户不经手卡号，也不持有任何能动钱的密钥（`docs/security.md` 第 11 节）。泊光已接入 Stripe 正式收款，Mooring 已成功拉取正式清单。首笔真实付款后的开通待验收。

## 当前状态

2026-11-06：v1 完成；同日加入订阅链接与配置下载（分支 `feature/cfm-subscription-links`）。2026-11-10：重做全站样式，首页主图加上会动的场景；下载页改为跟随 CFM 的最新发布；同日按运营者给的设计图重排首页和配色，主图换成灯塔夜景，场景改为完全由着色器画出，不再加载任何图片（分支 `feature/ui-refresh`，接在前一个分支之后）。2026-11-11：接入付款（分支 `feature/payments`，接在 `feature/ui-refresh` 之后）：下单、接收 Stripe 的通知、给 Mooring 的清单、运营者核对用的命令；只用合成的事件在本机走通过。2026-11-14：泊光在原数据库上切换到正式付款，开放预付套餐和流量包，验收记录见 [支付文档](docs/payments.md#2026-11-14-正式切换)。许可证尚未选定。

## 仓库结构

| 路径 | 职责 |
|---|---|
| `crates/core` | 纯领域逻辑：时间与日期、账号（邮箱、账号编号、密码策略）、Argon2id 与令牌、限流器、快照契约、CFM 配置包契约、对 GitHub 发布信息的读取与校验、付款（套餐文件、订单编号、Stripe 通知的验签与读取、付款与订单的核对、给 Mooring 的清单） |
| `crates/store` | SQLite 持久化：表结构与迁移、账号、会话、绑定、快照导入、配置包导入、订阅链接、下载页提供的 CFM 发布、订单、Stripe 通知的记账与对账、给 Mooring 的清单与游标、封禁的银行卡；文件以 0600 创建 |
| `crates/web` | HTTP 层（axum）：页面与模板（askama，编译期检查）、会话、CSRF、限流、安全响应头、中英文文案、品牌、样式与程序生成的主图；下单与订单页，以及给机器用的两个地址（Stripe 的通知、Mooring 的拉取） |
| `crates/web/assets/js` | 首页场景的脚本（ES 模块，不经打包，编进二进制）；它们的测试在 `test/` 子目录里，不随站点提供 |
| `crates/portal` | 二进制 `mooring-portal`：`serve` 与运营者命令 |
| `contract/fixtures/v1/` | 快照契约的合成样例与金样 |
| `contract/fixtures/cfm-profiles-v2/` | 配置包契约的合成样例与金样（凭据都是假的，地址都在文档保留网段） |
| `contract/fixtures/cfm-release/` | GitHub 对 CFM 0.5.1 这次发布的描述（删去了发布说明和上传者资料），以及每条校验规则各一份被拒绝的样例 |
| `contract/fixtures/payments/` | 合成的套餐文件（链接和编号都是编的） |
| `contract/fixtures/stripe-events/` | 合成的 Stripe 事件，照 Stripe 文档里的形状写成，付款人的资料都是编的；每种门户要处理的事件和几种它该放过的 |
| `contract/fixtures/entitlements-v1/` | 给 Mooring 的清单的金样 |
| `docs/contract/portal-snapshot-v1.md` | 快照契约规范，含 Mooring 侧导出器的映射建议 |
| `docs/contract/cfm-profiles-v2.md` | 配置包契约规范：格式、导入、CFM 0.5.1 拉取链接时的约束、Mooring 侧导出器要做的事、与第 1 版的区别 |
| `docs/contract/entitlements-v1.md` | 清单契约规范：Mooring 怎样拉取、每种记录的意思、Mooring 一侧要做的事 |
| `docs/security.md` | 安全设计、威胁模型与剩余风险 |
| `docs/subscription-links.md` | 订阅链接与配置文件的决定、设计与剩余风险 |
| `docs/payments.md` | 支付：运营者的决定、做法（Stripe Payment Links + Webhook，Mooring 来拉）、开通步骤、套餐文件的格式、运营者命令、还没做的事、本地演练 |
| `docs/releases.md` | 下载页怎样跟随 CFM 的发布：决定、门户读什么信什么、这改变了什么、部署时的定时器 |

依赖方向只有一条：`core ← store ← web ← portal`。

## 构建与检查

工具链固定为 Rust 1.99.1（`rust-toolchain.toml`，含 rustfmt、clippy、llvm-tools-preview）。交付前以下命令全部通过，零 error、零 warning：

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

```bash
cargo deny check
```

```bash
cargo llvm-cov --workspace --summary-only
```

```bash
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items
```

覆盖率要求 95% 以上（区域、函数、行三项）。编译输出只写仓库内的 `target/`，用完执行 `cargo clean`。

首页场景的脚本另有两条检查，要用 Node 22.8 或更新的版本（只在开发时用，构建和运行门户都不需要它）。第一条跑脚本的测试，并要求行覆盖率 95% 以上；测试用一个假的页面和假的显卡驱动脚本，不需要浏览器。第二条检查入口文件能被解析，它只有一行调用，测试不导入它：

```bash
node --test --experimental-test-coverage --test-coverage-lines=95 "crates/web/assets/js/test/*.test.mjs"
```

```bash
node --check crates/web/assets/js/hero.mjs
```

着色器能不能在显卡上编译、画出来对不对，这两条都检查不了。改了 `crates/web/assets/js/` 下任何一个 `shader*.mjs` 之后，要在浏览器里打开首页，确认场景出现（`.hero` 的 `data-scene` 是 `live`）、控制台没有报错。着色器里各处的位置、颜色和几条规则在 `crates/web/src/assets/hero.rs` 及其子模块里另有一份（静态主图用），改一处要两边一起改，`cargo test` 会在只改了一边时失败。

工作区 lint 禁止 `unsafe`、`unwrap`、`expect`、`panic`、`print`。集成测试的入口文件用 `#[cfg(test)] mod` 声明各模块，以表明它们都是测试代码，`clippy.toml` 里对测试的放宽才能覆盖其中的辅助函数。

### 依赖

依赖从少：axum、tokio、askama、rusqlite（内置 SQLite）、argon2、blake2、hmac、sha2、getrandom、subtle、serde、thiserror、clap、tracing。版本取 crates.io 上**发布满 14 天**的最新兼容版本（2026-11-06 核对；rusqlite 于 2026-11-10 升到 0.41.0：它内置的 SQLite 3.54 修掉了 RUSTSEC-2026-0459，而 0.41.1 发布还不满 14 天）。hmac 0.13.0 和 sha2 0.11.1 于 2026-11-11 加入，只用来验证 Stripe 通知的签名：算法（HMAC-SHA256）是 Stripe 定的，两者和 blake2 同属一个项目、共用一个 `digest`。`cargo deny check` 要求：每个依赖只有一个版本；只用 Apache-2.0、BSD-3-Clause、MIT、Unicode-3.0 许可；只从 crates.io 获取；公告库中无未处理的漏洞、无人维护或不健全的 crate。

## 本地运行与演示

```bash
mkdir -p target/demo && chmod 700 target/demo
```

```bash
cargo run -p mooring-portal -- --db target/demo/portal.db serve --origin http://localhost:8080
```

在浏览器打开 <http://localhost:8080>（必须用 `localhost`，与 `--origin` 一致）。Cookie 都带 `__Host-` 前缀和 `Secure`，浏览器只在 HTTPS 或 localhost 上接受它们。

演示流程（全部是合成数据）：

1. 在 `/register` 注册，例如 `demo@portal.test`，密码 `demo passphrase 2026`；
2. 用账户页上的账号编号绑定合成用户，再导入合成快照：

```bash
cargo run -p mooring-portal -- --db target/demo/portal.db bind --account <账号编号> --user pilotusera
```

```bash
cargo run -p mooring-portal -- --db target/demo/portal.db import contract/fixtures/v1/valid/pilot.json
```

`pilot.json` 里还有其他合成用户，覆盖其余状态：`pilotuserb` 接近配额，`pilotuserc` 因超额暂停，`pilotuserd` 用量无法获取，`pilotusere` 无个人配额，`pilotuserf` 已停用，`pilotuserg` 已到期。换绑前先 `unbind`。

3. 下载页在导入发布信息之前，会写明还没有取得。导入仓库里那份 GitHub 对 CFM 0.5.1 的描述，就能看到它：

```bash
cargo run -p mooring-portal -- --db target/demo/portal.db import-release contract/fixtures/cfm-release/valid/v0.5.1.json
```

4. 导入合成配置包后，订阅页会出现"在 CFM 中使用"，可以生成订阅链接或下载配置文件（合成配置里的地址在文档保留网段，CFM 会拒绝导入，只用于看页面）。合成配置包为 `pilotusera`、`pilotuserb` 交付配置，把 `pilotuserc` 列为不交付；绑定的是其他合成用户时，导入会因漏列被拒绝：

```bash
cargo run -p mooring-portal -- --db target/demo/portal.db import-profiles contract/fixtures/cfm-profiles-v2/valid/pilot.json
```

5. 下单和付款的演练不需要 Stripe 账户：用合成的套餐文件启动门户，下单，再把合成的 Stripe 事件签名后发给它，步骤见 `docs/payments.md` 第 7 节。

## 运营者命令

所有命令都在门户主机上运行，数据库就是 `serve` 用的那个文件，服务运行时也可以执行（SQLite WAL 模式，写操作在事务里串行；有测试）。数据库由 `serve` 第一次启动时创建；运营者命令要求文件已经存在，`--db` 写错时报 `there is no database at …`，不会新建一个空库。删除或改写数据的命令（`unbind`、`import`、`import-profiles`、`delete-account`、`ban`、`unban`、`dismiss-payment`、`discard-test-payments`）做完后，会把被删的内容从数据库文件和日志里抹掉，最后一行是 `erased the deleted rows from the database files`（`docs/security.md` 第 8 节）。

| 命令 | 作用 |
|---|---|
| `accounts` | 列出账号：编号、注册时间（UTC）、绑定的 Mooring 用户、邮箱 |
| `bound-users` | 按编号顺序输出一行 `--user <编号> …`，即全部已绑定账号的 Mooring 用户，直接接在 Mooring 的导出命令后面（Mooring 的配置包和快照导出都用 `--user` 点名） |
| `issue-intro-offer --account <编号> --user <编号> --link-id <plink_…> --link <测试网址>` | 运营者确认是 Mooring 新用户后，给已绑定账号发一次性沙盒首期优惠 |
| `intro-payments` | 列出待在 Stripe 创建的沙盒首期续费日程和未确认的期满不续请求 |
| `bind --account <编号> --user <用户编号>` | 绑定。一个账号只能绑一个用户，反过来也一样 |
| `unbind --account <编号>` | 解绑，并删除该用户在门户上的快照记录、CFM 配置和订阅链接。订单留着；账号有 Stripe 还可能扣款的订阅时会提醒：它已经没有 Mooring 用户可以开通了 |
| `import <文件>` | 导入快照：只保留已绑定用户；报告未绑定的人数，以及快照里缺少的已绑定用户 |
| `import-release <文件>` | 把 GitHub 上 CFM 的最新正式发布放到下载页上。文件是 GitHub 对 `releases/latest` 的回答，由主机上的定时器取来（`docs/releases.md` 第 4 节）。校验不过就什么也不改；版本没变时只更新"上次核对的时间"。不删除、不改写任何用户的数据，所以不做抹除 |
| `import-profiles <文件>` | 导入 CFM 配置包。配置包必须点名每一个已绑定账号的 Mooring 用户，漏了谁就整份拒绝，什么也不改，并列出漏掉的账号。导入后只保留其中已绑定用户的配置，其余全部删除。Mooring 不交付（列入 `withheld`）的已绑定账号没有配置：作废的，订阅链接永久停用；暂停或未确认约定的，链接暂停，再交付后照常可用。报告这些账号、原因和链接的去向，以及点名了却没有绑定账号的用户。导入后删除配置包文件 |
| `delete-account --account <编号>` | 删除账号，连同会话、绑定、快照记录、CFM 配置、订阅链接和订单。账号有 Stripe 还可能扣款的订阅，或有没了结的订单（付了款还没被 Mooring 取走，或在等核对、既没有全额退款也没有被争议）时拒绝，并说明先要做什么（`docs/payments.md` 第 2.8 节） |
| `ban --account <编号> [--card <指纹> …]` | 封禁：删除账号和它的一切，并记下该账号邮箱的摘要，这个邮箱以后不能再注册；`--card` 给出的银行卡指纹（在 Stripe 后台读出，可以重复）也记下摘要，以后用这些卡付的款不开通。不等付款了结，并列出还要在 Stripe 取消的订阅。换邮箱、换卡都拦不住（`docs/security.md` 第 10 节）。绑定过 Mooring 用户的，还要在 Mooring 里作废那个用户 |
| `ban-cards --email <地址> --card <指纹> …` | 给已经封禁的邮箱补记银行卡 |
| `unban --email <地址>` | 解除封禁：这个邮箱可以重新注册，随它记下的卡以后可以再付款（封禁期间用这些卡付的款仍然不能放行，退款后重新付）；被删除的账号不会恢复 |
| `orders` | 付款的账：等运营者核对的订单和原因、等 Stripe 通知的订单、不属于任何订单的付款、Stripe 还可能扣款的订阅、对不上订单的退款和争议、最近 20 张订单、Mooring 取到了哪里；每一项都写明下一步怎么做 |
| `approve-order --order <订单编号>` | 放行一张订单，只免掉 `orders` 给它列出的那一条原因：结账对不上的，接着按卡核对；卡无法核对、或 Stripe 一直没说卡的，列进给 Mooring 的清单。命令会说订单现在停在哪里。卡在封禁名单上、已经全额退款、被争议、已在清单里、还没付款，或账号不再绑定订单上那个 Mooring 用户的，拒绝（`docs/payments.md` 第 2.2 节） |
| `dismiss-payment --id <序号> [--subscription-ended <订阅号>]` | 把一笔不属于任何订单的付款从待办里拿掉。它的订阅没有订单开始过、Stripe 也还没说结束时拒绝，全额退款也要等这个订阅结束才会自动移除；Stripe 的结束通知一直不来，在 Stripe 后台确认订阅已经结束后加 `--subscription-ended` 和订阅号（`docs/payments.md` 第 2.6 节） |
| `pull-token` | 生成 Mooring 拉取清单用的令牌，替换以前的；只显示一次 |
| `revoke-pull-token` | 作废拉取令牌 |
| `discard-test-payments` | 在 Mooring 上关闭演练后，删掉原门户数据库的测试订单、付款和通知记录，并抹除副本。保留账号、绑定、封禁名单、拉取令牌和清单序号。装着真钱付款的数据库拒绝（`docs/payments.md` 第 2.9 节） |
| `erase-deleted` | 把已删除的行从数据库文件和日志里抹掉。删除或改写数据的命令都会自己做；它们报告别的连接一直占着数据库、没能做完时，再运行它 |

绑定流程：用户注册后，在 FaceTime 语音通话中把账户页上的账号编号念给运营者；运营者**按编号绑定，不按邮箱绑定**（原因见 `docs/security.md` 第 2 节）。编号不区分大小写，字母 o、i、l 会按 0、1、1 读入。

退出码：0 成功，1 失败（错误写在 stderr，以 `mooring-portal:` 开头），2 用法错误。

## 部署前提（已装在主机上，还没有对外）

第一次部署的步骤和配置（`moorlight.app`、Caddy、systemd 单元、上线前的检查）见 `docs/deploy.md`。2026-11-12 起，门户服务和下载页的定时器在 Linode 主机上运行，`https://moorlight.app` 已经能打开门户；注册只限 1 个账号，还没有对外开放。门户对主机的要求是：

- 门户只监听回环地址，由同机反向代理（如 Caddy、nginx）终止 TLS 后转发；代理须追加 `X-Forwarded-For`，门户以 `--behind-proxy` 启动。代理不得压缩 `/sub/` 的响应（CFM 0.5.1 只接受 identity 编码），也不得记录请求路径：路径里是订阅链接的令牌。
- 门户主机上要有一个定时器，每 5 分钟取一次 CFM 的最新发布并导入，下载页才会跟着更新；主机因此要能访问 `api.github.com`。取数据的进程用另一个读不到数据库的账号运行，导入的进程没有网络。单元文件见 `docs/releases.md` 第 4 节，2026-11-12 已在门户主机上运行。
- 代理可以压缩 `/assets/` 下的响应（样式、图片和脚本都是公开的固定内容，压缩后小得多），不要压缩页面（`docs/security.md` 第 6 节）。代理不得改写或去掉门户的 `Content-Security-Policy`；代理自己生成的错误页要带 `X-Content-Type-Options: nosniff`。
- `--origin` 写浏览器实际使用的 `https://` 地址，即 `https://moorlight.app`；只有 localhost 可以用 `http://`。订阅链接就建在这个地址上，CFM 只接受公网 HTTPS 域名或公网 IP。
- 收款时，`serve` 要加 `--payments <套餐文件>` 和 `--stripe-webhook-secret <密钥文件>`（两个一起给，`docs/payments.md` 第 3 节）。套餐文件只能让属主改写，密钥文件只能让属主读。一个数据库只装一种模式（测试或正式）的付款；数据库里还有没了结的付款时，不能去掉这两个参数启动（同一文档第 2.9 节）。Stripe 要能从公网访问 `/stripe/webhook`，Mooring 要能访问 `/api/mooring/entitlements`；代理把这两个地址原样转给门户，不改请求体（签名是对原始字节算的），不记录 `Authorization` 请求头。主机的时钟要准：签名时间与本机相差超过 5 分钟的通知会被拒绝。
- 数据库文件及其备份保持 0600，并当作节点凭据保管。删号和封禁抹不到已经做好的备份，备份要定期轮换。
- `serve` 收到 SIGINT 或 SIGTERM 后会处理完进行中的请求再退出（有测试）。

## 待定事项

以下事项需要运营者决定或去做；写明"已实现"的除外，代码里都还没有做：

1. **品牌**：已定为泊光 Moorlight（2026-11-06），以后要改只改 `brand.rs`。
2. **《使用约定》修订**（运营者 2026-11-06 同意修订；第 2 版的文本在 Mooring 仓库里）：门户会在联网主机上保存已绑定用户的编号、状态（暂停时含原因和时间）、用量、配额和到期时间、CFM 配置（含凭据），以及门户账号的邮箱和密码摘要；还引入了订阅链接。这与约定第 1 版第 2、4、5 条冲突，为真实用户绑定、导入真实配置包之前须先请用户确认第 2 版。第 2 版第 4 条承诺删除，并把两件事列为门户开放的前提：删除后数据库里不留副本，日志不点名账号；本分支已做到（`docs/security.md` 第 7、8 节）。运营者另定（2026-11-06）：所有用户都要用门户注册、绑定、付款，文件只在门户连不上时作备用；等付款做好再请用户确认第 2 版，在那之前门户不对用户开放。
3. **订阅链接**：已决定做（2026-11-06），本分支已实现，见 `docs/subscription-links.md`。
4. **邮箱验证或邀请码**：开放注册时，掌握大量网络的人能占满 `--max-accounts` 的注册名额（`docs/security.md` 第 12 节）。邮箱验证需要外部 SMTP 服务和凭据，没有它就无法自助找回密码，也挡不住用别人的邮箱注册；邀请码由运营者发放，不需要外部服务。按编号绑定已经保证了绑定不会因注册方式而出错。
5. **托管位置与域名**：主机已定为另一台英国 VPS，与运营者的 Mac 和伦敦节点分开（运营者 2026-11-06 决定）。服务商定为 Akamai（Linode），用伦敦机房的 Linode 2 GB（运营者 2026-11-12 决定，当天开通）；首选的 OVHcloud VPS-1 当天在伦敦缺货。选它有两个原因：它按小时计费，分到的地址从大陆连不上时可以删掉重建；它的备份可以不开（`docs/deploy.md` 第 2 节）。这台主机的服务商若控制了服务器，就能看到门户的数据，约定第 4 条会写明。域名定为 `moorlight.app`（运营者 2026-11-12 决定，当天在 Porkbun 注册）：`.app` 整个后缀在浏览器的 HSTS 预载名单里，第一次访问也只走 HTTPS；注册局是 Google；大陆的封锁按具体域名来，不按后缀。还没定的是要不要允许搜索引擎收录（现在是 `noindex`）。门户是公开网站，用户从大陆访问它这件事本身会被网络看到。
6. **支付**：已决定用 Stripe 在线收款，付款后由 Mooring 定时从门户拉取付款记录、自动开通；计费同时支持按月套餐和预付流量包，价格写在门户配置里（2026-11-06）。签约同时提供预付月和自动续费的月付；补偿与退款：运营者一侧（包括服务器被封锁）停机 8 小时及以上，补时长或流量，用户坚持时才按停机时长退款；因违约被暂停、作废或封禁的不退款（运营者 2026-11-06 决定）。只接受银行卡；到期满三个月没有续期的用户作废并删除门户账号；自动续费可以在门户上自助取消；这些和退款的细则见 `docs/payments.md` 第 1 节（运营者 2026-11-06 决定）。2026-11-11 运营者要求开通并接入付款：门户一侧已实现，见 `docs/payments.md`。开通还要运营者本人做这些事（同一文档第 3 节）：注册并验证 Stripe 账户，只留银行卡这一种付款方式，定价并为每个套餐建一个 Payment Link，建 Webhook 端点并把它的签名密钥放到门户主机上，写套餐文件，把门户部署到公网的 HTTPS 域名上，先在 Stripe 的测试模式里走一遍（走完用 `discard-test-payments` 清掉演练的记录，再换成正式模式）。Mooring 一侧来取清单、开通或续期的程序已经合入 Mooring（2026-11-11，契约 `docs/contract/entitlements-v1.md`）；按到期和配额自动暂停、恢复用户的每晚任务已经合入 Mooring（PR #46，2026-11-12），还没有部署。流量包只算进开通它的那个计费周期，没用完的不带到下个周期；已经开通的订单被全额退款时，收回它还没用到的时间和流量，已经用掉的不追回（运营者 2026-11-12 决定）。
7. **许可证**、证书与 DNS。
8. **Mooring 导出器**：已合并（Mooring PR #24，main `d478e91`）；《使用约定》第 2 版落地之前，导出命令对任何用户都拒绝。其中的配置包导出器写的是配置包契约第 1 版，要按第 2 版更新（见第 9 条）。
9. **配置包的范围**：已决定只导出已绑定用户（2026-11-06，运营者原话「只导出已绑定用户」）。配置包契约因此升为第 2 版（`docs/contract/cfm-profiles-v2.md`）：运营者在 Mooring 里点名全部已绑定用户，Mooring 不交付的写进 `withheld` 并注明原因（暂停、作废、未确认约定）；漏列的配置包整份拒绝。Mooring 导出器要按第 2 版改（契约第 7 节），改好之前门户不接受它写出的第 1 版配置包。
10. **暂停中的用户的订阅链接**：已决定（2026-11-06，运营者原话："额度用尽当然只暂停直到下一个计费周期"，"你用我的网从事非法活动被我抓了，我当然要封禁涉事账号"）。被暂停（包括超额、到期）或还没确认新版《使用约定》的已绑定用户，订阅链接只暂停，恢复交付后同一条链接照常「更新」；被作废的永久停用。恢复不是到点自动发生的：要等 Mooring 解除暂停，运营者再导出、导入一次配置包。按 Mooring 的设计，新计费周期的第一次 apply 会自动解除超额暂停（Mooring `docs/design.md` §13.6），这一步属于 M5，还没实现。Mooring 的作废不记原因，分不清违法被封和自己不用了，所以"封禁并不得重复注册"用门户的 `ban` 命令单独操作，不从配置包推断。它按规范化后的邮箱拦，换邮箱拦不住；封禁时还可以记下被封者付款用过的银行卡指纹的摘要（`ban --card`、`ban-cards`），用这些卡付的款不开通（运营者 2026-11-07 决定，`docs/payments.md` 第 1 节；已实现）；真正的关口是运营者绑定这一步（`docs/security.md` 第 10 节）。被封禁的人预付但没用完的时长或流量不退（运营者 2026-11-06 决定）。
11. **界面与首页的动画**：运营者 2026-11-10 要求把界面做得更好看，并说可以用 three.js；同日给了一张首页的设计图，要求界面照它做：星星要会眨，水要会动、有波光，灯塔里的光要会转，并且不要用贴图。首页已按设计图重排（顶栏、标题、两个按钮、两条事实、三步、页脚），全站配色随之统一；首页原有的服务介绍和四条承诺移到了订阅说明页。主图是一幅灯塔夜景，由着色器逐像素算出（WebGL 2），不加载任何图片；不开脚本时显示的是同一幅画的静态版本（SVG，由代码画出）。这是本站唯一的一段脚本，只有首页运行它，安全上的说明和代价见 `docs/security.md` 第 6 节。没有引入 three.js，理由也在那里。设计图里的文案照用了：导航和按钮里的“套餐”指向订阅说明页，那一页在门户没有配套餐文件时写的是“付款还没有开通”，配了之后列出套餐和价格（第 6 项）。场景只在 Chromium 里实际运行过。
12. **下载页跟随 CFM 的发布**：运营者 2026-11-10 要求 CFM 发布后下载页自己跟上，不能有人工步骤。已实现为"门户主机定时去取、门户校验后导入"，见 `docs/releases.md`。它的代价是没有人再为每个版本把关：GitHub 上标成最新的正式发布，几分钟内就出现在下载页上（`docs/releases.md` 第 3 节）。定时器要到部署时才装。发布后几秒内就更新的做法（由 CFM 的发布流程来推）没有做，那需要在 CFM 仓库的 CI 里放一把门户主机的密钥。
