//! The notices of a new message (`docs/notices.md`): the units that watch
//! the file the portal writes, and the scripts that tell the operator on
//! Telegram and in Feishu. No unit runs here, so these tests hold the notes
//! to what the portal writes and may write, and run each script against a
//! stand-in for curl.

use clap::Parser;

use super::{block_after, setting};
use crate::cli::{Cli, Command};

/// How the portal itself is run; the notices watch its file.
const DEPLOYMENT: &str = include_str!("../../../../docs/deploy.md");
const NOTES: &str = include_str!("../../../../docs/notices.md");

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
    let unit = block_after(
        DEPLOYMENT,
        "`/etc/systemd/system/mooring-portal.service`",
        "```ini",
    );
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
    // The deployment notes name the same file where they record the flag.
    assert!(DEPLOYMENT.contains(&format!("`--new-message-signal {signal}`")));
    // The path unit watches that file, which is in the directory the
    // portal may write, and made before the portal first writes it.
    let watch = block_after(
        NOTES,
        "`/etc/systemd/system/mooring-portal-notify.path`",
        "```ini",
    );
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
        NOTES,
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
/// called, prints an answer and ends as it is told to.
struct FakeCurl {
    dir: tempfile::TempDir,
}

impl FakeCurl {
    /// A curl that prints `answer` and succeeds.
    fn new(answer: &str) -> Self {
        Self::ending(answer, 0)
    }

    /// A curl that fails as curl does when the server answers an error:
    /// with `--fail` it prints nothing and ends with 22.
    fn failing() -> Self {
        Self::ending("", 22)
    }

