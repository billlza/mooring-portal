# 新消息提醒：Telegram 和飞书

运营者 2026-11-15 要求把右下角的反馈框做成实时聊天，并选了用 Telegram 机器人提醒运营者有新消息。门户自己不连任何外部服务：用户每发一条消息，门户把当时的时间写进一个文件（`--new-message-signal`）；主机上的 systemd 盯着这个文件，它一变，就运行一个小脚本，由脚本给运营者的 Telegram 发一句固定的话。2026-11-15 随 `91b72cb` 在主机上装好了第 3、4 步，以及第 5 步里建文件、改两处启动命令和重启门户这几步（`docs/deploy.md` 第 2 节）。第 1、2 步等运营者来做；做完之后再启用 path 单元、发一条试的提醒。

- **Telegram 知道什么**：只有"此刻有人在泊光留言"。提醒里没有账号、邮箱，也没有留言的内容；运营者到运营页面看对话、回复。文件里也只有时间。提醒里那个运营页面的地址不生成预览（`link_preview_options`），Telegram 的服务器不会为它来访问门户。
- **令牌放在哪**：机器人的令牌只在主机上，`/etc/mooring-portal/telegram`，root 所有、0600，由 systemd 交给脚本（`LoadCredential=`）。脚本只经标准输入把它交给 curl，它不出现在任何进程的命令行里，别的账号用 `ps` 看不到。拿到令牌的人能以这个机器人的名义给运营者发消息，能读到别人发给这个机器人的消息；读不到门户的任何东西。泄露了，在 @BotFather 里对这个机器人用 `/revoke` 换一个，再改这个文件。
- **失败时**：提醒发不出去（Telegram 连不上、令牌失效）不影响留言：留言已经存进数据库，运营页面照常显示，失败写在 `journalctl -u mooring-portal-notify.service` 里。门户写不了这个文件时也一样，留言照常发出，门户的日志里记一条错误（`telling the host of a new message failed`）。门户启动时先检查这个文件所在的目录在不在、文件是不是目录，不对就拒绝启动；能不能写，要等第一条留言才知道（启动时写一次就会发出一条提醒），所以装好后要按第 5 步留一句言试试。
- **多少条提醒**：一条留言一次提醒；脚本还在发上一条时又来的留言，并进这一次（path 单元在它启动的服务运行时不再启动它）。每个账号 24 小时最多发 20 条留言，所以提醒也有上限。运营者自己的回复不提醒。

**1. 建机器人**（运营者在自己的 Telegram 里做）

1. 和 @BotFather 对话，发 `/newbot`，按提示起名字，记下它给的令牌。令牌不要发给任何人，也不要贴进聊天、仓库或命令行。
2. 打开新机器人的对话，点"开始"，再随便发一句话，好让下一步查得到这个对话。

**2. 放令牌**（在门户主机上，以 root 运行）

```bash
install -o root -g root -m 0600 /dev/null /etc/mooring-portal/telegram
nano /etc/mooring-portal/telegram
```

写一行 `token=<令牌>`，保存。然后查出运营者这个对话的编号（令牌从文件里读，经管道交给 curl）：

```bash
sed -n 's|^token=\(.*\)$|url = "https://api.telegram.org/bot\1/getUpdates"|p' /etc/mooring-portal/telegram | curl --fail --silent --show-error --proto =https --config - | grep -o '"chat":{"id":[0-9-]*' | sort -u
```

输出形如 `"chat":{"id":123456789`。把这个数字写成第二行 `chat=123456789`。文件最后是两行：`token=...` 和 `chat=...`。

**3. 装脚本**

`/usr/local/lib/mooring-portal/notify-telegram`（root 所有，0755：`install -d -m 0755 /usr/local/lib/mooring-portal`，写好后 `chmod 0755`）：

