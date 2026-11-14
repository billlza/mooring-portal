//! What the operator's page of payments says of each order that waits,
//! and of each approval it refuses.

use portal_core::account::AccountCode;
use portal_core::payment::stripe::StripeId;
use portal_core::payment::{Currency, OrderId, OrderTerms, PlanId, PlanKind};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use portal_store::{Order, StoreError};

use super::*;
use crate::i18n::Lang;

/// A month's order, in review for `why`, or paid and waiting for word of
/// its card if `why` is `None`, with what its card's check found.
fn waiting(why: Option<ReviewReason>, card: Option<CardVerdict>) -> OperatorOrder {
    OperatorOrder {
        order: Order {
            id: OrderId::parse("0123456789abcdefghjk").unwrap(),
            user: MooringUserId::parse("pilotusera").unwrap(),
            terms: OrderTerms {
                plan: PlanId::parse("month").unwrap(),
                kind: PlanKind::Month,
                days: 30,
                traffic_bytes: 100_000_000_000,
                amount: 800,
                currency: Currency::parse("gbp").unwrap(),
                payment_link_id: "plink_1month".to_owned(),
            },
            created_at: UnixTime::from_secs(1_793_880_000).unwrap(),
            paid_at: Some(UnixTime::from_secs(1_793_883_600).unwrap()),
            progress: why.map_or(Progress::Checking, Progress::Review),
            refunded: 0,
            refunded_in_full: false,
            disputed: false,
            renewal_of: None,
        },
        account: AccountCode::from_bits(7),
        card,
        session: Some(StripeId::parse("cs_1").unwrap()),
        payment_intent: Some(StripeId::parse("pi_1").unwrap()),
        invoice: None,
        subscription: None,
    }
}

#[test]
fn what_a_form_says_was_shown_is_read_back_and_nothing_else() {
    for shown in ReviewReason::ALL.map(Some).into_iter().chain([None]) {
        assert_eq!(shown_from_key(shown_key(shown)), Some(shown));
    }
    for key in ["", "Checking", "card", "amount ", "approved"] {
        assert_eq!(shown_from_key(key), None, "{key:?}");
    }
}

#[test]
fn every_order_that_waits_says_why_and_what_to_do() {
    let t = Lang::Zh.strings();
    let r = &t.review;
    for card in [
        None,
        Some(CardVerdict::Clear),
        Some(CardVerdict::Banned),
        Some(CardVerdict::Unchecked),
    ] {
        let next = match card {
            None => r.next_unknown,
            Some(CardVerdict::Clear) => r.next_clear,
            Some(CardVerdict::Banned) => r.next_banned,
            Some(CardVerdict::Unchecked) => r.next_unchecked,
        };
        for why in ReviewReason::ALL {
            let view = WaitingView::of(&waiting(Some(why), card), t);
            assert_eq!(view.why, why_it_waits(why, t));
            assert_eq!(view.shown, why.as_str());
            let (todo, approvable) = match why {
                ReviewReason::CardBanned => (vec![r.todo_card_banned], false),
                ReviewReason::CardUnchecked => (vec![r.todo_card_unchecked], true),
                ReviewReason::Binding => (vec![r.todo_binding], true),
                ReviewReason::Unpaid
                | ReviewReason::Link
                | ReviewReason::Mode
                | ReviewReason::Amount
                | ReviewReason::Terms => (vec![next, r.todo_checkout], true),
            };
            assert_eq!(
                (view.todo, view.approvable),
                (todo, approvable),
                "{why:?} {card:?}"
            );
        }
        let paid = WaitingView::of(&waiting(None, card), t);
        assert_eq!(
            (paid.why, paid.todo, paid.approvable, paid.shown),
            (r.waits_charge, vec![r.todo_checking], true, "checking")
        );
    }
    // Each reason is its own.
    let mut said: Vec<&str> = ReviewReason::ALL
        .iter()
        .map(|why| why_it_waits(*why, t))
        .collect();
    said.sort_unstable();
    said.dedup();
    assert_eq!(said.len(), ReviewReason::ALL.len());
}

#[test]
fn the_stripe_ids_shown_are_those_there_are() {
    let t = Lang::Zh.strings();
    let r = &t.review;
    // Paid by an invoice whose payment is not known yet.
    let mut invoiced = waiting(None, None);
    invoiced.payment_intent = None;
    invoiced.invoice = Some(StripeId::parse("in_1").unwrap());
    invoiced.subscription = Some(StripeId::parse("sub_1").unwrap());
    let view = WaitingView::of(&invoiced, t);
    assert_eq!(view.why, r.waits_invoice);
    assert_eq!(
        view.at_stripe,
        [
            (r.id_checkout, "cs_1".to_owned()),
            (r.id_invoice, "in_1".to_owned()),
            (r.id_subscription, "sub_1".to_owned()),
        ]
    );
    assert_eq!(view.account, AccountCode::from_bits(7).to_string());
    // Disputed, nothing is to be done here, whatever it waited for.
    let mut disputed = waiting(Some(ReviewReason::Amount), None);
    disputed.order.disputed = true;
    let view = WaitingView::of(&disputed, t);
    assert_eq!(
        (view.why, view.todo.len(), view.approvable),
        (r.disputed, 0, false)
    );
}

#[test]
fn each_refusal_names_its_order_and_a_failure_is_no_refusal() {
    let t = Lang::Zh.strings();
    let r = &t.review;
    let id = OrderId::parse("0123456789abcdefghjk").unwrap();
    for (error, phrase) in [
        (ApproveError::Unknown(id), r.unknown_order),
        (ApproveError::Unpaid(id), r.refused_unpaid),
        (ApproveError::Granted(id), r.refused_granted),
        (ApproveError::Returned(id), r.refused_returned),
        (ApproveError::CardBanned(id), r.refused_card_banned),
        (ApproveError::Unbound(id), r.refused_unbound),
        (ApproveError::Changed(id), r.refused_changed),
    ] {
        assert_eq!(approve_refusal(&error, t), Some(fill(phrase, id)));
    }
    let failed = ApproveError::Store(StoreError::Missing { what: "order" });
    assert_eq!(approve_refusal(&failed, t), None);
}
