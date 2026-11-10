# 下载页跟随 CFM 的发布

## 1. 决定

运营者 2026-11-10 要求：CFM 发布新版本之后，门户的下载页要自己跟上，中间不能有人工步骤。此前下载页的版本、日期、下载地址和 SHA-256 写在代码里，CFM 0.5.1 在 2026-11-07 发布后，页面仍然显示 0.5.0。

以下是实现时的选择，不是运营者的原话：

- **门户主机定时去拉，而不是 CFM 的发布流程来推。** 门户主机上的定时器每 5 分钟向 GitHub 取一次 `billlza/cfw-rs` 的最新正式发布，交给门户的 `import-release` 命令。这样不需要任何密钥，门户也不用开一个接收推送的入口；代价是新版本最迟要过一个间隔才出现在页面上。
- **门户的程序自己不联网。** 取数据的是主机上的 `curl`，门户只读它存下的文件。门户的二进制里仍然没有任何发起网络连接的代码。
- **跟随 GitHub 上"最新发布"的标记，包括后退。** 如果运营者在 GitHub 上撤掉了最新的发布，"最新发布"变回上一个版本，下载页也跟着变回去，命令会写明这是一次后退。

要做到发布后几秒内更新，可以再让 CFM 的发布流程在最后主动触发一次导入。那需要在 CFM 仓库的 CI 里放一把能登录门户主机的密钥，这次没有做。

## 2. 门户读什么、信什么

`import-release <文件>` 读的是 GitHub REST API 对 `GET /repos/billlza/cfw-rs/releases/latest` 的回答，原样的 JSON。这份数据来自网络，门户不假定它是对的：

- 只接受正式发布：`draft` 和 `prerelease` 都必须是 `false`。
- `tag_name` 必须是 `v` 加三段数字（如 `v0.5.1`），不带预发布后缀。
- 发布里必须恰好有一个名为 `Clash.for.Mac_<版本>_arm64.dmg` 的文件，状态是 `uploaded`，大小不为 0，并带有 `sha256:` 开头、64 位小写十六进制的摘要。
- `published_at` 必须是 UTC 时间，精确到秒。
- `html_url` 和这个文件的 `browser_download_url`，必须与门户按版本号自己拼出来的地址逐字相同。
- 整份数据不超过 1 MiB。发布里的其他文件（更新包、签名、公证记录等）门户不看。

**页面上的地址不是从这份数据里抄的。** 门户只存三样：版本号、发布时间、磁盘映像的 SHA-256。发布页面和下载地址由版本号加上写死在代码里的仓库地址（`https://github.com/billlza/cfw-rs`）拼出来。所以无论拿到的数据错成什么样，下载页都不可能链接到别处。

任何一条不满足，整份数据被拒绝，下载页保持原样，命令以失败退出。拒绝的原因里不带数据中的任何文字：它们会进终端或者服务日志，而数据是外来的。

下载页上的发布日期是发布时间在北京时间里的那一天，与本站其他时间一致。

存下来的还有"上次核对的时间"：每次导入成功都更新它，即使版本没变。下载页写出这个时间；超过 24 小时没有核对成功，页面会提示这份信息可能不是最新的，请以发布页面为准。定时器坏了，页面上看得出来。

还没有导入过任何发布时，下载页如实写明还没有取得发布信息，并给出 GitHub 发布页面的链接；不显示某个写死的旧版本。

## 3. 这改变了什么

- **以前每次换版本都有一个人看过。** 版本写在代码里，改它要经过提交和门禁。现在 GitHub 上标成"最新"的正式发布，几分钟内就会出现在下载页上，没有人把关：这正是运营者要的。它的代价是，控制了 `billlza/cfw-rs` 仓库或其发布权限的人，能让门户向用户推荐他上传的文件。门户挡不住这一点；能挡住的是 GitHub 账号的保护（两步验证、发布权限）和 CFM 自己的签名与公证。下载页上"已用 Developer ID 签名并经 Apple 公证"是对正常发布流程的描述，门户没有验证它。
- **页面上的 SHA-256 现在来自 GitHub**，与下载的文件同一个来源。它能发现下载过程中的损坏，发现不了文件在 GitHub 上被换掉：那时摘要会跟着一起变。同一个版本的文件被换过时，命令会写明"重新发布"，在日志里能看到。
- **门户主机要能访问 `api.github.com`**（只出不进）。GitHub 对未登录的请求按来源地址限每小时 60 次，每 5 分钟一次用 12 次。

## 4. 部署

门户还没有部署（README"部署前提"）。部署时在门户主机上加一个服务和一个定时器，以运行门户的那个用户身份运行。下面的路径按实际安装位置改。

`/etc/systemd/system/mooring-portal-release.service`：

```ini
[Unit]
Description=Fetch the latest CFM release for the Mooring portal
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
User=mooring-portal
RuntimeDirectory=mooring-portal-release
RuntimeDirectoryMode=0700
ExecStart=/usr/bin/curl --fail --silent --show-error --location --max-time 30 --max-filesize 1048576 --header "Accept: application/vnd.github+json" --output %t/mooring-portal-release/latest.json https://api.github.com/repos/billlza/cfw-rs/releases/latest
ExecStart=/usr/local/bin/mooring-portal --db /var/lib/mooring-portal/portal.db import-release %t/mooring-portal-release/latest.json
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/mooring-portal
```

`/etc/systemd/system/mooring-portal-release.timer`：

```ini
[Unit]
Description=Check for a new CFM release every five minutes

[Timer]
OnBootSec=1min
OnUnitActiveSec=5min
RandomizedDelaySec=30s

[Install]
WantedBy=timers.target
```

```bash
systemctl enable --now mooring-portal-release.timer
```

两条 `ExecStart` 依次执行，`curl` 失败时不会运行导入。任何一步失败，这次运行记为失败，下载页不变；`systemctl status mooring-portal-release.service` 和 `journalctl -u mooring-portal-release.service` 能看到原因。连续失败超过 24 小时，下载页会自己提示。

这两个文件没有在真实主机上运行过。
