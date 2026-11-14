//! The operator issues a single offer and sees the exact dates to use in Stripe.

use std::process::ExitCode;

use portal_core::payment::stripe::{CheckoutSession, Event, EventKind, IntroPayment};
use portal_core::payment::{INTRO_PERIOD_SECS, INTRO_PERIODS, OrderTerms};
use portal_core::snapshot::MooringUserId;
use portal_store::Recorded;

use super::paying::{at, charged, checkout, oid, sid, terms};
use super::{Fixture, NOW};

#[test]
fn an_operator_sees_the_schedule_to_create_and_an_early_stop_request() {
    let f = Fixture::new();
    let (code, session) = f.signed_in("new@qq.com", 1);
    let user = MooringUserId::parse("pilotusera").unwrap();
    f.store().bind(&code, &user, at(0)).unwrap();
    let code = code.to_string();
    let give = [
        "issue-intro-offer",
        "--account",
        &code,
        "--user",
        "pilotusera",
        "--link-id",
        "plink_Private",
        "--link",
        "https://buy.stripe.com/test_Private",
    ];
    assert_ne!(f.run(&give).code, ExitCode::SUCCESS);
    let mut store = f.store();
    store.enter_payment_mode(false).unwrap();
    let listed = f.run(&give);
    assert_eq!(listed.code, ExitCode::SUCCESS, "{}", listed.err);
    assert_ne!(f.run(&give).code, ExitCode::SUCCESS);
    let offer = OrderTerms {
        renewal_amount: Some(1350),
        ..terms("month-100-auto")
    };
    let order = store
        .place_introductory_order(&session, &offer, oid(1), at(0))
        .unwrap();
    let initial = CheckoutSession {
        customer: Some(sid("cus_1").into()),
        payment_link: Some(sid("plink_Private")),
        ..checkout(1, order, "month-100")
    };
    for (number, kind) in [
        EventKind::CheckoutCompleted(initial),
        EventKind::IntroPaymentSucceeded(IntroPayment {
            id: sid("pi_1"),
            customer: sid("cus_1").into(),
        }),
        charged(1, 1),
    ]
    .into_iter()
    .enumerate()
    {
        let event = Event {
            id: sid(&format!("evt_Intro{number}")),
            created: at(0),
            livemode: false,
            kind,
        };
        assert_eq!(
            store
                .record_stripe_event(&event, std::slice::from_ref(&offer), at(0))
                .unwrap(),
            Recorded::New
        );
    }
    let account = store.session_account(&session, at(0)).unwrap().unwrap();
    store.request_introductory_stop(account.id, at(1)).unwrap();
    let report = f.run(&["intro-payments"]);
    assert_eq!(report.code, ExitCode::SUCCESS, "{}", report.err);
    assert!(
        report
            .out
            .contains(&format!("order {order}  customer cus_1  paid intent pi_1"))
    );
    assert!(report.out.contains(&format!(
        "start_date={}  end_date={}  end_behavior=cancel",
        NOW + INTRO_PERIOD_SECS,
        NOW + INTRO_PERIODS * INTRO_PERIOD_SECS
    )));
    assert!(
        report
            .out
            .contains("requests awaiting Stripe confirmation (1)")
    );
    assert!(report.out.contains("schedule not created yet"));
}
