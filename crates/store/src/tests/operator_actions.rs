//! The writes of the operator pages: made only for a session that may act
//! on them when the write runs, all of a write or none of it.

use portal_core::feedback::MessageBody;
use portal_core::payment::stripe::CardUsed;
use portal_core::payment::{OrderId, ReviewReason};
use portal_core::rental::{BookingTerms, Machine, Period};

use super::paying::{
    auto, charged_otherwise, completed, customer, event, month, place, progress, record_all, shop,
    short_paid, sid,
};
use super::*;
use crate::{
    ApproveError, Approved, AsOperator, BindError, NotAllowed, OPERATOR_CONFIRM_SECS,
    OperatorActions, Progress, ReplyError, StoreError, Unbound,
};

/// The operator of the tests: registered with code 50, signed in with the
/// session `digest(50)` for a day, and confirmed at [`NOW`] with the
/// session that replaces it, `digest(51)`.
fn confirmed_operator(store: &mut Store) -> Account {
    let operator = register(store, "op@qq.com", 50);
    store.grant_operator(&operator.code, at(NOW)).unwrap();
    store
        .start_session(&session(&operator, 50, NOW, 86_400))
        .unwrap();
    assert!(
        store
            .confirm_session(&digest(50), &session(&operator, 51, NOW, 86_400))
            .unwrap()
    );
    operator
}

/// Binds `code` to `user` from the operator pages, as the session `byte`,
/// at `now`.
fn bind_as(
    store: &mut Store,
    byte: u8,
    now: u64,
    code: AccountCode,
    to: &str,
) -> Result<AsOperator<()>, BindError> {
    let to = user(to);
    store.as_operator(&digest(byte), at(now), |pages| pages.bind(&code, &to))
}

/// Approves `order` from the operator pages, as the confirmed operator,
/// having shown that it waits for `shown`.
fn approve_as(
    store: &mut Store,
    order: OrderId,
    shown: Option<ReviewReason>,
) -> Result<AsOperator<Approved>, ApproveError> {
    store.as_operator(&digest(51), at(NOW + 1), |pages| {
        pages.approve_order(&order, shown)
    })
}

/// Whether `code` is bound.
fn bound(store: &Store, code: AccountCode) -> bool {
    store
        .accounts()
        .unwrap()
        .iter()
        .any(|account| account.code == code && account.binding.is_some())
}

#[test]
fn only_a_session_that_may_act_when_the_write_runs_makes_it() {
    let mut store = store();
    let operator = confirmed_operator(&mut store);
    let target = register(&mut store, "a@qq.com", 1);
    store
        .start_session(&session(&target, 1, NOW, 86_400))
        .unwrap();
    // The operator's other session never confirmed.
    store
        .start_session(&session(&operator, 52, NOW, 86_400))
        .unwrap();
    for (byte, now, not) in [
        (9, NOW, NotAllowed::SignedOut),
        // The session the confirmation replaced.
        (50, NOW, NotAllowed::SignedOut),
        (51, NOW + 86_400, NotAllowed::SignedOut),
        (1, NOW, NotAllowed::NotOperator),
        (52, NOW, NotAllowed::Unconfirmed),
        (51, NOW + OPERATOR_CONFIRM_SECS, NotAllowed::Unconfirmed),
        (51, NOW - 1, NotAllowed::Unconfirmed),
    ] {
        let mut ran = false;
        let refused = store
            .as_operator(&digest(byte), at(now), |_| {
                ran = true;
                Ok::<_, StoreError>(())
            })
            .unwrap();
        assert_eq!(refused, AsOperator::NotAllowed(not), "{byte} at {now}");
        assert!(!ran, "{byte} at {now}");
    }
    assert_eq!(
        bind_as(
            &mut store,
            51,
            NOW + OPERATOR_CONFIRM_SECS - 1,
            target.code,
            "pilotusera"
        )
        .unwrap(),
        AsOperator::Done(())
    );
    assert!(bound(&store, target.code));
}

