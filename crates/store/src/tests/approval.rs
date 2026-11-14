//! Approving an order: what the operator waives, and what stays to be
//! judged.

use portal_core::payment::stripe::{CardUsed, CheckoutSession, EventKind, PaymentStatus};
use portal_core::payment::{CardVerdict, OrderTerms, ReviewReason};

use super::paying::*;
use super::*;
use crate::{ApproveError, Approved, Progress};

const WAITING: Progress = Progress::Granted { pulled: false };

#[test]
fn approving_waives_what_the_operator_was_shown_and_nothing_else() {
    // An order placed before its plan's price changed.
    let dearer = OrderTerms {
        amount: 900,
        ..month()
    };
    // What a checkout can have wrong with it, and then what is found of
    // its card: nothing yet, a card that passes, a banned account's, a
    // wallet.
    for reason in [
        ReviewReason::Unpaid,
        ReviewReason::Link,
        ReviewReason::Amount,
        ReviewReason::Terms,
    ] {
        for card in [
            None,
            Some(CardUsed::Card(fingerprint(1))),
            Some(CardUsed::Card(fingerprint(7))),
            Some(CardUsed::Wallet),
        ] {
            let mut store = shop();
            ban_card(&mut store, 7);
            let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
            let terms = if reason == ReviewReason::Terms {
                dearer.clone()
            } else {
                month()
            };
            let order = place(&mut store, 1, &terms, 1);
            let good = checkout(1, order, &terms);
            let amiss = match reason {
                ReviewReason::Unpaid => CheckoutSession {
                    status: PaymentStatus::Other,
                    ..good
                },
                ReviewReason::Link => CheckoutSession {
                    payment_link: Some(sid("plink_1other")),
                    ..good
                },
                ReviewReason::Amount => CheckoutSession {
                    amount_total: Some(1),
                    ..good
                },
                _ => good,
            };
            record_all(
                &mut store,
                &[event(1, 60, EventKind::CheckoutCompleted(amiss))],
            );
            if let Some(card) = &card {
                record_all(
                    &mut store,
                    &[event(2, 61, charged_otherwise("pi_1", card.clone()))],
                );
            }
            let case = format!("{reason:?}, {card:?}");
            assert_eq!(
                progress(&store, &account, order),
                Progress::Review(reason),
                "{case}"
            );
            // Let through, it is judged by its card as any order is.
            let standing = match &card {
                None => Progress::Checking,
                Some(CardUsed::Card(used)) if *used == fingerprint(1) => WAITING,
                Some(CardUsed::Card(_)) => Progress::Review(ReviewReason::CardBanned),
                Some(_) => Progress::Review(ReviewReason::CardUnchecked),
            };
            assert_eq!(
                store.approve_order(&order, at(NOW + 500)).unwrap(),
                Approved {
                    account: account.code,
                    waived: Some(reason),
                    standing,
                },
                "{case}"
            );
            assert_eq!(progress(&store, &account, order), standing, "{case}");
            assert_eq!(
                entries(&store),
                if standing == WAITING {
                    vec![entry("grant", order, None)]
                } else {
                    vec![]
                },
                "{case}"
            );
        }
    }
}

#[test]
fn an_order_let_through_before_its_card_is_known_waits_for_the_card() {
    let mut store = shop();
    ban_card(&mut store, 7);
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let passes = short_paid(&mut store, 1, 1);
    let banned = short_paid(&mut store, 1, 2);
    for order in [passes, banned] {
        assert_eq!(
            store.approve_order(&order, at(NOW + 500)).unwrap().standing,
            Progress::Checking
        );
    }
    assert_eq!(entries(&store), []);
    record_all(
        &mut store,
        &[
            event(1, 600, charged("pi_1", 1)),
            event(2, 601, charged("pi_2", 7)),
        ],
    );
    assert_eq!(progress(&store, &account, passes), WAITING);
    assert_eq!(
        progress(&store, &account, banned),
        Progress::Review(ReviewReason::CardBanned)
    );
    assert_eq!(entries(&store), [entry("grant", passes, None)]);
}

#[test]
fn a_card_the_operator_passes_stays_passed() {
    for card in [CardUsed::Wallet, CardUsed::Other] {
        let mut store = shop();
        let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
        let order = place(&mut store, 1, &month(), 1);
        record_all(
            &mut store,
            &[
                event(1, 60, completed(1, order, &month())),
                event(2, 61, charged_otherwise("pi_1", card.clone())),
            ],
        );
        assert_eq!(
            progress(&store, &account, order),
            Progress::Review(ReviewReason::CardUnchecked)
        );
        assert_eq!(
            store.approve_order(&order, at(NOW + 500)).unwrap(),
            Approved {
                account: account.code,
                waived: Some(ReviewReason::CardUnchecked),
                standing: WAITING,
            },
            "{card:?}"
        );
        // Told of its card again, it does not go back to wait; what
        // happens to the money is entered after it.
        record_all(
            &mut store,
            &[
                event(3, 600, charged_otherwise("pi_1", card.clone())),
                event(4, 601, refunded("pi_1", 100, false)),
            ],
        );
        assert_eq!(progress(&store, &account, order), WAITING, "{card:?}");
        assert_eq!(
            entries(&store),
            [
                entry("grant", order, None),
                entry("refund", order, Some(100))
            ]
        );
        // Approved once is approved.
        assert!(matches!(
            store.approve_order(&order, at(NOW + 700)),
            Err(ApproveError::Granted(id)) if id == order
        ));
        assert_eq!(entries(&store).len(), 2);
    }
}

