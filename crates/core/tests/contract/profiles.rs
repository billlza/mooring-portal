//! The golden cases of the CFM profile bundle contract: the valid bundle,
//! and one refusal per rule.

use std::fs;
use std::path::PathBuf;

use portal_core::profile::{
    DocumentError, FoundFormat, MAX_BUNDLE_BYTES, Malformation, ProfileBundle, ProfileError,
    WithheldReason,
};
use portal_core::snapshot::MooringUserId;

/// The directory of the version-2 profile fixtures.
fn profile_path(sub: &str) -> PathBuf {
    [
        env!("CARGO_MANIFEST_DIR"),
        "../../contract/fixtures/cfm-profiles-v2",
        sub,
    ]
    .iter()
    .collect()
}

fn profile_fixture(path: &str) -> String {
    let file = profile_path(path);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

fn user(text: &str) -> MooringUserId {
    MooringUserId::parse(text).unwrap()
}

/// The users `bundle` withholds, with why.
fn withheld(bundle: &ProfileBundle) -> Vec<(&str, WithheldReason)> {
    bundle
        .withheld()
        .map(|(user, reason)| (user.as_str(), reason))
        .collect()
}

#[test]
fn each_reason_is_named_for_the_operator() {
    for (reason, text) in [
        (WithheldReason::Suspended, "suspended"),
        (WithheldReason::Revoked, "revoked"),
        (
            WithheldReason::AgreementNotAccepted,
            "not agreed to the current user agreement",
        ),
    ] {
        assert_eq!(reason.to_string(), text);
    }
}

/// The ids of `users`.
fn ids<'a>(users: impl Iterator<Item = &'a MooringUserId>) -> Vec<&'a str> {
    users.map(MooringUserId::as_str).collect()
}

#[test]
fn each_document_is_kept_as_the_exact_text_mooring_wrote() {
    let text = profile_fixture("valid/pilot.json");
    let bundle = ProfileBundle::parse(text.as_bytes()).unwrap();
    assert_eq!(bundle.generated_at().to_string(), "2026-11-06T09:00:00Z");
    let source: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        ids(bundle.profiles().map(|(user, _)| user)),
        ["pilotusera", "pilotuserb"]
    );
    assert_eq!(
        withheld(&bundle),
        [("pilotuserc", WithheldReason::Suspended)]
    );
    for named in ["pilotusera", "pilotuserb", "pilotuserc"] {
        assert!(bundle.names(&user(named)), "{named}");
    }
    assert!(!bundle.names(&user("pilotuserd")));
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

#[test]
fn a_bundle_may_withhold_every_user_it_names() {
    let bundle =
        ProfileBundle::parse(profile_fixture("valid/all-withheld.json").as_bytes()).unwrap();
    assert_eq!(bundle.profiles().count(), 0);
    assert_eq!(
        withheld(&bundle),
        [
            ("pilotusera", WithheldReason::AgreementNotAccepted),
            ("pilotuserb", WithheldReason::Revoked),
        ]
    );
    assert!(bundle.names(&user("pilotusera")));
    assert!(!bundle.names(&user("pilotuserc")));
}

#[test]
fn every_valid_bundle_has_a_case() {
    let listed = fs::read_dir(profile_path("valid")).unwrap().count();
    assert_eq!(listed, 2, "pilot.json and all-withheld.json");
}

/// What an invalid fixture must be refused with.
enum Refusal {
    /// A bundle that does not decode, refused with the kind and the place
    /// of the problem: the decoder's own message would name the key or
    /// quote the value.
    Malformed(Malformation),
    /// A rule of the contract.
    Rule(fn(&ProfileError) -> bool),
}

#[test]
fn every_invalid_bundle_is_refused_for_its_rule() {
    use Malformation::{Shape, Truncated};
    use Refusal::{Malformed, Rule};
    let cases = [
        (
            "wrong-format",
            Rule(|e| {
                matches!(
                    e,
                    ProfileError::WrongFormat {
                        found: FoundFormat::Snapshot
                    }
                )
            }),
        ),
        // What a Mooring build from before version 2 writes.
        (
            "version-1",
            Rule(|e| matches!(e, ProfileError::UnsupportedVersion { found: 1 })),
        ),
        ("missing-key", Malformed(Shape)),
        ("missing-withheld", Malformed(Shape)),
        ("unknown-key", Malformed(Shape)),
        ("unknown-entry-key", Malformed(Shape)),
        ("alias-as-user", Malformed(Shape)),
        ("alias-as-withheld", Malformed(Shape)),
        ("withheld-without-reason", Malformed(Shape)),
        ("unknown-reason", Malformed(Shape)),
        ("unknown-withheld-key", Malformed(Shape)),
        ("truncated", Malformed(Truncated)),
        // Shapes whose decoder messages quoted whole documents.
        ("bundle-as-string", Malformed(Shape)),
        ("entry-as-string", Malformed(Shape)),
        ("profiles-as-string", Malformed(Shape)),
        ("document-in-user", Malformed(Shape)),
        ("document-in-withheld", Malformed(Shape)),
        (
            "names-nobody",
            Rule(|e| matches!(e, ProfileError::NamesNobody)),
        ),
        ("duplicate-user", Rule(|e| named_twice(e, "pilotusera"))),
        (
            "delivered-and-withheld",
            Rule(|e| named_twice(e, "pilotusera")),
        ),
        ("withheld-twice", Rule(|e| named_twice(e, "pilotuserc"))),
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
            Malformed(expected) => assert!(
                matches!(
                    error,
                    ProfileError::Malformed { kind, line, column }
                        if kind == expected && line >= 1 && column >= 1
                ),
                "{name}: {error}"
            ),
            Rule(check) => assert!(check(&error), "{name}: {error}"),
        }
        // No refusal quotes a credential of the fixture's documents.
        for secret in ["00000000-0000-4000-8000-00000000000", "synthetic-"] {
            assert!(!error.to_string().contains(secret), "{name}: {error}");
        }
    }
}

/// Whether `error` says `user` is named twice.
fn named_twice(error: &ProfileError, user: &str) -> bool {
    matches!(error, ProfileError::DuplicateUser { user: twice } if twice.as_str() == user)
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
