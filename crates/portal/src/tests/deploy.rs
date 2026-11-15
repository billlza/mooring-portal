//! The deployment notes (`docs/deploy.md`) against what they configure:
//! the server's command line, and the proxy in front of it. Neither runs
//! here, so these tests hold the notes to the options the server has and
//! to the rules the proxy must keep (`docs/security.md`, sections 6 and 7).

use std::path::PathBuf;

use clap::Parser;
use portal_web::PublicOrigin;

use crate::cli::{Cli, Command, ServeArgs};

const NOTES: &str = include_str!("../../../../docs/deploy.md");

/// Each command line the notes start the server with, as it parses.
fn servers() -> Vec<(PathBuf, ServeArgs)> {
    NOTES
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("ExecStart=/usr/local/bin/mooring-portal ")
        })
        .map(|args| {
            let argv = std::iter::once("mooring-portal").chain(args.split_whitespace());
            let cli = Cli::try_parse_from(argv).expect("a command line the server takes");
            match cli.command {
                Command::Serve(serve) => (cli.db, serve),
                other => panic!("{args}: runs {other:?}, not serve"),
            }
        })
        .collect()
}

/// The Caddyfile, line by line: the first fenced block after its path.
fn caddyfile() -> Vec<&'static str> {
    let start = NOTES
        .find("`/etc/caddy/Caddyfile`")
        .expect("the notes give a Caddyfile");
    let mut lines = NOTES[start..]
        .lines()
        .skip_while(|line| !line.starts_with("```"));
    lines.next();
    lines.take_while(|line| !line.starts_with("```")).collect()
}

/// Where the site block starts: its address, unindented, opens a block.
fn site_start(caddyfile: &[&str]) -> usize {
    caddyfile
        .iter()
        .position(|line| {
            line.len() > 2 && line.ends_with(" {") && !line.starts_with(char::is_whitespace)
        })
        .expect("the Caddyfile has a site block")
}

#[test]
fn the_deployment_notes_start_the_server_with_options_it_has() {
    let servers = servers();
    // The unit file's line, and the one that replaces it for payments.
    assert_eq!(servers.len(), 2);
    for (db, args) in &servers {
        assert_eq!(db.to_str(), Some("/var/lib/mooring-portal/portal.db"));
        assert!(args.listen.ip().is_loopback(), "{}", args.listen);
        assert!(args.behind_proxy);
        PublicOrigin::parse(&args.origin).unwrap();
    }
    let (_, plain) = &servers[0];
    let (_, paying) = &servers[1];
    // Without payments the server reads nothing but its command line.
    assert!(plain.payments.is_none());
    crate::serve::config(plain).unwrap();
    assert!(paying.payments.is_some() && paying.stripe_webhook_secret.is_some());
    // Taking payments changes neither the address nor who may register.
    assert_eq!(paying.listen, plain.listen);
    assert_eq!(paying.origin, plain.origin);
    assert_eq!(paying.max_accounts, plain.max_accounts);
}

#[test]
fn the_proxy_in_the_deployment_notes_serves_the_origin_from_the_servers_address() {
    let caddyfile = caddyfile();
    let site = caddyfile[site_start(&caddyfile)].trim_end_matches(" {");
    let upstreams: Vec<&str> = caddyfile
        .iter()
        .filter_map(|line| line.trim().strip_prefix("reverse_proxy "))
        .collect();
    for (_, args) in servers() {
        assert_eq!(args.origin.strip_prefix("https://"), Some(site));
        assert_eq!(upstreams, [args.listen.to_string().as_str()]);
    }
}

#[test]
fn the_proxy_in_the_deployment_notes_logs_no_request_and_compresses_only_assets() {
    let caddyfile = caddyfile();
    let (global, site) = caddyfile.split_at(site_start(&caddyfile));
    let global: Vec<&str> = global.iter().map(|line| line.trim()).collect();
    let site: Vec<&str> = site.iter().map(|line| line.trim()).collect();
    // A subscription link's token is in its path. An access log would
    // write every path; Caddy's log of a failed request writes its path
    // and the client's address. Its admin endpoint would let any local
    // account turn logging back on.
    assert!(site.iter().all(|line| !line.starts_with("log")), "{site:?}");
    assert!(global.contains(&"exclude http.log.error"), "{global:?}");
    assert!(global.contains(&"admin off"), "{global:?}");
    // Pages carry CSRF tokens, so only the fixed public assets may be
    // compressed; `/sub/` must answer uncompressed.
    let encodes: Vec<&str> = site
        .iter()
        .copied()
        .filter(|line| line.starts_with("encode"))
        .collect();
    assert_eq!(encodes, ["encode @assets zstd gzip"]);
    assert!(site.contains(&"@assets path /assets/*"), "{site:?}");
}

