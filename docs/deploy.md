# 部署：moorlight.app

门户还没有部署。本文写第一次部署的做法：

- 一台英国 VPS，系统 Debian 12 或 13；
- 域名 `moorlight.app`（运营者 2026-11-12 决定）；
- 同一台主机上的 Caddy 终止 TLS，再转给门户。

下面的单元文件和 Caddy 配置都还没有在真实主机上运行过。第 6 节的检查要在任何用户注册之前做完。

有三步会对外生效：配 DNS（第 3 节）；Caddy 启动后申请证书（第 5 节）；门户上线。

## 1. 做法

- **反向代理用 Caddy**。它自己申请、续期证书。配置里不写，它就不记访问日志，也不压缩。nginx 也能用，但证书要另装工具，而且它的错误日志默认会写下请求行，路径里就有订阅链接的令牌。
- **关掉 Caddy 的管理接口**（`admin off`）。这个接口默认开在本机 2019 端口上，本机任何账号都能通过它改 Caddy 的配置，比如让它记下每个请求的路径和登录表单。关掉之后，改了配置要用 `systemctl restart caddy`，会断开几秒；`systemctl reload caddy` 会报错。
- **不记请求**。Caddy 没有配置访问日志。转发失败时，它会记一条错误日志（`http.log.error`），里面有请求的路径和客户端地址，所以这类日志整个排除掉。门户是否在运行，看 `systemctl status mooring-portal` 和门户自己的日志。
- **只压缩 `/assets/`**。页面里有 CSRF 令牌，压缩会引入 BREACH 类侧信道（`docs/security.md` 第 6 节）；`/sub/` 不能压缩，因为 CFM 0.5.1 只接受 identity 编码。
- **客户端地址**。门户只听本机的 `127.0.0.1:8080`，以 `--behind-proxy` 启动，取 `X-Forwarded-For` 的最后一个地址，只用于限流，不记日志。客户端自己带来的 `X-Forwarded-For`，Caddy 不予理会，只写上它自己看到的地址。
- **开放之前限住注册**。启动时用 `--max-accounts 1`，只够运营者自己注册一个账号来试。门户在用户确认《使用约定》第 2 版之后才开放（README 待定事项第 2 条）；开放时去掉这个参数（默认上限 200），或者改成想要的上限。

## 2. 主机

- **系统**：Debian 12 或 13，x86_64 或 arm64。要在主机上构建的话，内存至少 2 GB。
- **防火墙**：只放行 TCP 22（SSH）、80 和 443。80 用于申请证书时的 HTTP 验证，以及把 http 转到 https。UDP 443 是 HTTP/3，可以不放行，浏览器会改用 HTTP/2。
- **时钟**：要准。Stripe 通知的签名时间与本机时钟相差超过 5 分钟，通知就会被拒绝。Debian 默认的 `systemd-timesyncd` 就够。

门户的账号和数据目录，只需建一次：

```bash
useradd --system --no-create-home --shell /usr/sbin/nologin mooring-portal
install -d -o mooring-portal -g mooring-portal -m 0700 /var/lib/mooring-portal
```

**构建和安装**：

- 门户用仓库固定的 Rust 1.99.1 构建，SQLite 跟着一起编译，所以要有 C 编译器。
- 在主机上构建最简单。也可以在另一台架构相同的 Linux 上构建，再把二进制拷过来。
- 下面的 `<主机>` 写 SSH 里的主机名。主机的地址不写进这个仓库，也不贴进聊天。

```bash
# 在 Mac 上：把要部署的提交打包，传到主机
git archive --format=tar.gz --output portal.tar.gz HEAD
scp portal.tar.gz <主机>:

# 在主机上，用普通账号（不用 root）
sudo apt install --yes build-essential curl
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
. "$HOME/.cargo/env"
mkdir portal && tar -xzf portal.tar.gz -C portal && cd portal
rustup toolchain install
cargo build --release --locked -p mooring-portal
sudo install -o root -g root -m 0755 target/release/mooring-portal /usr/local/bin/mooring-portal
```

`rustup toolchain install` 不带参数时，装的是仓库 `rust-toolchain.toml` 指定的版本。

## 3. DNS

- 在注册商的 DNS 设置里，给 `moorlight.app` 加一条 A 记录，指向主机的 IPv4 地址。主机有 IPv6 的话，再加一条 AAAA 记录。
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

最后一行要看到 `HTTP/1.1 200 OK`。不带 `X-Forwarded-For` 时，门户会回一个错误，并在日志里写"the reverse proxy did not send X-Forwarded-For"：它在 `--behind-proxy` 下只认代理写上的地址。

几处写法的原因：

- **`Restart=on-abnormal`**：门户被信号杀掉时（例如内存不足）会重启。它因为配置不对而拒绝启动时不重启，原因在 `systemctl status mooring-portal` 里能看到；这类原因有套餐文件的权限、付款模式不符、数据库里有没了结的付款等（`docs/payments.md` 第 2.9 节）。
- **权限**：数据库由门户以 0600 新建（`docs/security.md` 第 8 节），所在目录是 0700。
- **运营者命令**：用同一个账号运行，在终端里手动跑。它们的输出里有账号编号，不要让 cron 或 systemd 存下来（`docs/security.md` 第 7 节）：

```bash
sudo -u mooring-portal /usr/local/bin/mooring-portal --db /var/lib/mooring-portal/portal.db accounts
```

## 5. Caddy

- **安装**：装 Caddy 官方 apt 源里的 `caddy` 包，步骤见 Caddy 文档的安装页（`caddyserver.com/docs/install`，Debian、Ubuntu 一节）。Debian 自带的 `caddy` 包版本较旧。
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
- **开放给用户**：用户确认第 2 版之后，去掉 `--max-accounts 1`（或者改成想要的上限），再重启门户。
- **备份**：要做的话，备份文件保持 0600，并定期轮换。删号和封禁抹不到已经做好的备份（README"部署前提"）。
