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
