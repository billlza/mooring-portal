# 留言翻译

运营者 2026-11-21 要求：不管用户用哪种语言留言，运营者看到的都是自己界面的语言；并决定译文由门户主机上跑的翻译模型生成，不用浏览器自带的翻译，也不用云服务。这是 README 待定事项第 14 条的第 2 步。

第 1 到 3 节记门户这一半：存译文的两张表、`translate` 命令、它和翻译服务之间的问答格式、运营者的对话页怎么显示译文。第 4 节记翻译服务本身，第 5 节记怎样把它装到主机上。两半都在 2026-11-22 写好，翻译服务同日在运营者的 Mac 上用真的引擎和模型试过；**都还没有装到主机上**，装之前要运营者同意。在那之前主机上没有东西运行 `translate`，对话页上不会出现译文，别的一切照旧。

## 1. 做法

门户自己不跑模型，`serve` 也不连任何东西。用户每发一条留言，门户照旧把时间写进 `--new-message-signal` 的文件（`docs/notices.md`）；主机上的单元看到文件变了，运行一次：

```bash
mooring-portal --db /var/lib/mooring-portal/portal.db translate --translator <翻译服务的 socket>
```

这个命令从数据库里取出还没问过的留言，一条条去问翻译服务：这是什么语言，译成页面的语言是什么；把答案写回数据库。运营者的对话页几秒内就把译文显示在那条留言下面。

- **谁看得到译文**：只有运营者的对话页（`/operator/feedback/<编号>`）。用户自己的"反馈问题"页、右下角的框，以及它们的脚本，都不显示、也拿不到译文，带着 `translated=` 去问也一样（有测试）。译文上面标着"机器翻译，可能有误"，原文始终在它上面。运营者的回复不翻译，照原样发出。
- **留言去了哪里**：哪里也没去。翻译服务在同一台主机上，交给它的只有一条留言的文字：没有账号、邮箱，也不说是谁写的；`translate` 除了数据库只连这一个 socket。翻译服务自己读不到数据库、没有网络，这两点由它的单元保证（第 5 节）。
- **日志里有什么**：`translate` 只说条数（`examined 3 message(s) and wrote 4 translation(s)`），失败时说留下了几条、第一条为什么失败。留言和译文里的字、账号，都不写；翻译服务答了读不懂的东西，也不引用它答的原文，因为里面可能有留言的字（有测试）。主机保存这些输出的时间比门户保存留言的时间长。
- **译文算谁的**：算那条留言的。留言被删（一段对话只留最近 200 条）、账号被删除或封禁时，译文和"问过了"的记录一起删除（外键级联），并从数据库文件里抹掉（有测试，`docs/security.md` 第 8 节）。
- **翻成什么**：页面有的两种语言，中文和英文。模型只在英文和别的语言之间互译，所以英文留言译成中文，中文留言译成英文，别的语言先译成英文、再把这份英文译成中文，两份都存下。运营者的页面是哪种语言，就显示哪一份；留言本来就是页面的语言时，不显示译文。认不出语言的留言，和翻译服务没有对应模型的留言，保持原文，记为问过，不再问。
- **先译最新的**：每次运行从最新的留言问起，往旧的走；运行期间新写来的，这次接着问。一条留言一次运行里只问一遍。这样运营者最先看到的是刚写来那条的译文，翻译服务每次都答不出的留言排在新留言后面，挡不住它们（有测试）。
- **译文不能显示时**：译文是空的、超过 6000 个字符，或带着留言不许有的字符（控制字符和文字方向控制字符，`docs/security.md` 第 13 节"反馈"），这份译文不存；留言照样记为问过。6000 是留言上限 1000 的 6 倍：译文比原文长是常事，长到这个倍数就不是译文了。

**失败时**：留言照常存下、照常显示，只是没有译文。

- 连不上翻译服务（socket 不在，或没有进程在听）：这次运行立即停下，后面的留言一条也不问。
- 别的失败（答的不是约定的格式、长度对不上、120 秒没有答）：跳过这一条，它等下一次运行；一次运行里失败满 3 条就停下，免得一个坏掉的翻译服务被每条留言各问一遍。
- 两种情形命令都以 1 退出，stderr 里一行：`mooring-portal: the translator at <socket> left <几条> message(s) for the next run: <第一条失败的原因>`。
- 等了 30 天还没有译出的留言不再问，保持原文。它要么写于还没有翻译服务的时候，要么是翻译服务每次都答不出的。装好翻译服务后的第一次运行，会把最近 30 天里的留言补译。
- 一条留言正在问的时候被删除（账号被删），答案不存；删除腾出的编号被下一条留言用上时，答案也不会记到那条留言头上：记录时按编号和原文一起找（有测试）。

