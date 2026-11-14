//! Which payments are an order's, and why the others are not.

use super::*;
use crate::payment::Catalogue;
use crate::payment::stripe::{BillingReason, StripeId};

/// A catalogue of one plan of each kind.
fn catalogue() -> Catalogue {
    Catalogue::parse(
        r#"{
            "format": "mooring-portal-payments",
            "version": 1,
            "livemode": false,
            "currency": "gbp",
            "plans": [
                {"id": "month", "kind": "month", "days": 30, "traffic_gb": 100, "amount": 800,
                 "payment_link": "https://buy.stripe.com/test_month", "payment_link_id": "plink_1Month"},
                {"id": "auto", "kind": "month_auto", "days": 30, "traffic_gb": 100, "amount": 750,
                 "payment_link": "https://buy.stripe.com/test_auto", "payment_link_id": "plink_1Auto"},
                {"id": "pack", "kind": "traffic", "days": 90, "traffic_gb": 50, "amount": 300,
                 "payment_link": "https://buy.stripe.com/test_pack", "payment_link_id": "plink_1Pack"}
            ],
            "manage_subscription_url": "https://billing.stripe.com/p/login/test_manage"
        }"#,
    )
    .unwrap()
}

fn terms(plan: &str) -> OrderTerms {
    OrderTerms::of(catalogue().plan(plan).unwrap())
}

fn id(text: &str) -> StripeId {
    StripeId::parse(text).unwrap()
}

fn gbp() -> Option<Currency> {
    Currency::parse("gbp")
}

/// The checkout an order for the one-payment plan `link` asks for.
fn paid_once(link: &str, amount: u64) -> CheckoutSession {
    CheckoutSession {
        customer: None,
        id: id("cs_test_1"),
        order: None,
        payment_link: Some(id(link)),
        mode: CheckoutMode::Payment,
        status: PaymentStatus::Paid,
        amount_total: Some(amount),
        currency: gbp(),
        payment_intent: Some(id("pi_3")),
        subscription: None,
        invoice: None,
    }
}

/// The checkout an order for the renewing plan asks for.
fn subscribed() -> CheckoutSession {
    CheckoutSession {
        mode: CheckoutMode::Subscription,
        payment_intent: None,
        subscription: Some(id("sub_1")),
        invoice: Some(id("in_1")),
        ..paid_once("plink_1Auto", 750)
    }
}

#[test]
fn an_orders_terms_are_its_plans_when_it_was_placed() {
    assert_eq!(
        terms("pack"),
        OrderTerms {
            plan: PlanId::parse("pack").unwrap(),
            kind: PlanKind::Traffic,
            days: 90,
            traffic_bytes: 50_000_000_000,
            amount: 300,
            renewal_amount: None,
            currency: Currency::parse("gbp").unwrap(),
            payment_link_id: "plink_1Pack".to_owned(),
        }
    );
    assert_ne!(terms("month"), terms("auto"));
}

#[test]
fn the_payment_an_order_asked_for_is_accepted() {
    assert_eq!(
        terms("month").accepts(&paid_once("plink_1Month", 800)),
        Ok(())
    );
    assert_eq!(
        terms("pack").accepts(&paid_once("plink_1Pack", 300)),
        Ok(())
    );
    assert_eq!(terms("auto").accepts(&subscribed()), Ok(()));
    // A subscription's checkout may name its first payment as well.
    let with_payment = CheckoutSession {
        payment_intent: Some(id("pi_3")),
        ..subscribed()
    };
    assert_eq!(terms("auto").accepts(&with_payment), Ok(()));
    // Which order it named is not these terms' to judge.
    let named = CheckoutSession {
        order: crate::payment::OrderId::parse("0123456789abcdefghjk").ok(),
        ..paid_once("plink_1Month", 800)
    };
    assert_eq!(terms("month").accepts(&named), Ok(()));
}

#[test]
fn a_checkout_without_a_payment_is_not_one() {
    for status in [PaymentStatus::Unpaid, PaymentStatus::Other] {
        let checkout = CheckoutSession {
            status,
            ..paid_once("plink_1Month", 800)
        };
        assert_eq!(terms("month").accepts(&checkout), Err(ReviewReason::Unpaid));
        // That comes first, whatever else is wrong with it.
        let wrong = CheckoutSession {
            status,
            ..paid_once("plink_1Pack", 1)
        };
        assert_eq!(terms("month").accepts(&wrong), Err(ReviewReason::Unpaid));
    }
}

#[test]
fn a_payment_on_another_page_is_not_the_orders() {
    for link in [
        Some("plink_1Pack"),
        Some("plink_1Mont"),
        Some("plink_1Month1"),
        None,
    ] {
        let checkout = CheckoutSession {
            payment_link: link.map(id),
            ..paid_once("plink_1Month", 800)
        };
        assert_eq!(
            terms("month").accepts(&checkout),
            Err(ReviewReason::Link),
            "{link:?}"
        );
    }
    // That comes before its kind and its price.
    let wrong = CheckoutSession {
        mode: CheckoutMode::Subscription,
        ..paid_once("plink_1Pack", 1)
    };
    assert_eq!(terms("month").accepts(&wrong), Err(ReviewReason::Link));
}

