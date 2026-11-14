//! What the payment tests share: customers, orders, and Stripe's events
//! as the portal reads them.

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use portal_core::auth::TokenDigest;
use portal_core::payment::stripe::{
    CardFingerprint, CardUsed, Charge, CheckoutMode, CheckoutSession, Event, EventKind,
    PaymentStatus, Refunded, Renewal, StripeId, SubscriptionChange,
};
use portal_core::payment::{Catalogue, OrderId, OrderTerms};
use portal_core::snapshot::MooringUserId;
use portal_core::time::UnixTime;
use portal_store::Recorded;

use super::{Fixture, NOW};

pub(super) const SECRET: &str = "whsec_test_portal_0123456789abcdef";
pub(super) const OTHER_SECRET: &str = "whsec_test_other_fedcba9876543210";

/// The demonstration's catalogue.
pub(super) fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../contract/fixtures/payments/demo.json")
}

fn catalogue() -> Catalogue {
    Catalogue::parse(&std::fs::read_to_string(demo()).unwrap()).unwrap()
}

pub(super) fn terms(plan: &str) -> OrderTerms {
    OrderTerms::of(catalogue().plan(plan).unwrap())
}

pub(super) fn at(after: u64) -> UnixTime {
    UnixTime::from_secs(NOW + after).unwrap()
}

pub(super) fn sid(text: &str) -> StripeId {
    StripeId::parse(text).unwrap()
}

/// The order id whose every symbol is the digit `n`.
pub(super) fn oid(n: u8) -> OrderId {
    OrderId::parse(&n.to_string().repeat(20)).unwrap()
}

impl Fixture {
    /// A file of `contents` with the permission bits `mode`.
    pub(super) fn file_with_mode(&self, name: &str, contents: &[u8], mode: u32) -> PathBuf {
        let path = self.file(name, contents);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    /// A copy of the demonstration's catalogue that only its owner can
    /// change, whatever the checkout's permission bits are.
    pub(super) fn catalogue(&self) -> PathBuf {
        self.file_with_mode("catalogue.json", &std::fs::read(demo()).unwrap(), 0o644)
    }

    /// A customer: registered with the code `bits`, signed in and bound
    /// to `user`.
    pub(super) fn customer(&self, email: &str, bits: u8, user: &str) -> TokenDigest {
        let (code, session) = self.signed_in(email, u64::from(bits));
        self.store()
            .bind(&code, &MooringUserId::parse(user).unwrap(), at(0))
            .unwrap();
        session
    }

    /// Places an order for `plan` with the id `oid(n)`.
    pub(super) fn order(&self, session: &TokenDigest, plan: &str, n: u8) -> OrderId {
        self.store()
            .place_order(session, &terms(plan), oid(n), at(0))
            .unwrap()
    }

    /// Records the events `kinds` as a portal that sells the
    /// demonstration's catalogue does: each made and received a minute
    /// after [`NOW`], numbered from `first`.
    pub(super) fn told(&self, first: u32, kinds: Vec<EventKind>) {
        self.told_as(false, first, kinds);
    }

    /// [`Self::told`], of payments made with real money if `livemode`.
    pub(super) fn told_as(&self, livemode: bool, first: u32, kinds: Vec<EventKind>) {
        let mut store = self.store();
        store.enter_payment_mode(livemode).unwrap();
        let on_sale: Vec<OrderTerms> = catalogue().plans().iter().map(OrderTerms::of).collect();
        for (n, kind) in (first..).zip(kinds) {
            let event = Event {
                id: sid(&format!("evt_{n}")),
                created: at(60),
                livemode,
                kind,
            };
            let recorded = store.record_stripe_event(&event, &on_sale, at(60)).unwrap();
            assert_eq!(recorded, Recorded::New, "event {n}");
        }
    }
}

/// The checkout numbered `n` that `order` on `plan` asks for.
pub(super) fn checkout(n: u8, order: OrderId, plan: &str) -> CheckoutSession {
    let terms = terms(plan);
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
        payment_intent: Some(sid(&format!("pi_{n}"))),
        subscription: renews.then(|| sid(&format!("sub_{n}"))),
        invoice: renews.then(|| sid(&format!("in_{n}"))),
    }
}

