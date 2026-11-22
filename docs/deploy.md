# 部署：moorlight.app

2026-11-12 起，`https://moorlight.app` 已经能打开门户，但还没有对外开放：注册只限 10 个账号（2026-11-21 起；之前是 5 个，最早是 1 个；第 1 节），开放的条件见第 7 节。本文写第一次部署的做法：

- 一台 Akamai（Linode）伦敦机房的 Linode 2 GB，系统 Debian 13（运营者 2026-11-12 决定）；
- 域名 `moorlight.app`（运营者 2026-11-12 决定，当天在 Porkbun 注册）；
- 同一台主机上的 Caddy 终止 TLS，再转给门户。

2026-11-12 在这台主机上，第 2 节和第 4 节都按本文做完了：
- 门户服务和下载页的定时器都已运行，单元文件取自本文和 `docs/releases.md`，并通过了 `systemd-analyze verify`；
- 构建在主机上用了 3 分 47 秒，没有警告。

Caddy 配置还没有运行过。第 6 节的检查要在任何用户注册之前做完。

有三步会对外生效：配 DNS（第 3 节）；Caddy 启动后申请证书（第 5 节）；门户上线。

## 1. 做法

- **反向代理用 Caddy**。它自己申请、续期证书。配置里不写，它就不记访问日志，也不压缩。nginx 也能用，但证书要另装工具，而且它的错误日志默认会写下请求行，路径里就有订阅链接的令牌。
- **关掉 Caddy 的管理接口**（`admin off`）。这个接口默认开在本机 2019 端口上，本机任何账号都能通过它改 Caddy 的配置，比如让它记下每个请求的路径和登录表单。关掉之后，改了配置要用 `systemctl restart caddy`，会断开几秒；`systemctl reload caddy` 会报错。
- **不记请求**。Caddy 没有配置访问日志。转发失败时，它会记一条错误日志（`http.log.error`），里面有请求的路径和客户端地址，所以这类日志整个排除掉。门户是否在运行，看 `systemctl status mooring-portal` 和门户自己的日志。
- **只压缩 `/assets/`**。页面里有 CSRF 令牌，压缩会引入 BREACH 类侧信道（`docs/security.md` 第 6 节）；`/sub/` 不能压缩，因为 CFM 0.5.1 只接受 identity 编码。
- **客户端地址**。门户只听本机的 `127.0.0.1:8080`，以 `--behind-proxy` 启动，取 `X-Forwarded-For` 的最后一个地址，只用于限流，不记日志。客户端自己带来的 `X-Forwarded-For`，Caddy 不予理会，只写上它自己看到的地址。
- **开放之前限住注册**。启动时用 `--max-accounts 1`，只够运营者自己注册一个账号来试。门户在用户确认《使用约定》第 2 版之后才开放（README 待定事项第 2 条）；开放时去掉这个参数（默认上限 200），或者改成想要的上限。2026-11-14 运营者决定在开放前先让几个人试用，线上上限改成了 5；2026-11-21 运营者又把它改成了 10。正式付款用 `mooring-portal.service.d/50-live-payments.conf` 换掉了 unit 里的整行启动命令，生效的是那里的 `--max-accounts`；unit 和它两处现在都是 10，以后改上限也要两处一起改（`systemctl cat mooring-portal` 能同时看到两处），改完运行 `systemctl daemon-reload`，重启门户，再看运行中的进程用的是不是新的数（`/proc/<pid>/cmdline`）。2026-11-21 改之前的两个文件留在主机的 `/root/mooring-portal-units-20261121/`。

## 2. 主机

主机是 Akamai（Linode）伦敦机房的一台 Linode 2 GB（运营者 2026-11-12 决定）。首选的 OVHcloud VPS-1 当天在伦敦缺货。在 Cloud Manager 里创建时这样选：