## 2. 问答格式

`translate` 每问一个问题连一次 socket。

问：一行，写明问什么、后面的文字有多少字节；然后是文字（UTF-8），恰好那么多字节。

```text
detect <字节数>\n<文字>
translate <原文的语言> <要译成的语言> <字节数>\n<文字>
```

`<原文的语言>` 是 `detect` 答出的代码，或 `en`；`<要译成的语言>` 是 `zh` 或 `en`。

答：一行，然后是内容；答完由翻译服务关闭连接。

```text
ok <字节数>\n<内容>
none\n
```

`ok` 后面的内容是语言代码（两到三个小写字母，前后可以有空白）或译文，恰好那么多字节。`none` 是没有可答的：认不出语言，或没有这个方向的模型。除此之外的回答都算失败：没有换行、`ok` 后面没有用数字写的字节数、`none` 后面还有内容、内容不是 UTF-8、整个回答超过 48032 字节。

翻译服务这一边也只认这个格式（第 4 节）：第一行最多 64 字节；文字最多 24000 字节，因为门户问的最长的文字是 6000 个字符的英文译文，一个字符最多 4 字节（有测试核对两边的数一致）；长度为 0、文字比说的多、文字不是 UTF-8 的问题，都不答。

两边都写明长度，谁也不靠"对方不再写了"来判断一段文字到哪里结束：

- **回答带长度**：翻译进程写到一半被停掉（内存不够，或超时）时，留下的半句不会被当成整句存下；把长度数错的实现（比如按字符数而不是字节数）也会被发现，而不是悄悄截断译文。
- **问题带长度，不用"关掉写的那一半"表示问完**：最初的做法是问的一方写完就关掉连接上写的那一半。2026-11-22 在运营者的 Mac（macOS）上，十次左右的全量测试里有三次，答的一方一直等不到"问完了"，两边各自等到 120 秒超时；对卡住的进程采样，问的线程和答的线程都停在 `recv` 里。单独写的小实验（先关写的一半，再让对方接受连接，900 次）没有复现，原因没有查到底，Linux 上也没有试过。改成带长度之后不依赖这个行为；同样的全量测试接着连跑 20 次，没有再出现。

## 3. 数据库和页面

- **两张表**（数据库第 9 版，`docs/deploy.md` 第 2 节有撤销的办法）：`feedback_examined` 记一条留言问过了、认出的语言（认不出为空）和时间；`feedback_translations` 记一份译文：哪条留言、哪种语言（只能是 `zh` 或 `en`）、译文、时间，一条留言每种语言最多一份。两张表的行都随留言删除。
- **译文的编号只增不减**：对话页开着的时候，脚本每 3 秒问一次"第几号之后的留言"和"第几号之后的译文"（`/operator/feedback/<编号>/messages?after=<n>&translated=<m>`）。译文随留言删除，被删的可能恰好是最新的几份；要是编号被后来的译文重新用上，页面已经越过这个号，就永远看不到它。所以这张表的编号不重用（`AUTOINCREMENT`，有测试）。
- **留言和译文一次读出**：页面和脚本拿到的留言和译文出自数据库的同一时刻（一个只读事务，`Store::conversation_translated`）。译文总是晚于它的留言；分两次读的话，两次之间写进来的留言和它的译文，会让脚本先拿到译文、越过它的编号，却还没有那条留言可以挂，之后再也不会补上。
- **部署前开着的页面**：旧的脚本只问 `after=`，照旧得到留言，不带译文（有测试）；刷新之后就有了。
- **脚本怎么写进页面**：和留言一样，译文作为文本写入，不解析成标记（有测试）；回答的形状不对（没有 `translations`，或其中一项缺编号、留言号或文字）就整个不用，留言也不写，当作连不上，稍后再问。

## 4. 翻译服务

翻译服务是 `crates/portal/translator/translator.mjs`，一个 Node 脚本，不编进门户的二进制。systemd 每收到一个连接就启动它一次，把连接交给它（socket 激活，`Accept=yes`，连接是第 3 号描述符）；它读一个问题，答完就退出，不常驻。