#[test]
fn what_ends_an_operators_session_or_rights_stops_its_next_write() {
    let mut store = store();
    let operator = confirmed_operator(&mut store);
    let target = register(&mut store, "a@qq.com", 1);
    store.revoke_operator(&operator.code).unwrap();
    assert_eq!(
        bind_as(&mut store, 51, NOW + 1, target.code, "pilotusera").unwrap(),
        AsOperator::NotAllowed(NotAllowed::NotOperator)
    );
    // Made an operator again, it must confirm again.
    store.grant_operator(&operator.code, at(NOW + 2)).unwrap();
    assert_eq!(
        bind_as(&mut store, 51, NOW + 3, target.code, "pilotusera").unwrap(),
        AsOperator::NotAllowed(NotAllowed::Unconfirmed)
    );
    assert!(
        store
            .confirm_session(&digest(51), &session(&operator, 53, NOW + 4, 86_400))
            .unwrap()
    );
    store.end_session(&digest(53)).unwrap();
    assert_eq!(
        bind_as(&mut store, 53, NOW + 5, target.code, "pilotusera").unwrap(),
        AsOperator::NotAllowed(NotAllowed::SignedOut)
    );
    assert!(!bound(&store, target.code));
}

#[test]
fn a_write_that_fails_leaves_nothing_of_itself() {
    let mut store = store();
    confirmed_operator(&mut store);
    let first = register(&mut store, "a@qq.com", 1);
    let second = register(&mut store, "b@qq.com", 2);
    let both = |pages: &OperatorActions<'_>| {
        pages.bind(&first.code, &user("pilotusera"))?;
        pages.bind(&second.code, &user("pilotusera"))
    };
    assert!(matches!(
        store.as_operator(&digest(51), at(NOW + 1), both),
        Err(BindError::UserBound { .. })
    ));
    assert!(!bound(&store, first.code));
    assert!(!bound(&store, second.code));
}

#[test]
fn each_write_is_the_hosts_own() {
    let mut store = shop();
    confirmed_operator(&mut store);
    let renewing = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let banned = customer(&mut store, "b@qq.com", 2, "pilotuserb");
    let order = place(&mut store, 1, &auto(), 1);
    record_all(&mut store, &[event(1, 60, completed(1, order, &auto()))]);
    let machine = Machine::find("m6-24-512").unwrap();
    store
        .book(
            &digest(1),
            &BookingTerms::of(machine, Period::Month),
            at(NOW),
        )
        .unwrap();
    let booking = store.bookings().unwrap()[0].booking.id;
    let body = MessageBody::parse("收到，正在处理").unwrap();
    let done = store
        .as_operator(&digest(51), at(NOW + 1), |pages| {
            pages.reply(&renewing.code, &body)?;
            let unbound = pages.unbind(&renewing.code)?;
            let ban = pages.ban(&banned.code)?;
            let closed = pages.close_booking(booking)?;
            // What the page will list, as this transaction leaves it.
            let listed = pages.accounts()?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((unbound, ban, closed, listed))
        })
        .unwrap();
    let AsOperator::Done((unbound, ban, closed, listed)) = done else {
        panic!("{done:?}");
    };
    assert_eq!(listed, store.accounts().unwrap());
    assert_eq!(
        listed
            .iter()
            .map(|account| (account.code, account.binding.is_some()))
            .collect::<Vec<_>>(),
        [(renewing.code, false), (AccountCode::from_bits(50), false)]
    );
    assert_eq!(
        unbound,
        Unbound {
            user: user("pilotusera"),
            subscriptions: vec![sid("sub_1")],
        }
    );
    assert_eq!(ban.unwrap().user, Some(user("pilotuserb")));
    assert_eq!(closed.unwrap().account, renewing.code);
    assert!(!bound(&store, renewing.code));
    assert_eq!(store.accounts().unwrap().len(), 2);
    assert_eq!(
        store.conversation_with(&renewing.code).unwrap().unwrap()[0].body,
        body
    );
    assert!(store.bookings().unwrap().is_empty());
}