- **区域、系统、套餐**：区域 London 2, UK（`gb-lon`），原来的 London（`eu-west`）新账号选不了；系统 Debian 13；套餐用 Shared CPU 里的 Linode 2 GB（1 核、2 GB 内存、50 GB 磁盘）。
- **SSH**：创建时加上门户专用的公钥 `~/.ssh/moorlight_portal_ed25519.pub`，以后只用钥匙登录（见下面的「第一次登录」）。
- **不开 Linode 的备份**。它会在机房另存整台机器的快照，门户删掉的账号会在快照里一直留到快照过期。而《使用约定》第 2 版说，删除门户账号时，门户上关于这个用户的记录随之删除。
- **防火墙**：用 Linode 的 Cloud Firewall，入站只放行 TCP 22（SSH）、80 和 443，其余丢弃。80 用于申请证书时的 HTTP 验证，以及把 http 转到 https。UDP 443 是 HTTP/3，可以不放行，浏览器会改用 HTTP/2。
- **先测大陆能不能连上**：配 DNS（第 3 节）之前，先从大陆测一下分到的地址，例如请大陆的用户或者用国内的多点测速网站，测 22 端口能不能连上。连不上就删掉这台，重建一台换个地址；Linode 按小时计费。
  - 2026-11-12 这一步是在 DNS 和证书之后才做的，测的是 `https://moorlight.app` 本身，用站长工具的国内测速（`tool.chinaz.com/speedtest`），连测两次：
    - 29 个大陆监测点里，19 个打开了首页（`200`），建立连接约 0.2 到 0.3 秒。
    - 解析出地址的点，拿到的都是这台主机的同一个地址，没有被污染。
    - 失败的 10 个点（超时或"系统异常"）连地址都没解析出来。同一时间测 `www.debian.org`，失败的基本是同一批点，所以是这些监测点自己的问题，不是主机或域名被封。
    - 监测点只有电信、联通和多线，没有移动，也没有测 IPv6。这只说明当时连得上，以后会不会被封是另一回事。
  - 移动另用 `ping.pe` 的 TCP 端口测试补测（只测 IPv4 能否建立连接，不经过大陆的 DNS）：
    - 电信、联通、腾讯云、阿里云的大陆节点，443 端口全部连上，约 0.2 到 0.3 秒；
    - 移动只有两个节点：丽水的 22、80、443 都能连上；江苏的三个端口都连不上，连测了四次。
    - 同一个江苏移动节点连 `www.debian.org` 的 443 是通的。所以是江苏移动到这台主机的地址不通，原因不清楚，可能是线路，也可能是省内的拦截。
    - 只有这一个节点，样本太少，说明不了整个移动网络。
  - ITDOG（移动的节点多）的安全验证在内置浏览器里一直不放行，没测成。
- **时钟**：要准。Stripe 通知的签名时间与本机时钟相差超过 5 分钟，通知就会被拒绝。Debian 默认的 `systemd-timesyncd` 就够。

**第一次登录**：在 Mac 的 `~/.ssh/config` 里给主机起个名字。主机的地址只写在这个文件里，不写进这个仓库，也不贴进聊天：

```text
Host moorlight-portal
	HostName <主机的地址>
	User root
	IdentityFile ~/.ssh/moorlight_portal_ed25519
	IdentitiesOnly yes
```

然后用 `ssh moorlight-portal` 登录。Linode 的镜像允许 root 用密码从 SSH 登录，而 22 端口对全网开着，所以登录后的第一件事是改成只许用钥匙登录：

```bash
printf '%s\n' 'PasswordAuthentication no' 'KbdInteractiveAuthentication no' 'PermitRootLogin prohibit-password' > /etc/ssh/sshd_config.d/00-keys-only.conf
sshd -t && systemctl reload ssh
sshd -T | grep -E '^(passwordauthentication|kbdinteractiveauthentication|permitrootlogin) '
```

最后一行的输出要满足三点：
- `passwordauthentication no`；
- `kbdinteractiveauthentication no`；
- `permitrootlogin` 后面是 `without-password` 或 `prohibit-password`。

文件名以 `00-` 开头，是因为 sshd 对同一项设置只认最先读到的值，这样能排在镜像自带的设置前面。这一步之后，root 密码只在 Linode 网页上的控制台（Lish）里用。

然后更新系统，并打开自动安全更新：

```bash
apt update && apt full-upgrade --yes
apt install --yes unattended-upgrades
apt-config dump | grep -E 'APT::Periodic::(Update-Package-Lists|Unattended-Upgrade) '
```

最后一行要看到这两项都是 `"1"`。不是的话，写上这两项：