- **认语言**：用 fastText 的命令行（主机上是 Debian 的 `fasttext` 0.9.2）和 fastText 的语言识别模型 `lid.176.ftz`（176 种语言）。fastText 一次读一行，所以留言里的换行和空白先换成一个空格。它最拿得准的那种语言，把握在 50% 以上才答出来，否则答 `none`：2026-11-22 试的时候，`hi` 只有 12%，`ok` 有 63%（英文），一句完整的话一般在 95% 以上。
- **翻译**：用 Mozilla 给 Firefox 的网页翻译做的引擎 bergamot-translator 0.6.0，编成 WebAssembly 的那一份（Firefox 发布记录里的 3.0），在 Node 里运行；模型用 Firefox 的，设置也照 Firefox（`translations-engine.worker.js`）。现在装五个方向：中译英、英译中、日译英、俄译英、西译英，都是 2026-11-22 时 Firefox 桌面正式版用的版本。没有模型的方向答 `none`，留言照原样显示。模型放在以门户的语言代码命名的目录里（`zh-en`、`en-zh`……）；Firefox 把中文叫 `zh-Hans`，试过把 `zh` 和 `zh-Hans` 交给引擎，译文一样。
- **一个问题一个进程**：每次都重新载入引擎（约 5 MB）和一个方向的模型（30 到 70 MB），答完内存全部还给系统，空闲时不占内存。模型文件直接读进引擎的内存，脚本里不另留一份。
- **日志里有什么**：只有失败的原因（比如 `the question asks neither detect nor translate`、`the engine stopped: …`），都是脚本自己写的字，不带问题里的文字。引擎自己打印的东西一概不留，因为说不定带着文字（有测试）。fastText 的错误输出进日志，它不复述读到的文字。
- **文件**：引擎的脚本和 WebAssembly、引擎的许可证、`lid.176.ftz` 和五个方向的模型，共 20 个文件、272 MB，列在 `crates/portal/translator/files.txt`：每个文件的 SHA-256、装在哪里、大小、从哪里取。引擎的 WebAssembly 和模型取自 Firefox 发布记录里的地址（Mozilla 的 CDN），SHA-256 也是记录里的；引擎的脚本和许可证取自 Firefox 源码里固定的提交；`lid.176.ftz` 取自 fastText 的网站，SHA-256 是 2026-11-22 下载后算的。许可证：引擎和模型是 MPL-2.0，`lid.176.ftz` 是 CC BY-SA 3.0。这些文件不进仓库。

**2026-11-22 在运营者的 Mac 上试过**（M1 Max，Node 26.3，fastText 0.9.2 从源码编译；不是主机的 Node 20 和 x86_64）：

| 问题 | 用时 | 一个进程最多用的内存 |
|---|---|---|
| 认语言 | fastText 本身 0.01 秒，连同启动 Node 约 0.1 秒 | fastText 6 MB |
| 俄译英、西译英，一句话 | 0.3 秒 | 274 MiB |
| 日译英，一句话 | 0.4 秒 | 475 MiB |
| 中译英，1000 个字（留言的上限） | 2.4 秒 | 505 MiB |
| 英译中，6000 个字符（门户会转译的最长的英文） | 2.4 秒 | 412 MiB |

- **端到端**：本机的演示库，门户的 `serve` 和 `translate` 照常运行，翻译服务由一个替身按 systemd 的方式启动（一个连接一个进程，第 3 号描述符），`translate` 在门户写新消息文件时运行。用户发了日、俄、西、英、中、韩六条留言，每条写来后 0.3 到 1.2 秒译文就存进了数据库；运营者的对话页在中文界面下显示日、俄、西、英四条的中文译文，在英文界面下显示日、俄、西、中四条的英文译文；韩文没有模型，记为问过，没有译文。
- **译文的质量**：日常的话大多通顺，但会错，比如"按流量计费的套餐"译成了 "a meal by flow"，"连不上"经英文转一道成了"无法联系"。页面上标着"机器翻译，可能有误"，原文在上面。
- **繁体中文译不好**：fastText 不分简繁，繁体的留言也认作 `zh`，交给简体的中译英模型，译出来的英文是乱的（试过"我的帳號無法登入"）。运营者在中文界面下看的是原文，所以只影响英文界面。要改，得加上 Firefox 的繁体模型，并在翻译服务里先分出简繁，还没有做。
- **主机的 Node 20**：按 Mozilla 的构建文件，引擎只用到 WebAssembly 的 SIMD128（`-msimd128`），Node 20 支持；脚本也只用 Node 20 有的东西。但都没有在 Node 20 上跑过。

## 5. 装到主机上

装之前要运营者同意。以下以 root 在门户主机上运行；`/home/builder/portal` 是部署时解开的那次提交（`docs/deploy.md` 第 2 节），门户的数据库要先到第 9 版。这些步骤不碰门户本身，不用重启门户。

