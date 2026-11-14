# 下载页跟随 CFM 的发布

## 1. 决定

运营者 2026-11-10 要求：CFM 发布新版本之后，门户的下载页要自己跟上，中间不能有人工步骤。此前下载页的版本、日期、下载地址和 SHA-256 写在代码里，CFM 0.5.1 在 2026-11-07 发布后，页面仍然显示 0.5.0。

以下是实现时的选择，不是运营者的原话：

- **门户主机定时去拉，而不是 CFM 的发布流程来推。** 门户主机上的定时器每 5 分钟向 GitHub 取一次 `billlza/cfw-rs` 的最新正式发布，交给门户的 `import-release` 命令。这样不需要任何密钥，门户也不用开一个接收推送的入口；代价是新版本最迟要过一个间隔才出现在页面上。
- **门户的程序自己不联网。** 取数据的是主机上的 `curl`，门户只读它存下的文件。门户的源码里没有一处发起网络连接，依赖里也没有 TLS 库。取数据的进程用另一个账号运行，读不到数据库（第 4 节）：从网络上读字节的进程，不该拿着存有全部节点凭据的那个文件。
- **跟随 GitHub 上"最新发布"的标记，包括后退。** 如果运营者在 GitHub 上撤掉了最新的发布，"最新发布"变回上一个版本，下载页也跟着变回去，命令会写明这是一次后退。

要做到发布后几秒内更新，可以再让 CFM 的发布流程在最后主动触发一次导入。那需要在 CFM 仓库的 CI 里放一把能登录门户主机的密钥，这次没有做。

## 2. 门户读什么、信什么

`import-release <文件>` 读的是 GitHub REST API 对 `GET /repos/billlza/cfw-rs/releases/latest` 的回答，原样的 JSON。这份数据来自网络，门户不假定它是对的：

- 只接受正式发布：`draft` 和 `prerelease` 都必须是 `false`。
- `tag_name` 必须是 `v` 加三段数字（如 `v0.5.1`）。CFM 重新构建同一个版本时，标签后面加 build 号（如 `v0.5.1-build.51006`），这种也接受（见下面"重新构建的版本"）。别的后缀（如 `-rc.1`）一律拒绝。
- 发布里必须恰好有一个名为 `Clash.for.Mac_<三段数字>_arm64.dmg` 的文件，状态是 `uploaded`，大小不为 0，并带有 `sha256:` 开头、64 位小写十六进制的摘要。重新构建的发布也一样，文件名里没有 build 号。
- `published_at` 必须是 UTC 时间，精确到秒。
- `html_url` 和这个文件的 `browser_download_url`，必须与门户按完整的标签（连同 build 号）自己拼出来的地址逐字相同。
- 整份数据不超过 1 MiB。同一个键写了两次、或者对象后面还跟着别的内容，都按格式错误拒绝。
- 发布里的其他文件（更新包、签名、公证记录等）门户不看：只读名字对得上的那一项，其余各项里写了什么、甚至是不是一项，都不影响结果。

**页面上的地址不是从这份数据里抄的。** 门户只存三样：版本号、发布时间、磁盘映像的 SHA-256。发布页面和下载地址由版本号加上写死在代码里的仓库地址（`https://github.com/billlza/cfw-rs`）拼出来。所以无论拿到的数据错成什么样，下载页都不可能链接到别处。

任何一条不满足，整份数据被拒绝，下载页保持原样，命令以失败退出。拒绝的原因里不带数据中的任何文字：它们会进终端或者服务日志，而数据是外来的。

下载页上的发布日期是发布时间在北京时间里的那一天，与本站其他时间一致。

**重新构建的版本**（运营者 2026-11-14 决定接受）：