#[test]
fn a_paid_order_that_never_hears_of_its_card_can_be_passed_unchecked() {
    let mut store = shop();
    ban_card(&mut store, 7);
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    // One payment whose charge Stripe never told of; and a subscription
    // whose invoice was never said to be paid by any payment.
    let once = place(&mut store, 1, &month(), 1);
    let renewing = place(&mut store, 1, &auto(), 2);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, once, &month())),
            event(2, 61, completed(2, renewing, &auto())),
        ],
    );
    for order in [once, renewing] {
        assert_eq!(progress(&store, &account, order), Progress::Checking);
        assert_eq!(
            store.approve_order(&order, at(NOW + 500)).unwrap(),
            Approved {
                account: account.code,
                waived: None,
                standing: WAITING,
            }
        );
    }
    assert_eq!(
        entries(&store),
        [entry("grant", once, None), entry("grant", renewing, None)]
    );
    // Word that comes late changes nothing, whatever it says.
    record_all(
        &mut store,
        &[
            event(3, 600, charged("pi_1", 7)),
            event(4, 601, invoice_paid_by("in_2", "pi_2")),
            event(5, 602, charged_otherwise("pi_2", CardUsed::Wallet)),
        ],
    );
    assert_eq!(progress(&store, &account, once), WAITING);
    assert_eq!(progress(&store, &account, renewing), WAITING);
    assert_eq!(store.orders_waiting().unwrap(), []);
}

#[test]
fn a_payment_with_a_banned_accounts_card_is_never_approved() {
    let mut store = shop();
    ban_card(&mut store, 7);
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 61, charged("pi_1", 7)),
        ],
    );
    let stuck = Progress::Review(ReviewReason::CardBanned);
    assert_eq!(progress(&store, &account, order), stuck);
    let refused = store.approve_order(&order, at(NOW + 500));
    assert!(
        matches!(&refused, Err(ApproveError::CardBanned(id)) if *id == order),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        format!(
            "order {order} was paid with a card of a banned account: such a payment is \
             refunded at Stripe, never granted"
        )
    );
    // Nor once the ban is lifted: what was found of the payment stands.
    assert!(store.unban(&ban_digest("banned@qq.com")).unwrap());
    assert!(matches!(
        store.approve_order(&order, at(NOW + 501)),
        Err(ApproveError::CardBanned(_))
    ));
    // It is said of an order whose account is unbound, too: binding the
    // account would not help.
    store.unbind(&account.code).unwrap();
    assert!(matches!(
        store.approve_order(&order, at(NOW + 502)),
        Err(ApproveError::CardBanned(_))
    ));
    assert_eq!(progress(&store, &account, order), stuck);
    assert_eq!(entries(&store), []);
    // Refunded in full, it waits no more.
    record_all(&mut store, &[event(3, 700, refunded("pi_1", 800, true))]);
    assert_eq!(store.orders_waiting().unwrap(), []);
    assert!(matches!(
        store.approve_order(&order, at(NOW + 800)),
        Err(ApproveError::Returned(_))
    ));
}

#[test]
fn what_is_in_the_journal_is_not_taken_back_by_word_of_its_card() {
    let mut store = shop();
    ban_card(&mut store, 7);
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 61, charged("pi_1", 1)),
        ],
    );
    assert_eq!(progress(&store, &account, order), WAITING);
    // Stripe charges a payment once. Were it to tell of a second charge,
    // with a banned account's card, the order is Mooring's already.
    record_all(&mut store, &[event(3, 600, charged("pi_1", 7))]);
    assert_eq!(progress(&store, &account, order), WAITING);
    assert_eq!(store.orders_waiting().unwrap(), []);
    assert_eq!(entries(&store), [entry("grant", order, None)]);
    // The operator's list says what was found.
    assert_eq!(
        store.recent_orders(1).unwrap()[0].card,
        Some(CardVerdict::Banned)
    );
}

#[test]
fn an_order_waiting_for_its_unchecked_card_waits_on_word_that_the_card_is_banned() {
    let mut store = shop();
    ban_card(&mut store, 7);
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 61, charged_otherwise("pi_1", CardUsed::Wallet)),
        ],
    );
    let unchecked = Progress::Review(ReviewReason::CardUnchecked);
    assert_eq!(progress(&store, &account, order), unchecked);
    // Stripe charges a payment once. Were it to tell of a second charge,
    // a card that passes leaves the payment unchecked, and a banned
    // account's card makes it banned: the operator may pass the one and
    // never the other.
    record_all(&mut store, &[event(3, 600, charged("pi_1", 1))]);
    assert_eq!(progress(&store, &account, order), unchecked);
    record_all(&mut store, &[event(4, 601, charged("pi_1", 7))]);
    let banned = Progress::Review(ReviewReason::CardBanned);
    assert_eq!(progress(&store, &account, order), banned);
    assert!(matches!(
        store.approve_order(&order, at(NOW + 700)),
        Err(ApproveError::CardBanned(id)) if id == order
    ));
    assert_eq!(progress(&store, &account, order), banned);
    assert_eq!(entries(&store), []);
}