**1. 装 Node 和 fastText**

```bash
apt install nodejs fasttext
```

**2. 装脚本，取文件**

```bash
install -d -m 0755 /usr/local/lib/mooring-portal/translator
install -o root -g root -m 0644 /home/builder/portal/crates/portal/translator/translator.mjs /usr/local/lib/mooring-portal/translator.mjs
cd /usr/local/lib/mooring-portal/translator
grep -v '^#' /home/builder/portal/crates/portal/translator/files.txt | while read -r sum path size url; do install -d -m 0755 "$(dirname "$path")" && curl --fail --silent --show-error --location --proto =https --output "$path" "$url"; done
grep -v '^#' /home/builder/portal/crates/portal/translator/files.txt | awk '{ print $1 "  " $2 }' | sha256sum --check --strict
```

最后一行要对 20 个文件都说 `OK`。没取到、取坏了的文件在这里报出来，重新运行倒数第二行，再查一遍。

**3. 五个单元**

`/etc/systemd/system/mooring-portal-translator.socket`：

```ini
[Unit]
Description=Take questions for the Moorlight portal's translator

[Socket]
ListenStream=/run/mooring-portal-translator.sock
SocketGroup=mooring-portal
SocketMode=0660
Accept=yes
MaxConnections=1

[Install]
WantedBy=sockets.target
```

`/etc/systemd/system/mooring-portal-translator@.service`：

```ini
[Unit]
Description=Answer one question of the Moorlight portal's translate command
CollectMode=inactive-or-failed

[Service]
DynamicUser=yes
ExecStart=/usr/bin/node /usr/local/lib/mooring-portal/translator.mjs --engine /usr/local/lib/mooring-portal/translator/engine --models /usr/local/lib/mooring-portal/translator/models --fasttext /usr/bin/fasttext --identifier /usr/local/lib/mooring-portal/translator/lid.176.ftz
RuntimeMaxSec=100
MemoryMax=900M
MemorySwapMax=0
CPUWeight=20
Nice=10
PrivateNetwork=yes
RestrictAddressFamilies=AF_UNIX
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
InaccessiblePaths=-/var/lib/mooring-portal -/etc/mooring-portal
```

`/etc/systemd/system/mooring-portal-translate.service`：

```ini
[Unit]
Description=Have users' messages to the Moorlight operator translated
Requires=mooring-portal-translator.socket
After=mooring-portal-translator.socket
StartLimitIntervalSec=0

[Service]
Type=oneshot
User=mooring-portal
ExecStart=/usr/local/bin/mooring-portal --db /var/lib/mooring-portal/portal.db translate --translator /run/mooring-portal-translator.sock
TimeoutStartSec=1h
PrivateNetwork=yes
RestrictAddressFamilies=AF_UNIX
NoNewPrivileges=yes
PrivateTmp=yes
PrivateDevices=yes
ProtectSystem=strict
ProtectHome=yes
ReadWritePaths=/var/lib/mooring-portal
```

`/etc/systemd/system/mooring-portal-translate.path`：

```ini
[Unit]
Description=Watch for messages to the Moorlight operator, for translation

[Path]
PathChanged=/var/lib/mooring-portal/new-message
Unit=mooring-portal-translate.service

[Install]
WantedBy=paths.target
```

`/etc/systemd/system/mooring-portal-translate.timer`：

```ini
[Unit]
Description=Translate what the watch for new messages missed

[Timer]
OnBootSec=5min
OnUnitInactiveSec=15min
Unit=mooring-portal-translate.service

[Install]
WantedBy=timers.target
```

**4. 启用，先问一个问题**

```bash
systemd-analyze verify /etc/systemd/system/mooring-portal-translator.socket /etc/systemd/system/mooring-portal-translate.service /etc/systemd/system/mooring-portal-translate.path /etc/systemd/system/mooring-portal-translate.timer
systemctl daemon-reload
systemctl enable --now mooring-portal-translator.socket
python3 -c 'import socket; s = socket.socket(socket.AF_UNIX); s.connect("/run/mooring-portal-translator.sock"); s.sendall(b"translate en zh 25\nWhere is my subscription?"); print(s.makefile("rb").read().decode())'
journalctl -u 'mooring-portal-translator@*' -n 5 -o cat
```

