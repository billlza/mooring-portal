//! What the portal reads of an event, type by type, and what it refuses.

use super::*;

mod objects;

/// 2026-11-11T00:00:00Z.
const MADE: u64 = 1_794_355_200;
const ORDER: &str = "0123456789abcdefghjk";

/// An event of `kind` about `object`, rendered for the release the portal
/// reads.
fn event(kind: &str, object: &str) -> String {
    rendered(kind, object, "\"2026-10-28.endive\"")
}

fn rendered(kind: &str, object: &str, api_version: &str) -> String {
    format!(
        r#"{{"id":"evt_1Synth","object":"event","api_version":{api_version},"created":{MADE},"data":{{"object":{object}}},"livemode":false,"pending_webhooks":1,"type":"{kind}"}}"#
    )
}

fn read(kind: &str, object: &str) -> EventKind {
    Event::parse(event(kind, object).as_bytes())
        .unwrap_or_else(|error| panic!("{kind} {object}: {error}"))
        .kind
}

fn refused(document: &str) -> EventError {
    Event::parse(document.as_bytes()).unwrap_err()
}

fn id(text: &str) -> StripeId {
    StripeId::parse(text).unwrap()
}

fn gbp() -> Option<Currency> {
    Currency::parse("gbp")
}

/// A checkout for one payment, with `changes` in place of its keys.
fn session(changes: &[(&str, &str)]) -> String {
    let mut keys = vec![
        ("id", "\"cs_test_a1\""),
        ("object", "\"checkout.session\""),
        ("client_reference_id", "\"0123456789abcdefghjk\""),
        ("payment_link", "\"plink_1\""),
        ("mode", "\"payment\""),
        ("payment_status", "\"paid\""),
        ("amount_total", "800"),
        ("currency", "\"gbp\""),
        ("payment_intent", "\"pi_3\""),
        ("subscription", "null"),
        ("invoice", "null"),
        (
            "customer_details",
            r#"{"email":"payer@example.test","name":"Synthetic Payer","address":{"postal_code":"ZZ1 1ZZ"}}"#,
        ),
    ];
    object(&mut keys, changes)
}

/// `keys` as a JSON object, each of `changes` replacing the key it names,
/// adding it, or (with an empty value) leaving it out.
fn object(keys: &mut Vec<(&'static str, &'static str)>, changes: &[(&str, &str)]) -> String {
    let mut written: Vec<(String, String)> = keys
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();
    for (key, value) in changes {
        written.retain(|(name, _)| name != key);
        if !value.is_empty() {
            written.push(((*key).to_owned(), (*value).to_owned()));
        }
    }
    let body: Vec<String> = written
        .iter()
        .map(|(key, value)| format!("\"{key}\":{value}"))
        .collect();
    format!("{{{}}}", body.join(","))
}

