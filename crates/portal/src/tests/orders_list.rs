//! `orders`: what the operator is shown of payments.

use std::process::ExitCode;

use portal_core::payment::stripe::{
    BillingReason, CheckoutMode, CheckoutSession, Dispute, EventKind, Invoice, PaymentStatus,
};
use portal_core::payment::{OrderId, OrderTerms};

use super::Fixture;
use super::paying::*;

#[test]
fn with_nothing_ordered_the_list_says_so() {
    let fixture = Fixture::new();
    let listed = fixture.run(&["orders"]);
    assert_eq!(listed.code, ExitCode::SUCCESS, "{}", listed.err);
    assert_eq!(
        listed.out,
        "Waiting (0 order(s), 0 payment(s) that are no order's)\n\
         Subscriptions Stripe may charge again (0)\n\
         Refunds and disputes of payments no order names (0)\n\
         Latest orders (0)\n\
         Mooring has no pull token and cannot pull; make one with `pull-token`\n\
         Mooring has pulled through entry 0 of 0, and has never pulled\n"
    );
}

#[test]
fn the_list_shows_what_waits_and_for_whom_and_what_to_do() {
    let fixture = busy();
    let listed = fixture.run(&["orders"]);
    assert_eq!(listed.code, ExitCode::SUCCESS, "{}", listed.err);
    assert_eq!(
        listed.out,
        "Waiting (2 order(s), 1 payment(s) that are no order's)\n  \
           order 22222222222222222222  account 0000-0000-0001  pack-50  3.00 gbp\n    \
             at Stripe: checkout cs_2, payment pi_2\n    \
             waits for you: what was paid is not its price, or not in its currency\n    \
             Stripe has not told of its card: let through, it will wait for that\n    \
             refund it in full at Stripe, or let it through with `approve-order --order 22222222222222222222`\n  \
           order 33333333333333333333  account 0000-0000-0002  month-100  8.00 gbp\n    \
             at Stripe: checkout cs_3, payment pi_3\n    \
             waits for Stripe to tell of its charge (is charge.succeeded among the endpoint's events?)\n    \
             Stripe's Dashboard can send an event again; if it never comes, `approve-order --order 33333333333333333333` passes the card unchecked\n  \
           payment #1  noticed 2026-11-05T12:01:00Z  8.00 gbp\n    \
             at Stripe: checkout cs_7, payment pi_7\n    \
             no order's: its checkout names no order of the portal\n    \
             100 hundredths of it are refunded\n    \
             refund it in full at Stripe, which takes it off this list, or take it off with `dismiss-payment --id 1`\n\
         Subscriptions Stripe may charge again (1)\n  \
           sub_5  account 0000-0000-0002  month-100-auto  ending\n\
         Refunds and disputes of payments no order names (1)\n  \
           payment pi_99  800 hundredths refunded\n\
         Latest orders (5)\n  \
           11111111111111111111  0000-0000-0001  2026-11-05T12:00:00Z  pulled    month-100  8.00 gbp\n  \
           22222222222222222222  0000-0000-0001  2026-11-05T12:00:00Z  review    pack-50  3.00 gbp\n  \
           33333333333333333333  0000-0000-0002  2026-11-05T12:00:00Z  checking  month-100  8.00 gbp\n  \
           44444444444444444444  0000-0000-0002  2026-11-05T12:00:00Z  unpaid    pack-50  3.00 gbp\n  \
           55555555555555555555  0000-0000-0002  2026-11-05T12:00:00Z  granted   month-100-auto  8.00 gbp\n\
         Mooring's pull token was made 2026-11-05T12:01:40Z\n\
         Mooring has pulled through entry 1 of 2, last at 2026-11-05T12:03:20Z\n"
    );
}

