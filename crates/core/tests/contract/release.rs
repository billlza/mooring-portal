//! What the portal reads from GitHub's description of a CFM release: the
//! description GitHub gave of CFM 0.5.1, and one refusal per rule.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use portal_core::json::Malformation;
use portal_core::release::{CfmRelease, MAX_RELEASE_BYTES, REPOSITORY, ReleaseError};

fn release_path(sub: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "../../contract/fixtures/cfm-release",
        sub,
    ]
    .iter()
    .collect()
}

fn release_fixture(path: &str) -> Vec<u8> {
    let file = release_path(path);
    fs::read(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// Each refused description, by file name, and why it is refused.
const REFUSED: [(&str, ReleaseError); 37] = [
    ("draft", ReleaseError::Draft),
    ("prerelease", ReleaseError::Prerelease),
    ("not-published", ReleaseError::PublishedAt),
    ("published-with-offset", ReleaseError::PublishedAt),
    ("published-on-no-day", ReleaseError::PublishedAt),
    ("published-at-no-hour", ReleaseError::PublishedAt),
    ("tag-without-v", ReleaseError::Tag),
    ("tag-prerelease", ReleaseError::Tag),
    ("tag-two-parts", ReleaseError::Tag),
    ("tag-leading-zero", ReleaseError::Tag),
    ("tag-too-large", ReleaseError::Tag),
    ("page-of-another-repository", ReleaseError::Page),
    ("page-of-another-tag", ReleaseError::Page),
    ("no-tag", ReleaseError::Malformed(Malformation::Shape)),
    ("no-assets", ReleaseError::Malformed(Malformation::Shape)),
    ("no-dmg", ReleaseError::NoDiskImage),
    ("two-dmgs", ReleaseError::DiskImages),
    ("dmg-of-another-version", ReleaseError::NoDiskImage),
    ("dmg-not-uploaded", ReleaseError::NotUploaded),
    ("dmg-empty", ReleaseError::Empty),
    ("dmg-no-digest", ReleaseError::Digest),
    ("dmg-digest-key-missing", ReleaseError::Digest),
    ("dmg-digest-sha512", ReleaseError::Digest),
    ("dmg-digest-other-algorithm", ReleaseError::Digest),
    ("dmg-digest-no-algorithm", ReleaseError::Digest),
    ("dmg-digest-uppercase", ReleaseError::Digest),
    ("dmg-digest-short", ReleaseError::Digest),
    ("dmg-digest-not-hex", ReleaseError::Digest),
    ("dmg-from-another-host", ReleaseError::Download),
    ("dmg-from-another-repository", ReleaseError::Download),
    ("dmg-from-another-tag", ReleaseError::Download),
    ("dmg-over-http", ReleaseError::Download),
    (
        "draft-as-text",
        ReleaseError::Malformed(Malformation::Shape),
    ),
    ("size-as-text", ReleaseError::Malformed(Malformation::Shape)),
    (
        "truncated",
        ReleaseError::Malformed(Malformation::Truncated),
    ),
    (
        "not-an-object",
        ReleaseError::Malformed(Malformation::Shape),
    ),
    (
        "release-as-text",
        ReleaseError::Malformed(Malformation::Shape),
    ),
];

#[test]
fn the_latest_release_is_read_as_github_describes_it() {
    let release = CfmRelease::from_github(&release_fixture("valid/v0.5.1.json")).unwrap();
    assert_eq!(release.version().to_string(), "0.5.1");
    assert_eq!(release.published_at().to_string(), "2026-11-07T15:27:01Z");
    assert_eq!(
        release.dmg_sha256(),
        "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f"
    );
    // The addresses are built from the version and the fixed repository.
    assert_eq!(REPOSITORY, "https://github.com/billlza/cfw-rs");
    assert_eq!(release.dmg_name(), "Clash.for.Mac_0.5.1_arm64.dmg");
    assert_eq!(
        release.dmg_url(),
        "https://github.com/billlza/cfw-rs/releases/download/v0.5.1/Clash.for.Mac_0.5.1_arm64.dmg"
    );
    assert_eq!(
        release.page_url(),
        "https://github.com/billlza/cfw-rs/releases/tag/v0.5.1"
    );
}

#[test]
fn every_rule_refuses_its_description() {
    for (name, why) in REFUSED {
        let description = release_fixture(&format!("invalid/{name}.json"));
        assert_eq!(CfmRelease::from_github(&description), Err(why), "{name}");
    }
    // Every refused description on disk is in the table.
    let on_disk: BTreeSet<String> = fs::read_dir(release_path("invalid"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    let listed: BTreeSet<String> = REFUSED
        .iter()
        .map(|(name, _)| format!("{name}.json"))
        .collect();
    assert_eq!(on_disk, listed);
}

#[test]
fn no_refusal_repeats_the_description() {
    // A refusal goes to a terminal or a log, and the description is a
    // stranger's text.
    for (name, why) in REFUSED {
        let message = why.to_string();
        for theirs in [
            "someone-else",
            "example.com",
            "rc.1",
            "starter",
            "sha512",
            "0.5.0",
            "4294967296",
            "+08:00",
        ] {
            assert!(!message.contains(theirs), "{name}: {message}");
        }
    }
}

#[test]
fn only_the_disk_image_is_looked_at_among_the_files() {
    let mut description: serde_json::Value =
        serde_json::from_slice(&release_fixture("valid/v0.5.1.json")).unwrap();
    let assets = description["assets"].as_array_mut().unwrap();
    assert!(assets.len() > 10);
    for asset in assets.iter_mut() {
        if asset["name"] != "Clash.for.Mac_0.5.1_arm64.dmg" {
            asset["digest"] = serde_json::Value::Null;
            asset["state"] = "starter".into();
            asset["size"] = 0.into();
            asset["browser_download_url"] = "https://example.com/elsewhere".into();
        }
    }
    let read = CfmRelease::from_github(description.to_string().as_bytes()).unwrap();
    assert_eq!(read.version().to_string(), "0.5.1");
    // A file whose name only contains the disk image's is another file.
    description["assets"]
        .as_array_mut()
        .unwrap()
        .retain(|asset| asset["name"] != "Clash.for.Mac_0.5.1_arm64.dmg");
    assert_eq!(
        CfmRelease::from_github(description.to_string().as_bytes()),
        Err(ReleaseError::NoDiskImage)
    );
}

#[test]
fn a_description_too_large_is_refused_unread() {
    let mut padded = release_fixture("valid/v0.5.1.json");
    padded.resize(MAX_RELEASE_BYTES, b' ');
    assert!(CfmRelease::from_github(&padded).is_ok());
    padded.push(b' ');
    assert_eq!(
        CfmRelease::from_github(&padded),
        Err(ReleaseError::TooLarge)
    );
}
