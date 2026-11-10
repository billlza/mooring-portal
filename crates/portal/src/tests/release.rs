//! `import-release`: the download page follows GitHub's latest release.

use std::path::PathBuf;
use std::process::ExitCode;

use portal_core::release::{MAX_RELEASE_BYTES, REPOSITORY};
use portal_core::time::UnixTime;

use super::{Fixture, NOW};

const SHA: &str = "c59907ad90eaad911e3660d3c27df982205c5b81cfde743f16dee5f7a985069f";
const OTHER_SHA: &str = "fc9a1e62700a1b5f66ec42fcda0e5eb90efc052251321927f52b97c9223b7b93";
const PUBLISHED: &str = "2026-11-07T15:27:01Z";

fn release_fixture(name: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contract/fixtures/cfm-release")
        .join(name)
}

/// GitHub's description of CFM 0.5.1, rewritten for another version, time
/// of publication or disk image.
fn described(version: &str, published: &str, sha: &str) -> Vec<u8> {
    let text = std::fs::read_to_string(release_fixture("valid/v0.5.1.json")).unwrap();
    let published_key = format!("\"published_at\": \"{PUBLISHED}\"");
    assert!(text.contains(&published_key) && text.contains(SHA));
    text.replace("0.5.1", version)
        .replace(
            &published_key,
            &format!("\"published_at\": \"{published}\""),
        )
        .replace(SHA, sha)
        .into_bytes()
}

impl Fixture {
    /// Imports `description` as `latest.json`.
    fn import_release(&self, description: &[u8]) -> super::Outcome {
        let file = self.file("latest.json", description);
        self.run(&["import-release", file.to_str().unwrap()])
    }

    /// The version on offer and when it was last checked.
    fn offered(&self) -> Option<(String, u64)> {
        self.store().offered_release().unwrap().map(|offered| {
            (
                offered.release.version().to_string(),
                offered.checked_at.as_secs(),
            )
        })
    }
}

#[test]
fn the_latest_release_goes_on_the_page_and_finding_it_again_only_moves_the_check() {
    let fixture = Fixture::new();
    assert_eq!(fixture.offered(), None);
    let file = release_fixture("valid/v0.5.1.json");
    let first = fixture.run(&["import-release", file.to_str().unwrap()]);
    assert_eq!(first.code, ExitCode::SUCCESS, "{}", first.err);
    assert_eq!(
        first.out,
        "CFM 0.5.1 (published 2026-11-07T15:27:01Z) is now on the download page\n"
    );
    assert_eq!(first.err, "");
    assert_eq!(fixture.offered(), Some(("0.5.1".to_owned(), NOW)));
    let offered = fixture.store().offered_release().unwrap().unwrap().release;
    assert_eq!(offered.dmg_sha256(), SHA);
    assert_eq!(
        offered.published_at(),
        UnixTime::parse_utc(PUBLISHED).unwrap()
    );
    // The description is the host's file to keep or delete, not the command's.
    assert!(file.exists());

    fixture.clock.advance(300).unwrap();
    let again = fixture.run(&["import-release", file.to_str().unwrap()]);
    assert_eq!(again.code, ExitCode::SUCCESS);
    assert_eq!(
        again.out,
        "CFM 0.5.1 is still the release described; the download page is unchanged\n"
    );
    assert_eq!(fixture.offered(), Some(("0.5.1".to_owned(), NOW + 300)));
}

#[test]
fn a_newer_an_older_and_a_republished_release_are_each_said() {
    let fixture = Fixture::new();
    fixture.import_release(&described("0.5.1", PUBLISHED, SHA));
    let newer = fixture.import_release(&described("0.5.10", "2026-12-01T08:00:00Z", OTHER_SHA));
    assert_eq!(newer.code, ExitCode::SUCCESS, "{}", newer.err);
    assert_eq!(
        newer.out,
        "CFM 0.5.10 (published 2026-12-01T08:00:00Z) replaces 0.5.1 on the download page\n"
    );
    assert_eq!(fixture.offered().unwrap().0, "0.5.10");

    // GitHub's latest is older than what the page offers: the newer
    // release was withdrawn, and the page goes back with it.
    let older = fixture.import_release(&described("0.5.1", PUBLISHED, SHA));
    assert_eq!(older.code, ExitCode::SUCCESS);
    assert_eq!(
        older.out,
        "warning: the release described is CFM 0.5.1 (published 2026-11-07T15:27:01Z), older \
         than the 0.5.10 on the download page; the page now offers 0.5.1\n"
    );
    assert_eq!(fixture.offered().unwrap().0, "0.5.1");

    // The same version with another disk image.
    let swapped = fixture.import_release(&described("0.5.1", PUBLISHED, OTHER_SHA));
    assert_eq!(swapped.code, ExitCode::SUCCESS);
    assert_eq!(
        swapped.out,
        "warning: the description of CFM 0.5.1 is not the one imported before: its disk \
         image or its time of publication (now 2026-11-07T15:27:01Z) has changed; the \
         download page follows it\n"
    );
    let offered = fixture.store().offered_release().unwrap().unwrap().release;
    assert_eq!(offered.dmg_sha256(), OTHER_SHA);
}