```bash
printf '%s\n' 'APT::Periodic::Update-Package-Lists "1";' 'APT::Periodic::Unattended-Upgrade "1";' > /etc/apt/apt.conf.d/20auto-upgrades
```

内核更新要重启才生效，挑用户少的时候运行 `reboot`。

门户的账号和数据目录，只需建一次：

```bash
useradd --system --no-create-home --shell /usr/sbin/nologin mooring-portal
install -d -o mooring-portal -g mooring-portal -m 0700 /var/lib/mooring-portal
```

**构建和安装**：

- 门户用仓库固定的 Rust 1.99.1 构建，SQLite 跟着一起编译，所以要有 C 编译器。
- 在主机上构建最简单。也可以在另一台架构相同的 Linux 上构建，再把二进制拷过来。
- Linode 2 GB 的内存够构建：在只有 1 核的机器上，cargo 一次只跑一个编译任务。2026-11-12 在 Mac 上用 `cargo build --release -j1` 实测，最大的一个进程约用 550 MB 内存，用时不到 2 分钟。在 Linode 的 1 核上会慢一些。
- 编译不在 root 下进行：用一个没有 sudo 的账号 `builder`，Rust 工具链也装在它名下。
- 下面的 `moorlight-portal` 是第一次登录时在 `~/.ssh/config` 里起的主机名。

```bash
# 在 Mac 上：把要部署的提交打包，传到主机
git archive --format=tar.gz --output portal.tar.gz HEAD
scp portal.tar.gz moorlight-portal:/tmp/

# 在主机上，以 root 运行
apt install --yes build-essential curl
adduser --disabled-password --comment '' builder    # 只需一次
install -o builder -g builder -m 0644 /tmp/portal.tar.gz /home/builder/portal.tar.gz
su - builder

# 以 builder 运行
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
. "$HOME/.cargo/env"
rm -rf portal && mkdir portal && tar -xzf portal.tar.gz -C portal && cd portal
rustup toolchain install
cargo build --release --locked -p mooring-portal
exit

# 回到 root
install -o root -g root -m 0755 /home/builder/portal/target/release/mooring-portal /usr/local/bin/mooring-portal
```

`rustup toolchain install` 不带参数时，装的是仓库 `rust-toolchain.toml` 指定的版本。

**换版本**：按上面的步骤构建新的提交，装到 `/usr/local/bin/mooring-portal`，再运行 `systemctl restart mooring-portal`。门户收到停止信号后，先处理完手上的请求再退出；新版本启动时，自动补上数据库缺少的迁移（`crates/store/src/schema.rs`）。旧版本不认识新的迁移，遇到更新的数据库会拒绝启动，所以要换回旧版本，得先撤销新的迁移。存下的 CFM 发布信息也有同样的问题：`7ea4956` 及更早的程序读不懂带 build 号的版本，换回这样的程序要按 `docs/releases.md` 第 2 节"重新构建的版本"里的四步来。

2026-11-14 这样换上了提交 `5d96155`（加入 Mac 租赁，`docs/rentals.md`；运营者当天决定立即部署）：
- 换之前，线上跑的是 `af2e7ce` 的代码（2026-11-13 02:35 UTC 构建），数据库是第 5 版。`af2e7ce` 之后、Mac 租赁之前的提交只改了测试和文档，所以这次上线的只有 Mac 租赁。
- 在主机上构建用了 3 分 39 秒，没有警告。重启时服务中断约 0.2 秒；新版本把数据库迁到第 6 版，加上了 `mac_bookings` 表。
- 第 6 节的前三条重新查过，结果和以前一样；`/mac` 和其他公开页面都是 `200`。
- 旧的二进制留在主机的 `/root/mooring-portal-af2e7ce`。要换回它，先停掉门户，用门户的账号删掉 `mac_bookings` 表，并把 `user_version` 改回 5，否则它拒绝启动。