/// The block fenced as `fence` that comes first after `title` in the notes.
fn block_after(title: &str, fence: &str) -> String {
    let start = NOTES
        .find(title)
        .unwrap_or_else(|| panic!("the notes have {title}"));
    let mut lines = NOTES[start..].lines().skip_while(|line| *line != fence);
    lines.next();
    let mut block: Vec<&str> = lines.take_while(|line| !line.starts_with("```")).collect();
    block.push("");
    block.join("\n")
}

/// The value of the first `key` line in the `[section]` of the unit file
/// `block`: a setting in the wrong section means something else to
/// systemd, or nothing.
fn setting<'a>(block: &'a str, section: &str, key: &str) -> &'a str {
    let header = format!("[{section}]");
    block
        .lines()
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .find_map(|line| line.strip_prefix(key))
        .unwrap_or_else(|| panic!("no {key} in {header} of {block}"))
}

/// The signal file the notes have the portal write.
fn signal_in_the_notes() -> &'static str {
    let flag = "`--new-message-signal ";
    let start = NOTES.find(flag).expect("the notes give the flag") + flag.len();
    let rest = &NOTES[start..];
    &rest[..rest.find('`').unwrap()]
}

#[test]
fn the_notice_watches_the_file_the_portal_writes_where_it_may_write() {
    let signal = signal_in_the_notes();
    // The flag, added to the unit's command line, is one the server takes.
    let unit = block_after("`/etc/systemd/system/mooring-portal.service`", "```ini");
    let command = setting(&unit, "Service", "ExecStart=/usr/local/bin/mooring-portal ");
    let argv = std::iter::once("mooring-portal")
        .chain(command.split_whitespace())
        .chain(["--new-message-signal", signal]);
    let Command::Serve(args) = Cli::try_parse_from(argv).unwrap().command else {
        panic!("not serve");
    };
    let config = crate::serve::config(&args).unwrap();
    assert_eq!(
        config.new_message_signal.unwrap().path(),
        std::path::Path::new(signal)
    );
    // The path unit watches that file, which is in the directory the
    // portal may write, and made before the portal first writes it.
    let watch = block_after("`/etc/systemd/system/mooring-portal-notify.path`", "```ini");
    assert_eq!(setting(&watch, "Path", "PathChanged="), signal);
    assert_eq!(
        setting(&watch, "Path", "Unit="),
        "mooring-portal-notify.service"
    );
    let writable = setting(&unit, "Service", "ReadWritePaths=");
    assert_eq!(
        std::path::Path::new(signal).parent(),
        Some(std::path::Path::new(writable))
    );
    assert!(NOTES.contains(&format!(
        "install -o mooring-portal -g mooring-portal -m 0600 /dev/null {signal}"
    )));
    // The service reads the token as systemd hands it, and nothing of the
    // portal's.
    let service = block_after(
        "`/etc/systemd/system/mooring-portal-notify.service`",
        "```ini",
    );
    assert_eq!(
        setting(&service, "Service", "LoadCredential="),
        "telegram:/etc/mooring-portal/telegram"
    );
    assert_eq!(
        setting(&service, "Service", "ExecStart="),
        "/usr/local/lib/mooring-portal/notify-telegram"
    );
    assert_eq!(setting(&service, "Service", "DynamicUser="), "yes");
    assert_eq!(
        setting(&service, "Service", "InaccessiblePaths="),
        format!("-{writable}")
    );
    // A start limit is a setting of the unit, not of its service.
    assert_eq!(setting(&service, "Unit", "StartLimitIntervalSec="), "0");
}

/// A directory with a stand-in for curl, which writes down how it was
/// called, and prints `answer`.
struct FakeCurl {
    dir: tempfile::TempDir,
}

