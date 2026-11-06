# mooring-portal

Mooring 的用户门户，对外品牌为 **泊光 Moorlight**：夜里停泊的船上，那盏一直亮着的灯。品牌名、标语和释义只写在 `crates/web/src/brand.rs` 一个文件里；crate、二进制和路径都不带品牌名，改名只改这一个文件。

门户是一个服务端渲染、不依赖 JavaScript 的网站，供少数受信任的 Mooring 用户查看自己的账户和订阅：

- 公开页面：首页、下载（CFM 客户端）、订阅说明、帮助；
- 账号：注册、登录、退出、修改密码；中英文切换，默认中文；
- 账户页：账号编号、邮箱、注册时间、绑定状态；
- 订阅页：状态、本计费周期用量、配额、到期时间，数据来自 Mooring 导出的快照；
- 运营者命令：列出账号、按账号编号绑定或解绑 Mooring 用户、导入快照、删除账号。

门户不交付任何配置文件，不经手节点凭据，也不接受付款（Mooring 免费，见《Mooring 使用约定》第 1 条）。

## 当前状态

2026-11-06：v1 完成，只在本地运行过，没有部署。仓库只在本地，没有远程仓库。许可证尚未选定。

## 仓库结构

| 路径 | 职责 |
|---|---|
| `crates/core` | 纯领域逻辑：时间与日期、账号（邮箱、账号编号、密码策略）、Argon2id 与令牌、限流器、快照契约 |
| `crates/store` | SQLite 持久化：表结构与迁移、账号、会话、绑定、快照导入；文件以 0600 创建 |
| `crates/web` | HTTP 层（axum）：页面与模板（askama，编译期检查）、会话、CSRF、限流、安全响应头、中英文文案、品牌、样式与程序生成的主图 |
| `crates/portal` | 二进制 `mooring-portal`：`serve` 与运营者命令 |
| `contract/fixtures/v1/` | 快照契约的合成样例与金样 |
| `docs/contract/portal-snapshot-v1.md` | 快照契约规范，含 Mooring 侧导出器的映射建议 |
| `docs/security.md` | 安全设计、威胁模型与剩余风险 |
| `docs/subscription-links.md` | 订阅链接的威胁模型与方案（v1 不实现） |

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

工作区 lint 禁止 `unsafe`、`unwrap`、`expect`、`panic`、`print`。集成测试的入口文件用 `#[cfg(test)] mod` 声明各模块，以表明它们都是测试代码，`clippy.toml` 里对测试的放宽才能覆盖其中的辅助函数。

### 依赖

依赖从少：axum、tokio、askama、rusqlite（内置 SQLite）、argon2、blake2、getrandom、subtle、serde、thiserror、clap、tracing。版本取 crates.io 上**发布满 14 天**的最新兼容版本（2026-11-06 核对；因此 rusqlite 用 0.40.2，没有用 10 月下旬才发布的 0.41）。`cargo deny check` 要求：每个依赖只有一个版本；只用 Apache-2.0、BSD-3-Clause、MIT、Unicode-3.0 许可；只从 crates.io 获取；公告库中无未处理的漏洞、无人维护或不健全的 crate。

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

## 运营者命令

所有命令都在门户主机上运行，数据库就是 `serve` 用的那个文件，服务运行时也可以执行（SQLite WAL 模式，写操作在事务里串行；有测试）。数据库由 `serve` 第一次启动时创建；运营者命令要求文件已经存在，`--db` 写错时报 `there is no database at …`，不会新建一个空库。

| 命令 | 作用 |
|---|---|
| `accounts` | 列出账号：编号、注册时间（UTC）、绑定的 Mooring 用户、邮箱 |
| `bind --account <编号> --user <用户编号>` | 绑定。一个账号只能绑一个用户，反过来也一样 |
| `unbind --account <编号>` | 解绑，并删除该用户在门户上的快照记录 |
| `import <文件>` | 导入快照：只保留已绑定用户；报告未绑定的人数，以及快照里缺少的已绑定用户 |
| `delete-account --account <编号>` | 删除账号，连同会话、绑定和快照记录 |

绑定流程：用户注册后，在 FaceTime 语音通话中把账户页上的账号编号念给运营者；运营者**按编号绑定，不按邮箱绑定**（原因见 `docs/security.md` 第 2 节）。编号不区分大小写，字母 o、i、l 会按 0、1、1 读入。

退出码：0 成功，1 失败（错误写在 stderr，以 `mooring-portal:` 开头），2 用法错误。

## 部署前提（尚未部署）

- 门户只监听回环地址，由同机反向代理（如 Caddy、nginx）终止 TLS 后转发；代理须追加 `X-Forwarded-For`，门户以 `--behind-proxy` 启动。
- `--origin` 写浏览器实际使用的 `https://` 地址；只有 localhost 可以用 `http://`。
- 数据库文件及其备份保持 0600。
- `serve` 收到 SIGINT 或 SIGTERM 后会处理完进行中的请求再退出（有测试）。

## 待定事项

以下事项需要运营者决定，代码里都还没有做：

1. **品牌**：已定为泊光 Moorlight（2026-11-06），以后要改只改 `brand.rs`。
2. **《使用约定》修订**：门户会在联网主机上保存已绑定用户的编号、状态、用量、配额和到期时间，以及门户账号的邮箱和密码摘要。这些都不在约定第 1 版第 4 条的范围内，为真实用户绑定前须先修订约定。
3. **订阅链接**：v1 不做，见 `docs/subscription-links.md`。
4. **邮箱验证或邀请码**：开放注册时，掌握大量网络的人能占满 `--max-accounts` 的注册名额（`docs/security.md` 第 9 节）。邮箱验证需要外部 SMTP 服务和凭据，没有它就无法自助找回密码，也挡不住用别人的邮箱注册；邀请码由运营者发放，不需要外部服务。按编号绑定已经保证了绑定不会因注册方式而出错。
5. **托管位置与域名**：门户放在哪台主机；用什么域名；要不要允许搜索引擎收录（现在是 `noindex`）。门户是公开网站，用户从大陆访问它这件事本身会被网络看到。
6. **支付**：v1 不接。Mooring 现在承诺免费，不接受分摊；出口是运营者的个人手机线路。收费会涉及约定、运营商条款，还需要支付服务商的凭据。
7. **许可证**、远程仓库、证书与 DNS。
8. **Mooring 导出器**：按 `docs/contract/portal-snapshot-v1.md` 第 8 节在 Mooring 仓库单独提交。