`python3` 那一行问一句英文怎样译成中文，应当打印 `ok` 和字节数，下一行是中文译文；什么也没打印，就看最后一行里翻译服务说的原因。最后一行里还有这个进程最多用了多少内存（`memory peak`），这是它在主机上的第一个数，和第 4 节 Mac 上的数比一比。然后：

```bash
systemctl enable --now mooring-portal-translate.path mooring-portal-translate.timer
systemctl start mooring-portal-translate.service
journalctl -u mooring-portal-translate.service -n 5 -o cat
```

第一次运行把最近 30 天里用户的留言都问一遍，最后一行应当是 `examined … message(s) and wrote … translation(s)`。之后用户每发一条留言，几秒内运营者的对话页上就有译文。

几处写法的原因：

- **一个连接一个进程、`MaxConnections=1`**：`translate` 一次只问一个问题，一个进程最多用 500 MiB 左右（第 4 节），2 GB 的主机上不让两个同时跑。多出来的连接被 systemd 拒绝，`translate` 记一次失败，下次再问。
- **`RuntimeMaxSec=100`**：门户等一个回答最多 120 秒（`PATIENCE`）。卡住的翻译进程在门户放弃之前就被停掉，不会在门户走后还占着内存。
- **`MemoryMax=900M`、`MemorySwapMax=0`**：一个进程在 Mac 上最多用过 505 MiB，900M 留出近一倍的余地；超过就被停掉，不去挤门户和 Caddy 的内存，也不换到交换区里慢慢跑。被停掉的问题，`translate` 记一次失败。
- **`CPUWeight=20`、`Nice=10`**：主机只有一个 vCPU。翻译一段长留言要两三秒，这期间网站的请求先跑。
- **`DynamicUser=yes`、`PrivateNetwork=yes`、`InaccessiblePaths=`**：翻译服务用一个临时的账号运行，什么也不拥有；它没有网络（它的连接是 systemd 交给它的，不受影响），看不到门户的数据目录和 `/etc/mooring-portal` 里的凭据。它只读得到它自己的文件和留言的文字。
- **socket 的权限**：`/run/mooring-portal-translator.sock` 属于 root 和 `mooring-portal` 组，0660，只有 root 和门户的账号能连。
- **`translate` 用门户的账号运行**：它要读写数据库；和运营者命令一样（`docs/deploy.md` 第 4 节），可以和正在运行的门户同时打开数据库。它也没有网络，只连翻译服务的 socket。它的输出只有条数。
- **`TimeoutStartSec=1h`、`StartLimitIntervalSec=0`**：第一次运行要问的留言多，一个 oneshot 服务默认 90 秒就被停掉。被停掉也不会丢东西：问过的已经记下，下次从没问过的接着问。不设启动次数的上限，理由和 `docs/notices.md` 的提醒一样。
- **path 单元和定时器**：path 单元在 `translate` 运行期间不再启动它，那期间最后一刻写来的留言要等下一次；定时器在每次运行结束 15 分钟后再运行一次，开机 5 分钟后也运行一次。
- **有测试核对**：`crates/portal/src/tests/translator_units.rs` 核对这些单元和门户的命令行、门户自己的单元、提醒盯的文件对得上；`crates/portal/translator/test/` 核对翻译服务的命令行就是这里的 `ExecStart`、`files.txt` 里的文件放在它找的地方，并把翻译服务当作一个进程，按 systemd 的方式交给它连接跑过。

撤下：`systemctl disable --now mooring-portal-translate.timer mooring-portal-translate.path mooring-portal-translator.socket`，删掉这五个单元文件、`/usr/local/lib/mooring-portal/translator.mjs` 和 `/usr/local/lib/mooring-portal/translator/`，再 `systemctl daemon-reload`。已经存下的译文留在数据库里，对话页照常显示；要连译文一起撤掉，见 `docs/deploy.md` 第 2 节第 9 版的撤销。`apt` 装的 `nodejs` 和 `fasttext` 不用卸，别的东西不用它们。

## 6. 还没做的

- **装到主机上**：等运营者同意。主机的 Node 20、x86_64 和 Linux 上的问答（第 2 节）都还没有跑过，第 5 节第 4 步的问题和日志里的内存就是第一次核对。
- **繁体中文**：见第 4 节。
- **更多语言**：Firefox 有 59 种语言译成英文的模型。要加一种，就在 `files.txt` 里加它的三四个文件（地址和 SHA-256 取自 Firefox 的发布记录），装到 `models/<代码>-en/`。加哪几种，等运营者定。
- **Telegram 转发**（README 第 14 条第 3 步）会带上译文。上线之前要先改《使用约定》和页面上的说明。