同一天又换上了提交 `7ea4956`（单账号导入 `import-profile`、反馈和运营页面；运营者当天决定部署）：
- 换之前线上跑的是 `5d96155`，数据库是第 6 版。`cfm_profiles` 是空的，第 7 版的迁移没有要搬的配置。
- 在主机上构建用了 4 分 9 秒，没有警告。重启时服务中断约 0.15 秒。新版本把数据库一次迁到第 8 版（两版各自的内容见下面两段）；迁移后 `quick_check` 是 `ok`，`foreign_key_check` 没有输出。
- 随后在主机上用 `grant-operator` 把运营者自己的账号设为运营者。
- 第 6 节的前三条重新查过，结果和以前一样。公开页面都是 `200`；没登录时打开 `/feedback` 和 `/operator`，都转到登录页；没签名的 Stripe 通知得到 `400`，不带令牌的拉取得到 `401`。
- 旧的二进制留在主机的 `/root/mooring-portal-5d96155`。要换回它，先停掉门户，按下面两段先撤销第 8 版、再撤销第 7 版，然后用它运行一次 `erase-deleted`，再启动门户。
- 主机上没有装 `sqlite3` 命令行。撤销时可以用系统自带的 `python3` 里的 `sqlite3` 模块执行这些语句，也可以先 `apt install sqlite3`。

同一天又换上了提交 `d103dc0`（下载页跟上 CFM 的重新构建，运营页面的概况加上"下载页上的 CFM"一格；运营者当天决定部署）：
- 换之前线上跑的是 `7ea4956`。从 2026-11-12 19:05 UTC 起，每次导入 CFM 的发布都失败：CFM 最新发布的标签是 `v0.5.1-build.51006`，`7ea4956` 不认（`docs/releases.md` 第 2 节"重新构建的版本"）。这次没有新的迁移，数据库仍是第 8 版。
- 在主机上构建用了 3 分 52 秒，没有警告。重启时服务中断约 0.12 秒；之后 `quick_check` 是 `ok`。
- 重启后立刻手动导入了一次（`systemctl start mooring-portal-release.service`）。下载页从 11-07 的 0.5.1 换成"0.5.1（build 51006）"，发布页面和磁盘映像的地址都在 `v0.5.1-build.51006` 这个标签下，GitHub 上这个下载地址能用。定时器的下一次导入用的是新程序，也成功了。
- 第 6 节的前三条重新查过，结果和以前一样。公开页面都是 `200`；没登录时打开 `/operator`，转到登录页；没签名的 Stripe 通知得到 `400`，不带令牌的拉取得到 `401`。
- 旧的二进制留在主机的 `/root/mooring-portal-7ea4956`。库里存的版本现在带 build 号，它读不懂，所以要换回它，按 `docs/releases.md` 第 2 节"重新构建的版本"里的四步来；数据库结构没变，不用撤销迁移。

2026-11-15 换上了提交 `91b72cb`（右下角的反馈框改成实时聊天，加上新消息提醒，`docs/notices.md`；运营者当天决定部署）：
- 换之前线上跑的是 `d103dc0`。这次没有新的迁移，数据库仍是第 8 版。
- 在主机上构建用了 3 分 58 秒，没有警告。从发出重启到门户第一次答复用了 1.18 秒：curl 第一次没连上，隔 1 秒重试，所以服务中断不超过这么久。之后 `quick_check` 是 `ok`。
- 两处启动命令（第 4 节的单元，和 `50-live-payments.conf`）末尾都加上了 `--new-message-signal /var/lib/mooring-portal/new-message`；`docs/notices.md` 的脚本、两个单元和这个文件也装好了。path 单元还没有启用，等运营者建好机器人、放好令牌（`docs/notices.md` 第 1、2 步）再启用，并发一条试的提醒。在那之前，用户的留言照常存下、照常显示，只是没有提醒。
- 第 6 节的前三条重新查过，结果和以前一样。公开页面都是 `200`。没登录时打开 `/operator` 和 `/feedback`，都转到登录页；不带页面令牌请求 `/feedback/unread` 和 `/feedback/messages`，得到 `403`。没签名的 Stripe 通知得到 `400`，不带令牌的拉取得到 `401`。聊天脚本以 `text/javascript` 提供。登录后的页面没有在线上看过（没有任何账号的密码），只在本机的演示里看过。
- 旧的二进制留在主机的 `/root/mooring-portal-d103dc0`，两份启动配置改动前的样子留在 `/root/mooring-portal.service.before-91b72cb` 和 `/root/50-live-payments.conf.before-91b72cb`。`d103dc0` 不认识 `--new-message-signal`，带着它会拒绝启动。所以要换回它：先 `systemctl disable --now mooring-portal-notify.path`（如果已经启用），把这两份配置放回原处（或者从两处启动命令里删掉这个参数），`systemctl daemon-reload`，再换回二进制、重启。数据库结构没变，不用撤销迁移。