    fn ending(answer: &str, status: u8) -> Self {
        use std::os::unix::fs::PermissionsExt as _;
        // The answer stands between single quotes in the stand-in.
        assert!(!answer.contains('\''), "{answer}");
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        let curl = dir.path().join("bin/curl");
        std::fs::write(
            &curl,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$CALLED/argv\"\ncat > \"$CALLED/stdin\"\n\
                 printf '%s' '{answer}'\nexit {status}\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }

    /// Runs `script` with `sh`, with `credential` under `name` where
    /// systemd puts it, and this curl first on the path.
    fn run(&self, script: &str, name: &str, credential: &str) -> std::process::Output {
        let credentials = self.dir.path().join("credentials");
        std::fs::create_dir_all(&credentials).unwrap();
        std::fs::write(credentials.join(name), credential).unwrap();
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
    let script = block_after(
        NOTES,
        "`/usr/local/lib/mooring-portal/notify-telegram`",
        "```sh",
    );
    assert!(script.starts_with("#!/bin/sh\n"));
    let credential = format!("token={TOKEN}\nchat=-1001234\n");
    let curl = FakeCurl::new("{\"ok\":true}");
    let ran = curl.run(&script, "telegram", &credential);
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
    // A notice Telegram does not take fails, for the journal to show.
    let ran = FakeCurl::failing().run(&script, "telegram", &credential);
    assert_eq!(ran.status.code(), Some(22));
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
        let ran = curl.run(&script, "telegram", &telegram);
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
    let ran = curl.run(&script, "telegram", &format!("token={TOKEN}\n"));
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

/// Webhook addresses as Feishu and Lark write them, made up.
const FEISHU_HOOK: &str =
    "https://open.feishu.cn/open-apis/bot/v2/hook/0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
const LARK_HOOK: &str =
    "https://open.larksuite.com/open-apis/bot/v2/hook/0a1b2c3d-4e5f-6071-8293-a4b5c6d7e8f9";
/// What Feishu answers when it has taken a message, as its page on custom
/// bots gives it.
const TAKEN: &str =
    "{\"StatusCode\":0,\"StatusMessage\":\"success\",\"code\":0,\"data\":{},\"msg\":\"success\"}";
/// The sentence the operator's group is to show.
const FEISHU_SENTENCE: &str = "泊光有新消息，请到运营页面的「反馈」查看。";

/// The script that tells the operator in Feishu.
fn feishu_script() -> String {
    let script = block_after(
        NOTES,
        "`/usr/local/lib/mooring-portal/notify-feishu`",
        "```sh",
    );
    assert!(script.starts_with("#!/bin/sh\n"));
    script
}

#[test]
fn the_feishu_notice_watches_the_same_file_from_units_of_its_own() {
    let watch = block_after(
        NOTES,
        "`/etc/systemd/system/mooring-portal-notify-feishu.path`",
        "```ini",
    );
    assert_eq!(
        setting(&watch, "Path", "PathChanged="),
        signal_in_the_notes()
    );
    assert_eq!(
        setting(&watch, "Path", "Unit="),
        "mooring-portal-notify-feishu.service"
    );
    assert_eq!(setting(&watch, "Install", "WantedBy="), "paths.target");
    // Its service is Telegram's but for what it says of itself, the
    // credential it is handed and the script it runs: the same account of
    // its own, behind the same walls.
    let telegram = block_after(
        NOTES,
        "`/etc/systemd/system/mooring-portal-notify.service`",
        "```ini",
    );
    let feishu = block_after(
        NOTES,
        "`/etc/systemd/system/mooring-portal-notify-feishu.service`",
        "```ini",
    );
    assert_eq!(telegram.lines().count(), feishu.lines().count());
    let differing: Vec<(&str, &str)> = telegram
        .lines()
        .zip(feishu.lines())
        .filter(|(theirs, ours)| theirs != ours)
        .collect();
    assert_eq!(
        differing,
        [
            (
                "Description=Tell the Moorlight operator on Telegram that a message waits",
                "Description=Tell the Moorlight operator in Feishu that a message waits"
            ),
            (
                "LoadCredential=telegram:/etc/mooring-portal/telegram",
                "LoadCredential=feishu:/etc/mooring-portal/feishu"
            ),
            (
                "ExecStart=/usr/local/lib/mooring-portal/notify-telegram",
                "ExecStart=/usr/local/lib/mooring-portal/notify-feishu"
            ),
        ]
    );
    // The notes have the operator put the address where the service looks.
    assert!(NOTES.contains("install -o root -g root -m 0600 /dev/null /etc/mooring-portal/feishu"));
}

#[test]
fn the_feishu_notice_hands_curl_the_address_on_its_standard_input_alone() {
    let script = feishu_script();
    for hook in [FEISHU_HOOK, LARK_HOOK] {
        let curl = FakeCurl::new(TAKEN);
        let ran = curl.run(&script, "feishu", &format!("url={hook}\n"));
        assert!(
            ran.status.success(),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        let (argv, stdin) = curl.called().unwrap();
        assert_eq!(stdin, format!("url = \"{hook}\"\n"));
        assert!(
            argv.iter()
                .all(|arg| !arg.contains("hook") && !arg.contains("0a1b2c3d")),
            "{argv:?}"
        );
        for expected in [
            "--config",
            "-",
            "--fail",
            "--proto",
            "=https",
            "Content-Type: application/json",
            &format!("{{\"msg_type\":\"text\",\"content\":{{\"text\":\"{FEISHU_SENTENCE}\"}}}}"),
        ] {
            assert!(
                argv.iter().any(|arg| arg == expected),
                "{expected}: {argv:?}"
            );
        }
    }
}

#[test]
fn the_feishu_notice_has_the_bots_keyword_and_no_address() {
    // Feishu takes a message only if it has the keyword the operator gave
    // the bot. Every place the notes name the keyword names the same one.
    let lead = "\"自定义关键词\"，填 `";
    let keywords: Vec<&str> = NOTES
        .match_indices(lead)
        .map(|(at, _)| {
            let rest = &NOTES[at + lead.len()..];
            &rest[..rest.find('`').unwrap()]
        })
        .collect();
    assert_eq!(keywords, ["泊光", "泊光"]);
    assert!(FEISHU_SENTENCE.contains(keywords[0]));
    // The notice leads Feishu's servers nowhere.
    assert!(!FEISHU_SENTENCE.contains("http") && !FEISHU_SENTENCE.contains("moorlight"));
    // The script sends that sentence, and the notes tell the operator to
    // look for it in the group.
    assert_eq!(feishu_script().matches(FEISHU_SENTENCE).count(), 1);
    assert_eq!(NOTES.matches(FEISHU_SENTENCE).count(), 2);
}

#[test]
fn the_feishu_notice_counts_only_an_answer_of_code_0_as_sent() {
    let script = feishu_script();
    let credential = format!("url={FEISHU_HOOK}\n");
    for taken in [
        TAKEN,
        "{\n  \"StatusCode\": 0,\n  \"code\": 0,\n  \"data\": {},\n  \"msg\": \"success\"\n}",
        "{\"msg\":\"success\",\"code\":0}",
    ] {
        let ran = FakeCurl::new(taken).run(&script, "feishu", &credential);
        assert!(
            ran.status.success(),
            "{taken}: {}",
            String::from_utf8_lossy(&ran.stderr)
        );
    }
    // Feishu's own refusals, an answer that is not its own, and none.
    for refused in [
        "{\"code\":19024,\"msg\":\"Key Words Not Found\"}",
        "{\n  \"code\": 19022,\n  \"msg\": \"Ip Not Allowed\"\n}",
        "{\"code\":9499,\"msg\":\"Bad Request\",\"data\":{}}",
        "{\"StatusCode\":0,\"StatusMessage\":\"success\",\"code\":19021,\"msg\":\"sign match fail\"}",
        "{\"code\":01}",
        "<html>gateway</html>",
        "",
    ] {
        let curl = FakeCurl::new(refused);
        let ran = curl.run(&script, "feishu", &credential);
        assert_eq!(ran.status.code(), Some(1), "{refused}");
        let stderr = String::from_utf8_lossy(&ran.stderr);
        assert!(
            stderr.contains(&format!(
                "notify-feishu: Feishu did not take the notice: {refused}"
            )),
            "{stderr}"
        );
        // It was asked, and the address stayed off its command line.
        let (argv, _) = curl.called().unwrap();
        assert!(argv.iter().all(|arg| !arg.contains("hook")));
    }
    // When curl itself fails, so does the script, with curl's own word.
    let ran = FakeCurl::failing().run(&script, "feishu", &credential);
    assert_eq!(ran.status.code(), Some(22));
    assert!(!String::from_utf8_lossy(&ran.stderr).contains("did not take"));
}

#[test]
fn the_feishu_notice_refuses_an_address_written_wrong_before_curl_runs() {
    let script = feishu_script();
    for feishu in [
        String::new(),
        "url=\n".to_owned(),
        // Another key, or the key not at the line's start.
        format!("URL={FEISHU_HOOK}\n"),
        format!(" url={FEISHU_HOOK}\n"),
        // Not https, another host, a host that only starts like Feishu's.
        format!("url={}\n", FEISHU_HOOK.replace("https://", "http://")),
        "url=https://example.com/open-apis/bot/v2/hook/0a1b2c3d\n".to_owned(),
        "url=https://open.feishu.cn.example.com/open-apis/bot/v2/hook/0a1b2c3d\n".to_owned(),
        // No hook, or more than a hook after the fixed path.
        "url=https://open.feishu.cn/open-apis/bot/v2/hook/\n".to_owned(),
        format!("url={FEISHU_HOOK}/more\n"),
        format!("url={FEISHU_HOOK}?x=1\n"),
        // What an editor or a careless paste leaves.
        format!("url={FEISHU_HOOK} \n"),
        format!("url={FEISHU_HOOK}\r\n"),
        format!("url={FEISHU_HOOK}\"\n"),
        format!("url={FEISHU_HOOK}\nurl={LARK_HOOK}\n"),
    ] {
        let curl = FakeCurl::new(TAKEN);
        let ran = curl.run(&script, "feishu", &feishu);
        assert_eq!(ran.status.code(), Some(1), "{feishu:?}");
        let stderr = String::from_utf8_lossy(&ran.stderr);
        assert!(
            stderr.contains("needs one line url="),
            "{feishu:?}: {stderr}"
        );
        assert!(curl.called().is_none(), "{feishu:?}");
    }
}