- CFM 在 2026-11-12 发布了 `v0.5.1-build.51006`，是 0.5.1 的重新构建：磁盘映像换了，文件名没变，放在新的标签下。
- 门户当时只认三段数字，从 19:05 UTC 起每次导入都失败。下载页一直停在 11-07 的 0.5.1 上，过了 24 小时开始提示信息可能不是最新的。这件事到 2026-11-14 部署运营页面时才发现，当天随 `d103dc0` 修好并部署（`docs/deploy.md` 第 2 节），下载页换上了这次重新构建。
- 现在版本号连同 build 号一起存，发布页面和下载地址按完整的标签拼。页面上写成"0.5.1（build 51006）"，和 CFM 给这个发布起的名字一致。
- 先后按数字比：重新构建排在它所重建的版本之后，build 号大的在后，再往后才是下一个版本（0.5.1 < 0.5.1-build.51006 < 0.5.2）。所以从 0.5.1 换到它的重新构建，命令报告的是换上了新版本，不是同一版本的文件被换了。
- 此前的门户程序（`7ea4956` 及更早）读不懂库里带 build 号的版本：下载页会出错，导入也一直失败。要换回这样的程序，按这个顺序：
  1. 先停掉下载页的定时器，连同可能正在跑的那一轮导入：`systemctl stop mooring-portal-release.timer mooring-portal-release.service mooring-portal-release-fetch.service`。它们不随门户服务停下；不停的话，删掉的那一行会被新程序重新导入，换回旧程序之后，旧程序既读不懂它，也盖不掉它。
  2. 按第 4 节末尾删掉存下的那一行。
  3. 换回旧程序，重启门户。
  4. 再启动定时器：`systemctl start mooring-portal-release.timer`。旧程序导入不了带 build 号的发布，下载页会写明还没有取得发布信息。

存下来的还有"上次核对的时间"：每次导入成功都更新它，即使版本没变。下载页写出这个时间；最近 24 小时内没有核对成功的记录时，页面会提示这份信息可能不是最新的，请以发布页面为准。核对时间比现在晚 5 分钟以上也算在内：那说明记下它的时候主机的时钟不对，这条记录说明不了信息有多旧。定时器停了，一天之内页面上看得出来。

命令只知道文件里写了什么。它报告的"仍是描述里的那个版本""描述里的版本比页面上的旧"，说的都是这份文件，不是 GitHub 此刻的状态。

还没有导入过任何发布时，下载页如实写明还没有取得发布信息，并给出 GitHub 发布页面的链接；不显示某个写死的旧版本。

## 3. 这改变了什么

- **以前每次换版本都有一个人看过。** 版本写在代码里，改它要经过提交和门禁。现在 GitHub 上标成"最新"的正式发布，几分钟内就会出现在下载页上，没有人把关：这正是运营者要的。它的代价是，控制了 `billlza/cfw-rs` 仓库或其发布权限的人，能让门户向用户推荐他上传的文件。门户挡不住这一点；能挡住的只有 GitHub 账号的保护（两步验证、发布权限）。签名与公证帮不了第一次下载的人：它们说明的是"这个文件由某个 Apple 开发者签名、经 Apple 扫描、之后没有被改动"，说明不了签名的是不是 CFM 的开发者，换一个开发者签名的文件同样通得过。下载页上"已用 Developer ID 签名并经 Apple 公证"是对正常发布流程的描述，门户没有验证它。
- **页面上的 SHA-256 现在来自 GitHub**，与下载的文件同一个来源。它能发现下载过程中的损坏，发现不了文件在 GitHub 上被换掉：那时摘要会跟着一起变。同一个版本的磁盘映像或发布时间与上次导入的不同时，命令会写明这份描述变了，在日志里能看到（第 4 节）。
- **门户主机要能访问 `api.github.com`**（只出不进）。GitHub 对未登录的请求按来源地址限每小时 60 次，每 5 分钟一次用 12 次。

## 4. 部署

部署时在门户主机上加两个服务和一个定时器。2026-11-12 已在门户主机上照本节装好（`docs/deploy.md`）：第一次运行把 CFM 0.5.1 放上了下载页。取数据和导入分开，各用一个账号：

- 取数据的服务从网络上读字节，所以用一个专门的账号 `mooring-release` 运行。这个账号什么也不拥有，读不到数据库。
- 导入的服务以运行门户的账号运行，能写数据库，但没有网络（`PrivateNetwork=yes`）。

先建那个账号，只需一次：

```bash
useradd --system --no-create-home --shell /usr/sbin/nologin mooring-release
```

下面的路径按实际安装位置改。

`/etc/systemd/system/mooring-portal-release-fetch.service`：

```ini
[Unit]
Description=Fetch GitHub's description of the latest CFM release
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
User=mooring-release
RuntimeDirectory=mooring-portal-release
RuntimeDirectoryPreserve=yes
TimeoutStartSec=120
LimitFSIZE=1M
ExecStart=/usr/bin/curl --fail --silent --show-error --proto =https --max-time 30 --max-filesize 1048576 --header "Accept: application/vnd.github+json" --output %t/mooring-portal-release/latest.json https://api.github.com/repos/billlza/cfw-rs/releases/latest
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
InaccessiblePaths=-/var/lib/mooring-portal
```