2026-11-19 换上了提交 `8959a7d`（首页主图的远岸改为三层山，`docs/security.md` 第 6 节；运营者当天决定部署）：
- 换之前线上跑的是 `91b72cb`。这次只改了主图和场景怎么画，没有新的迁移，也没有改启动参数，数据库仍是第 8 版。
- 在主机上构建用了 4 分 2 秒，没有警告。从发出重启到门户第一次答复用了 1.13 秒：curl 第一次没连上，隔 1 秒重试；门户的日志里，从收到停止信号到重新开始服务约 0.16 秒。之后 `quick_check` 是 `ok`。
- 首页引用的场景脚本目录从 `js.de2406d35149bbff` 换成了 `js.77b3499e55b1da48`，主图从 `hero.25e716ea4ec988ae.svg` 换成了 `hero.81d0ff863098f145.svg`（138201 字节，三层山都在）。线上的八个场景脚本和提交里的逐字节相同；旧的目录已经是 `404`。
- 第 6 节的前三条重新查过，结果和以前一样。公开页面（包括 `/mac`）都是 `200`。没登录时打开 `/operator` 和 `/feedback`，都转到登录页；不带页面令牌请求 `/feedback/unread` 和 `/feedback/messages`，得到 `403`。没签名的 Stripe 通知得到 `400`，不带令牌的拉取得到 `401`。聊天脚本仍以 `text/javascript` 提供。
- 线上只在内置浏览器里看过静态主图（浏览器面板隐藏时场景不启动）；会动的场景只在本机的 Chromium（M1 Max）里看过。Safari、Firefox、手机和集成显卡都还没看过。
- 旧的二进制留在主机的 `/root/mooring-portal-91b72cb`。启动参数和数据库结构都没变，要换回它，换回二进制、重启就行。

数据库第 7 版（每份 CFM 配置记着自己配置包的生成时间，`import-profile` 要用；分支 `feature/one-account-profile`，2026-11-14 随 `7ea4956` 部署）：
- 迁移时 `cfm_profiles` 要么是空的，要么每份配置都有绑定、`profile_bundle` 也有时间。否则迁移失败、门户不启动，数据库仍是第 6 版，不受影响。
- 要撤销第 7 版，先停掉门户，用门户的账号在数据库上执行 `ALTER TABLE cfm_profiles DROP COLUMN generated_at;`，再把 `user_version` 改回 6。只改 `user_version` 不够：第 6 版的程序照样启动，但每次 `import-profiles` 都会因为 `generated_at` 不能为空而失败。这两点 2026-11-14 在按迁移建出的测试库上核对过。

数据库第 8 版（运营者名单、会话确认密码的时间、反馈；分支 `feature/feedback-and-operator-pages`，接在第 7 版之后，2026-11-14 随 `7ea4956` 部署）：
- 迁移只新建两张表、给 `sessions` 加一列，不改已有的行，线上的数据不会让它失败。
- 部署后在主机上用 `grant-operator --account <编号>` 把运营者自己的账号设为运营者，网页上才有"运营"。
- 要撤销第 8 版，先停掉门户，用门户的账号在数据库上执行 `DROP TABLE feedback; DROP TABLE operators; ALTER TABLE sessions DROP COLUMN confirmed_at; PRAGMA user_version = 7;`。反馈和运营者名单随之删除，会话保留。执行后的结构和第 7 版完全相同（`crates/store/src/tests/operators.rs` 里有测试）。删表之后，反馈的原文还可能留在数据库文件的空闲页或预写日志里（没开 `secure_delete` 时留在空闲页；主机上的 SQLite 默认开着，但改动先写进预写日志，旧的内容要等日志并回数据库才被盖掉）：换回旧的程序后、启动门户之前，用它运行一次 `erase-deleted`，它重整数据库文件、清空预写日志，把这些内容从文件里抹掉。