#[test]
fn every_reason_to_wait_is_said_in_words() {
    let fixture = Fixture::new();
    let alice = fixture.customer("alice@qq.com", 1, "pilotusera");
    let carol = fixture.customer("carol@qq.com", 3, "pilotuserc");
    // A banned account's card, for what follows.
    fixture.signed_in("banned@qq.com", 9);
    let card = fingerprint(7);
    fixture.run(&["ban", "--account", "0000-0000-0009", "--card", &card]);
    // What a checkout can have wrong with it.
    paid_amiss(&fixture, &alice, 1, |checkout| CheckoutSession {
        status: PaymentStatus::Other,
        ..checkout
    });
    paid_amiss(&fixture, &alice, 2, |checkout| CheckoutSession {
        payment_link: Some(sid("plink_1Elsewhere")),
        ..checkout
    });
    paid_amiss(&fixture, &alice, 3, |checkout| CheckoutSession {
        mode: CheckoutMode::Subscription,
        ..checkout
    });
    // What its card can.
    paid_amiss(&fixture, &alice, 4, |checkout| checkout);
    paid_amiss(&fixture, &alice, 5, |checkout| checkout);
    fixture.told(100, vec![charged(4, 7), from_a_wallet(5)]);
    // Paid as ordered, on terms its plan no longer has.
    let dearer = OrderTerms {
        amount: 900,
        ..terms("month-100")
    };
    let ordered = fixture
        .store()
        .place_order(&alice, &dearer, oid(7), at(0))
        .unwrap();
    let as_ordered = CheckoutSession {
        amount_total: Some(900),
        ..checkout(7, ordered, "month-100")
    };
    fixture.told(70, vec![EventKind::CheckoutCompleted(as_ordered)]);
    // An account unbound before the card was known.
    paid_amiss(&fixture, &carol, 6, |checkout| checkout);
    fixture.run(&["unbind", "--account", "0000-0000-0003"]);
    fixture.told(110, vec![charged(6, 1)]);
    // Disputed before it was granted; and a renewing plan whose invoice's
    // payment Stripe has not told of.
    paid_amiss(&fixture, &alice, 8, |checkout| checkout);
    let renewing = fixture.order(&alice, "month-100-auto", 0);
    let subscribed = CheckoutSession {
        payment_intent: None,
        ..checkout(0, renewing, "month-100-auto")
    };
    fixture.told(
        120,
        vec![
            EventKind::DisputeOpened(Dispute {
                payment_intent: sid("pi_8"),
                amount: 800,
            }),
            EventKind::CheckoutCompleted(subscribed),
        ],
    );
    // Payments that are no order's, of each kind.
    let stray = |n: u8, change: fn(CheckoutSession) -> CheckoutSession| {
        EventKind::CheckoutCompleted(change(CheckoutSession {
            order: None,
            ..checkout(n, oid(9), "month-100")
        }))
    };
    let invoice = |id: &str, subscription: &str, reason| {
        EventKind::InvoicePaid(Invoice {
            id: sid(id),
            subscription: sid(subscription),
            reason,
            amount_paid: 750,
            currency: None,
        })
    };
    fixture.told(
        130,
        vec![
            stray(21, |checkout| CheckoutSession {
                amount_total: None,
                ..checkout
            }),
            stray(22, |checkout| CheckoutSession {
                order: Some(oid(4)),
                currency: None,
                ..checkout
            }),
            charged(22, 7),
            invoice("in_23", "sub_23", BillingReason::Cycle),
            invoice("in_24", "sub_0", BillingReason::Other),
            // And a dispute of a payment nothing names.
            EventKind::DisputeOpened(Dispute {
                payment_intent: sid("pi_77"),
                amount: 300,
            }),
        ],
    );
    let listed = fixture.run(&["orders"]);
    assert_eq!(listed.code, ExitCode::SUCCESS, "{}", listed.err);
    let out = listed.out;
    assert!(
        out.starts_with("Waiting (9 order(s), 4 payment(s) that are no order's)\n"),
        "{out}"
    );
    for said in [
        "waits for you: its checkout was completed without a payment\n",
        "waits for you: it was paid on another payment page than its plan's\n",
        "waits for you: its checkout is not the kind its plan asks for: one payment, or a \
         subscription\n",
        "waits for you: it was paid as ordered, but its plan is no longer sold on the terms it \
         was ordered on\n",
        "waits for you: it was paid with a card of a banned account\n    \
         refund it in full at Stripe: such a payment is never granted\n",
        "waits for you: it was paid from a wallet or by another method, so its card could not \
         be checked against the ban list\n    \
         refund it in full at Stripe, or pass its card unchecked with `approve-order --order \
         55555555555555555555`\n",
        "waits for you: its account is not bound to the Mooring user it names\n    \
         refund it in full at Stripe, or bind the account to that user again and then let it \
         through with `approve-order --order 66666666666666666666`\n",
        "at Stripe: checkout cs_8, payment pi_8\n    \
         its payer disputes the payment, which was never granted: answer the dispute at Stripe; \
         there is nothing to do here\n",
        "waits for Stripe to tell which payment paid its invoice (is invoice_payment.paid among \
         the endpoint's events?)\n    \
         Stripe's Dashboard can send an event again; if it never comes, `approve-order --order \
         00000000000000000000` passes the card unchecked\n",
        "at Stripe: checkout cs_0, invoice in_0, subscription sub_0\n",
        "payment #1  noticed 2026-11-05T12:01:00Z  an amount Stripe did not say\n",
        "no order's: its checkout names no order of the portal\n",
        "payment #2  noticed 2026-11-05T12:01:00Z  800 hundredths of another currency\n",
        "no order's: its checkout names an order that another checkout had paid\n",
        "    it was paid with a card of a banned account\n    refund it in full",
        "payment #3  noticed 2026-11-05T12:01:00Z  750 hundredths of another currency\n    \
         at Stripe: invoice in_23, subscription sub_23\n    \
         no order's: it renews a subscription that no order began\n",
        "no order's: it is an invoice that neither began a subscription nor renewed one\n",
        "Subscriptions Stripe may charge again (1)\n  sub_0  account 0000-0000-0001  \
         month-100-auto  active\n",
        "Refunds and disputes of payments no order names (1)\n  \
         payment pi_77  0 hundredths refunded, 300 hundredths disputed\n",
        "Latest orders (9)\n",
    ] {
        assert!(out.contains(said), "{said:?} is not in:\n{out}");
    }
    // Every order that approving can do something for is told so: not
    // the one paid with a banned account's card, nor the disputed one.
    assert_eq!(out.matches("`approve-order --order ").count(), 7);
    for never in [4, 8] {
        assert!(
            !entry(&out, never).contains("approve-order"),
            "{}",
            entry(&out, never)
        );
    }
}