`/etc/systemd/system/mooring-portal-release.service`：

```ini
[Unit]
Description=Put the latest CFM release on the Mooring portal's download page
Requires=mooring-portal-release-fetch.service
After=mooring-portal-release-fetch.service time-sync.target
Wants=time-sync.target

[Service]
Type=oneshot
User=mooring-portal
TimeoutStartSec=120
ExecStart=/usr/local/bin/mooring-portal --db /var/lib/mooring-portal/portal.db import-release /run/mooring-portal-release/latest.json
NoNewPrivileges=yes
PrivateNetwork=yes
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
OnCalendar=*:0/5
RandomizedDelaySec=30s

[Install]
WantedBy=timers.target
```

```bash
systemctl enable --now mooring-portal-release.timer
```

定时器启动的是导入服务，导入服务要求先取数据（`Requires=`）；取数据失败时导入不会运行。任何一步失败，这一次记为失败，下载页不变。几处写法的原因：

- `RuntimeDirectoryPreserve=yes`：取数据的服务结束后，`/run/mooring-portal-release/latest.json` 要留给导入的服务读。文件里是公开信息，目录和文件对其他账号可读。
- `LimitFSIZE=1M`：由内核限制这个进程能写出的文件大小。`--max-filesize` 在 curl 8.4 之前只在服务器声明了长度时才生效，GitHub 的这个回答不带长度，而 Debian 12 的 curl 是 7.88；`/run` 又在内存里。导入命令自己另有 1 MiB 的上限。
- `--proto =https`，没有 `--location`：只取这一个地址，不跟着跳转去别处。（在 shell 里手工试这条命令时，`=https` 要加引号，zsh 会把它当成别的写法。）仓库改名后 GitHub 会回一个跳转，那时取到的不是发布描述，导入会拒绝它，直到把这里的地址和代码里的仓库地址一起改掉（有测试核对两处一致）。
- `OnCalendar` 而不是"上次运行之后 5 分钟"：上一次失败与否，都不影响下一次按时启动。
- `time-sync.target`：导入时记下的"核对时间"取自主机时钟。只有主机启用了等待对时的服务（如 `systemd-time-wait-sync.service`）时，这一项才真的等到时钟对准；没有启用时它在开机时立刻达成。时钟不准时记下的核对时间，页面按第 2 节处理。

**出了问题怎么知道。** 没有人会主动通知运营者：

- `systemctl --failed` 会列出失败的服务；`journalctl -u mooring-portal-release-fetch.service -u mooring-portal-release.service` 能看到原因。一次运行超过 120 秒会被终止并记为失败。
- 最近 24 小时内一次都没成功，下载页自己会提示，这时用户也看得到。
- 运营页面的概况有一格"下载页上的 CFM"，写着页面上的版本和上次核对的时间。还没导入过，或者 24 小时内没有核对成功，这一格标成警示色，并提示去主机上查两个服务的日志；存下的那一行读不出来时，这一格写"读不出来"（2026-11-14 起；在那之前导入失败了两天才被发现，见第 2 节"重新构建的版本"）。
- 想在失败时收到通知，在两个服务的 `[Unit]` 里各加一行 `OnFailure=`，指向主机上已有的报警单元。
- 页面退回了旧版本、同一版本的文件被换过，这两种情况导入照常成功，只在日志里留一行以 `warning:` 开头的话：`journalctl -u mooring-portal-release.service -g '^warning'`。

**存下的那一行读不出来时**（例如有人手工改过，或者换回了读不懂它的旧程序）：下载页返回错误页，之后每次导入也都失败；运营页面概况的那一格写"读不出来"，其余各格照常。门户不会把读不懂的一行当成"还没有"而盖掉它。确认原因之后，删掉这一行，再等定时器导入一次。主机上没有 `sqlite3` 命令行，用系统自带的 `python3` 删：

```bash
runuser -u mooring-portal -- python3 -c "import sqlite3; c = sqlite3.connect('/var/lib/mooring-portal/portal.db'); c.execute('DELETE FROM cfm_release'); c.commit()"
```

2026-11-12 起，这些单元在门户主机上运行，通过了 `systemd-analyze verify`（`docs/deploy.md` 第 2 节）。
