//! The golden cases of the CFM profile bundle contract: the valid bundle,
//! and one refusal per rule.

use std::fs;
use std::path::PathBuf;

use portal_core::profile::{DocumentError, MAX_BUNDLE_BYTES, ProfileBundle, ProfileError};

/// The directory of the version-1 profile fixtures.
fn profile_path(sub: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "../../contract/fixtures/cfm-profiles-v1",
        sub,
    ]
    .iter()
    .collect()
}

fn profile_fixture(path: &str) -> String {
    let file = profile_path(path);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

#[test]
fn each_document_is_kept_as_the_exact_text_mooring_wrote() {
    let text = profile_fixture("valid/pilot.json");
    let bundle = ProfileBundle::parse(text.as_bytes()).unwrap();
    assert_eq!(bundle.generated_at().to_string(), "2026-11-06T09:00:00Z");
    let source: serde_json::Value = serde_json::from_str(&text).unwrap();
    let users: Vec<&str> = bundle.profiles().map(|(user, _)| user.as_str()).collect();
    assert_eq!(users, ["pilotusera", "pilotuserb"]);
    for (index, (user, profile)) in bundle.profiles().enumerate() {
        // Byte for byte: the document is a slice of the bundle's text...
        assert!(text.contains(profile.as_str()), "{user}");
        assert!(profile.as_str().starts_with("{\n"), "{user}");
        // ...and it is the user's own entry.
        let served: serde_json::Value = serde_json::from_str(profile.as_str()).unwrap();
        assert_eq!(served, source["profiles"][index]["document"], "{user}");
        assert_eq!(source["profiles"][index]["user"], user.as_str());
    }
}

/// What an invalid fixture must be refused with.
enum Refusal {
    /// A problem with a key of the bundle, named in the decoder's message.
    Field(&'static str),
    /// A rule of the contract.
    Rule(fn(&ProfileError) -> bool),
}

#[test]
fn every_invalid_bundle_is_refused_for_its_rule() {
    use Refusal::{Field, Rule};
    let cases = [
        (
            "wrong-format",
            Rule(
                |e| matches!(e, ProfileError::WrongFormat { found } if found == "mooring-portal-snapshot"),
            ),
        ),
        (
            "version-2",
            Rule(|e| matches!(e, ProfileError::UnsupportedVersion { found: 2 })),
        ),
        ("missing-key", Field("missing field `generated_at`")),
        ("unknown-key", Field("unknown field `nodes`")),
        ("unknown-entry-key", Field("unknown field `alias`")),
        (
            "alias-as-user",
            Field("\"k7qd-m2xa\" is not a Mooring user id"),
        ),
        ("truncated", Field("EOF while parsing")),
        (
            "no-profiles",
            Rule(|e| matches!(e, ProfileError::NoProfiles)),
        ),
        (
            "duplicate-user",
            Rule(
                |e| matches!(e, ProfileError::DuplicateUser { user } if user.as_str() == "pilotusera"),
            ),
        ),
        (
            "full-sing-box-config",
            Rule(|e| {
                refused_document(e, |reason| {
                    matches!(reason, DocumentError::NotNodeList { .. })
                })
            }),
        ),
        (
            "clash-yaml-as-text",
            Rule(|e| {
                refused_document(e, |reason| {
                    matches!(reason, DocumentError::NotNodeList { .. })
                })
            }),
        ),
        (
            "no-outbounds",
            Rule(|e| refused_document(e, |reason| *reason == DocumentError::NoOutbounds)),
        ),
        (
            "unnamed-outbound",
            Rule(|e| refused_document(e, |reason| *reason == DocumentError::Unnamed { index: 0 })),
        ),
    ];
    let listed = fs::read_dir(profile_path("invalid")).unwrap().count();
    assert_eq!(listed, cases.len(), "every invalid fixture has a case");
    for (name, refusal) in cases {
        let text = profile_fixture(&format!("invalid/{name}.json"));
        let error = ProfileBundle::parse(text.as_bytes()).expect_err(name);
        match refusal {
            Field(message) => {
                assert!(matches!(error, ProfileError::Json(_)), "{name}: {error}");
                assert!(error.to_string().contains(message), "{name}: {error}");
            }
            Rule(check) => assert!(check(&error), "{name}: {error}"),
        }
        // No refusal quotes a credential of the fixture's documents.
        for secret in ["00000000-0000-4000-8000-00000000000", "synthetic-"] {
            assert!(!error.to_string().contains(secret), "{name}: {error}");
        }
    }
}

/// Whether `error` refuses pilotuserb's document for a reason `check`
/// accepts.
fn refused_document(error: &ProfileError, check: fn(&DocumentError) -> bool) -> bool {
    matches!(error, ProfileError::Document { user, reason } if user.as_str() == "pilotuserb" && check(reason))
}

#[test]
fn a_bundle_over_the_limit_is_refused_unread() {
    let mut text = profile_fixture("valid/pilot.json").into_bytes();
    text.resize(MAX_BUNDLE_BYTES, b' ');
    assert!(ProfileBundle::parse(&text).is_ok());
    text.push(b' ');
    assert!(matches!(
        ProfileBundle::parse(&text),
        Err(ProfileError::TooLarge { size }) if size == MAX_BUNDLE_BYTES + 1
    ));
}