#[test]
fn a_checkout_of_another_kind_is_not_the_orders() {
    let once = terms("month");
    for checkout in [
        CheckoutSession {
            mode: CheckoutMode::Subscription,
            ..paid_once("plink_1Month", 800)
        },
        CheckoutSession {
            mode: CheckoutMode::Other,
            ..paid_once("plink_1Month", 800)
        },
        // One payment, but with nothing to find its card by.
        CheckoutSession {
            payment_intent: None,
            ..paid_once("plink_1Month", 800)
        },
        // One payment that also began a subscription.
        CheckoutSession {
            subscription: Some(id("sub_1")),
            ..paid_once("plink_1Month", 800)
        },
    ] {
        assert_eq!(
            once.accepts(&checkout),
            Err(ReviewReason::Mode),
            "{checkout:?}"
        );
    }
    let renewing = terms("auto");
    for checkout in [
        paid_once("plink_1Auto", 750),
        CheckoutSession {
            mode: CheckoutMode::Other,
            ..subscribed()
        },
        // A subscription with nothing to find its renewals by.
        CheckoutSession {
            subscription: None,
            ..subscribed()
        },
        // Or its first payment's card.
        CheckoutSession {
            invoice: None,
            ..subscribed()
        },
    ] {
        assert_eq!(
            renewing.accepts(&checkout),
            Err(ReviewReason::Mode),
            "{checkout:?}"
        );
    }
    // That comes before its price.
    let wrong = CheckoutSession {
        amount_total: Some(1),
        ..paid_once("plink_1Auto", 750)
    };
    assert_eq!(renewing.accepts(&wrong), Err(ReviewReason::Mode));
}

#[test]
fn a_payment_of_another_price_is_not_the_orders() {
    let month = terms("month");
    for (amount, currency) in [
        (Some(799), gbp()),
        (Some(801), gbp()),
        (Some(0), gbp()),
        (None, gbp()),
        (Some(800), Currency::parse("usd")),
        (Some(800), None),
    ] {
        let checkout = CheckoutSession {
            amount_total: amount,
            currency,
            ..paid_once("plink_1Month", 800)
        };
        assert_eq!(
            month.accepts(&checkout),
            Err(ReviewReason::Amount),
            "{amount:?} {currency:?}"
        );
    }
}

#[test]
fn a_renewal_is_accepted_at_the_orders_price_only() {
    let auto = terms("auto");
    let invoice = |amount_paid, currency| Invoice {
        introductory_order: None,
        id: id("in_2"),
        subscription: id("sub_1"),
        reason: BillingReason::Cycle,
        amount_paid,
        currency,
    };
    assert_eq!(auto.accepts_renewal(&invoice(750, gbp())), Ok(()));
    for (amount, currency) in [
        (749, gbp()),
        (751, gbp()),
        (0, gbp()),
        (750, Currency::parse("eur")),
        (750, None),
    ] {
        assert_eq!(
            auto.accepts_renewal(&invoice(amount, currency)),
            Err(ReviewReason::Amount),
            "{amount} {currency:?}"
        );
    }
}

#[test]
fn terms_are_still_sold_only_as_the_catalogue_has_them_now() {
    let on_sale: Vec<OrderTerms> = catalogue().plans().iter().map(OrderTerms::of).collect();
    for plan in ["month", "auto", "pack"] {
        assert_eq!(terms(plan).still_sold(&on_sale), Ok(()), "{plan}");
    }
    // Each thing an order's terms hold is one the catalogue may change.
    let month = terms("month");
    for changed in [
        OrderTerms {
            plan: terms("pack").plan,
            ..month.clone()
        },
        OrderTerms {
            kind: PlanKind::Traffic,
            ..month.clone()
        },
        OrderTerms {
            days: 31,
            ..month.clone()
        },
        OrderTerms {
            traffic_bytes: month.traffic_bytes + 1,
            ..month.clone()
        },
        OrderTerms {
            amount: 799,
            ..month.clone()
        },
        OrderTerms {
            currency: Currency::parse("usd").unwrap(),
            ..month.clone()
        },
        OrderTerms {
            payment_link_id: "plink_1Other".to_owned(),
            ..month.clone()
        },
    ] {
        assert_eq!(
            changed.still_sold(&on_sale),
            Err(ReviewReason::Terms),
            "{changed:?}"
        );
    }
    assert_eq!(month.still_sold(&[]), Err(ReviewReason::Terms));
    assert_eq!(month.still_sold(&on_sale[1..]), Err(ReviewReason::Terms));
}

#[test]
fn reasons_and_verdicts_are_written_and_read_back() {
    assert_eq!(
        ReviewReason::ALL.map(ReviewReason::as_str),
        [
            "unpaid",
            "link",
            "mode",
            "amount",
            "terms",
            "card_banned",
            "card_unchecked",
            "binding"
        ]
    );
    for reason in ReviewReason::ALL {
        assert_eq!(ReviewReason::parse(reason.as_str()), Some(reason));
    }
    assert_eq!(ReviewReason::parse("Unpaid"), None);
    assert_eq!(ReviewReason::parse(""), None);
    assert_eq!(
        CardVerdict::ALL.map(CardVerdict::as_str),
        ["clear", "banned", "unchecked"]
    );
    for verdict in CardVerdict::ALL {
        assert_eq!(CardVerdict::parse(verdict.as_str()), Some(verdict));
    }
    assert_eq!(CardVerdict::parse("Clear"), None);
    assert_eq!(CardVerdict::parse(""), None);
}
