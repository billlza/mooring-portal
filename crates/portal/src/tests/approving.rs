//! `approve-order` and `discard-test-payments`: what the operator decides
//! about payments, and what the portal says came of it.

use std::process::ExitCode;

use portal_core::payment::stripe::{CheckoutSession, Dispute, EventKind};
use portal_store::ModeError;

use super::Fixture;
use super::paying::*;

const AMOUNT: &str = "what was paid is not its price, or not in its currency";
const WALLET: &str = "it was paid from a wallet or by another method, so its card could not be \
                      checked against the ban list";
const BANNED: &str = "it was paid with a card of a banned account";

#[test]
fn approving_waives_what_the_list_showed_and_says_where_the_order_stands() {
    let fixture = Fixture::new();
    let alice = fixture.customer("alice@qq.com", 1, "pilotusera");
    fixture.signed_in("banned@qq.com", 9);
    let card = fingerprint(7);
    fixture.run(&["ban", "--account", "0000-0000-0009", "--card", &card]);
    // Four orders paid at another price: with a card no ban names, with
    // a banned account's, from a wallet, and with one Stripe has not
    // told of.
    for n in 1..=4 {
        paid_amiss(&fixture, &alice, n, |checkout| CheckoutSession {
            amount_total: Some(700),
            ..checkout
        });
    }
    fixture.told(100, vec![charged(1, 1), charged(2, 7), from_a_wallet(3)]);
    // The list says of each what letting it through would come to.
    let listed = fixture.run(&["orders"]).out;
    for (n, of_its_card) in [
        (
            1,
            "its card has passed the ban list: let through, it is entered for Mooring to grant",
        ),
        (
            2,
            "it was also paid with a card of a banned account: let through, it would wait for \
             you again, and such a payment is never granted",
        ),
        (
            3,
            "it was also paid from a wallet or by another method: let through, it will wait for \
             you again, for its card",
        ),
        (
            4,
            "Stripe has not told of its card: let through, it will wait for that",
        ),
    ] {
        let id = oid(n);
        assert_eq!(
            entry(&listed, n),
            format!(
                "  order {id}  account 0000-0000-0001  month-100  8.00 gbp\n    \
                   at Stripe: checkout cs_{n}, payment pi_{n}\n    \
                   waits for you: {AMOUNT}\n    \
                   {of_its_card}\n    \
                   refund it in full at Stripe, or let it through with `approve-order --order \
                 {id}`"
            )
        );
    }
    // And that is what it comes to.
    let approved = |n: u8, waived: &str, stands: &str| {
        let outcome = fixture.run(&["approve-order", "--order", oid(n).as_str()]);
        assert_eq!(outcome.code, ExitCode::SUCCESS, "{}", outcome.err);
        assert_eq!(
            outcome.out,
            format!(
                "approved order {} of account 0000-0000-0001 ({waived})\n{stands}\n",
                oid(n)
            )
        );
    };
    approved(1, AMOUNT, "it is entered for Mooring to grant");
    approved(
        2,
        AMOUNT,
        &format!(
            "it is not granted: it waits for you again, because {BANNED} (`orders` says what to \
             do)"
        ),
    );
    approved(
        3,
        AMOUNT,
        &format!(
            "it is not granted: it waits for you again, because {WALLET} (`orders` says what to \
             do)"
        ),
    );
    approved(
        4,
        AMOUNT,
        "it is not granted yet: it waits for Stripe to tell of its card (`orders` shows it)",
    );
    // The card is waived only once the list has shown that it is the
    // card the order waits for.
    approved(3, WALLET, "it is entered for Mooring to grant");
    approved(
        4,
        "Stripe has not told of its card, which is passed unchecked",
        "it is entered for Mooring to grant",
    );
    // A banned account's card is never waived.
    let refused = fixture.run(&["approve-order", "--order", oid(2).as_str()]);
    assert_eq!(refused.code, ExitCode::FAILURE);
    assert_eq!(
        refused.err,
        "mooring-portal: order 22222222222222222222 was paid with a card of a banned account: \
         such a payment is refunded at Stripe, never granted\n"
    );
    assert!(refused.out.is_empty());
    let listed = fixture.run(&["orders"]).out;
    assert!(
        listed.starts_with("Waiting (1 order(s), 0 payment(s) that are no order's)\n"),
        "{listed}"
    );
    assert_eq!(
        entry(&listed, 2),
        format!(
            "  order 22222222222222222222  account 0000-0000-0001  month-100  8.00 gbp\n    \
               at Stripe: checkout cs_2, payment pi_2\n    \
               waits for you: {BANNED}\n    \
               refund it in full at Stripe: such a payment is never granted"
        )
    );
    for n in [1, 3, 4] {
        let granted = format!(
            "  {}  0000-0000-0001  2026-11-05T12:00:00Z  granted ",
            oid(n)
        );
        assert!(listed.contains(&granted), "{listed}");
    }
}