fn completed(changes: &[(&str, &str)]) -> CheckoutSession {
    match read("checkout.session.completed", &session(changes)) {
        EventKind::CheckoutCompleted(session) => session,
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_event_is_its_id_its_time_its_mode_and_what_it_says() {
    let parsed = Event::parse(event("checkout.session.completed", &session(&[])).as_bytes());
    assert_eq!(
        parsed,
        Ok(Event {
            id: id("evt_1Synth"),
            created: UnixTime::from_secs(MADE).unwrap(),
            livemode: false,
            kind: EventKind::CheckoutCompleted(CheckoutSession {
                id: id("cs_test_a1"),
                order: Some(OrderId::parse(ORDER).unwrap()),
                payment_link: Some(id("plink_1")),
                mode: CheckoutMode::Payment,
                status: PaymentStatus::Paid,
                amount_total: Some(800),
                currency: gbp(),
                payment_intent: Some(id("pi_3")),
                subscription: None,
                invoice: None,
            }),
        })
    );
    let live = event("checkout.session.completed", &session(&[]))
        .replace("\"livemode\":false", "\"livemode\":true");
    assert!(Event::parse(live.as_bytes()).unwrap().livemode);
    // A payment that settles later is told of in the same shape.
    assert!(matches!(
        read("checkout.session.async_payment_succeeded", &session(&[])),
        EventKind::CheckoutCompleted(_)
    ));
}

#[test]
fn nothing_that_says_who_paid_is_read() {
    let parsed = Event::parse(event("checkout.session.completed", &session(&[])).as_bytes());
    let shown = format!("{parsed:?}");
    for private in ["payer@example.test", "Synthetic Payer", "ZZ1"] {
        assert!(!shown.contains(private), "{private} in {shown}");
    }
    let charged = read(
        "charge.succeeded",
        r#"{"payment_intent":"pi_3","billing_details":{"name":"Synthetic Payer"},"payment_method_details":{"type":"card","card":{"fingerprint":"Xt5EWLLDS7FJjR1c","last4":"4242","wallet":null}}}"#,
    );
    let shown = format!("{charged:?}");
    for private in ["Synthetic Payer", "4242", "Xt5EWLLDS7FJjR1c"] {
        assert!(!shown.contains(private), "{private} in {shown}");
    }
}

#[test]
fn a_type_the_portal_does_not_act_on_is_accepted_unread() {
    for kind in [
        "payment_intent.succeeded",
        "customer.created",
        "checkout.session.expired",
        "checkout.session.async_payment_failed",
        "charge.failed",
        "invoice.payment_failed",
        "customer.subscription.created",
        "charge.dispute.closed",
        "Checkout.Session.Completed",
        "",
    ] {
        // Whatever its object is, and whatever release it is rendered for.
        for object in ["{}", "null", "[1,2]", "\"text\""] {
            for version in ["\"2026-10-28.endive\"", "\"2019-12-03\"", "null"] {
                let parsed = Event::parse(rendered(kind, object, version).as_bytes());
                assert_eq!(
                    parsed.map(|event| event.kind),
                    Ok(EventKind::Unused),
                    "{kind} {object} {version}"
                );
            }
        }
    }
}

#[test]
fn a_type_the_portal_acts_on_must_be_of_the_release_it_reads() {
    for kind in [
        "checkout.session.completed",
        "checkout.session.async_payment_succeeded",
        "charge.succeeded",
        "invoice.paid",
        "invoice_payment.paid",
        "customer.subscription.updated",
        "customer.subscription.deleted",
        "charge.refunded",
        "charge.dispute.created",
    ] {
        for version in [
            "\"2025-09-30.clover\"",
            "\"2026-10-28.endives\"",
            "\"2026-10-28.Endive\"",
            "\"2026-10-28.endive.preview\"",
            "\"endive\"",
            "\"2019-12-03\"",
            "\"\"",
            "null",
        ] {
            // Refused before the object is looked at.
            assert_eq!(
                refused(&rendered(kind, "{}", version)),
                EventError::ApiRelease,
                "{kind} {version}"
            );
        }
        // Without the key at all.
        let without = rendered(kind, "{}", "null").replace("\"api_version\":null,", "");
        assert_eq!(refused(&without), EventError::ApiRelease, "{kind}");
        // Of the release, the object is read, and a number is not one.
        assert_eq!(
            refused(&event(kind, "7")),
            EventError::Object(Malformation::Shape),
            "{kind}"
        );
    }
    // A later version of the same release is read.
    let later = rendered(
        "checkout.session.completed",
        &session(&[]),
        "\"2027-01-27.endive\"",
    );
    assert!(Event::parse(later.as_bytes()).is_ok());
    assert_eq!(API_RELEASE, "endive");
}

#[test]
fn a_checkouts_reference_names_an_order_or_nothing() {
    assert_eq!(completed(&[]).order, Some(OrderId::parse(ORDER).unwrap()));
    for reference in [
        "null",
        "\"\"",
        "\"0123456789ABCDEFGHJK\"",
        "\"0123456789abcdefghj\"",
        "\"someone-elses-reference\"",
    ] {
        assert_eq!(
            completed(&[("client_reference_id", reference)]).order,
            None,
            "{reference}"
        );
    }
    assert_eq!(completed(&[("client_reference_id", "")]).order, None);
}

#[test]
fn a_checkout_says_what_it_set_up_and_whether_it_is_paid() {
    for (mode, read) in [
        ("\"payment\"", CheckoutMode::Payment),
        ("\"subscription\"", CheckoutMode::Subscription),
        ("\"setup\"", CheckoutMode::Other),
        ("\"something_new\"", CheckoutMode::Other),
    ] {
        assert_eq!(completed(&[("mode", mode)]).mode, read, "{mode}");
    }
    for (status, read) in [
        ("\"paid\"", PaymentStatus::Paid),
        ("\"unpaid\"", PaymentStatus::Unpaid),
        ("\"no_payment_required\"", PaymentStatus::Other),
        ("\"PAID\"", PaymentStatus::Other),
    ] {
        assert_eq!(
            completed(&[("payment_status", status)]).status,
            read,
            "{status}"
        );
    }
    let subscribed = completed(&[
        ("mode", "\"subscription\""),
        ("payment_intent", "null"),
        ("subscription", "\"sub_1\""),
        ("invoice", "\"in_1\""),
    ]);
    assert_eq!(subscribed.payment_intent, None);
    assert_eq!(subscribed.subscription, Some(id("sub_1")));
    assert_eq!(subscribed.invoice, Some(id("in_1")));
    // What may be absent is read as absent, not refused.
    let bare = completed(&[
        ("payment_link", "null"),
        ("amount_total", "null"),
        ("currency", "null"),
        ("payment_intent", ""),
        ("subscription", ""),
        ("invoice", ""),
    ]);
    assert_eq!(
        (bare.payment_link, bare.amount_total, bare.currency),
        (None, None, None)
    );
    assert_eq!(
        (bare.payment_intent, bare.subscription, bare.invoice),
        (None, None, None)
    );
    // A currency the portal does not price in is no currency of its.
    for currency in ["\"jpy\"", "\"GBP\"", "\"\""] {
        assert_eq!(
            completed(&[("currency", currency)]).currency,
            None,
            "{currency}"
        );
    }
    assert_eq!(completed(&[("amount_total", "0")]).amount_total, Some(0));
}

#[test]
fn a_checkout_that_is_not_one_is_refused() {
    for changes in [
        [("id", "")],
        [("id", "\"cs test\"")],
        [("mode", "")],
        [("mode", "7")],
        [("payment_status", "")],
        [("amount_total", "\"800\"")],
        [("amount_total", "-800")],
        [("amount_total", "8.5")],
        [("payment_link", "{\"id\":\"plink_1\"}")],
        [("payment_intent", "17")],
        [("client_reference_id", "17")],
        [("currency", "826")],
    ] {
        assert_eq!(
            refused(&event("checkout.session.completed", &session(&changes))),
            EventError::Object(Malformation::Shape),
            "{changes:?}"
        );
    }
    // A key written twice could be read either way.
    let twice = session(&[]).replacen('{', "{\"amount_total\":1,", 1);
    assert_eq!(
        refused(&event("checkout.session.completed", &twice)),
        EventError::Object(Malformation::Shape)
    );
}

#[test]
fn what_is_not_an_event_is_refused() {
    let good = event("customer.created", "{}");
    assert!(Event::parse(good.as_bytes()).is_ok());
    for (what, document) in [
        ("no id", good.replace("\"id\":\"evt_1Synth\",", "")),
        ("an id that is not one", good.replace("evt_1Synth", "evt 1")),
        ("no time", good.replace(&format!("\"created\":{MADE},"), "")),
        (
            "a time past the year 9999",
            good.replace(&format!("\"created\":{MADE}"), "\"created\":253402300800"),
        ),
        (
            "a time before 1970",
            good.replace(&format!("\"created\":{MADE}"), "\"created\":-1"),
        ),
        ("no mode", good.replace("\"livemode\":false,", "")),
        (
            "a mode in text",
            good.replace("\"livemode\":false", "\"livemode\":\"false\""),
        ),
        (
            "no type",
            good.replace(",\"type\":\"customer.created\"", ""),
        ),
        (
            "a type that is no text",
            good.replace("\"type\":\"customer.created\"", "\"type\":7"),
        ),
        ("no data", good.replace("\"data\":{\"object\":{}},", "")),
        (
            "data without an object",
            good.replace("{\"object\":{}}", "{}"),
        ),
        ("an id twice", good.replacen('{', "{\"id\":\"evt_2\",", 1)),
        ("a list", format!("[{good}]")),
        ("text", "\"event\"".to_owned()),
    ] {
        assert_eq!(
            refused(&document),
            EventError::Malformed(Malformation::Shape),
            "{what}"
        );
    }
    assert_eq!(refused(""), EventError::Malformed(Malformation::Truncated));
    assert_eq!(
        refused(&good[..good.len() - 1]),
        EventError::Malformed(Malformation::Truncated)
    );
    assert_eq!(
        refused(&format!("{good} x")),
        EventError::Malformed(Malformation::Syntax)
    );
    assert_eq!(
        Event::parse(b"\xff\xfe"),
        Err(EventError::Malformed(Malformation::Syntax))
    );
}

#[test]
fn an_event_may_be_as_large_as_the_limit_and_no_larger() {
    let event = event("customer.created", "{}");
    let padded = format!("{event}{}", " ".repeat(MAX_EVENT_BYTES - event.len()));
    assert_eq!(padded.len(), MAX_EVENT_BYTES);
    assert!(Event::parse(padded.as_bytes()).is_ok());
    assert_eq!(refused(&format!("{padded} ")), EventError::TooLarge);
    assert_eq!(MAX_EVENT_BYTES, 524_288);
}

#[test]
fn refusals_say_what_is_wrong_and_quote_nothing() {
    assert_eq!(
        EventError::TooLarge.to_string(),
        "the event is larger than 524288 bytes"
    );
    assert_eq!(
        EventError::Malformed(Malformation::Syntax).to_string(),
        "the event does not parse: it is not JSON"
    );
    assert!(
        EventError::ApiRelease.to_string().starts_with(
            "the event is rendered for another release of Stripe's API than \"endive\""
        )
    );
    assert_eq!(
        EventError::Object(Malformation::Truncated).to_string(),
        "the event's object is not what its type says: it ends too early"
    );
}