数据库第 9 版（留言的译文，`docs/translation.md`；分支 `feature/host-translation`，接在第 8 版之后，还没有部署）：
- 迁移只新建两张表（`feedback_examined`、`feedback_translations`），不改已有的行，线上的数据不会让它失败。
- 部署这一版本身不会让页面上出现译文：主机上还没有翻译服务，也没有运行 `translate` 的单元（`docs/translation.md` 第 4 节）。
- 要撤销第 9 版，先停掉门户和运行 `translate` 的单元，用门户的账号在数据库上执行 `DROP TABLE feedback_translations; DROP TABLE feedback_examined; PRAGMA user_version = 8;`。译文和"问过了"的记录随之删除，留言保留。执行后的结构和第 8 版完全相同（`crates/store/src/tests/translations_schema.rs` 里有测试）；要接着撤到第 7 版，先做这一步，再做上面第 8 版那一步（也有测试）。删表之后，译文还可能留在数据库文件的空闲页或预写日志里：和第 8 版一样，换回旧的程序后、启动门户之前，用它运行一次 `erase-deleted`。

## 3. DNS

- Porkbun 注册时自动加了两条记录，把 `moorlight.app` 和它的所有子域名都指向 Porkbun 的停放页（一条 `ALIAS`、一条 `*` 的 `CNAME`，目标都是 `pixie.porkbun.com`）。先删掉这两条，否则 `www` 这类子域名会指向 Porkbun。
- 在注册商 Porkbun 的 DNS 设置里，给 `moorlight.app` 加一条 A 记录，指向主机的 IPv4 地址。主机有 IPv6 的话，再加一条 AAAA 记录。
- 2026-11-12 照这两条做完：两条默认记录已删除；A 和 AAAA 记录都已加上，TTL 600 秒；Porkbun 的四台权威服务器和三个公共解析器返回的地址，都与主机的地址相同。
- Porkbun 账号能改这些记录。控制了这个账号的人，能把域名指到自己的服务器，再申请到这个域名的证书，用户的浏览器看不出区别。所以登录这个账号，不能只靠密码。
  - 2026-11-12 运营者决定，先只用 Porkbun 的「新设备登录发邮件验证码」（已开）：从没用过的设备登录，要输入发到账号邮箱的验证码。用验证器 App 的两步验证以后再开。
  - 这样一来，账号邮箱就是最后一道门，邮箱自己要开两步验证。
- 不要 `www`。
- 第 5 节启动 Caddy 之前，这两条记录要已经生效：在别的电脑上运行 `dig +short moorlight.app`，要能查到主机的地址。

## 4. 门户服务

`/etc/systemd/system/mooring-portal.service`：

```ini
[Unit]
Description=Serve the Mooring portal
Wants=time-sync.target
After=time-sync.target

[Service]
Type=exec
User=mooring-portal
ExecStart=/usr/local/bin/mooring-portal --db /var/lib/mooring-portal/portal.db serve --listen 127.0.0.1:8080 --origin https://moorlight.app --behind-proxy --max-accounts 1
Restart=on-abnormal
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/mooring-portal

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload
systemctl enable --now mooring-portal.service
curl --silent --head --header 'X-Forwarded-For: 127.0.0.1' http://127.0.0.1:8080/ | head -n 1
```

最后一行要看到 `HTTP/1.1 200 OK`。在 `--behind-proxy` 下，门户只认代理写上的地址：
- 要按地址限流的请求不带 `X-Forwarded-For` 时，门户回 500，并在日志里写"the reverse proxy did not send X-Forwarded-For"。这类请求有登录、注册、订阅链接，以及 Stripe 和 Mooring 用的两个地址。
- 首页这类请求用不到地址，不受影响。

几处写法的原因：

- **`Restart=on-abnormal`**：门户被信号杀掉时（例如内存不足）会重启。它因为配置不对而拒绝启动时不重启，原因在 `systemctl status mooring-portal` 里能看到；这类原因有套餐文件的权限、付款模式不符、数据库里有没了结的付款等（`docs/payments.md` 第 2.9 节）。
- **权限**：数据库由门户以 0600 新建（`docs/security.md` 第 8 节），所在目录是 0700。
- **运营者命令**：用同一个账号运行，在终端里手动跑。它们的输出里有账号编号，不要让 cron 或 systemd 存下来（`docs/security.md` 第 7 节）：