#[test]
fn an_order_is_approved_by_its_id_and_only_while_it_can_still_be_granted() {
    let fixture = busy();
    // The order in review is disputed, and the account of the one that
    // waits for its card is unbound.
    fixture.told(
        80,
        vec![EventKind::DisputeOpened(Dispute {
            payment_intent: sid("pi_2"),
            amount: 250,
        })],
    );
    fixture.run(&["unbind", "--account", "0000-0000-0002"]);
    let before = fixture.run(&["orders"]).out;
    for (order, said) in [
        (
            "11111111111111111111",
            "order 11111111111111111111 is already in the journal for Mooring; nothing changed",
        ),
        (
            "22222222222222222222",
            "order 22222222222222222222 was refunded in full or is disputed, so it is not to be \
             granted",
        ),
        (
            "33333333333333333333",
            "the account of order 33333333333333333333 is not bound to the Mooring user the \
             order names; bind it first",
        ),
        (
            "44444444444444444444",
            "order 44444444444444444444 has not been paid; nothing changed",
        ),
        (
            "99999999999999999999",
            "no order has the id 99999999999999999999",
        ),
    ] {
        let refused = fixture.run(&["approve-order", "--order", order]);
        assert_eq!(refused.code, ExitCode::FAILURE, "{order}");
        assert_eq!(refused.err, format!("mooring-portal: {said}\n"));
        assert!(refused.out.is_empty());
    }
    assert_eq!(fixture.run(&["orders"]).out, before);
    // What is not an order's id is a usage error.
    let malformed = fixture.run(&["approve-order", "--order", "short"]);
    assert_eq!(malformed.code, ExitCode::from(2));
    assert!(
        malformed.err.contains("not an order id"),
        "{}",
        malformed.err
    );
}

#[test]
fn a_rehearsal_is_discarded_whole_before_real_payments_are_taken() {
    let fixture = busy();
    // While the rehearsal is there, the database is not served for real
    // money, and is told which command makes way.
    let held = fixture.store().enter_payment_mode(true).unwrap_err();
    assert!(
        matches!(held, ModeError::Held { livemode: false }),
        "{held:?}"
    );
    assert!(
        held.to_string()
            .ends_with("remove them with `discard-test-payments` first"),
        "{held}"
    );
    let discarded = fixture.run(&["discard-test-payments"]);
    assert_eq!(discarded.code, ExitCode::SUCCESS, "{}", discarded.err);
    assert_eq!(
        discarded.out,
        "discarded 5 order(s) and 1 payment(s) that were no order's, all of Stripe's test mode, \
         with what Stripe said of them; the accounts, the ban list and Mooring's pull token \
         stay\n\
         erased the deleted rows from the database files\n"
    );
    assert_eq!(
        fixture.run(&["orders"]).out,
        "Waiting (0 order(s), 0 payment(s) that are no order's)\n\
         Subscriptions Stripe may charge again (0)\n\
         Refunds and disputes of payments no order names (0)\n\
         Latest orders (0)\n\
         Mooring's pull token was made 2026-11-05T12:01:40Z\n\
         Mooring has pulled through entry 1 of 2, last at 2026-11-05T12:03:20Z\n"
    );
    // Nothing of the rehearsal is left in the files.
    for trace in [
        "11111111111111111111",
        "cs_1",
        "pi_2",
        "pi_99",
        "in_5",
        "sub_5",
        "evt_3",
    ] {
        assert!(!fixture.files_hold(trace.as_bytes()), "{trace}");
    }
    assert!(fixture.run(&["accounts"]).out.ends_with("2 account(s)\n"));
    // With nothing left to discard, the command says so and changes
    // nothing; and the database can be served for real money.
    let again = fixture.run(&["discard-test-payments"]);
    assert_eq!(again.code, ExitCode::SUCCESS, "{}", again.err);
    assert!(
        again
            .out
            .starts_with("discarded 0 order(s) and 0 payment(s) that were no order's, "),
        "{}",
        again.out
    );
    fixture.store().enter_payment_mode(true).unwrap();
}

#[test]
fn payments_made_with_real_money_are_never_discarded() {
    let fixture = Fixture::new();
    let alice = fixture.customer("alice@qq.com", 1, "pilotusera");
    let order = fixture.order(&alice, "month-100", 1);
    let stray = CheckoutSession {
        order: None,
        ..checkout(7, oid(7), "month-100")
    };
    fixture.told_as(
        true,
        1,
        vec![
            paid(1, order, "month-100"),
            charged(1, 1),
            EventKind::CheckoutCompleted(stray),
        ],
    );
    let before = fixture.run(&["orders"]).out;
    assert!(
        before.starts_with("Waiting (0 order(s), 1 payment(s) that are no order's)\n"),
        "{before}"
    );
    let refused = fixture.run(&["discard-test-payments"]);
    assert_eq!(refused.code, ExitCode::FAILURE);
    assert_eq!(
        refused.err,
        "mooring-portal: the database is for payments made with real money, which are never \
         discarded; nothing changed\n"
    );
    assert!(refused.out.is_empty());
    assert_eq!(fixture.run(&["orders"]).out, before);
}