```sh
#!/bin/sh
# Tells the operator on Telegram that a user has written on Moorlight.
# The notice says only that; the operator reads the message on the site.
# The bot's token reaches curl on its standard input, never on a command
# line.
set -eu
credential="$CREDENTIALS_DIRECTORY/telegram"
token=$(sed -n 's/^token=//p' "$credential")
chat=$(sed -n 's/^chat=//p' "$credential")
case "$token" in
  '' | *[!0-9A-Za-z:_-]*)
    echo "notify-telegram: $credential needs one line token=<the bot's token>" >&2
    exit 1
    ;;
esac
case "$chat" in
  '' | *[!0-9-]*)
    echo "notify-telegram: $credential needs one line chat=<the chat's number>" >&2
    exit 1
    ;;
esac
printf 'url = "https://api.telegram.org/bot%s/sendMessage"\ndata-urlencode = "chat_id=%s"\n' "$token" "$chat" |
  curl --fail --silent --show-error --proto =https --max-time 30 --output /dev/null \
    --config - \
    --data-urlencode 'text=泊光有新消息，请到 https://moorlight.app/operator/feedback 查看。' \
    --data-urlencode 'link_preview_options={"is_disabled":true}'
```

**4. 两个单元**

`/etc/systemd/system/mooring-portal-notify.path`：

```ini
[Unit]
Description=Watch for messages to the Moorlight operator

[Path]
PathChanged=/var/lib/mooring-portal/new-message
Unit=mooring-portal-notify.service

[Install]
WantedBy=paths.target
```

`/etc/systemd/system/mooring-portal-notify.service`：

```ini
[Unit]
Description=Tell the Moorlight operator on Telegram that a message waits
Wants=network-online.target
After=network-online.target
StartLimitIntervalSec=0

[Service]
Type=oneshot
DynamicUser=yes
LoadCredential=telegram:/etc/mooring-portal/telegram
TimeoutStartSec=60
ExecStart=/usr/local/lib/mooring-portal/notify-telegram
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
InaccessiblePaths=-/var/lib/mooring-portal
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
```

**5. 让门户写这个文件**：先把文件建好；再在门户的两处启动命令（`docs/deploy.md` 第 4 节的单元，和正式收款用的 `mooring-portal.service.d/50-live-payments.conf`，见该文第 1 节）末尾都加上 `--new-message-signal /var/lib/mooring-portal/new-message`。然后：

```bash
install -o mooring-portal -g mooring-portal -m 0600 /dev/null /var/lib/mooring-portal/new-message
systemd-analyze verify /etc/systemd/system/mooring-portal-notify.path /etc/systemd/system/mooring-portal-notify.service
systemctl daemon-reload
systemctl enable --now mooring-portal-notify.path
systemctl start mooring-portal-notify.service
systemctl restart mooring-portal
```

倒数第二行先发一条试的提醒，运营者的 Telegram 应当收到"泊光有新消息"，下面不带网页预览；收不到时看 `journalctl -u mooring-portal-notify.service -n 20`（Telegram 不接受请求时，curl 在这里报出 HTTP 状态）。重启门户之后，用一个试用账号在网站上留一句言，应当再收到一条；收不到时先看 `journalctl -u mooring-portal -n 50` 里有没有 `telling the host of a new message failed`（门户写不了这个文件），再看提醒服务的日志。

几处写法的原因：

