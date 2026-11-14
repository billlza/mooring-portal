//! What the payment tests share: plans, customers, and Stripe's events as
//! the portal reads them.

use portal_core::payment::stripe::{
    BillingReason, CardFingerprint, CardUsed, Charge, CheckoutMode, CheckoutSession, Dispute,
    Event, EventKind, Invoice, InvoicePayment, PaymentStatus, Refunded, Renewal, StripeId,
    SubscriptionChange,
};
use portal_core::payment::{Currency, OrderId, OrderTerms, PlanId, PlanKind};

use super::*;
use crate::{Order, Progress, Recorded};

pub(super) fn gbp() -> Currency {
    Currency::parse("gbp").unwrap()
}

fn terms(plan: &str, kind: PlanKind, days: u32, gigabytes: u64, amount: u64) -> OrderTerms {
    OrderTerms {
        plan: PlanId::parse(plan).unwrap(),
        kind,
        days,
        traffic_bytes: gigabytes * 1_000_000_000,
        amount,
        renewal_amount: None,
        currency: gbp(),
        payment_link_id: format!("plink_1{plan}").replace('-', ""),
    }
}

/// A month, paid once.
pub(super) fn month() -> OrderTerms {
    terms("month", PlanKind::Month, 30, 100, 800)
}

/// A month that renews by itself.
pub(super) fn auto() -> OrderTerms {
    terms("auto", PlanKind::MonthAuto, 30, 100, 750)
}

/// A pack of traffic.
pub(super) fn pack() -> OrderTerms {
    terms("pack", PlanKind::Traffic, 90, 50, 300)
}

/// What the catalogue offers: one plan of each kind.
pub(super) fn on_sale() -> [OrderTerms; 3] {
    [month(), auto(), pack()]
}

/// A database as a server leaves it that has started with a catalogue of
/// Stripe's test mode: the events the tests make are of that mode.
pub(super) fn shop() -> Store {
    let mut store = store();
    store.enter_payment_mode(false).unwrap();
    store
}

pub(super) fn sid(text: &str) -> StripeId {
    StripeId::parse(text).unwrap()
}

/// The order id whose every symbol is the `n`th of the ids' alphabet.
pub(super) fn oid(n: u8) -> OrderId {
    let symbol = char::from(b"0123456789abcdefghjkmnpqrstvwxyz"[usize::from(n)]);
    OrderId::parse(&symbol.to_string().repeat(20)).unwrap()
}

/// An account registered as `address` with code `bits`, signed in with
/// the session whose token has `digest(bits)`, and bound to `user`.
pub(super) fn customer(store: &mut Store, address: &str, bits: u8, user_id: &str) -> Account {
    let account = register(store, address, u64::from(bits));
    store
        .start_session(&session(&account, bits, NOW, 1_000_000))
        .unwrap();
    store.bind(&account.code, &user(user_id), at(NOW)).unwrap();
    account
}

/// Places an order on `terms` for the customer signed in with session
/// `bits`, offering the id `oid(fresh)`.
pub(super) fn place(store: &mut Store, bits: u8, terms: &OrderTerms, fresh: u8) -> OrderId {
    store
        .place_order(&digest(bits), terms, oid(fresh), at(NOW))
        .unwrap()
}

/// The event numbered `n`, made `after` seconds after [`NOW`].
pub(super) fn event(n: u32, after: u64, kind: EventKind) -> Event {
    Event {
        id: sid(&format!("evt_{n}")),
        created: at(NOW + after),
        livemode: false,
        kind,
    }
}

/// Records `event` a second after it was made.
pub(super) fn record(store: &mut Store, event: &Event) -> Recorded {
    let received = at(event.created.as_secs() + 1);
    store
        .record_stripe_event(event, &on_sale(), received)
        .unwrap()
}

/// Records each of `events`, which must all be new.
pub(super) fn record_all(store: &mut Store, events: &[Event]) {
    for event in events {
        assert_eq!(record(store, event), Recorded::New, "{event:?}");
    }
}

/// The checkout numbered `n` that an order on `terms` asks for: one
/// payment `pi_n`, or the subscription `sub_n` with its first invoice
/// `in_n`.
pub(super) fn checkout(n: u8, order: OrderId, terms: &OrderTerms) -> CheckoutSession {
    let renews = terms.kind.renews();
    CheckoutSession {
        customer: None,
        id: sid(&format!("cs_{n}")),
        order: Some(order),
        payment_link: Some(sid(&terms.payment_link_id)),
        mode: if renews {
            CheckoutMode::Subscription
        } else {
            CheckoutMode::Payment
        },
        status: PaymentStatus::Paid,
        amount_total: Some(terms.amount),
        currency: Some(terms.currency),
        payment_intent: (!renews).then(|| sid(&format!("pi_{n}"))),
        subscription: renews.then(|| sid(&format!("sub_{n}"))),
        invoice: renews.then(|| sid(&format!("in_{n}"))),
    }
}