#[test]
fn an_order_is_approved_only_if_it_waits_and_can_still_be_granted() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    // No such order; one not paid.
    assert!(matches!(
        store.approve_order(&oid(9), at(NOW)),
        Err(ApproveError::Unknown(id)) if id == oid(9)
    ));
    let pending = place(&mut store, 1, &pack(), 8);
    assert!(matches!(
        store.approve_order(&pending, at(NOW)),
        Err(ApproveError::Unpaid(id)) if id == pending
    ));
    // Refunded in full, or disputed: it is not to be granted.
    let refunded_order = short_paid(&mut store, 1, 1);
    record_all(&mut store, &[event(1, 100, refunded("pi_1", 1, true))]);
    assert!(matches!(
        store.approve_order(&refunded_order, at(NOW)),
        Err(ApproveError::Returned(id)) if id == refunded_order
    ));
    let disputed_order = short_paid(&mut store, 1, 2);
    record_all(&mut store, &[event(2, 100, disputed("pi_2", 1))]);
    assert!(matches!(
        store.approve_order(&disputed_order, at(NOW)),
        Err(ApproveError::Returned(_))
    ));
    // Refunded in part, it can be.
    let part = short_paid(&mut store, 1, 3);
    record_all(
        &mut store,
        &[
            event(3, 100, refunded("pi_3", 1, false)),
            event(4, 101, charged("pi_3", 1)),
        ],
    );
    store.approve_order(&part, at(NOW + 200)).unwrap();
    assert_eq!(progress(&store, &account, part), WAITING);
    // One already in the journal has nothing left to approve.
    assert!(matches!(
        store.approve_order(&part, at(NOW + 201)),
        Err(ApproveError::Granted(id)) if id == part
    ));
    // The account must be bound to the user the order names.
    let unbound = short_paid(&mut store, 1, 4);
    record_all(&mut store, &[event(5, 102, charged("pi_4", 1))]);
    store.unbind(&account.code).unwrap();
    assert!(matches!(
        store.approve_order(&unbound, at(NOW)),
        Err(ApproveError::Unbound(id)) if id == unbound
    ));
    store
        .bind(&account.code, &user("pilotuserb"), at(NOW))
        .unwrap();
    assert!(matches!(
        store.approve_order(&unbound, at(NOW)),
        Err(ApproveError::Unbound(_))
    ));
    store.unbind(&account.code).unwrap();
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    store.approve_order(&unbound, at(NOW + 300)).unwrap();
    assert_eq!(progress(&store, &account, unbound), WAITING);
    // Nothing that was refused was entered.
    assert_eq!(
        entries(&store),
        [
            entry("grant", part, None),
            entry("refund", part, Some(1)),
            entry("grant", unbound, None),
        ]
    );
    for (error, said) in [
        (
            ApproveError::Unknown(oid(9)),
            format!("no order has the id {}", oid(9)),
        ),
        (
            ApproveError::Unpaid(pending),
            format!("order {pending} has not been paid; nothing changed"),
        ),
        (
            ApproveError::Granted(part),
            format!("order {part} is already in the journal for Mooring; nothing changed"),
        ),
        (
            ApproveError::Returned(part),
            format!("order {part} was refunded in full or is disputed, so it is not to be granted"),
        ),
        (
            ApproveError::Unbound(part),
            format!(
                "the account of order {part} is not bound to the Mooring user the order names; \
                 bind it first"
            ),
        ),
    ] {
        assert_eq!(error.to_string(), said);
    }
}

#[test]
fn an_order_that_waited_for_its_binding_is_approved_once_it_is_bound_again() {
    let mut store = shop();
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    store.unbind(&account.code).unwrap();
    record_all(
        &mut store,
        &[
            event(1, 60, completed(1, order, &month())),
            event(2, 61, charged("pi_1", 1)),
        ],
    );
    assert_eq!(
        progress(&store, &account, order),
        Progress::Review(ReviewReason::Binding)
    );
    assert!(matches!(
        store.approve_order(&order, at(NOW)),
        Err(ApproveError::Unbound(_))
    ));
    store
        .bind(&account.code, &user("pilotusera"), at(NOW))
        .unwrap();
    assert_eq!(
        store.approve_order(&order, at(NOW + 100)).unwrap(),
        Approved {
            account: account.code,
            waived: Some(ReviewReason::Binding),
            standing: WAITING,
        }
    );
    assert_eq!(progress(&store, &account, order), WAITING);
}