#[test]
fn an_operators_account_is_not_banned_from_the_pages() {
    let mut store = store();
    let operator = confirmed_operator(&mut store);
    let other = register(&mut store, "op2@qq.com", 2);
    store.grant_operator(&other.code, at(NOW)).unwrap();
    for code in [operator.code, other.code] {
        let refused = store
            .as_operator(&digest(51), at(NOW + 1), |pages| pages.ban(&code))
            .unwrap();
        assert_eq!(refused, AsOperator::Done(None));
    }
    assert_eq!(store.accounts().unwrap().len(), 2);
}

#[test]
fn a_reply_reaches_only_an_account_there_is() {
    let mut store = store();
    confirmed_operator(&mut store);
    let body = MessageBody::parse("在").unwrap();
    let nobody = AccountCode::from_bits(9);
    assert!(matches!(
        store.as_operator(&digest(51), at(NOW + 1), |pages| pages
            .reply(&nobody, &body)),
        Err(ReplyError::UnknownAccount(_))
    ));
}

#[test]
fn an_order_is_approved_from_the_pages_for_what_they_showed_alone() {
    let mut store = shop();
    confirmed_operator(&mut store);
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    // Paid at the wrong price, and from a wallet.
    let order = short_paid(&mut store, 1, 1);
    record_all(
        &mut store,
        &[event(101, 61, charged_otherwise("pi_1", CardUsed::Wallet))],
    );
    let review = |why| Progress::Review(why);
    assert_eq!(
        progress(&store, &account, order),
        review(ReviewReason::Amount)
    );
    // What the page did not show is not waived.
    for shown in [None, Some(ReviewReason::CardUnchecked)] {
        assert!(
            matches!(
                approve_as(&mut store, order, shown),
                Err(ApproveError::Changed(id)) if id == order
            ),
            "{shown:?}"
        );
    }
    assert_eq!(
        progress(&store, &account, order),
        review(ReviewReason::Amount)
    );
    assert_eq!(
        approve_as(&mut store, order, Some(ReviewReason::Amount)).unwrap(),
        AsOperator::Done(Approved {
            account: account.code,
            waived: Some(ReviewReason::Amount),
            standing: review(ReviewReason::CardUnchecked),
        })
    );
    // The same form again waives nothing: the order waits for its card
    // now, which that page never showed.
    assert!(matches!(
        approve_as(&mut store, order, Some(ReviewReason::Amount)),
        Err(ApproveError::Changed(_))
    ));
    assert_eq!(
        progress(&store, &account, order),
        review(ReviewReason::CardUnchecked)
    );
    assert_eq!(
        approve_as(&mut store, order, Some(ReviewReason::CardUnchecked)).unwrap(),
        AsOperator::Done(Approved {
            account: account.code,
            waived: Some(ReviewReason::CardUnchecked),
            standing: Progress::Granted { pulled: false },
        })
    );
    // Granted, it is said to be, whatever the page showed.
    assert!(matches!(
        approve_as(&mut store, order, Some(ReviewReason::CardUnchecked)),
        Err(ApproveError::Granted(_))
    ));
}

#[test]
fn a_paid_order_is_passed_unchecked_from_the_pages_while_it_waits_for_its_card() {
    let mut store = shop();
    confirmed_operator(&mut store);
    let account = customer(&mut store, "a@qq.com", 1, "pilotusera");
    let order = place(&mut store, 1, &month(), 1);
    record_all(&mut store, &[event(1, 60, completed(1, order, &month()))]);
    assert_eq!(progress(&store, &account, order), Progress::Checking);
    assert!(matches!(
        approve_as(&mut store, order, Some(ReviewReason::CardUnchecked)),
        Err(ApproveError::Changed(_))
    ));
    assert_eq!(
        approve_as(&mut store, order, None).unwrap(),
        AsOperator::Done(Approved {
            account: account.code,
            waived: None,
            standing: Progress::Granted { pulled: false },
        })
    );
}