impl FakeCurl {
    fn new(answer: &str) -> Self {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        let curl = dir.path().join("bin/curl");
        std::fs::write(
            &curl,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CALLED/argv\"\ncat > \"$CALLED/stdin\"\n\
                 printf '%s' '{answer}'\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }

    /// Runs `script` with `sh`, with the credential `telegram` where
    /// systemd puts it, and this curl first on the path.
    fn run(&self, script: &str, telegram: &str) -> std::process::Output {
        let credentials = self.dir.path().join("credentials");
        std::fs::create_dir_all(&credentials).unwrap();
        std::fs::write(credentials.join("telegram"), telegram).unwrap();
        let path = format!("{}:/usr/bin:/bin", self.dir.path().join("bin").display());
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .env_clear()
            .env("PATH", path)
            .env("LC_ALL", "C")
            .env("CREDENTIALS_DIRECTORY", &credentials)
            .env("CALLED", self.dir.path())
            .output()
            .unwrap()
    }

    /// How curl was called: its arguments, and its standard input.
    fn called(&self) -> Option<(Vec<String>, String)> {
        let argv = std::fs::read_to_string(self.dir.path().join("argv")).ok()?;
        let stdin = std::fs::read_to_string(self.dir.path().join("stdin")).unwrap();
        Some((argv.lines().map(str::to_owned).collect(), stdin))
    }
}

/// A token as @BotFather writes them, made up.
const TOKEN: &str = "123456789:AAH-made_up-token";

#[test]
fn the_notice_hands_curl_the_token_on_its_standard_input_alone() {
    let script = block_after("`/usr/local/lib/mooring-portal/notify-telegram`", "```sh");
    assert!(script.starts_with("#!/bin/sh\n"));
    let curl = FakeCurl::new("{\"ok\":true}");
    let ran = curl.run(&script, &format!("token={TOKEN}\nchat=-1001234\n"));
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let (argv, stdin) = curl.called().unwrap();
    assert_eq!(
        stdin,
        format!(
            "url = \"https://api.telegram.org/bot{TOKEN}/sendMessage\"\n\
             data-urlencode = \"chat_id=-1001234\"\n"
        )
    );
    assert!(
        argv.iter()
            .all(|arg| !arg.contains(TOKEN) && !arg.contains("1001234"))
    );
    for expected in [
        "--config",
        "-",
        "--fail",
        "--proto",
        "=https",
        "text=泊光有新消息，请到 https://moorlight.app/operator/feedback 查看。",
        "link_preview_options={\"is_disabled\":true}",
    ] {
        assert!(
            argv.iter().any(|arg| arg == expected),
            "{expected}: {argv:?}"
        );
    }
    // A credential written wrong is refused before curl runs.
    for (telegram, said) in [
        ("chat=1\n".to_owned(), "token="),
        (format!("token={TOKEN}\n"), "chat="),
        (format!("token={TOKEN} \nchat=1\n"), "token="),
        (format!("token={TOKEN}\r\nchat=1\r\n"), "token="),
        (format!("token={TOKEN}\"x\nchat=1\n"), "token="),
        (format!("token={TOKEN}\ntoken={TOKEN}\nchat=1\n"), "token="),
        (format!("token={TOKEN}\nchat=12a\n"), "chat="),
    ] {
        let curl = FakeCurl::new("");
        let ran = curl.run(&script, &telegram);
        assert!(!ran.status.success(), "{telegram:?}");
        let stderr = String::from_utf8_lossy(&ran.stderr);
        assert!(
            stderr.contains(&format!("needs one line {said}")),
            "{stderr}"
        );
        assert!(curl.called().is_none(), "{telegram:?}");
    }
}

#[test]
fn the_notes_find_the_operators_chat_without_the_token_on_a_command_line() {
    let line = NOTES
        .lines()
        .find(|line| line.starts_with("sed -n 's|^token="))
        .expect("the notes find the chat");
    let curl = FakeCurl::new(
        "{\"ok\":true,\"result\":[{\"update_id\":1,\"message\":{\"message_id\":2,\
         \"chat\":{\"id\":123456789,\"type\":\"private\"},\"date\":3,\"text\":\"hi\"}}]}",
    );
    let credential = curl.dir.path().join("credentials/telegram");
    let script = line.replace("/etc/mooring-portal/telegram", credential.to_str().unwrap());
    let ran = curl.run(&script, &format!("token={TOKEN}\n"));
    assert!(ran.status.success());
    assert_eq!(
        String::from_utf8(ran.stdout).unwrap(),
        "\"chat\":{\"id\":123456789\n"
    );
    let (argv, stdin) = curl.called().unwrap();
    assert_eq!(
        stdin,
        format!("url = \"https://api.telegram.org/bot{TOKEN}/getUpdates\"\n")
    );
    assert!(argv.iter().all(|arg| !arg.contains(TOKEN)));
}
