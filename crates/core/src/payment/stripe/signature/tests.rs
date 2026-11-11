//! The signature check, against signatures another program computed
//! (Python's `hmac`, which reproduces RFC 4231's second test case).

use super::*;

const BODY: &[u8] = br#"{"id":"evt_test_1","object":"event"}"#;
/// 2026-11-11T00:00:00Z.
const SIGNED_AT: u64 = 1_794_355_200;
const FIRST: &str = "whsec_test_first_0123456789abcdef";
const SECOND: &str = "whsec_test_second_fedcba9876543210";
/// `BODY` at `SIGNED_AT` under `FIRST`, and under `SECOND`.
const BY_FIRST: &str = "5c9bd906e92a3bd86d28ddae553df71c38ef18c4ea279802cd89ac3599d7f162";
const BY_SECOND: &str = "ef114cee08f60f2ff5d73d842a5e50d4b91c8dd5be40ac0423cfca5ad1ee8713";
/// `BODY` a second later under `FIRST`, and `BODY` with a space after it.
const A_SECOND_LATER: &str = "4cf2afe544f875a17dd013944049e3b8cccd076a9638c174311a9d555ca1ffe6";
const OF_ANOTHER_BODY: &str = "165423263beb10bedfd1f12cad2734a4a71f1780c321ea7d0ea3cf4b50e6ad92";

fn secret(text: &str) -> SigningSecret {
    SigningSecret::parse(text).unwrap()
}

fn at(secs: u64) -> UnixTime {
    UnixTime::from_secs(secs).unwrap()
}

fn check(secrets: &[&str], header: &str, body: &[u8], now: u64) -> Result<(), SignatureError> {
    let secrets: Vec<SigningSecret> = secrets.iter().map(|text| secret(text)).collect();
    verify(&secrets, header, body, at(now))
}

#[test]
fn a_delivery_signed_under_the_secret_is_believed() {
    let header = format!("t={SIGNED_AT},v1={BY_FIRST}");
    assert_eq!(check(&[FIRST], &header, BODY, SIGNED_AT), Ok(()));
    // A sandbox's extra scheme beside it changes nothing.
    let header = format!("t={SIGNED_AT},v1={BY_FIRST},v0={BY_SECOND}");
    assert_eq!(check(&[FIRST], &header, BODY, SIGNED_AT), Ok(()));
}

#[test]
fn a_delivery_is_believed_for_five_minutes_either_way() {
    let header = format!("t={SIGNED_AT},v1={BY_FIRST}");
    for (now, believed) in [
        (SIGNED_AT + 300, true),
        (SIGNED_AT + 301, false),
        (SIGNED_AT - 300, true),
        (SIGNED_AT - 301, false),
    ] {
        assert_eq!(
            check(&[FIRST], &header, BODY, now),
            if believed {
                Ok(())
            } else {
                Err(SignatureError::Stale)
            },
            "{now}"
        );
    }
    assert_eq!(TOLERANCE_SECS, 300);
}

#[test]
fn another_body_another_time_or_another_secret_is_not() {
    let header = format!("t={SIGNED_AT},v1={BY_FIRST}");
    // The body with one byte more.
    let mut longer = BODY.to_vec();
    longer.push(b' ');
    assert_eq!(
        check(&[FIRST], &header, &longer, SIGNED_AT),
        Err(SignatureError::Mismatch)
    );
    // That body's own signature is believed, so the body is what is signed.
    let header_of_longer = format!("t={SIGNED_AT},v1={OF_ANOTHER_BODY}");
    assert_eq!(
        check(&[FIRST], &header_of_longer, &longer, SIGNED_AT),
        Ok(())
    );
    // A signature for another time, under this header's time.
    let moved = format!("t={SIGNED_AT},v1={A_SECOND_LATER}");
    assert_eq!(
        check(&[FIRST], &moved, BODY, SIGNED_AT),
        Err(SignatureError::Mismatch)
    );
    let later = format!("t={},v1={A_SECOND_LATER}", SIGNED_AT + 1);
    assert_eq!(check(&[FIRST], &later, BODY, SIGNED_AT), Ok(()));
    // Another secret, and no secret at all.
    assert_eq!(
        check(&[SECOND], &header, BODY, SIGNED_AT),
        Err(SignatureError::Mismatch)
    );
    assert_eq!(
        check(&[], &header, BODY, SIGNED_AT),
        Err(SignatureError::Mismatch)
    );
    // A wrong signature is a mismatch however old it claims to be: what
    // is not signed says nothing about the time.
    let old = format!("t={SIGNED_AT},v1={BY_SECOND}");
    assert_eq!(
        check(&[FIRST], &old, BODY, SIGNED_AT + 10_000),
        Err(SignatureError::Mismatch)
    );
}

