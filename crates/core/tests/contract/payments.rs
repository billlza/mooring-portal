//! The payment contracts against their synthetic documents: the
//! catalogue the demonstration uses (`contract/fixtures/payments/`), what
//! the portal reads of Stripe's events (`contract/fixtures/stripe-events/`,
//! each a whole event as Stripe's API release `endive` writes one, with a
//! made-up payer), and the entitlements Mooring pulls
//! (`contract/fixtures/entitlements-v1/`), which the portal writes.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use portal_core::payment::feed::{self, Entry, EntryKind, Page};
use portal_core::payment::stripe::{
    BillingReason, CardFingerprint, CardUsed, Charge, CheckoutMode, CheckoutSession, Dispute,
    Event, EventKind, Invoice, InvoicePayment, PaymentStatus, Refunded, Renewal, StripeId,
    SubscriptionChange,
};
use portal_core::payment::{Catalogue, Currency, OrderId, OrderTerms, PlanKind};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;

fn path(sub: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../../contract/fixtures", sub]
        .iter()
        .collect()
}

fn document(sub: &str) -> String {
    let file = path(sub);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

fn id(text: &str) -> StripeId {
    StripeId::parse(text).unwrap()
}

fn at(secs: u64) -> UnixTime {
    UnixTime::from_secs(secs).unwrap()
}

fn order(text: &str) -> OrderId {
    OrderId::parse(text).unwrap()
}

fn demo() -> Catalogue {
    Catalogue::parse(&document("payments/demo.json")).unwrap()
}

/// The order every checkout among the events names.
const ORDER: &str = "0123456789abcdefghjk";

#[test]
fn the_demonstrations_catalogue_sells_one_plan_of_each_kind() {
    let catalogue = demo();
    assert!(!catalogue.livemode());
    let plans: Vec<(&str, PlanKind, u32, u64, u64, &str)> = catalogue
        .plans()
        .iter()
        .map(|plan| {
            (
                plan.id.as_str(),
                plan.kind,
                plan.days,
                plan.traffic_bytes,
                plan.amount,
                plan.payment_link_id.as_str(),
            )
        })
        .collect();
    assert_eq!(
        plans,
        [
            (
                "month-100",
                PlanKind::Month,
                30,
                100_000_000_000,
                800,
                "plink_1SynthMonth"
            ),
            (
                "month-100-auto",
                PlanKind::MonthAuto,
                30,
                100_000_000_000,
                800,
                "plink_1SynthAuto"
            ),
            (
                "pack-50",
                PlanKind::Traffic,
                90,
                50_000_000_000,
                300,
                "plink_1SynthPack"
            ),
        ]
    );
    assert_eq!(
        catalogue.manage_url(),
        Some("https://billing.stripe.com/p/login/test_SynthManage")
    );
}

fn checkout(
    session: &str,
    link: &str,
    amount: u64,
    mode: CheckoutMode,
    payment_intent: Option<&str>,
    subscription: Option<&str>,
    invoice: Option<&str>,
) -> EventKind {
    EventKind::CheckoutCompleted(CheckoutSession {
        customer: None,
        id: id(session),
        order: Some(order(ORDER)),
        payment_link: Some(id(link)),
        mode,
        status: PaymentStatus::Paid,
        amount_total: Some(amount),
        currency: Currency::parse("gbp"),
        payment_intent: payment_intent.map(id),
        subscription: subscription.map(id),
        invoice: invoice.map(id),
    })
}

fn charged(payment_intent: &str) -> EventKind {
    EventKind::ChargeSucceeded(Charge {
        payment_intent: id(payment_intent),
        card: CardUsed::Card(CardFingerprint::parse("Xt5EWLLDS7FJjR1c").unwrap()),
    })
}

fn invoiced(invoice: &str, reason: BillingReason) -> EventKind {
    EventKind::InvoicePaid(Invoice {
        introductory_order: None,
        id: id(invoice),
        subscription: id("sub_1SynthAuto"),
        reason,
        amount_paid: 800,
        currency: Currency::parse("gbp"),
    })
}

fn renewal(state: Renewal) -> EventKind {
    EventKind::SubscriptionChanged(SubscriptionChange {
        id: id("sub_1SynthAuto"),
        state,
    })
}

/// Each event among the fixtures: its file, its id's ending, when it was
/// made, and what the portal reads of it.
fn events() -> Vec<(&'static str, &'static str, u64, EventKind)> {
    vec![
        (
            "checkout-payment",
            "CheckoutPayment",
            1_794_355_260,
            checkout(
                "cs_test_a1SynthPayment",
                "plink_1SynthMonth",
                800,
                CheckoutMode::Payment,
                Some("pi_3SynthPayment"),
                None,
                None,
            ),
        ),
        (
            "checkout-pack",
            "CheckoutPack",
            1_794_355_261,
            checkout(
                "cs_test_a1SynthPack",
                "plink_1SynthPack",
                300,
                CheckoutMode::Payment,
                Some("pi_3SynthPack"),
                None,
                None,
            ),
        ),
        (
            "checkout-subscription",
            "CheckoutSubscription",
            1_794_355_262,
            checkout(
                "cs_test_a1SynthAuto",
                "plink_1SynthAuto",
                800,
                CheckoutMode::Subscription,
                None,
                Some("sub_1SynthAuto"),
                Some("in_1SynthFirst"),
            ),
        ),
        (
            "charge-payment",
            "ChargePayment",
            1_794_355_256,
            charged("pi_3SynthPayment"),
        ),
        (
            "charge-pack",
            "ChargePack",
            1_794_355_257,
            charged("pi_3SynthPack"),
        ),
        (
            "charge-first-invoice",
            "ChargeFirstInvoice",
            1_794_355_258,
            charged("pi_3SynthFirstInvoice"),
        ),
        (
            "charge-cycle-invoice",
            "ChargeCycleInvoice",
            1_796_947_258,
            charged("pi_3SynthCycleInvoice"),
        ),
        (
            "charge-wallet",
            "ChargeWallet",
            1_794_355_256,
            EventKind::ChargeSucceeded(Charge {
                payment_intent: id("pi_3SynthPayment"),
                card: CardUsed::Wallet,
            }),
        ),
        (
            "invoice-paid-first",
            "InvoicePaidFirst",
            1_794_355_263,
            invoiced("in_1SynthFirst", BillingReason::Create),
        ),
        (
            "invoice-paid-cycle",
            "InvoicePaidCycle",
            1_796_947_263,
            invoiced("in_1SynthCycle", BillingReason::Cycle),
        ),
        (
            "invoice-payment-first",
            "InvoicePaymentFirst",
            1_794_355_264,
            EventKind::InvoicePaymentPaid(InvoicePayment {
                invoice: id("in_1SynthFirst"),
                payment_intent: id("pi_3SynthFirstInvoice"),
            }),
        ),
        (
            "invoice-payment-cycle",
            "InvoicePaymentCycle",
            1_796_947_264,
            EventKind::InvoicePaymentPaid(InvoicePayment {
                invoice: id("in_1SynthCycle"),
                payment_intent: id("pi_3SynthCycleInvoice"),
            }),
        ),
        (
            "subscription-ending",
            "SubscriptionEnding",
            1_795_000_001,
            renewal(Renewal::Ending),
        ),
        (
            "subscription-resumed",
            "SubscriptionResumed",
            1_795_000_500,
            renewal(Renewal::Active),
        ),
        (
            "subscription-deleted",
            "SubscriptionDeleted",
            1_796_947_201,
            renewal(Renewal::Ended),
        ),
        (
            "charge-refunded-part",
            "ChargeRefundedPart",
            1_794_400_000,
            EventKind::ChargeRefunded(Refunded {
                payment_intent: id("pi_3SynthPayment"),
                amount_refunded: 300,
                in_full: false,
            }),
        ),
        (
            "charge-refunded-all",
            "ChargeRefundedAll",
            1_794_500_000,
            EventKind::ChargeRefunded(Refunded {
                payment_intent: id("pi_3SynthPayment"),
                amount_refunded: 800,
                in_full: true,
            }),
        ),
        (
            "dispute-created",
            "DisputeCreated",
            1_794_600_000,
            EventKind::DisputeOpened(Dispute {
                payment_intent: id("pi_3SynthPayment"),
                amount: 800,
            }),
        ),
        (
            "unused-type",
            "UnusedType",
            1_794_355_256,
            EventKind::Unused,
        ),
    ]
}

#[test]
fn every_event_is_read_for_what_the_portal_acts_on() {
    for (file, ending, made, kind) in events() {
        let parsed = Event::parse(document(&format!("stripe-events/{file}.json")).as_bytes());
        assert_eq!(
            parsed,
            Ok(Event {
                id: id(&format!("evt_1Synth{ending}")),
                created: at(made),
                livemode: false,
                kind,
            }),
            "{file}"
        );
    }
}

#[test]
fn every_event_among_the_fixtures_is_listed() {
    let listed: BTreeSet<String> = events()
        .iter()
        .map(|(file, ..)| format!("{file}.json"))
        .collect();
    let present: BTreeSet<String> = fs::read_dir(path("stripe-events"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(listed, present);
}

#[test]
fn nothing_of_the_payer_is_read_from_any_event() {
    for (file, ..) in events() {
        let text = document(&format!("stripe-events/{file}.json"));
        let shown = format!("{:?}", Event::parse(text.as_bytes()));
        for private in [
            "payer@example.test",
            "Synthetic Payer",
            "ZZ1 1ZZ",
            "4242",
            "Xt5EWLLDS7FJjR1c",
            "cus_SynthPayer",
        ] {
            assert!(!shown.contains(private), "{file}: {private}");
        }
    }
    // The events do carry them: the test would pass for nothing otherwise.
    assert!(document("stripe-events/checkout-payment.json").contains("payer@example.test"));
    assert!(document("stripe-events/charge-payment.json").contains("Xt5EWLLDS7FJjR1c"));
}

/// The golden document's entries.
fn pilot() -> Page {
    let terms = |plan: &str| OrderTerms::of(demo().plan(plan).unwrap());
    let user = |text: &str| MooringUserId::parse(text).unwrap();
    let month = |seq, kind, recorded_at| Entry {
        seq,
        kind,
        order: order(ORDER),
        user: user("pilotusera"),
        terms: terms("month-100"),
        paid_at: at(1_794_355_260),
        renewal_of: None,
        recorded_at: at(recorded_at),
    };
    let auto = |seq, kind, id: OrderId, paid_at, renewal_of, recorded_at| Entry {
        seq,
        kind,
        order: id,
        user: user("pilotuserb"),
        terms: terms("month-100-auto"),
        paid_at: at(paid_at),
        renewal_of,
        recorded_at: at(recorded_at),
    };
    let first = order("zzzzzzzzzzzzzzzzzzzz");
    let renewal = OrderId::for_renewal(&id("in_1SynthCycle"));
    Page {
        generated_at: at(1_797_000_000),
        livemode: false,
        after: 0,
        entries: vec![
            month(1, EntryKind::Grant, 1_794_355_261),
            auto(
                2,
                EntryKind::Grant,
                first,
                1_794_355_262,
                None,
                1_794_355_265,
            ),
            month(
                4,
                EntryKind::Refund {
                    amount_refunded: 300,
                    in_full: false,
                },
                1_794_400_001,
            ),
            auto(
                5,
                EntryKind::Grant,
                renewal,
                1_796_947_263,
                Some(first),
                1_796_947_265,
            ),
            month(
                6,
                EntryKind::Dispute {
                    amount_disputed: 800,
                },
                1_796_999_999,
            ),
            auto(
                7,
                EntryKind::Refund {
                    amount_refunded: 800,
                    in_full: true,
                },
                renewal,
                1_796_947_263,
                Some(first),
                1_797_000_000,
            ),
        ],
        more: true,
    }
}

/// A golden document as the portal sends it: without the white space it
/// is kept with. No value in the format holds white space.
fn compact(golden: &str) -> String {
    golden.split_whitespace().collect()
}

#[test]
fn the_portal_writes_the_golden_entitlements() {
    assert_eq!(
        pilot().to_json().unwrap(),
        compact(&document("entitlements-v1/pilot.json"))
    );
    let empty = Page {
        generated_at: at(1_797_000_000),
        livemode: true,
        after: 6,
        entries: Vec::new(),
        more: false,
    };
    assert_eq!(
        empty.to_json().unwrap(),
        compact(&document("entitlements-v1/empty.json"))
    );
}

#[test]
fn a_document_holds_two_hundred_entries_at_most() {
    // Mooring is told this number; a larger one is a change of the
    // contract, not of a constant.
    assert_eq!(feed::MAX_ENTRIES, 200);
}

#[test]
fn the_next_cursor_is_the_last_entrys_number_or_the_same_again() {
    assert_eq!(pilot().next(), 7);
    let mut page = pilot();
    page.entries.truncate(3);
    assert_eq!(page.next(), 4);
    page.entries.clear();
    page.after = 41;
    assert_eq!(page.next(), 41);
    assert!(
        page.to_json()
            .unwrap()
            .contains("\"after\":41,\"entries\":[],\"next\":41,")
    );
}