- **`PathChanged=`、先建好文件**：门户每次都是打开、写入、关上同一个文件（原地改写，不换文件），关上时 systemd 就启动服务。文件事先建好，它的属主和权限就由这一步定下（门户的账号、0600），不取决于门户第一次写它时的 umask，启动时的检查也查得到它。文件在门户的数据目录里（0700，只有门户的账号能进），systemd 以 root 盯着它，不需要放宽权限。
- **不生成预览**：提醒里的地址要是生成预览，Telegram 的服务器会来取这个网页（取到的是登录页）。`link_preview_options` 让它不取，提醒也只是一行字。
- **`StartLimitIntervalSec=0`**：systemd 默认 10 秒内启动同一个服务超过 5 次，就拒绝再启动，path 单元也跟着停下。一位用户连发几条留言就会碰到这个上限，之后的提醒全部丢失，所以这里不设上限；留言本身有上限（上面"多少条提醒"）。
- **`DynamicUser=yes`、`InaccessiblePaths=`**：脚本用一个临时的账号运行，什么也不拥有，看不到门户的数据库。它只需要令牌文件（由 systemd 交给它）和网络。
- **脚本先检查令牌和对话编号的写法**：令牌只能是数字、字母和 `:_-`，编号只能是数字和 `-`。写错的（比如编辑器留下的空格或 `\r`）在这里报错，而不是变成一条读不懂的请求；它们也就不可能把别的东西塞进交给 curl 的配置。
- **有测试核对**（`crates/portal/src/tests/deploy.rs`）：path 单元盯的就是门户写的那个文件，它在门户能写的目录里；本文的脚本用一个假的 curl 跑过：令牌和对话编号只出现在 curl 的标准输入里，不在它的命令行上，写错的令牌和编号被拒绝。

**飞书的提醒**

运营者 2026-11-21 要求在飞书里也收到新消息的提醒，并决定留言的内容以后只可以转到 Telegram，飞书里只有提醒。做法和上面一样，但用它自己的两个单元：同一个文件一变，另一个 path 单元启动另一个脚本，由它往飞书群里的自定义机器人发一句固定的话。两边互不影响，哪一边没装、发不出去，另一边照常提醒。这一部分写好时（2026-11-21）还没有在主机上装过，飞书那边的回答也只按它的文档核对过，没有真的发过。

- **飞书知道什么**：只有"此刻有人在泊光留言"，以及这个群和运营者的飞书账号的联系。提醒里没有账号、邮箱、留言的内容，也没有运营页面的地址，飞书的服务器没有地址可取（它会不会为机器人消息里的地址生成预览，没有核对过，所以干脆不带）。
- **地址就是凭据**：自定义机器人的 webhook 地址，谁拿到都能往这个群里发消息；读不到群里的消息，也读不到门户的任何东西。它只在主机上，`/etc/mooring-portal/feishu`，root 所有、0600，由 systemd 交给脚本；脚本只经标准输入把它交给 curl，它不出现在任何进程的命令行里。泄露了，在群里删掉这个机器人、重新加一个，再改这个文件。
- **安全设置选关键词**：飞书强烈建议给自定义机器人加安全设置，可以开一种，也可以开几种。选"自定义关键词"，填 `泊光`（提醒里有这两个字）。还可以再开"IP 白名单"，填门户主机的地址，这样别处拿到地址也发不了；主机换了地址要记得改。不用"签名校验"：它只在"地址泄露了、密钥没有泄露"时多一层保护，而两者放在同一个文件里；算签名还得把密钥交给另一个程序。
- **飞书不收时**：飞书不收一条消息（没有关键词、来的地址不在白名单里、发得太快）时，原因写在回答的 `code` 里。脚本只把 `code` 为 0 的回答当作发出去了，别的都算失败，连同回答记在 `journalctl -u mooring-portal-notify-feishu.service` 里。和 Telegram 的提醒一样，发不出去不影响留言。

**6. 建飞书机器人**（运营者在自己的飞书里做）

1. 建一个只有自己的群，在群设置里打开"群机器人"，添加"自定义机器人"，起个名字。
2. 安全设置选"自定义关键词"，填 `泊光`，保存。记下它给的 webhook 地址。地址不要发给任何人，也不要贴进聊天、仓库或命令行。

**7. 放地址**（在门户主机上，以 root 运行）

```bash
install -o root -g root -m 0600 /dev/null /etc/mooring-portal/feishu
nano /etc/mooring-portal/feishu
```

写一行 `url=<webhook 地址>`，保存。飞书的地址以 `https://open.feishu.cn/open-apis/bot/v2/hook/` 开头，Lark 的以 `https://open.larksuite.com/open-apis/bot/v2/hook/` 开头；别的地址脚本不接受。

