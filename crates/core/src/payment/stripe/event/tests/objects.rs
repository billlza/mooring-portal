//! What is read of the objects that follow a checkout: the charge, the
//! invoice and its payment, the subscription, a refund and a dispute.

use super::*;

fn charged(details: &str) -> EventKind {
    read(
        "charge.succeeded",
        &format!(r#"{{"id":"ch_3","payment_intent":"pi_3","payment_method_details":{details}}}"#),
    )
}

#[test]
fn a_charge_says_what_paid() {
    let card = |fingerprint: &str| {
        EventKind::ChargeSucceeded(Charge {
            payment_intent: id("pi_3"),
            card: CardUsed::Card(CardFingerprint::parse(fingerprint).unwrap()),
        })
    };
    let other = |card| {
        EventKind::ChargeSucceeded(Charge {
            payment_intent: id("pi_3"),
            card,
        })
    };
    assert_eq!(
        charged(r#"{"type":"card","card":{"fingerprint":"Xt5EWLLDS7FJjR1c","wallet":null}}"#),
        card("Xt5EWLLDS7FJjR1c")
    );
    // The wallet's key may be left out.
    assert_eq!(
        charged(r#"{"type":"card","card":{"fingerprint":"Xt5EWLLDS7FJjR1d"}}"#),
        card("Xt5EWLLDS7FJjR1d")
    );
    // A wallet, whatever fingerprint comes with it.
    for wallet in [r#"{"type":"apple_pay","apple_pay":{}}"#, "{}", "\"link\""] {
        assert_eq!(
            charged(&format!(
                r#"{{"type":"card","card":{{"fingerprint":"Xt5EWLLDS7FJjR1c","wallet":{wallet}}}}}"#
            )),
            other(CardUsed::Wallet),
            "{wallet}"
        );
    }
    for details in [
        // A card without a fingerprint, or with one that is not one.
        r#"{"type":"card","card":{"fingerprint":null,"wallet":null}}"#,
        r#"{"type":"card","card":{"wallet":null}}"#,
        r#"{"type":"card","card":{"fingerprint":"short","wallet":null}}"#,
        r#"{"type":"card","card":{"fingerprint":"Xt5EWLLDS7FJjR1!","wallet":null}}"#,
        r#"{"type":"card","card":null}"#,
        r#"{"type":"card"}"#,
        // Not a card, even with a card's details beside it.
        r#"{"type":"sepa_debit","sepa_debit":{"fingerprint":"Xt5EWLLDS7FJjR1c"}}"#,
        r#"{"type":"link","card":{"fingerprint":"Xt5EWLLDS7FJjR1c","wallet":null}}"#,
        "null",
    ] {
        assert_eq!(charged(details), other(CardUsed::Other), "{details}");
    }
    // No details at all.
    assert_eq!(
        read("charge.succeeded", r#"{"payment_intent":"pi_3"}"#),
        other(CardUsed::Other)
    );
    // A charge made without a payment is none of a checkout's.
    for object in [
        r#"{"payment_intent":null,"payment_method_details":{"type":"card","card":{"fingerprint":"Xt5EWLLDS7FJjR1c"}}}"#,
        r#"{"id":"ch_3"}"#,
    ] {
        assert_eq!(
            read("charge.succeeded", object),
            EventKind::Unused,
            "{object}"
        );
    }
    for object in [
        r#"{"payment_intent":"pi 3"}"#,
        r#"{"payment_intent":"pi_3","payment_method_details":{"card":{}}}"#,
        r#"{"payment_intent":"pi_3","payment_method_details":{"type":"card","card":{"fingerprint":7}}}"#,
        r#"{"payment_intent":"pi_3","payment_method_details":"card"}"#,
    ] {
        assert_eq!(
            refused(&event("charge.succeeded", object)),
            EventError::Object(Malformation::Shape),
            "{object}"
        );
    }
}

fn invoiced(changes: &[(&str, &str)]) -> EventKind {
    let mut keys = vec![
        ("id", "\"in_1\""),
        (
            "parent",
            r#"{"type":"subscription_details","quote_details":null,"subscription_details":{"metadata":{},"subscription":"sub_1"}}"#,
        ),
        ("billing_reason", "\"subscription_cycle\""),
        ("amount_paid", "800"),
        ("currency", "\"gbp\""),
        ("customer_email", "\"payer@example.test\""),
    ];
    read("invoice.paid", &object(&mut keys, changes))
}

#[test]
fn an_invoice_says_which_subscription_it_bills_and_why() {
    let invoice = |reason, amount_paid, currency| {
        EventKind::InvoicePaid(Invoice {
            introductory_order: None,
            id: id("in_1"),
            subscription: id("sub_1"),
            reason,
            amount_paid,
            currency,
        })
    };
    assert_eq!(invoiced(&[]), invoice(BillingReason::Cycle, 800, gbp()));
    for (reason, read) in [
        ("\"subscription_create\"", BillingReason::Create),
        ("\"subscription_cycle\"", BillingReason::Cycle),
        ("\"subscription_update\"", BillingReason::Other),
        ("\"manual\"", BillingReason::Other),
        ("null", BillingReason::Other),
        ("", BillingReason::Other),
    ] {
        assert_eq!(
            invoiced(&[("billing_reason", reason)]),
            invoice(read, 800, gbp()),
            "{reason}"
        );
    }
    assert_eq!(
        invoiced(&[("amount_paid", "0"), ("currency", "\"jpy\"")]),
        invoice(BillingReason::Cycle, 0, None)
    );
    // An invoice that bills no subscription is not read.
    for parent in [
        "null",
        "",
        r#"{"type":"quote_details","quote_details":{"quote":"qt_1"},"subscription_details":null}"#,
        r#"{"type":"quote_details"}"#,
    ] {
        assert_eq!(
            invoiced(&[("parent", parent)]),
            EventKind::Unused,
            "{parent}"
        );
    }
    for changes in [
        [("id", "")],
        [("amount_paid", "")],
        [("amount_paid", "-1")],
        [("currency", "")],
        [(
            "parent",
            r#"{"subscription_details":{"subscription":null}}"#,
        )],
        [(
            "parent",
            r#"{"subscription_details":{"subscription":{"id":"sub_1"}}}"#,
        )],
        [("parent", "\"sub_1\"")],
    ] {
        let mut keys = vec![
            ("id", "\"in_1\""),
            (
                "parent",
                r#"{"subscription_details":{"subscription":"sub_1"}}"#,
            ),
            ("amount_paid", "800"),
            ("currency", "\"gbp\""),
        ];
        assert_eq!(
            refused(&event("invoice.paid", &object(&mut keys, &changes))),
            EventError::Object(Malformation::Shape),
            "{changes:?}"
        );
    }
}

#[test]
fn an_invoices_payment_says_which_payment_paid_it() {
    assert_eq!(
        read(
            "invoice_payment.paid",
            r#"{"id":"inpay_1","invoice":"in_1","payment":{"type":"payment_intent","payment_intent":"pi_3"},"status":"paid"}"#
        ),
        EventKind::InvoicePaymentPaid(InvoicePayment {
            invoice: id("in_1"),
            payment_intent: id("pi_3"),
        })
    );
    // Paid some other way, there is no payment whose card could be read.
    for payment in [
        r#"{"type":"charge","charge":"ch_3"}"#,
        r#"{"type":"charge","charge":"ch_3","payment_intent":"pi_3"}"#,
        r#"{"type":"payment_record","payment_record":"payrec_1"}"#,
        r#"{"type":"payment_intent","payment_intent":null}"#,
        r#"{"type":"payment_intent"}"#,
    ] {
        assert_eq!(
            read(
                "invoice_payment.paid",
                &format!(r#"{{"invoice":"in_1","payment":{payment}}}"#)
            ),
            EventKind::Unused,
            "{payment}"
        );
    }
    for object in [
        r#"{"payment":{"type":"payment_intent","payment_intent":"pi_3"}}"#,
        r#"{"invoice":"in_1"}"#,
        r#"{"invoice":"in_1","payment":{"payment_intent":"pi_3"}}"#,
        r#"{"invoice":{"id":"in_1"},"payment":{"type":"payment_intent","payment_intent":"pi_3"}}"#,
    ] {
        assert_eq!(
            refused(&event("invoice_payment.paid", object)),
            EventError::Object(Malformation::Shape),
            "{object}"
        );
    }
}

fn changed(kind: &str, status: &str, at_period_end: bool, cancel_at: &str) -> EventKind {
    read(
        kind,
        &format!(
            r#"{{"id":"sub_1","status":"{status}","cancel_at_period_end":{at_period_end},"cancel_at":{cancel_at}}}"#
        ),
    )
}

#[test]
fn a_subscription_says_whether_it_will_be_charged_again() {
    let state = |state| {
        EventKind::SubscriptionChanged(SubscriptionChange {
            id: id("sub_1"),
            state,
        })
    };
    let updated = "customer.subscription.updated";
    for (status, at_period_end, cancel_at, read) in [
        ("active", false, "null", Renewal::Active),
        ("trialing", false, "null", Renewal::Active),
        ("past_due", false, "null", Renewal::Active),
        ("unpaid", false, "null", Renewal::Active),
        ("paused", false, "null", Renewal::Active),
        ("something_new", false, "null", Renewal::Active),
        ("active", true, "1796947200", Renewal::Ending),
        ("active", true, "null", Renewal::Ending),
        ("active", false, "1796947200", Renewal::Ending),
        ("past_due", true, "null", Renewal::Ending),
        ("canceled", false, "null", Renewal::Ended),
        ("canceled", true, "1796947200", Renewal::Ended),
        ("incomplete_expired", false, "null", Renewal::Ended),
    ] {
        assert_eq!(
            changed(updated, status, at_period_end, cancel_at),
            state(read),
            "{status} {at_period_end} {cancel_at}"
        );
    }
    // The key for a set date may be left out.
    assert_eq!(
        read(
            updated,
            r#"{"id":"sub_1","status":"active","cancel_at_period_end":false}"#
        ),
        state(Renewal::Active)
    );
    // Once deleted it is over, whatever else the object says.
    let deleted = "customer.subscription.deleted";
    for (status, at_period_end) in [("canceled", false), ("active", false), ("active", true)] {
        assert_eq!(
            changed(deleted, status, at_period_end, "null"),
            state(Renewal::Ended),
            "{status}"
        );
    }
    for kind in [updated, deleted] {
        for object in [
            r#"{"status":"active","cancel_at_period_end":false}"#,
            r#"{"id":"sub_1","cancel_at_period_end":false}"#,
            r#"{"id":"sub_1","status":"active"}"#,
            r#"{"id":"sub_1","status":"active","cancel_at_period_end":"no"}"#,
            r#"{"id":"sub_1","status":"active","cancel_at_period_end":false,"cancel_at":"soon"}"#,
        ] {
            assert_eq!(
                refused(&event(kind, object)),
                EventError::Object(Malformation::Shape),
                "{kind} {object}"
            );
        }
    }
    for state in Renewal::ALL {
        assert_eq!(Renewal::parse(state.as_str()), Some(state));
    }
    assert_eq!(
        Renewal::ALL.map(Renewal::as_str),
        ["active", "ending", "ended"]
    );
    assert_eq!(Renewal::parse("Active"), None);
    assert_eq!(Renewal::parse(""), None);
}

#[test]
fn a_refund_says_how_much_of_the_payment_has_gone_back() {
    let refund = |amount_refunded, in_full| {
        EventKind::ChargeRefunded(Refunded {
            payment_intent: id("pi_3"),
            amount_refunded,
            in_full,
        })
    };
    assert_eq!(
        read(
            "charge.refunded",
            r#"{"id":"ch_3","amount":800,"payment_intent":"pi_3","amount_refunded":300,"refunded":false}"#
        ),
        refund(300, false)
    );
    assert_eq!(
        read(
            "charge.refunded",
            r#"{"payment_intent":"pi_3","amount_refunded":800,"refunded":true}"#
        ),
        refund(800, true)
    );
    for object in [
        r#"{"payment_intent":null,"amount_refunded":800,"refunded":true}"#,
        r#"{"amount_refunded":800,"refunded":true}"#,
    ] {
        assert_eq!(
            read("charge.refunded", object),
            EventKind::Unused,
            "{object}"
        );
    }
    for object in [
        r#"{"payment_intent":"pi_3","refunded":true}"#,
        r#"{"payment_intent":"pi_3","amount_refunded":800}"#,
        r#"{"payment_intent":"pi_3","amount_refunded":-1,"refunded":true}"#,
        r#"{"payment_intent":"pi_3","amount_refunded":800,"refunded":"yes"}"#,
    ] {
        assert_eq!(
            refused(&event("charge.refunded", object)),
            EventError::Object(Malformation::Shape),
            "{object}"
        );
    }
}

#[test]
fn a_dispute_says_which_payment_is_contested_and_for_how_much() {
    assert_eq!(
        read(
            "charge.dispute.created",
            r#"{"id":"du_1","charge":"ch_3","payment_intent":"pi_3","amount":800,"reason":"fraudulent"}"#
        ),
        EventKind::DisputeOpened(Dispute {
            payment_intent: id("pi_3"),
            amount: 800,
        })
    );
    for object in [
        r#"{"payment_intent":null,"amount":800}"#,
        r#"{"amount":800}"#,
    ] {
        assert_eq!(
            read("charge.dispute.created", object),
            EventKind::Unused,
            "{object}"
        );
    }
    for object in [
        r#"{"payment_intent":"pi_3"}"#,
        r#"{"payment_intent":"pi_3","amount":"800"}"#,
        r#"{"payment_intent":{"id":"pi_3"},"amount":800}"#,
    ] {
        assert_eq!(
            refused(&event("charge.dispute.created", object)),
            EventError::Object(Malformation::Shape),
            "{object}"
        );
    }
}