#[test]
fn the_deployment_notes_fetch_from_the_repository_the_portal_reads() {
    // The repository is named in the code, which builds the page's
    // addresses from it, and again in the unit that fetches the
    // description. A fetch from anywhere else would be refused, every
    // five minutes, for not being this repository's.
    let notes = include_str!("../../../../docs/releases.md");
    let named = REPOSITORY.strip_prefix("https://github.com/").unwrap();
    let fetched = format!("https://api.github.com/repos/{named}/releases/latest");
    assert_eq!(notes.matches("https://api.github.com/").count(), 1);
    assert!(notes.contains(&format!(" {fetched}\n")), "{fetched}");
}

#[test]
fn a_description_that_is_refused_changes_nothing_and_repeats_nothing() {
    let fixture = Fixture::new();
    fixture.import_release(&described("0.5.1", PUBLISHED, SHA));
    for (name, why) in [
        ("draft", "the release is a draft"),
        ("prerelease", "the release is a pre-release"),
        (
            "dmg-from-another-repository",
            "the release's disk image is not downloaded from the release in \
             https://github.com/billlza/cfw-rs",
        ),
        (
            "truncated",
            "the description is not a release as GitHub describes one: it ends too early",
        ),
    ] {
        let file = release_fixture(&format!("invalid/{name}.json"));
        let outcome = fixture.run(&["import-release", file.to_str().unwrap()]);
        assert_eq!(outcome.code, ExitCode::FAILURE, "{name}");
        assert_eq!(outcome.out, "", "{name}");
        assert_eq!(
            outcome.err,
            format!("mooring-portal: {}: {why}\n", file.display()),
            "{name}"
        );
        assert!(!outcome.err.contains("someone-else"));
    }
    // Neither the release nor the time it was last found to be the latest
    // has moved: a failing check must come to show on the page.
    fixture.clock.advance(900).unwrap();
    let draft = release_fixture("invalid/draft.json");
    fixture.run(&["import-release", draft.to_str().unwrap()]);
    assert_eq!(fixture.offered(), Some(("0.5.1".to_owned(), NOW)));
}

#[test]
fn a_missing_file_and_one_too_large_are_refused() {
    let fixture = Fixture::new();
    let missing = fixture.dir.path().join("absent.json");
    let outcome = fixture.run(&["import-release", missing.to_str().unwrap()]);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome.err.starts_with("mooring-portal: cannot read ") && outcome.err.contains("absent"),
        "{}",
        outcome.err
    );
    let mut padded = described("0.5.1", PUBLISHED, SHA);
    padded.resize(MAX_RELEASE_BYTES + 1, b' ');
    let outcome = fixture.import_release(&padded);
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome
            .err
            .ends_with("the description is larger than the 1048576 bytes a release's may be\n"),
        "{}",
        outcome.err
    );
    // A megabyte exactly is read.
    padded.truncate(MAX_RELEASE_BYTES);
    assert_eq!(fixture.import_release(&padded).code, ExitCode::SUCCESS);
    assert_eq!(fixture.offered().unwrap().0, "0.5.1");
    // The command needs its file, and a database that exists.
    assert_eq!(fixture.run(&["import-release"]).code, ExitCode::from(2));
    let nowhere = fixture.dir.path().join("no.db");
    let file = release_fixture("valid/v0.5.1.json");
    let outcome = super::run_line(
        &[
            "mooring-portal",
            "--db",
            nowhere.to_str().unwrap(),
            "import-release",
            file.to_str().unwrap(),
        ],
        &fixture.clock,
    );
    assert_eq!(outcome.code, ExitCode::FAILURE);
    assert!(
        outcome.err.contains("there is no database at"),
        "{}",
        outcome.err
    );
    assert!(!nowhere.exists());
}