pub(super) fn completed(n: u8, order: OrderId, terms: &OrderTerms) -> EventKind {
    EventKind::CheckoutCompleted(checkout(n, order, terms))
}

/// An order of the customer signed in with session `bits`, paid by the
/// checkout numbered `n` at the wrong price: it waits for the operator.
pub(super) fn short_paid(store: &mut Store, bits: u8, n: u8) -> OrderId {
    let order = place(store, bits, &month(), n);
    let short = CheckoutSession {
        amount_total: Some(1),
        ..checkout(n, order, &month())
    };
    record_all(
        store,
        &[event(
            u32::from(n) * 100,
            60,
            EventKind::CheckoutCompleted(short),
        )],
    );
    order
}

/// The fingerprint of the card numbered `n`.
pub(super) fn fingerprint(n: u8) -> CardFingerprint {
    CardFingerprint::parse(&format!("CardNumber{n:06}")).unwrap()
}

/// Bans an account, registered as `banned@qq.com`, with the card numbered
/// `card`.
pub(super) fn ban_card(store: &mut Store, card: u8) {
    let banned = register(store, "banned@qq.com", 99);
    store
        .ban(&banned.code, &[fingerprint(card).digest()], at(NOW))
        .unwrap();
}

/// The payment `payment_intent` charged to the card numbered `card`.
pub(super) fn charged(payment_intent: &str, card: u8) -> EventKind {
    EventKind::ChargeSucceeded(Charge {
        payment_intent: sid(payment_intent),
        card: CardUsed::Card(fingerprint(card)),
    })
}

/// The payment `payment_intent` charged to something that is no card
/// typed into Stripe's page.
pub(super) fn charged_otherwise(payment_intent: &str, card: CardUsed) -> EventKind {
    EventKind::ChargeSucceeded(Charge {
        payment_intent: sid(payment_intent),
        card,
    })
}

pub(super) fn refunded(payment_intent: &str, amount_refunded: u64, in_full: bool) -> EventKind {
    EventKind::ChargeRefunded(Refunded {
        payment_intent: sid(payment_intent),
        amount_refunded,
        in_full,
    })
}

pub(super) fn disputed(payment_intent: &str, amount: u64) -> EventKind {
    EventKind::DisputeOpened(Dispute {
        payment_intent: sid(payment_intent),
        amount,
    })
}

pub(super) fn invoiced(
    invoice: &str,
    subscription: &str,
    reason: BillingReason,
    amount_paid: u64,
) -> EventKind {
    EventKind::InvoicePaid(Invoice {
        introductory_order: None,
        id: sid(invoice),
        subscription: sid(subscription),
        reason,
        amount_paid,
        currency: Some(gbp()),
    })
}

pub(super) fn invoice_paid_by(invoice: &str, payment_intent: &str) -> EventKind {
    EventKind::InvoicePaymentPaid(InvoicePayment {
        invoice: sid(invoice),
        payment_intent: sid(payment_intent),
    })
}

pub(super) fn subscription(id: &str, state: Renewal) -> EventKind {
    EventKind::SubscriptionChanged(SubscriptionChange { id: sid(id), state })
}

/// The order `id` of `account`.
pub(super) fn order_of(store: &Store, account: &Account, id: OrderId) -> Order {
    store.order_of(account.id, &id).unwrap().unwrap()
}

/// Where the order `id` of `account` stands.
pub(super) fn progress(store: &Store, account: &Account, id: OrderId) -> Progress {
    order_of(store, account, id).progress
}

/// The journal: each entry's number, kind, order and amount.
pub(super) fn journal(store: &Store) -> Vec<(i64, String, String, Option<i64>)> {
    store
        .conn
        .prepare("SELECT seq, kind, order_id, amount FROM entitlements ORDER BY seq")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The journal without its numbers: kind, order and amount of each entry,
/// in the journal's order.
pub(super) fn entries(store: &Store) -> Vec<(String, String, Option<i64>)> {
    journal(store)
        .into_iter()
        .map(|(_, kind, order, amount)| (kind, order, amount))
        .collect()
}

/// A journal entry as [`entries`] gives it.
pub(super) fn entry(
    kind: &str,
    order: OrderId,
    amount: Option<i64>,
) -> (String, String, Option<i64>) {
    (kind.to_owned(), order.to_string(), amount)
}

/// How many rows `table` holds.
pub(super) fn rows(store: &Store, table: &str) -> i64 {
    store
        .conn
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

/// Every way of ordering `items`.
pub(super) fn orderings<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut all = Vec::new();
    for index in 0..items.len() {
        let mut rest = items.to_vec();
        let first = rest.remove(index);
        for mut tail in orderings(&rest) {
            tail.insert(0, first.clone());
            all.push(tail);
        }
    }
    all
}