pub(super) fn paid(n: u8, order: OrderId, plan: &str) -> EventKind {
    EventKind::CheckoutCompleted(checkout(n, order, plan))
}

/// Places an order for the month and pays it by the checkout numbered
/// `n`, changed by `amiss`.
pub(super) fn paid_amiss(
    fixture: &Fixture,
    session: &TokenDigest,
    n: u8,
    amiss: impl FnOnce(CheckoutSession) -> CheckoutSession,
) {
    let order = fixture.order(session, "month-100", n);
    let checkout = amiss(checkout(n, order, "month-100"));
    fixture.told(
        u32::from(n) * 10,
        vec![EventKind::CheckoutCompleted(checkout)],
    );
}

pub(super) fn fingerprint(n: u8) -> String {
    format!("CardNumber{n:06}")
}

pub(super) fn charged(n: u8, card: u8) -> EventKind {
    EventKind::ChargeSucceeded(Charge {
        payment_intent: sid(&format!("pi_{n}")),
        card: CardUsed::Card(CardFingerprint::parse(&fingerprint(card)).unwrap()),
    })
}

/// The payment numbered `n` was made from a wallet.
pub(super) fn from_a_wallet(n: u8) -> EventKind {
    EventKind::ChargeSucceeded(Charge {
        payment_intent: sid(&format!("pi_{n}")),
        card: CardUsed::Wallet,
    })
}

/// The lines `orders` shows of the order `oid(n)` where it waits.
pub(super) fn entry(listed: &str, n: u8) -> String {
    let head = format!("  order {}  ", oid(n));
    listed
        .lines()
        .skip_while(|line| !line.starts_with(&head))
        .take_while(|line| line.starts_with(&head) || line.starts_with("    "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A database with one of everything the list shows.
pub(super) fn busy() -> Fixture {
    let fixture = Fixture::new();
    let alice = fixture.customer("alice@qq.com", 1, "pilotusera");
    let bob = fixture.customer("bob@163.com", 2, "pilotuserb");
    // Granted and pulled; in review for its price; waiting for its
    // charge; unpaid; a renewing plan, granted. And a payment that is no
    // order's, made with a card no ban names.
    let granted = fixture.order(&alice, "month-100", 1);
    let short = fixture.order(&alice, "pack-50", 2);
    let checking = fixture.order(&bob, "month-100", 3);
    fixture.order(&bob, "pack-50", 4);
    let renewing = fixture.order(&bob, "month-100-auto", 5);
    let wrong_price = CheckoutSession {
        amount_total: Some(250),
        ..checkout(2, short, "pack-50")
    };
    let stray = CheckoutSession {
        order: None,
        ..checkout(7, oid(7), "month-100")
    };
    fixture.told(
        1,
        vec![
            paid(1, granted, "month-100"),
            charged(1, 1),
            EventKind::CheckoutCompleted(wrong_price),
            paid(3, checking, "month-100"),
            paid(5, renewing, "month-100-auto"),
            charged(5, 2),
            EventKind::CheckoutCompleted(stray),
            charged(7, 1),
            EventKind::ChargeRefunded(Refunded {
                payment_intent: sid("pi_7"),
                amount_refunded: 100,
                in_full: false,
            }),
            EventKind::ChargeRefunded(Refunded {
                payment_intent: sid("pi_99"),
                amount_refunded: 800,
                in_full: true,
            }),
            EventKind::SubscriptionChanged(SubscriptionChange {
                id: sid("sub_5"),
                state: Renewal::Ending,
            }),
        ],
    );
    let token = TokenDigest::from_bytes([9; 32]);
    let mut store = fixture.store();
    store.set_pull_token(&token, at(100)).unwrap();
    store.pull_entitlements(&token, 1, 10, at(200)).unwrap();
    fixture
}