```bash
runuser -u mooring-portal -- /usr/local/bin/mooring-portal --db /var/lib/mooring-portal/portal.db accounts
```

## 5. Caddy

- **安装**：装 Caddy 官方 apt 源里的 `caddy` 包，步骤见 Caddy 文档的安装页（`caddyserver.com/docs/install`，Debian、Ubuntu 一节）。Debian 自带的 `caddy` 包版本较旧。Debian 13 的最小安装没有 `gpg`，导入签名密钥的 `--dearmor` 要用它，所以先装上；安装页列出的 `debian-keyring` 和 `apt-transport-https` 用不到。这个源的签名密钥指纹是 `6576 0C51 EDEA 2017 CEA2  CA15 155B 6D79 CA56 EA34`。
- 装好后，它作为 `caddy` 服务、以 `caddy` 账号运行，证书存在 `/var/lib/caddy` 下面。

`/etc/caddy/Caddyfile`：

```text
{
	admin off
	log default {
		exclude http.log.error
	}
}

moorlight.app {
	@assets path /assets/*
	encode @assets zstd gzip

	reverse_proxy 127.0.0.1:8080

	handle_errors {
		header X-Content-Type-Options nosniff
		header Content-Type "text/plain; charset=utf-8"
		respond "{err.status_code} {err.status_text}"
	}
}
```

```bash
systemctl restart caddy
```

**证书**：

- 重启之后，Caddy 马上去申请 `moorlight.app` 的证书：先向 Let's Encrypt 申请，不成再试 ZeroSSL。验证要用到 80 或 443 端口，所以第 3 节的记录要先生效。
- 证书由 Caddy 自己续期。
- 申请证书用的邮箱不填。Let's Encrypt 已经不发到期提醒，填了只是多交出一个联系方式。
- 2026-11-12 照本节做完：装的是 Caddy 2.11.9；Let's Encrypt 用 tls-alpn-01 验证之后签发了证书，证书里只有 `moorlight.app`，有效期 90 天；ACME 账号没有联系邮箱。

**日志里预期会有的几行**：

- Caddy 每次启动都记三行 `warn`：
  - `admin endpoint disabled`，因为 `admin off`；
  - 80 端口上的 `HTTP/2 skipped because it requires TLS` 和 `HTTP/3 skipped because it requires TLS`。80 端口只做跳转到 HTTPS，这两种协议要 TLS。
- 重启时，退出的旧进程还会记一行 `exiting`。
- 申请和续期证书时，日志会记下 Let's Encrypt 验证服务器的地址（`served key authentication certificate`），那不是用户的地址。

几处写法的原因：

- **`admin off`、`exclude http.log.error`、只压缩 `/assets/`**：见第 1 节。
- **`handle_errors`**：门户没在运行时，Caddy 自己回 502。这样它的错误页也带 `nosniff`，内容是固定的一行字。门户自己的错误页（404 等）由门户返回，Caddy 原样转发，不经过这里。
- **没有 `log`**：所以 Caddy 不记访问日志。
- **HTTP/3**：默认开着（UDP 443），防火墙不放行也没关系。
- **有测试核对**（`crates/portal/src/tests/deploy.rs`）：
  - 这里的站点名就是门户的 `--origin`，转发的地址就是门户的 `--listen`；
  - 站点里没有 `log`，`encode` 只用于 `/assets/`；
  - 第 4 节和第 7 节的两行 `ExecStart` 能被门户的命令行解析。

## 6. 让用户进来之前的检查

在主机以外的电脑上运行。前三条都用 GET（`--output /dev/null --dump-header -` 只打印响应头），查的是浏览器真正拿到的响应。

1. **页面不压缩，安全头都在**：

   ```bash
   curl --silent --output /dev/null --dump-header - --header 'Accept-Encoding: gzip, zstd' https://moorlight.app/
   ```

   要看到：
   - `200`；
   - `content-security-policy`、`strict-transport-security`、`x-content-type-options: nosniff`；
   - 没有 `content-encoding`。

2. **固定的资源压缩**：

   ```bash
   curl --silent --output /dev/null --dump-header - --header 'Accept-Encoding: gzip' https://moorlight.app/assets/site.css
   ```

   要看到 `content-encoding: gzip`。