**8. 装脚本和两个单元**

`/usr/local/lib/mooring-portal/notify-feishu`（root 所有，0755）：

```sh
#!/bin/sh
# Tells the operator in a Feishu group that a user has written on
# Moorlight. The notice says only that; the operator reads the message on
# the site. The bot's webhook address is its credential: it reaches curl
# on its standard input, never on a command line. Feishu names its reason
# for not taking a notice in the answer's code, so only an answer whose
# code is 0 counts as sent.
set -eu
credential="$CREDENTIALS_DIRECTORY/feishu"
url=$(sed -n 's/^url=//p' "$credential")
case "$url" in
  https://open.feishu.cn/open-apis/bot/v2/hook/*)
    hook=${url#https://open.feishu.cn/open-apis/bot/v2/hook/}
    ;;
  https://open.larksuite.com/open-apis/bot/v2/hook/*)
    hook=${url#https://open.larksuite.com/open-apis/bot/v2/hook/}
    ;;
  *)
    hook=
    ;;
esac
case "$hook" in
  '' | *[!0-9A-Za-z-]*)
    echo "notify-feishu: $credential needs one line url=<the bot's webhook address>" >&2
    exit 1
    ;;
esac
answer=$(printf 'url = "%s"\n' "$url" |
  curl --fail --silent --show-error --proto =https --max-time 30 \
    --config - \
    --header 'Content-Type: application/json' \
    --data '{"msg_type":"text","content":{"text":"泊光有新消息，请到运营页面的「反馈」查看。"}}')
case "$(printf '%s' "$answer" | tr -d ' \t\r\n')" in
  *'"code":0,'* | *'"code":0}'*) ;;
  *)
    printf 'notify-feishu: Feishu did not take the notice: %.300s\n' "$answer" >&2
    exit 1
    ;;
esac
```

`/etc/systemd/system/mooring-portal-notify-feishu.path`：

```ini
[Unit]
Description=Watch for messages to the Moorlight operator, for Feishu

[Path]
PathChanged=/var/lib/mooring-portal/new-message
Unit=mooring-portal-notify-feishu.service

[Install]
WantedBy=paths.target
```

`/etc/systemd/system/mooring-portal-notify-feishu.service`：

```ini
[Unit]
Description=Tell the Moorlight operator in Feishu that a message waits
Wants=network-online.target
After=network-online.target
StartLimitIntervalSec=0

[Service]
Type=oneshot
DynamicUser=yes
LoadCredential=feishu:/etc/mooring-portal/feishu
TimeoutStartSec=60
ExecStart=/usr/local/lib/mooring-portal/notify-feishu
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
InaccessiblePaths=-/var/lib/mooring-portal
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6
```

除了说明、凭据和脚本这三行，它和 Telegram 的那个服务一字不差（有测试），几处写法的原因也和上面一样。

**9. 启用**

```bash
systemd-analyze verify /etc/systemd/system/mooring-portal-notify-feishu.path /etc/systemd/system/mooring-portal-notify-feishu.service
systemctl daemon-reload
systemctl enable --now mooring-portal-notify-feishu.path
systemctl start mooring-portal-notify-feishu.service
```

最后一行先发一条试的提醒，飞书群里应当出现"泊光有新消息，请到运营页面的「反馈」查看。"；没有的话看 `journalctl -u mooring-portal-notify-feishu.service -n 20`，飞书的回答在里面。门户不用重启：它已经在写这个文件（第 5 步）。

这一部分也有测试核对（`crates/portal/src/tests/deploy.rs`）：这个 path 单元盯的是门户写的同一个文件；脚本用一个假的 curl 跑过：地址只出现在 curl 的标准输入里，不在它的命令行上；提醒里有关键词、没有地址；飞书答的 `code` 不是 0、或者 curl 失败时，脚本报错；写错的地址（别的主机、不是 https、多出一段路径、带引号或空白、写了两行）在 curl 运行之前就被拒绝。