#[test]
fn while_a_secret_is_rolled_either_signature_and_either_secret_will_do() {
    let both = format!("t={SIGNED_AT},v1={BY_SECOND},v1={BY_FIRST}");
    assert_eq!(check(&[FIRST], &both, BODY, SIGNED_AT), Ok(()));
    assert_eq!(check(&[SECOND], &both, BODY, SIGNED_AT), Ok(()));
    let other_way = format!("t={SIGNED_AT},v1={BY_FIRST},v1={BY_SECOND}");
    assert_eq!(check(&[FIRST], &other_way, BODY, SIGNED_AT), Ok(()));
    let one = format!("t={SIGNED_AT},v1={BY_FIRST}");
    assert_eq!(check(&[SECOND, FIRST], &one, BODY, SIGNED_AT), Ok(()));
    assert_eq!(check(&[FIRST, SECOND], &one, BODY, SIGNED_AT), Ok(()));
}

#[test]
fn a_header_without_a_time_and_a_signature_is_refused() {
    for header in [
        String::new(),
        format!("v1={BY_FIRST}"),
        format!("t={SIGNED_AT}"),
        format!("t={SIGNED_AT},v0={BY_FIRST}"),
        format!("t=,v1={BY_FIRST}"),
        format!("t=soon,v1={BY_FIRST}"),
        format!("t=+{SIGNED_AT},v1={BY_FIRST}"),
        format!("t= {SIGNED_AT},v1={BY_FIRST}"),
        format!("t={SIGNED_AT}0000000000000,v1={BY_FIRST}"),
        format!("t={SIGNED_AT},t={SIGNED_AT},v1={BY_FIRST}"),
        format!("t={SIGNED_AT},v1={BY_FIRST},t=1"),
        format!("T={SIGNED_AT},v1={BY_FIRST}"),
        format!("t={SIGNED_AT} v1={BY_FIRST}"),
    ] {
        assert_eq!(
            check(&[FIRST], &header, BODY, SIGNED_AT),
            Err(SignatureError::Header),
            "{header}"
        );
    }
}

#[test]
fn a_signature_is_sixty_four_lowercase_hexadecimal_digits() {
    for written in [
        BY_FIRST.to_uppercase(),
        BY_FIRST[..62].to_owned(),
        format!("{BY_FIRST}00"),
        format!("{BY_FIRST}0"),
        format!("{}g", &BY_FIRST[..63]),
        format!(" {BY_FIRST}"),
        String::new(),
    ] {
        let header = format!("t={SIGNED_AT},v1={written}");
        assert_eq!(
            check(&[FIRST], &header, BODY, SIGNED_AT),
            Err(SignatureError::Mismatch),
            "{written:?}"
        );
        // One that cannot be read beside one that matches does no harm.
        let header = format!("t={SIGNED_AT},v1={written},v1={BY_FIRST}");
        assert_eq!(
            check(&[FIRST], &header, BODY, SIGNED_AT),
            Ok(()),
            "{written:?}"
        );
    }
    assert_eq!(digest("0f"), None);
    assert_eq!(
        digest(&format!("{}0", "0f".repeat(32))),
        None,
        "a digit beyond the last pair is not ignored"
    );
    assert_eq!(
        digest(&"0f".repeat(32)),
        Some([0x0f; 32]),
        "each pair is one byte, its first digit the high half"
    );
    assert_eq!(digest(&"a9".repeat(32)), Some([0xa9; 32]));
}

#[test]
fn a_signing_secret_is_as_stripe_shows_one_and_is_never_printed() {
    for good in [
        FIRST,
        SECOND,
        &format!("whsec_{}", "a".repeat(16)),
        &format!("whsec_{}", "A1_".repeat(83)),
    ] {
        assert!(SigningSecret::parse(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "whsec_",
        "whsec_short",
        "sk_test_0123456789abcdef0123",
        "WHSEC_0123456789abcdef0123",
        "whsec_0123456789abcdef 0123",
        "whsec_0123456789abcdef-0123",
        &format!("whsec_{}", "a".repeat(15)),
        &format!("whsec_{}", "a".repeat(251)),
        " whsec_0123456789abcdef0123",
    ] {
        assert_eq!(
            SigningSecret::parse(bad).err(),
            Some(SecretError),
            "{bad:?}"
        );
    }
    assert_eq!(format!("{:?}", secret(FIRST)), "SigningSecret(<redacted>)");
    assert!(SecretError.to_string().contains("whsec_"));
    assert_eq!(
        SignatureError::Stale.to_string(),
        "the delivery was signed more than 300 seconds from now"
    );
    assert!(
        SignatureError::Header
            .to_string()
            .contains("Stripe-Signature")
    );
    assert!(
        SignatureError::Mismatch
            .to_string()
            .contains("no signature")
    );
}