3. **订阅链接的回答不压缩**：

   ```bash
   curl --silent --output /dev/null --dump-header - --header 'Accept-Encoding: gzip' https://moorlight.app/sub/check-the-proxy
   ```

   要看到门户自己的回答（没有这个链接，所以是 404），没有 `content-encoding`。

4. **门户停着时**，看 Caddy 的错误页和日志。先在主机上停掉门户：

   ```bash
   systemctl stop mooring-portal
   ```

   在别的电脑上请求一次：

   ```bash
   curl --silent --include https://moorlight.app/sub/check-the-proxy-log
   ```

   回到主机，查 Caddy 的日志，再启动门户：

   ```bash
   journalctl -u caddy --since '10 min ago' --no-pager | grep -c -E 'check-the-proxy-log|remote_ip|client_ip'
   systemctl start mooring-portal
   ```

   要看到：
   - 请求得到 `502`，并且带 `x-content-type-options: nosniff`；
   - `grep -c` 输出 `0`：日志里既没有这条路径，也没有客户端地址。

   不是 `0` 的话，先不要让用户进来，改好 Caddy 的配置再查一遍。

2026-11-12 在运营者的 Mac 上跑过这四条，结果都符合：
- 首页 `200`，三个头都在，没有压缩；
- CSS 用了 `gzip`；
- 订阅链接得到门户自己的 `404`，没有压缩；
- 门户停着时得到带 `nosniff` 的 `502`，日志里匹配 0 行。

另外，80 端口的请求以 `308` 转到 HTTPS；首页经 IPv4 和 IPv6 都打得开。

## 7. 之后

- **下载页**：定时器和两个服务见 `docs/releases.md` 第 4 节。
- **收款**：先按 `docs/payments.md` 第 3 节在 Stripe 那边建好，然后把套餐文件和签名密钥放到主机上：

  ```bash
  install -d -o root -g root -m 0755 /etc/mooring-portal
  install -o root -g root -m 0644 catalogue.json /etc/mooring-portal/catalogue.json
  install -o mooring-portal -g mooring-portal -m 0600 stripe-webhook-secret /etc/mooring-portal/stripe-webhook-secret
  ```

  再把单元文件里的 `ExecStart` 换成下面这一行，运行 `systemctl daemon-reload`，然后 `systemctl restart mooring-portal`：

  ```ini
  ExecStart=/usr/local/bin/mooring-portal --db /var/lib/mooring-portal/portal.db serve --listen 127.0.0.1:8080 --origin https://moorlight.app --behind-proxy --max-accounts 1 --payments /etc/mooring-portal/catalogue.json --stripe-webhook-secret /etc/mooring-portal/stripe-webhook-secret
  ```

  这两个文件的权限要求：
  - 套餐文件只有属主能改（这里属主是 root）；
  - 签名密钥只有属主能读（这里属主是门户的账号）。

  不符合时门户拒绝启动。

- **Mooring**：
  - 在门户主机上用 `pull-token` 生成拉取令牌，令牌只显示这一次（`docs/payments.md` 第 3 节第 8 步）；
  - 在运营者的 Mac 上运行 `mooringctl portal connect --origin https://moorlight.app`，把令牌存进 Mooring。
- **更新门户**：按第 2 节重新构建、安装，然后运行 `systemctl restart mooring-portal`。
- **开放给用户**：用户确认第 2 版之后，去掉 `--max-accounts`（或者改成想要的上限；两处都要改，见第 1 节），再重启门户。开放之前，先请一位用大陆移动网络的用户（最好在江苏）直接打开门户。2026-11-12 测到江苏移动连不上这台主机（第 2 节），运营者当天决定先不换地址，开放前再测。
- **备份**：要做的话，备份文件保持 0600，并定期轮换。删号和封禁抹不到已经做好的备份（README"部署前提"）。

## 8. 新消息提醒

用户给运营者留言时，主机可以提醒到运营者的 Telegram 和飞书。怎么装、各自知道什么、令牌和地址放在哪，都在 `docs/notices.md`（2026-11-21 从这一节搬过去，本文不再重复）。门户这边只有一处相关：两处启动命令末尾的 `--new-message-signal /var/lib/mooring-portal/new-message`（第 2 节 2026-11-15 的记录）。