#[test]
fn the_list_ends_with_the_twenty_newest_orders() {
    let fixture = Fixture::new();
    // Seven customers, each with an unpaid order for every plan, placed a
    // second apart.
    let ids: Vec<OrderId> = "0123456789abcdefghjkm"
        .chars()
        .map(|symbol| OrderId::parse(&symbol.to_string().repeat(20)).unwrap())
        .collect();
    let mut placed = ids.iter().zip(0_u64..);
    for (bits, letter) in (1_u8..).zip("abcdefg".chars()) {
        let customer = fixture.customer(
            &format!("{letter}@qq.com"),
            bits,
            &format!("pilotuser{letter}"),
        );
        for plan in ["month-100", "month-100-auto", "pack-50"] {
            let (id, after) = placed.next().unwrap();
            fixture
                .store()
                .place_order(&customer, &terms(plan), *id, at(after))
                .unwrap();
        }
    }
    assert!(placed.next().is_none());
    let out = fixture.run(&["orders"]).out;
    let (_, latest) = out.split_once("Latest orders (20)\n").expect(&out);
    let shown: Vec<&str> = latest
        .lines()
        .take_while(|line| line.starts_with("  "))
        .map(|line| &line[2..22])
        .collect();
    // Newest first, and the oldest of the twenty-one is not among them.
    let newest_first: Vec<&str> = ids[1..].iter().rev().map(OrderId::as_str).collect();
    assert_eq!(shown, newest_first);
}
